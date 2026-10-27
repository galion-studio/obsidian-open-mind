// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Subcommands: `find`, `list`, `approve`, `deny`, `join`, `announce`,
//! `route`, `serve-route`.
//! Anything else falls through to the legacy flag form in `cli.rs`.

use crate::admit::{self, ApprovalsLog, Decision};
use crate::mdns::{self, Announcer, BrowseOpts, Service, DEFAULT_SERVICES, MAX_BROWSE};
use crate::probe::{self, Net24, ProbeOpts, DEFAULT_PORTS, DEFAULT_RATE};
use crate::routing;
use crate::{cli, fold_records, read_fixture_file, CliError, CliOut, DeviceKind};
use openmind_provider::route::{Capability, Kind};
use openmind_provider::{Approval, Registry};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_REGISTRY: &str = "discovery-providers.json";
const MAX_ANNOUNCE_SECS: u64 = 3600;

/// Run the CLI with real I/O. `args` excludes the program name.
pub fn run(args: Vec<String>) -> Result<CliOut, CliError> {
    let Some(first) = args.first() else {
        return cli(args, read_fixture_file);
    };
    let rest = &args[1..];
    match first.as_str() {
        "find" => find(rest),
        "list" => list(rest),
        "approve" => decide(rest, Decision::Approve),
        "deny" => decide(rest, Decision::Deny),
        "join" => join(rest),
        "announce" => announce(rest),
        "route" => route(rest),
        "serve-route" => serve_route(rest),
        "help" => Ok(CliOut::Help),
        _ => cli(args, read_fixture_file),
    }
}

fn usage(msg: impl Into<String>) -> CliError {
    CliError::Usage(msg.into())
}

fn fail(msg: impl std::fmt::Display) -> CliError {
    CliError::Fail(msg.to_string())
}

/// Parsed `--flag value` options plus positionals.
#[derive(Debug, Default)]
struct Opts {
    values: BTreeMap<String, Vec<String>>,
    switches: Vec<String>,
    positional: Vec<String>,
}

impl Opts {
    fn parse(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Self, CliError> {
        let mut out = Opts::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let a = arg.as_str();
            if a == "--help" || a == "-h" {
                return Err(CliError::Usage(String::new()));
            }
            if with_value.contains(&a) {
                let Some(v) = it.next() else {
                    return Err(usage(format!("missing value after {a}")));
                };
                if v.starts_with("--") {
                    return Err(usage(format!("missing value after {a}")));
                }
                out.values.entry(a.to_string()).or_default().push(v.clone());
            } else if switches.contains(&a) {
                out.switches.push(a.to_string());
            } else if a.starts_with('-') {
                let shown: String = a.chars().take(64).collect();
                return Err(usage(format!("unknown argument: {shown}")));
            } else {
                out.positional.push(arg.clone());
            }
        }
        for (k, v) in &out.values {
            if v.len() > 1 && k != "--service" && k != "--gpu" {
                return Err(usage(format!("duplicate {k}")));
            }
        }
        Ok(out)
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
    }

    fn all(&self, key: &str) -> &[String] {
        self.values.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    fn has(&self, key: &str) -> bool {
        self.switches.iter().any(|s| s == key)
    }

    fn num<T: std::str::FromStr>(&self, key: &str, default: T) -> Result<T, CliError> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| usage(format!("bad number for {key}: {v}"))),
        }
    }

    fn registry(&self) -> PathBuf {
        PathBuf::from(self.get("--registry").unwrap_or(DEFAULT_REGISTRY))
    }

    fn log(&self) -> ApprovalsLog {
        match self.get("--log") {
            Some(p) => ApprovalsLog::new(p),
            None => {
                let mut p = self.registry().into_os_string();
                p.push(".approvals.log");
                ApprovalsLog::new(p)
            }
        }
    }

    fn by(&self) -> String {
        let raw = self
            .get("--by")
            .map(str::to_string)
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "admin".into());
        let by: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
            .take(64)
            .collect();
        if by.is_empty() {
            "admin".into()
        } else {
            by
        }
    }

    fn one_id(&self) -> Result<&str, CliError> {
        match self.positional.as_slice() {
            [id] => Ok(id.as_str()),
            [] => Err(usage("missing provider id")),
            _ => Err(usage("give exactly one provider id")),
        }
    }

    fn no_positional(&self) -> Result<(), CliError> {
        match self.positional.first() {
            None => Ok(()),
            Some(p) => Err(usage(format!(
                "unexpected argument: {}",
                p.chars().take(64).collect::<String>()
            ))),
        }
    }
}

