// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Read-only health probes for the local OpenMind stack.
//!
//! Each probe is one `GET` with a timeout. The result is `up`, `down`, or
//! `unknown`. Nothing is restarted, no other method is sent, and redirects
//! are not followed. Cortex is observed only: a `mode` field in its health
//! body is copied into the report and never written. The desktop does not
//! set cortex to live, and it does not set it back to dry-run.
//!
//! Endpoints come from [`ProbeConfig::defaults`] (the name `localhost` and a
//! port) or from a spec. The spec is `name=host:port/path`, comma-separated.
//! `name=-` leaves that service unconfigured, so no socket is opened. An
//! optional `timeout=MILLISECONDS` entry sets the per-probe budget (50 to
//! 10000). No address is built into the source.

use galion_common::http::{send_prefix, ClientError, ClientRequest};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// How long one probe may spend resolving and reading. Overridable.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(DEFAULT_TIMEOUT_MS);

/// [`DEFAULT_TIMEOUT`] in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 2_000;

/// Host used when a spec does not replace an endpoint. A name, not an address.
pub const DEFAULT_HOST: &str = "localhost";

/// Shortest accepted `timeout=` value, in milliseconds.
const MIN_TIMEOUT_MS: u64 = 50;

/// Longest accepted `timeout=` value, in milliseconds.
const MAX_TIMEOUT_MS: u64 = 10_000;

/// Bytes of a health body kept for the status line and a short JSON object.
const MAX_BODY: usize = 4_096;

/// Addresses tried after a refused connection. A timeout stops the walk.
const MAX_ADDRS: usize = 4;

/// One stack service the desktop knows how to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// Nerve `GET /health`.
    Nerve,
    /// Cortex `GET /health`. Mode is read, never set.
    Cortex,
    /// Console `GET /api/v1/health`.
    Console,
    /// Hermes `GET /health`.
    Hermes,
    /// Voice `GET /health`.
    Voice,
    /// Galion Desk `GET /health`. Read only.
    Desk,
    /// Open Mind memory `GET /health`. Read only.
    Memory,
    /// Galion One `GET /health`. Read only.
    GalionOne,
}

/// The eight services, in report order.
pub const SERVICES: [Service; 8] = [
    Service::Nerve,
    Service::Cortex,
    Service::Console,
    Service::Hermes,
    Service::Voice,
    Service::Desk,
    Service::Memory,
    Service::GalionOne,
];

impl Service {
    /// Spec and JSON name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Service::Nerve => "nerve",
            Service::Cortex => "cortex",
            Service::Console => "console",
            Service::Hermes => "hermes",
            Service::Voice => "voice",
            Service::Desk => "desk",
            Service::Memory => "memory",
            Service::GalionOne => "galion-one",
        }
    }

    /// Port used by [`ProbeConfig::defaults`].
    ///
    /// Nerve 8670 (`openmind-nerve` default), cortex 8674 (`gaia-cortex`
    /// default), console 18080 (Rust console API), Hermes 8642, voice 8002
    /// (Galion One mouth), desk 8646 (`galion-desk` default), memory 8420
    /// (`openmind-memory` default), Galion One 8091 (`ONE_BIND` default).
    #[must_use]
    pub fn default_port(self) -> u16 {
        match self {
            Service::Nerve => 8670,
            Service::Cortex => 8674,
            Service::Console => 18080,
            Service::Hermes => 8642,
            Service::Voice => 8002,
            Service::Desk => 8646,
            Service::Memory => 8420,
            Service::GalionOne => 8091,
        }
    }

    /// Path used by [`ProbeConfig::defaults`].
    #[must_use]
    pub fn default_path(self) -> &'static str {
        match self {
            Service::Console => "/api/v1/health",
            Service::Nerve
            | Service::Cortex
            | Service::Hermes
            | Service::Voice
            | Service::Desk
            | Service::Memory
            | Service::GalionOne => "/health",
        }
    }

    /// Parses a spec name. `None` when `name` is not one of the eight.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        SERVICES
            .into_iter()
            .find(|service| service.as_str() == name)
    }

    fn index(self) -> usize {
        match self {
            Service::Nerve => 0,
            Service::Cortex => 1,
            Service::Console => 2,
            Service::Hermes => 3,
            Service::Voice => 4,
            Service::Desk => 5,
            Service::Memory => 6,
            Service::GalionOne => 7,
        }
    }
}

/// What one probe decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// HTTP status 200 to 299.
    Up,
    /// The endpoint refused the connection, or answered outside 200 to 299.
    Down,
    /// No endpoint, the name did not resolve, the budget ran out, or the
    /// bytes were not an HTTP response. The desktop does not guess.
    Unknown,
}