fn load(opts: &Opts) -> Result<Registry, CliError> {
    Registry::load(opts.registry()).map_err(fail)
}

/// Normalize `exo`, `_exo._tcp` or `_exo._tcp.local.` to `_exo._tcp.local.`.
pub fn service_type(text: &str) -> Result<String, String> {
    let t = text.trim().trim_end_matches('.').to_ascii_lowercase();
    let t = t.strip_suffix(".local").unwrap_or(&t);
    let (label, proto) = match t.strip_prefix('_') {
        Some(rest) => match rest.split_once("._") {
            Some((label, proto)) => (label.to_string(), proto.to_string()),
            None => return Err(format!("bad service type {text:?}")),
        },
        None => (t.to_string(), "tcp".to_string()),
    };
    let label_ok = (1..=15).contains(&label.len())
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !label.starts_with('-');
    if !label_ok || !(proto == "tcp" || proto == "udp") {
        return Err(format!("bad service type {text:?}"));
    }
    Ok(format!("_{label}._{proto}.local."))
}

fn find(args: &[String]) -> Result<CliOut, CliError> {
    let opts = Opts::parse(
        args,
        &[
            "--registry",
            "--log",
            "--timeout-ms",
            "--mdns-port",
            "--iface",
            "--services",
            "--by",
            "--probe",
            "--probe-ports",
            "--probe-hosts",
            "--rate",
        ],
        &["--no-mdns", "--dry-run"],
    )?;
    opts.no_positional()?;
    for k in ["--probe-ports", "--probe-hosts", "--rate"] {
        if opts.get(k).is_some() && opts.get("--probe").is_none() {
            return Err(usage(format!("{k} needs --probe")));
        }
    }
    // Validate every argument before any socket is opened.
    let probe_opts = probe_opts(&opts)?;
    let dry_run = opts.has("--dry-run");
    let mut registry = load(&opts)?;
    let log = opts.log();
    let mut report = serde_json::Map::new();
    let mut candidates = Vec::new();

    if opts.has("--no-mdns") {
        report.insert("mdns".into(), Value::Null);
    } else {
        let timeout_ms: u64 = opts.num("--timeout-ms", 2000)?;
        if timeout_ms == 0 || Duration::from_millis(timeout_ms) > MAX_BROWSE {
            return Err(usage("--timeout-ms must be 1..=30000"));
        }
        let services = match opts.get("--services") {
            None => DEFAULT_SERVICES.iter().map(|s| s.to_string()).collect(),
            Some(list) => list
                .split(',')
                .filter(|s| !s.is_empty())
                .map(service_type)
                .collect::<Result<Vec<_>, _>>()
                .map_err(usage)?,
        };
        if services.is_empty() || services.len() > 16 {
            return Err(usage("--services: give 1..=16 service types"));
        }
        let iface = match opts.get("--iface") {
            None => None,
            Some(v) => Some(
                v.parse::<Ipv4Addr>()
                    .map_err(|_| usage(format!("bad --iface {v}")))?,
            ),
        };
        let browse_opts = BrowseOpts {
            services: services.clone(),
            timeout: Duration::from_millis(timeout_ms),
            port: opts.num("--mdns-port", mdns::MDNS_PORT)?,
            iface,
        };
        let out = mdns::browse(&browse_opts).map_err(|e| fail(format!("mdns browse: {e}")))?;
        report.insert(
            "mdns".into(),
            json!({
                "services": services,
                "port": browse_opts.port,
                "iface": out.iface,
                "timeout_ms": timeout_ms,
                "packets_sent": out.packets_sent,
                "packets_received": out.packets_received,
                "responders": out.responders,
            }),
        );
        candidates.extend(fold_records(&out.records));
    }

    if let Some(popts) = probe_opts {
        let net = popts.net;
        let rate = popts.rate;
        let out = probe::probe(&popts);
        report.insert(
            "probe".into(),
            json!({
                "net": net.to_string(),
                "hosts": format!("{}-{}", popts.hosts.0, popts.hosts.1),
                "ports": popts.ports.iter().map(|(k, p)| format!("{}:{p}", k.as_str())).collect::<Vec<_>>(),
                "rate_per_s": rate,
                "attempts": out.attempts,
                "open": out.open,
                "elapsed_ms": out.elapsed_ms as u64,
            }),
        );
        candidates.extend(out.candidates);
    } else {
        report.insert("probe".into(), Value::Null);
    }

    let (stored, skipped) =
        admit::store_found(&mut registry, &log, &candidates, &opts.by(), dry_run).map_err(fail)?;
    report.insert("dry_run".into(), json!(dry_run));
    report.insert(
        "registry".into(),
        json!(registry.path().display().to_string()),
    );
    report.insert("log".into(), json!(log.path().display().to_string()));
    report.insert("devices".into(), json!(stored));
    report.insert("skipped".into(), json!(skipped));
    Ok(CliOut::Report(Value::Object(report)))
}