impl State {
    /// `up`, `down`, or `unknown`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            State::Up => "up",
            State::Down => "down",
            State::Unknown => "unknown",
        }
    }
}

/// Where one service is asked. `enabled` false means the probe opens no socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Which service this row is.
    pub service: Service,
    /// Hostname from the spec. Defaults to [`DEFAULT_HOST`].
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Absolute path. The method is always `GET`.
    pub path: String,
    /// When false, [`probe_one`] returns [`State::Unknown`] without dialing.
    pub enabled: bool,
}

impl Endpoint {
    fn disabled(service: Service) -> Self {
        Self {
            service,
            host: DEFAULT_HOST.to_string(),
            port: service.default_port(),
            path: service.default_path().to_string(),
            enabled: false,
        }
    }
}

/// Time budget and the eight endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeConfig {
    /// Per-service budget for resolve plus the HTTP exchange.
    pub timeout: Duration,
    /// Nerve, cortex, console, Hermes, voice, desk, memory, Galion One, in that order.
    pub endpoints: [Endpoint; 8],
}

impl ProbeConfig {
    /// Local defaults. The host is [`DEFAULT_HOST`] for every service.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
            endpoints: SERVICES.map(|service| Endpoint {
                service,
                host: DEFAULT_HOST.to_string(),
                port: service.default_port(),
                path: service.default_path().to_string(),
                enabled: true,
            }),
        }
    }

    /// The default endpoints as a spec, without a timeout entry.
    #[must_use]
    pub fn default_spec() -> String {
        SERVICES
            .iter()
            .map(|service| {
                format!(
                    "{}={DEFAULT_HOST}:{}{}",
                    service.as_str(),
                    service.default_port(),
                    service.default_path()
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parses `spec` over [`Self::defaults`].
    ///
    /// Entries are separated by commas or semicolons. A service entry replaces
    /// that service. `timeout=MILLISECONDS` replaces the budget. Anything else
    /// is an error, and a repeated key is an error.
    ///
    /// # Errors
    /// A message naming the bad entry. The message contains no secret.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("empty probe spec".into());
        }
        let mut cfg = Self::defaults();
        let mut seen = [false; SERVICES.len()];
        let mut timeout_seen = false;
        for entry in spec.split([',', ';']) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let Some((key, value)) = entry.split_once('=') else {
                return Err(format!("probe entry {entry:?} needs name=host:port/path"));
            };
            let key = key.trim();
            let value = value.trim();
            if key == "timeout" {
                if timeout_seen {
                    return Err("timeout given twice".into());
                }
                timeout_seen = true;
                cfg.timeout = parse_timeout(value)?;
                continue;
            }
            let Some(service) = Service::parse(key) else {
                return Err(format!("unknown probe {key:?}"));
            };
            let index = service.index();
            if seen[index] {
                return Err(format!("{key} given twice"));
            }
            seen[index] = true;
            cfg.endpoints[index] = parse_endpoint(service, value)?;
        }
        Ok(cfg)
    }
}

/// One finished probe. `observed_mode` is set only for cortex, and only when
/// its body carried a short `mode` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeReport {
    /// Service this row describes.
    pub service: Service,
    /// False when the spec turned the probe off. No socket was opened.
    pub enabled: bool,
    /// Up, down, or unknown.
    pub state: State,
    /// Configured host. Not the address the resolver returned.
    pub host: String,
    /// Configured port.
    pub port: u16,
    /// Configured path.
    pub path: String,
    /// HTTP status, when a status line arrived.
    pub http_status: Option<u16>,
    /// Short reason: `200`, `http 503`, `connection refused`,
    /// `connection closed`, `timeout`, `not configured`, `resolve failed`,
    /// `not http`.
    pub detail: String,
    /// Cortex health `mode`, copied from the body. Never sent back.
    pub observed_mode: Option<String>,
}

/// The eight reports from one [`probe_all`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSet {
    /// Budget that was used.
    pub timeout: Duration,
    /// One row per configured endpoint, in service order.
    pub probes: Vec<ProbeReport>,
}

impl ProbeSet {
    /// True when at least one probe was enabled and every enabled probe is up.
    #[must_use]
    pub fn confirmed(&self) -> bool {
        let mut any = false;
        for probe in &self.probes {
            if !probe.enabled {
                continue;
            }
            any = true;
            if probe.state != State::Up {
                return false;
            }
        }
        any
    }