fn probe_opts(opts: &Opts) -> Result<Option<ProbeOpts>, CliError> {
    let Some(net) = opts.get("--probe") else {
        return Ok(None);
    };
    let net = Net24::parse(net).map_err(usage)?;
    let ports = match opts.get("--probe-ports") {
        Some(p) => probe::parse_ports(p).map_err(usage)?,
        None => DEFAULT_PORTS.to_vec(),
    };
    let hosts = match opts.get("--probe-hosts") {
        None => (1u8, 254u8),
        Some(h) => {
            let (a, b) = h.split_once('-').unwrap_or((h, h));
            let a: u8 = a.parse().map_err(|_| usage("bad --probe-hosts"))?;
            let b: u8 = b.parse().map_err(|_| usage("bad --probe-hosts"))?;
            if a == 0 || b == 255 || a > b {
                return Err(usage("--probe-hosts must be N-M inside 1..=254"));
            }
            (a, b)
        }
    };
    let rate: u32 = opts.num("--rate", DEFAULT_RATE)?;
    if rate == 0 || rate > probe::MAX_RATE {
        return Err(usage(format!("--rate must be 1..={}", probe::MAX_RATE)));
    }
    Ok(Some(ProbeOpts {
        net,
        ports,
        rate,
        connect_timeout: Duration::from_millis(300),
        hosts,
    }))
}

fn list(args: &[String]) -> Result<CliOut, CliError> {
    let opts = Opts::parse(args, &["--registry", "--approval"], &[])?;
    opts.no_positional()?;
    let filter = match opts.get("--approval") {
        None => None,
        Some("pending") => Some(Approval::Pending),
        Some("approved") => Some(Approval::Approved),
        Some("rejected") | Some("denied") => Some(Approval::Rejected),
        Some(other) => return Err(usage(format!("bad --approval {other}"))),
    };
    let registry = load(&opts)?;
    let rows: Vec<Value> = registry
        .providers()
        .iter()
        .filter(|r| filter.is_none_or(|f| r.approval() == f))
        .map(admit::record_json)
        .collect();
    Ok(CliOut::Report(json!({
        "registry": registry.path().display().to_string(),
        "providers": rows,
    })))
}

fn decide(args: &[String], decision: Decision) -> Result<CliOut, CliError> {
    let opts = Opts::parse(args, &["--registry", "--log", "--by", "--note"], &[])?;
    let id = opts.one_id()?;
    let mut registry = load(&opts)?;
    let line = admit::decide(
        &mut registry,
        &opts.log(),
        id,
        decision,
        &opts.by(),
        opts.get("--note"),
    )
    .map_err(fail)?;
    Ok(CliOut::Report(line))
}

fn join(args: &[String]) -> Result<CliOut, CliError> {
    let opts = Opts::parse(args, &["--registry", "--log", "--by", "--timeout-ms"], &[])?;
    let id = opts.one_id()?;
    let timeout_ms: u64 = opts.num("--timeout-ms", 1000)?;
    if !(1..=10_000).contains(&timeout_ms) {
        return Err(usage("--timeout-ms must be 1..=10000"));
    }
    let mut registry = load(&opts)?;
    let line = admit::join(
        &mut registry,
        &opts.log(),
        id,
        &opts.by(),
        Duration::from_millis(timeout_ms),
    )
    .map_err(fail)?;
    Ok(CliOut::Report(line))
}

fn router(opts: &Opts) -> Result<routing::Router, CliError> {
    let timeout_ms: u64 = opts.num("--timeout-ms", 2000)?;
    if !(1..=120_000).contains(&timeout_ms) {
        return Err(usage("--timeout-ms must be 1..=120000"));
    }
    let mut extra = Vec::new();
    for (i, url) in opts.all("--gpu").iter().enumerate() {
        extra.push(routing::operator_row(&format!("gpu-{i}"), Kind::Gpu, url).map_err(usage)?);
    }
    if let Some(url) = opts.get("--cpu") {
        extra.push(routing::operator_row("cpu-local", Kind::CpuLocal, url).map_err(usage)?);
    }
    Ok(routing::Router {
        registry: opts.registry(),
        extra,
        timeout: Duration::from_millis(timeout_ms),
    })
}

/// `route`: print the current fallback chain; with `--chat TEXT`, send one
/// chat completion through it.
fn route(args: &[String]) -> Result<CliOut, CliError> {
    let opts = Opts::parse(
        args,
        &[
            "--registry",
            "--gpu",
            "--cpu",
            "--timeout-ms",
            "--chat",
            "--model",
        ],
        &[],
    )?;
    opts.no_positional()?;
    let r = router(&opts)?;
    let rows = r.chain(Capability::Llm).map_err(fail)?;
    let chain: Vec<Value> = rows
        .iter()
        .map(|row| json!({"id": row.id, "kind": routing::kind_str(row.kind), "endpoint": row.endpoint}))
        .collect();
    let mut report = json!({
        "registry": r.registry.display().to_string(),
        "need": "llm",
        "chain": chain,
        "first": rows.first().map(|row| row.id.clone()),
    });
    if let Some(text) = opts.get("--chat") {
        let body = json!({
            "model": opts.get("--model").unwrap_or("auto"),
            "messages": [{"role": "user", "content": text}],
        });
        let fwd = r
            .forward(
                Capability::Llm,
                "POST",
                "/v1/chat/completions",
                body.to_string().as_bytes(),
            )
            .map_err(fail)?;
        report["provider"] = json!(fwd.provider);
        report["status"] = json!(fwd.status);
        report["attempts"] = json!(fwd.attempts);
        report["response"] = serde_json::from_slice(&fwd.body)
            .unwrap_or_else(|_| json!(String::from_utf8_lossy(&fwd.body)));
    }
    Ok(CliOut::Report(report))
}

/// `serve-route`: an OpenAI-compatible loopback router over the chain.
fn serve_route(args: &[String]) -> Result<CliOut, CliError> {
    let opts = Opts::parse(
        args,
        &[
            "--registry",
            "--gpu",
            "--cpu",
            "--timeout-ms",
            "--bind",
            "--duration-s",
        ],
        &[],
    )?;
    opts.no_positional()?;
    let r = router(&opts)?;
    let bind: std::net::SocketAddr = opts
        .get("--bind")
        .unwrap_or("127.0.0.1:19312")
        .parse()
        .map_err(|_| usage("--bind wants IP:PORT"))?;
    if !bind.ip().is_loopback() {
        return Err(usage("--bind must be a loopback address"));
    }
    let duration = match opts.get("--duration-s") {
        None => None,
        Some(_) => {
            let s: u64 = opts.num("--duration-s", 0)?;
            if !(1..=86_400).contains(&s) {
                return Err(usage("--duration-s must be 1..=86400"));
            }
            Some(Duration::from_secs(s))
        }
    };
    let registry = r.registry.display().to_string();
    // Long-running HTTP path: heartbeat (no-op unless NERVE_URL is set).
    nerve_client::start_heartbeat("openmind-discovery", 60);
    let stats = routing::serve(r, &routing::ServeOpts { bind, duration })
        .map_err(|e| fail(format!("serve-route {bind}: {e}")))?;
    Ok(CliOut::Report(json!({
        "bind": bind.to_string(),
        "registry": registry,
        "stats": stats,
    })))
}