    /// One line for stderr when [`Self::confirmed`] is false.
    #[must_use]
    pub fn summary(&self) -> String {
        let parts: Vec<String> = self
            .probes
            .iter()
            .filter(|probe| probe.enabled)
            .map(|probe| format!("{}={}", probe.service.as_str(), probe.state.as_str()))
            .collect();
        if parts.is_empty() {
            "openmind-desktop: no probes enabled".into()
        } else {
            format!("openmind-desktop: probes {}", parts.join(" "))
        }
    }

    /// Compact JSON. `writes` is always 0: the probe run sent no body and
    /// changed no mode.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(1024);
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);
        out.push_str("{\"read_only\":true,\"writes\":0,\"timeout_ms\":");
        out.push_str(&timeout_ms.to_string());
        out.push_str(",\"probes\":[");
        for (i, probe) in self.probes.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"service\":");
            galion_common::json::push_str(&mut out, probe.service.as_str());
            out.push_str(",\"enabled\":");
            out.push_str(if probe.enabled { "true" } else { "false" });
            out.push_str(",\"state\":");
            galion_common::json::push_str(&mut out, probe.state.as_str());
            out.push_str(",\"host\":");
            galion_common::json::push_str(&mut out, &probe.host);
            out.push_str(",\"port\":");
            out.push_str(&probe.port.to_string());
            out.push_str(",\"path\":");
            galion_common::json::push_str(&mut out, &probe.path);
            out.push_str(",\"http_status\":");
            match probe.http_status {
                Some(code) => out.push_str(&code.to_string()),
                None => out.push_str("null"),
            }
            out.push_str(",\"detail\":");
            galion_common::json::push_str(&mut out, &probe.detail);
            out.push_str(",\"observed_mode\":");
            match &probe.observed_mode {
                Some(mode) => galion_common::json::push_str(&mut out, mode),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

/// Probes every endpoint. Each service has its own timeout, and the eight
/// run together so one slow service does not eat the others' budgets.
#[must_use]
pub fn probe_all(cfg: &ProbeConfig) -> ProbeSet {
    let probes = thread::scope(|scope| {
        let mut joins = Vec::with_capacity(cfg.endpoints.len());
        for endpoint in &cfg.endpoints {
            joins.push(scope.spawn(|| probe_one(endpoint, cfg.timeout)));
        }
        joins
            .into_iter()
            .zip(cfg.endpoints.iter())
            .map(|(handle, endpoint)| {
                handle.join().unwrap_or_else(|_| {
                    report(endpoint, State::Unknown, None, "probe failed", None)
                })
            })
            .collect()
    });
    ProbeSet {
        timeout: cfg.timeout,
        probes,
    }
}

/// One `GET`. Disabled endpoints do not dial.
#[must_use]
pub fn probe_one(endpoint: &Endpoint, timeout: Duration) -> ProbeReport {
    if !endpoint.enabled || endpoint.host.is_empty() || endpoint.port == 0 {
        return report(endpoint, State::Unknown, None, "not configured", None);
    }
    let started = Instant::now();
    let addrs = match resolve(&endpoint.host, endpoint.port, timeout) {
        Ok(addrs) => addrs,
        Err(ResolveError::Timeout) => {
            return report(endpoint, State::Unknown, None, "timeout", None);
        }
        Err(ResolveError::Failed) => {
            return report(endpoint, State::Unknown, None, "resolve failed", None);
        }
    };
    let host_header = host_header(&endpoint.host, endpoint.port);
    let request = ClientRequest::get(&endpoint.path, &host_header);
    if request.to_bytes().is_err() {
        return report(endpoint, State::Unknown, None, "invalid request", None);
    }
    let deadline = started + timeout;
    let mut refused = false;
    for addr in addrs {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return report(endpoint, State::Unknown, None, "timeout", None);
        };
        if left.is_zero() {
            return report(endpoint, State::Unknown, None, "timeout", None);
        }
        match send_prefix(addr, &request, left, MAX_BODY) {
            Ok(response) => {
                let mode = observed_mode(endpoint.service, &response.text());
                return classify_http(endpoint, response.status, mode);
            }
            Err(ClientError::Connect(err)) => {
                let (state, detail) = io_failure(&err);
                if state == State::Down {
                    refused = true;
                    continue;
                }
                return report(endpoint, state, None, detail, None);
            }
            Err(ClientError::Read(err) | ClientError::Write(err)) => {
                let (state, detail) = io_failure(&err);
                return report(endpoint, state, None, detail, None);
            }
            Err(ClientError::BadStatus(_)) => {
                return report(endpoint, State::Unknown, None, "not http", None);
            }
            Err(ClientError::TooLarge) => {
                return report(endpoint, State::Unknown, None, "response too large", None);
            }
            Err(ClientError::Invalid(_)) => {
                return report(endpoint, State::Unknown, None, "invalid request", None);
            }
        }
    }
    if refused {
        report(endpoint, State::Down, None, "connection refused", None)
    } else {
        report(endpoint, State::Unknown, None, "resolve failed", None)
    }
}