/// Parse `TYPE:INSTANCE:PORT[:k=v;k=v]`.
pub fn parse_service(text: &str) -> Result<Service, String> {
    let mut parts = text.splitn(4, ':');
    let (Some(st), Some(instance), Some(port)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(format!(
            "bad --service {text:?}: want TYPE:INSTANCE:PORT[:k=v;k=v]"
        ));
    };
    let service_type = service_type(st)?;
    let instance = instance.to_ascii_lowercase();
    if !(1..=63).contains(&instance.len())
        || instance.starts_with('-')
        || !instance
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!(
            "bad instance {instance:?}: want 1..=63 of [a-z0-9-]"
        ));
    }
    let port: u16 = port
        .parse()
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| format!("bad port in {text:?}"))?;
    let mut txt = Vec::new();
    if let Some(pairs) = parts.next() {
        for pair in pairs.split(';').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let ok = !k.is_empty()
                && k.len() <= 63
                && k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && v.len() <= 200
                && v.chars().all(|c| (' '..='~').contains(&c));
            if !ok || txt.len() >= 16 {
                return Err(format!("bad TXT pair {pair:?}"));
            }
            txt.push((k.to_ascii_lowercase(), v.to_string()));
        }
    }
    Ok(Service {
        instance,
        service_type,
        port,
        txt,
    })
}

fn default_host() -> String {
    let raw = std::fs::read_to_string("/etc/hostname").unwrap_or_default();
    let label: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(40)
        .collect();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "openmind-discovery".into()
    } else {
        format!("galion-{label}")
    }
}