fn classify_http(endpoint: &Endpoint, status: u16, mode: Option<String>) -> ProbeReport {
    if (200..300).contains(&status) {
        report(endpoint, State::Up, Some(status), &status.to_string(), mode)
    } else {
        report(
            endpoint,
            State::Down,
            Some(status),
            &format!("http {status}"),
            mode,
        )
    }
}

fn report(
    endpoint: &Endpoint,
    state: State,
    http_status: Option<u16>,
    detail: &str,
    observed_mode: Option<String>,
) -> ProbeReport {
    ProbeReport {
        service: endpoint.service,
        enabled: endpoint.enabled,
        state,
        host: endpoint.host.clone(),
        port: endpoint.port,
        path: endpoint.path.clone(),
        http_status,
        detail: detail.to_string(),
        observed_mode,
    }
}

/// Cortex health `mode` copied from `body`.
///
/// Other services return [`None`], so a health body cannot be mistaken for a
/// cortex mode change. The string is kept only when it is 1 to 32 characters
/// of ASCII letters, digits, or `-`.
#[must_use]
pub fn observed_mode(service: Service, body: &str) -> Option<String> {
    if service != Service::Cortex {
        return None;
    }
    let raw = json_string_field(body, "mode")?;
    if (1..=32).contains(&raw.len())
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Some(raw)
    } else {
        None
    }
}

fn json_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut rest = body;
    while let Some(index) = rest.find(&needle) {
        let after = rest[index + needle.len()..].trim_start();
        if let Some(after) = after.strip_prefix(':') {
            let after = after.trim_start();
            if let Some(value) = after.strip_prefix('"') {
                return finish_json_string(value);
            }
        }
        rest = &rest[index + needle.len()..];
    }
    None
}

fn finish_json_string(value: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            ch => out.push(ch),
        }
    }
    None
}

enum ResolveError {
    Failed,
    Timeout,
}

fn resolve(host: &str, port: u16, timeout: Duration) -> Result<Vec<SocketAddr>, ResolveError> {
    let host = host.to_string();
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("desktop-probe-dns".into())
        .spawn(move || {
            let found = (host.as_str(), port)
                .to_socket_addrs()
                .map(|iter| iter.take(MAX_ADDRS).collect::<Vec<_>>());
            let _ = tx.send(found);
        });
    if spawned.is_err() {
        return Err(ResolveError::Failed);
    }
    match rx.recv_timeout(timeout) {
        Ok(Ok(list)) if list.is_empty() => Err(ResolveError::Failed),
        Ok(Ok(list)) => Ok(list),
        Ok(Err(_)) => Err(ResolveError::Failed),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(ResolveError::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(ResolveError::Failed),
    }
}

fn host_header(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn io_failure(err: &io::Error) -> (State, &'static str) {
    if is_timeout(err) {
        return (State::Unknown, "timeout");
    }
    match err.kind() {
        io::ErrorKind::ConnectionRefused => (State::Down, "connection refused"),
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => {
            (State::Down, "connection closed")
        }
        _ => (State::Unknown, "connect failed"),
    }
}

fn is_timeout(err: &io::Error) -> bool {
    if matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        return true;
    }
    let text = err.to_string().to_ascii_lowercase();
    text.contains("timed out") || text.contains("timeout")
}

fn parse_timeout(value: &str) -> Result<Duration, String> {
    let Ok(ms) = value.parse::<u64>() else {
        return Err(format!("timeout {value:?} is not milliseconds"));
    };
    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&ms) {
        return Err(format!(
            "timeout must be from {MIN_TIMEOUT_MS} to {MAX_TIMEOUT_MS} milliseconds"
        ));
    }
    Ok(Duration::from_millis(ms))
}

fn parse_endpoint(service: Service, value: &str) -> Result<Endpoint, String> {
    if value == "-" {
        return Ok(Endpoint::disabled(service));
    }
    let value = strip_http(service, value)?;
    let (host, rest) = split_host(service, value)?;
    let (port, path) = split_port_path(service, rest)?;
    validate_host(service, &host)?;
    validate_path(service, &path)?;
    Ok(Endpoint {
        service,
        host,
        port,
        path,
        enabled: true,
    })
}