fn announce(args: &[String]) -> Result<CliOut, CliError> {
    let opts = Opts::parse(
        args,
        &[
            "--service",
            "--host",
            "--addr",
            "--mdns-port",
            "--duration-s",
            "--ttl",
        ],
        &[],
    )?;
    opts.no_positional()?;
    let services = opts
        .all("--service")
        .iter()
        .map(|s| parse_service(s))
        .collect::<Result<Vec<_>, _>>()
        .map_err(usage)?;
    if services.is_empty() || services.len() > 8 {
        return Err(usage("announce: give 1..=8 --service"));
    }
    let host = opts
        .get("--host")
        .map(str::to_string)
        .unwrap_or_else(default_host);
    let host = host
        .trim_end_matches('.')
        .trim_end_matches(".local")
        .to_ascii_lowercase();
    if !(1..=63).contains(&host.len())
        || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(usage("bad --host: want 1..=63 of [a-z0-9-]"));
    }
    let addr = match opts.get("--addr") {
        Some(v) => v
            .parse::<Ipv4Addr>()
            .map_err(|_| usage(format!("bad --addr {v}")))?,
        None => mdns::local_ipv4(),
    };
    let secs: u64 = opts.num("--duration-s", 30)?;
    if !(1..=MAX_ANNOUNCE_SECS).contains(&secs) {
        return Err(usage("--duration-s must be 1..=3600"));
    }
    let ttl: u32 = opts.num("--ttl", 120)?;
    if !(1..=4500).contains(&ttl) {
        return Err(usage("--ttl must be 1..=4500"));
    }
    let announcer = Announcer {
        host: format!("{host}.local."),
        addr,
        services,
        port: opts.num("--mdns-port", mdns::MDNS_PORT)?,
        ttl,
    };
    for svc in &announcer.services {
        let kind = DeviceKind::from_service_type(&svc.service_type)
            .map(DeviceKind::as_str)
            .unwrap_or("other");
        eprintln!(
            "{} announce-start {} ({kind}) port {} host {} addr {} mdns-port {}",
            zagreb_time::now_rfc3339(),
            svc.fqdn(),
            svc.port,
            announcer.host,
            announcer.addr,
            announcer.port
        );
    }
    let until = Instant::now() + Duration::from_secs(secs);
    // Long-running announce path: heartbeat (no-op unless NERVE_URL is set).
    nerve_client::start_heartbeat("openmind-discovery", 60);
    let stats = announcer
        .run(until, |line| {
            eprintln!("{} {line}", zagreb_time::now_rfc3339());
        })
        .map_err(|e| fail(format!("announce: {e}")))?;
    Ok(CliOut::Report(json!({
        "host": announcer.host,
        "addr": announcer.addr.to_string(),
        "mdns_port": announcer.port,
        "services": announcer.services.iter().map(Service::fqdn).collect::<Vec<_>>(),
        "duration_s": secs,
        "queries_seen": stats.queries_seen,
        "replies_sent": stats.replies_sent,
        "announcements_sent": stats.announcements_sent,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn service_type_forms() {
        assert_eq!(service_type("exo").unwrap(), "_exo._tcp.local.");
        assert_eq!(service_type("_galion._tcp").unwrap(), "_galion._tcp.local.");
        assert_eq!(
            service_type("_llama-rpc._tcp.local.").unwrap(),
            "_llama-rpc._tcp.local."
        );
        assert!(service_type("_x._sctp").is_err());
        assert!(service_type("waytoolonglabelname").is_err());
        assert!(service_type("_a b._tcp").is_err());
    }

    #[test]
    fn service_spec() {
        let svc = parse_service("galion:Box-1:19111:id=box-1;caps=llm").unwrap();
        assert_eq!(svc.fqdn(), "box-1._galion._tcp.local.");
        assert_eq!(svc.port, 19111);
        assert_eq!(
            svc.txt,
            vec![("id".into(), "box-1".into()), ("caps".into(), "llm".into())]
        );
        assert!(parse_service("galion:box").is_err());
        assert!(parse_service("galion:box:0").is_err());
        assert!(parse_service("galion:b_x:1").is_err());
    }

    #[test]
    fn argument_errors_are_usage_and_touch_nothing() {
        for args in [
            s(&["find", "--probe-ports", "exo:1"]),
            s(&["find", "--probe", "8.8.8.0/24"]), // repo-check: allow (test fixture address, never dialled)
            s(&["find", "--probe", "10.0.0.0/16"]),
            s(&["find", "--timeout-ms", "0"]),
            s(&["find", "--timeout-ms", "60000"]),
            s(&[
                "find",
                "--probe",
                "127.0.0.0/24",
                "--rate",
                "5",
                "--rate",
                "6",
            ]),
            s(&[
                "find",
                "--probe",
                "127.0.0.0/24",
                "--no-mdns",
                "--rate",
                "1000",
            ]),
            s(&[
                "find",
                "--probe",
                "127.0.0.0/24",
                "--no-mdns",
                "--probe-hosts",
                "0-3",
            ]),
            s(&["find", "--bogus"]),
            s(&["approve"]),
            s(&["deny", "a", "b"]),
            s(&["join", "x", "--timeout-ms", "0"]),
            s(&["announce"]),
            s(&["announce", "--service", "galion:box:1", "--duration-s", "0"]),
            s(&["announce", "--service", "galion:box:1", "--addr", "nope"]),
            s(&["list", "--approval", "maybe"]),
        ] {
            let res = run(args.clone());
            assert!(
                matches!(res, Err(CliError::Usage(_))),
                "{args:?} -> {res:?}"
            );
        }
    }

    #[test]
    fn probe_only_find_on_loopback_stores_pending() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let dir =
            std::env::temp_dir().join(format!("openmind-discovery-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let reg = dir.join("p.json");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\n\r\n{\"status\":\"ok\"}");
        });
        let out = run(s(&[
            "find",
            "--no-mdns",
            "--probe",
            "127.0.0.0/24",
            "--probe-hosts",
            "1-1",
            "--probe-ports",
            &format!("llamacpp:{port}"),
            "--registry",
            reg.to_str().unwrap(),
            "--by",
            "test",
        ]))
        .unwrap();
        server.join().unwrap();
        let CliOut::Report(v) = out else { panic!() };
        assert_eq!(v["probe"]["attempts"], 1);
        assert_eq!(v["devices"][0]["status"], "new");
        assert_eq!(v["devices"][0]["approval"], "pending");
        assert_eq!(v["devices"][0]["verified"], true);
        let id = v["devices"][0]["provider_id"].as_str().unwrap().to_string();
        assert_eq!(id, format!("llamacpp-127-0-0-1-{port}"));
        let CliOut::Report(l) = run(s(&["list", "--registry", reg.to_str().unwrap()])).unwrap()
        else {
            panic!()
        };
        assert_eq!(l["providers"][0]["approval"], "pending");
        assert_eq!(l["providers"][0]["health"], "up");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