fn strip_http(service: Service, value: &str) -> Result<&str, String> {
    if let Some((scheme, rest)) = value.split_once("://") {
        if scheme == "http" {
            if rest.is_empty() {
                return Err(format!(
                    "{}: probe endpoint needs host:port/path",
                    service.as_str()
                ));
            }
            return Ok(rest);
        }
        return Err(format!("{}: probes are plain GET only", service.as_str()));
    }
    Ok(value)
}

fn split_host(service: Service, value: &str) -> Result<(String, &str), String> {
    if let Some(rest) = value.strip_prefix('[') {
        let Some((host, rest)) = rest.split_once("]:") else {
            return Err(format!(
                "{}: bracketed host needs [host]:port/path",
                service.as_str()
            ));
        };
        if host.is_empty() {
            return Err(format!("{}: empty host", service.as_str()));
        }
        return Ok((host.to_string(), rest));
    }
    let Some((host, rest)) = value.split_once(':') else {
        return Err(format!(
            "{}: probe endpoint needs host:port/path",
            service.as_str()
        ));
    };
    if host.is_empty() {
        return Err(format!("{}: empty host", service.as_str()));
    }
    Ok((host.to_string(), rest))
}

fn split_port_path(service: Service, value: &str) -> Result<(u16, String), String> {
    let Some((port_text, path)) = value.split_once('/') else {
        return Err(format!(
            "{}: probe endpoint needs host:port/path",
            service.as_str()
        ));
    };
    if port_text.is_empty() || !port_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{}: bad port", service.as_str()));
    }
    let Ok(port) = port_text.parse::<u16>() else {
        return Err(format!("{}: bad port", service.as_str()));
    };
    if port == 0 {
        return Err(format!("{}: bad port", service.as_str()));
    }
    Ok((port, format!("/{path}")))
}

fn validate_host(service: Service, host: &str) -> Result<(), String> {
    if host.len() > 253
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'))
        || !host.bytes().any(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(format!("{}: bad host", service.as_str()));
    }
    Ok(())
}

fn validate_path(service: Service, path: &str) -> Result<(), String> {
    if path.len() > 128
        || !path.starts_with('/')
        || path.contains("//")
        || path.split('/').any(|part| part == "..")
        || !path
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte) && !matches!(byte, b'?' | b'#' | b'\\'))
    {
        return Err(format!("{}: bad path", service.as_str()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn looks_like_ipv4(host: &str) -> bool {
        let parts: Vec<&str> = host.split('.').collect();
        parts.len() == 4
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }

    #[test]
    fn defaults_use_a_name_and_the_eight_health_paths() {
        let cfg = ProbeConfig::defaults();
        assert_eq!(cfg.timeout, DEFAULT_TIMEOUT);
        assert_eq!(cfg.endpoints.len(), 8);
        assert_eq!(Service::Memory.default_port(), 8420);
        assert_eq!(Service::GalionOne.default_port(), 8091);
        assert_eq!(Service::GalionOne.default_path(), "/health");
        assert_eq!(Service::parse("galion-one"), Some(Service::GalionOne));
        for (endpoint, service) in cfg.endpoints.iter().zip(SERVICES) {
            assert_eq!(endpoint.service, service);
            assert_eq!(endpoint.host, DEFAULT_HOST);
            assert!(endpoint.enabled);
            assert!(!looks_like_ipv4(&endpoint.host));
            assert!(!endpoint.host.contains(':'));
            assert_eq!(endpoint.port, service.default_port());
            assert_eq!(endpoint.path, service.default_path());
        }
        let spec = ProbeConfig::default_spec();
        assert!(!looks_like_ipv4_in(spec.as_str()));
        assert_eq!(ProbeConfig::parse(&spec).unwrap(), cfg);
    }

    fn looks_like_ipv4_in(text: &str) -> bool {
        text.split(|ch: char| !ch.is_ascii_digit() && ch != '.')
            .any(looks_like_ipv4)
    }

    #[test]
    fn spec_replaces_one_endpoint_and_can_disable_another() {
        let cfg =
            ProbeConfig::parse("cortex=http://cortex.local:9/health,voice=-,timeout=250").unwrap();
        assert_eq!(cfg.timeout, Duration::from_millis(250));
        assert_eq!(cfg.endpoints[Service::Nerve.index()].port, 8670);
        let cortex = &cfg.endpoints[Service::Cortex.index()];
        assert_eq!(cortex.host, "cortex.local");
        assert_eq!(cortex.port, 9);
        assert_eq!(cortex.path, "/health");
        assert!(cortex.enabled);
        let voice = &cfg.endpoints[Service::Voice.index()];
        assert!(!voice.enabled);
        assert_eq!(voice.host, DEFAULT_HOST);
    }

    #[test]
    fn bad_specs_are_refused() {
        for spec in [
            "",
            "nerve",
            "nope=localhost:1/health",
            "nerve=https://localhost:1/health",
            "nerve=localhost/health",
            "nerve=localhost:8670",
            "nerve=localhost:0/health",
            "nerve=localhost:8670/health?mode=live",
            "nerve=localhost:8670/health,nerve=localhost:1/health",
            "timeout=1",
            "timeout=nope",
            "cortex=localhost:8674/../health",
            "hermes=user@localhost:8642/health",
        ] {
            assert!(ProbeConfig::parse(spec).is_err(), "{spec}");
        }
    }

    #[test]
    fn disabled_probe_opens_no_socket() {
        let hits = Arc::new(Mutex::new(0u32));
        let listener = TcpListener::bind((DEFAULT_HOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let counter = Arc::clone(&hits);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                *counter.lock().expect("hits") += 1;
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
            }
        });
        let mut endpoint = endpoint_at(Service::Nerve, addr, "/health");
        endpoint.enabled = false;
        let report = probe_one(&endpoint, Duration::from_millis(200));
        assert_eq!(report.state, State::Unknown);
        assert_eq!(report.detail, "not configured");
        assert!(report.observed_mode.is_none());
        thread::sleep(Duration::from_millis(50));
        assert_eq!(*hits.lock().expect("hits"), 0);
    }

    #[test]
    fn a_refused_error_is_down() {
        let err = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        let (state, detail) = io_failure(&err);
        assert_eq!(state, State::Down);
        assert_eq!(detail, "connection refused");
    }

    #[test]
    fn up_down_timeout_and_garbage() {
        let up = serve_status(200, "{\"status\":\"ok\"}");
        let up_report = probe_one(
            &endpoint_at(Service::Nerve, up.addr, "/health"),
            Duration::from_secs(1),
        );
        assert_eq!(up_report.state, State::Up);
        assert_eq!(up_report.http_status, Some(200));
        assert_eq!(up_report.detail, "200");
        assert!(up_report.observed_mode.is_none());
        assert_eq!(up.methods(), vec!["GET".to_string()]);

        let down = serve_status(503, "{\"ok\":false,\"status\":\"loading\"}");
        let down_report = probe_one(
            &endpoint_at(Service::Voice, down.addr, "/health"),
            Duration::from_secs(1),
        );
        assert_eq!(down_report.state, State::Down);
        assert_eq!(down_report.http_status, Some(503));
        assert_eq!(down_report.detail, "http 503");
        assert!(down_report.observed_mode.is_none());

        // The port stays owned and is not listening, so the stack refuses the
        // handshake. Dropping the listener instead made this race two ways:
        // another process could bind the port, and the Windows runner drops a
        // SYN to a port nobody owns, so connect_timeout waited out the budget
        // and the probe reported Unknown (runs 43655186465 and 43655827496).
        let (_owned, refused_addr) = refusing_socket();
        let refused_report = probe_one(
            &endpoint_at(Service::Hermes, refused_addr, "/health"),
            Duration::from_millis(400),
        );
        assert!(refused_report.http_status.is_none());
        assert_eq!(refused_report.state, State::Down);
        assert_eq!(refused_report.detail, "connection refused");

        // The peer accepts and then holds past the budget. It must not close
        // on its own: an 800 ms close raced a 200 ms budget, and a late
        // timeout came back as "connection closed" or blew the wall clock.
        let hung = TcpListener::bind((DEFAULT_HOST, 0)).unwrap();
        let hung_addr = hung.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((sock, _)) = hung.accept() {
                thread::sleep(Duration::from_secs(10));
                drop(sock);
            }
        });
        let started = Instant::now();
        let hung_report = probe_one(
            &endpoint_at(Service::Console, hung_addr, "/api/v1/health"),
            Duration::from_millis(200),
        );
        assert_eq!(hung_report.state, State::Unknown);
        assert_eq!(hung_report.detail, "timeout");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "budget should fire while the peer is still holding: {:?}",
            started.elapsed()
        );

        let garbage = serve_raw(b"hello\r\n");
        let garbage_report = probe_one(
            &endpoint_at(Service::Nerve, garbage.addr, "/health"),
            Duration::from_secs(1),
        );
        assert_eq!(garbage_report.state, State::Unknown);
        assert_eq!(garbage_report.detail, "not http");
    }

    #[test]
    fn cortex_mode_is_observed_and_not_written() {
        for mode in ["dry-run", "live"] {
            let mock = serve_cortex(mode);
            let report = probe_one(
                &endpoint_at(Service::Cortex, mock.addr, "/health"),
                Duration::from_secs(1),
            );
            assert_eq!(report.state, State::Up, "{mode}");
            assert_eq!(report.observed_mode.as_deref(), Some(mode));
            let seen = mock.requests.lock().expect("requests");
            assert_eq!(seen.len(), 1, "{mode}");
            let request = &seen[0];
            assert!(request.starts_with("GET /health "), "{request}");
            assert!(!request.to_ascii_uppercase().contains("POST"), "{request}");
            assert!(!request.contains("CORTEX_ACTIONS"), "{request}");
            assert!(request.split("\r\n\r\n").nth(1).unwrap_or("").is_empty());
            assert_eq!(*mock.mode.lock().expect("mode"), mode);
        }
    }

    #[test]
    fn a_redirect_is_not_followed() {
        let hits = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind((DEFAULT_HOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let recorded = Arc::clone(&hits);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = read_head(&mut sock);
                recorded.lock().expect("hits").push(request);
                let body = "HTTP/1.1 302 Found\r\nlocation: /restart\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
                let _ = sock.write_all(body.as_bytes());
                let _ = sock.shutdown(Shutdown::Both);
            }
            // A followed redirect would connect again.
            let _ = listener.accept();
        });
        let report = probe_one(
            &endpoint_at(Service::Cortex, addr, "/health"),
            Duration::from_millis(400),
        );
        assert_eq!(report.state, State::Down);
        assert_eq!(report.http_status, Some(302));
        thread::sleep(Duration::from_millis(80));
        assert_eq!(hits.lock().expect("hits").len(), 1);
    }

    #[test]
    fn timeouts_run_together() {
        let mut cfg = ProbeConfig::defaults();
        cfg.timeout = Duration::from_millis(200);
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        for endpoint in &mut cfg.endpoints {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let addr = listener.local_addr().unwrap();
            endpoint.host = addr.ip().to_string();
            endpoint.port = addr.port();
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            thread::spawn(move || {
                if let Ok((sock, _)) = listener.accept() {
                    let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    // Hold well past one budget so a serial walk cannot finish
                    // by the server closing, and a late timeout is still a timeout.
                    thread::sleep(Duration::from_secs(10));
                    drop(sock);
                }
            });
        }
        let started = Instant::now();
        let report = probe_all(&cfg);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(3),
            "probes should share one budget, not the server hold: {elapsed:?}"
        );
        let in_flight = peak.load(Ordering::SeqCst);
        assert!(in_flight >= 2, "probes in flight at once: {in_flight}");
        assert!(report
            .probes
            .iter()
            .all(|probe| probe.state == State::Unknown));
        assert!(report.probes.iter().all(|probe| probe.detail == "timeout"));
        assert!(!report.confirmed());
        assert!(report.to_json().contains("\"writes\":0"));
        assert!(report.to_json().contains("\"read_only\":true"));
    }

    #[test]
    fn nerve_mode_field_is_not_a_cortex_mode() {
        let mock = serve_status(200, "{\"status\":\"ok\",\"mode\":\"live\"}");
        let report = probe_one(
            &endpoint_at(Service::Nerve, mock.addr, "/health"),
            Duration::from_secs(1),
        );
        assert_eq!(report.state, State::Up);
        assert!(report.observed_mode.is_none());
    }

    #[test]
    fn a_refused_address_does_not_hide_a_later_one() {
        let host = "walk.localhost";
        let preview: Vec<std::net::IpAddr> = (host, 9u16)
            .to_socket_addrs()
            .expect("localhost name")
            .map(|addr| addr.ip())
            .collect();
        assert!(
            !preview.is_empty(),
            "{host} did not resolve on this machine"
        );
        let serve_ip = *preview.last().expect("address");
        let listener = TcpListener::bind((serve_ip, 0)).expect("serve");
        let port = listener.local_addr().expect("port").port();
        let mut held = Vec::new();
        for ip in &preview[..preview.len() - 1] {
            held.push(refusing_socket_at(SocketAddr::new(*ip, port)));
        }
        let requests = Arc::new(Mutex::new(0u32));
        let counted = Arc::clone(&requests);
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                *counted.lock().expect("hits") += 1;
                let _ = sock.write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                );
                let _ = sock.shutdown(Shutdown::Both);
            }
        });
        let report = probe_one(
            &Endpoint {
                service: Service::Nerve,
                host: host.into(),
                port,
                path: "/health".into(),
                enabled: true,
            },
            Duration::from_secs(1),
        );
        assert_eq!(report.state, State::Up, "{}", report.detail);
        assert_eq!(report.http_status, Some(200));
        assert_eq!(*requests.lock().expect("hits"), 1);
        assert_eq!(held.len(), preview.len() - 1);
    }

    #[test]
    fn unresolved_name_is_unknown() {
        // A space cannot be a DNS name, so this fails inside getaddrinfo.
        // `not-a-host.invalid` is a real lookup: a slow resolver burns the
        // budget and the probe reports "timeout" instead of "resolve failed".
        let endpoint = Endpoint {
            service: Service::Console,
            host: "not a host".into(),
            port: 9,
            path: "/api/v1/health".into(),
            enabled: true,
        };
        let started = Instant::now();
        let report = probe_one(&endpoint, Duration::from_secs(2));
        assert_eq!(report.state, State::Unknown);
        assert_eq!(report.detail, "resolve failed");
        assert!(report.observed_mode.is_none());
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "resolve consulted a resolver: {:?}",
            started.elapsed()
        );
    }

    struct Mock {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
        mode: Arc<Mutex<String>>,
    }

    impl Mock {
        fn methods(&self) -> Vec<String> {
            self.requests
                .lock()
                .expect("requests")
                .iter()
                .filter_map(|request| request.split_whitespace().next().map(str::to_string))
                .collect()
        }
    }

    /// Bound, not listening. The port cannot be stolen, and the handshake is
    /// refused on every target this crate tests.
    fn refusing_socket() -> (socket2::Socket, SocketAddr) {
        let socket = refusing_socket_at(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)));
        let addr = socket
            .local_addr()
            .expect("local addr")
            .as_socket()
            .expect("ip socket");
        (socket, addr)
    }

    fn refusing_socket_at(addr: SocketAddr) -> socket2::Socket {
        let domain = if addr.is_ipv4() {
            socket2::Domain::IPV4
        } else {
            socket2::Domain::IPV6
        };
        let socket =
            socket2::Socket::new(domain, socket2::Type::STREAM, Some(socket2::Protocol::TCP))
                .expect("tcp socket");
        if addr.is_ipv6() {
            socket.set_only_v6(true).expect("ipv6 only");
        }
        socket.bind(&addr.into()).expect("bind");
        socket
    }

    fn endpoint_at(service: Service, addr: SocketAddr, path: &str) -> Endpoint {
        Endpoint {
            service,
            host: addr.ip().to_string(),
            port: addr.port(),
            path: path.to_string(),
            enabled: true,
        }
    }

    fn serve_status(status: u16, body: &str) -> Mock {
        let bytes = format!(
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        serve_raw(bytes.as_bytes())
    }

    fn serve_raw(bytes: &[u8]) -> Mock {
        let bytes = bytes.to_vec();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let listener = TcpListener::bind((DEFAULT_HOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = read_head(&mut sock);
                recorded.lock().expect("requests").push(request);
                let _ = sock.write_all(&bytes);
                let _ = sock.shutdown(Shutdown::Both);
            }
        });
        Mock {
            addr,
            requests,
            mode: Arc::new(Mutex::new(String::new())),
        }
    }

    fn serve_cortex(mode: &str) -> Mock {
        let body = format!("{{\"ok\":true,\"mode\":\"{mode}\"}}");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stored_mode = Arc::new(Mutex::new(mode.to_string()));
        let recorded = Arc::clone(&requests);
        let stored = Arc::clone(&stored_mode);
        let listener = TcpListener::bind((DEFAULT_HOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let request = read_head(&mut sock);
                let method = request.split_whitespace().next().unwrap_or("");
                if method != "GET" {
                    *stored.lock().expect("mode") = "changed".into();
                }
                recorded.lock().expect("requests").push(request);
                let bytes = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(bytes.as_bytes());
                let _ = sock.shutdown(Shutdown::Both);
            }
        });
        Mock {
            addr,
            requests,
            mode: stored_mode,
        }
    }

    fn read_head(sock: &mut TcpStream) -> String {
        let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
        let mut buf = [0u8; 2048];
        let mut got = Vec::new();
        loop {
            match sock.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    got.extend_from_slice(&buf[..n]);
                    if got.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        String::from_utf8_lossy(&got).into_owned()
    }
}
