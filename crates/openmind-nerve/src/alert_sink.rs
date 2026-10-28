// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 382 (nerve-alerts-stage): optional loopback alert webhook.
//!
//! Off unless `NERVE_ALERT_WEBHOOK` is set. The target must be a plain
//! `http://` URL on a loopback address (127.0.0.0/8, `::1`, `localhost`) with
//! no userinfo, no query and no fragment, so no credential can ever sit in the
//! nerve's environment or in the repo; a relay that holds any upstream token
//! (ntfy server, Hermes, a Telegram bridge) lives behind that loopback port.
//!
//! * `NERVE_ALERT_WEBHOOK_FORMAT=json|ntfy` (default `json`): `json` POSTs
//!   `{"source","ts","severity","component","key","msg"}`; `ntfy` POSTs the
//!   message as `text/plain` with ntfy's `Title` / `Priority` / `Tags`
//!   headers, so a self-hosted ntfy on loopback works with no glue.
//! * `NERVE_ALERT_WEBHOOK_MIN=info|warn|crit` (default `warn`).
//! * `NERVE_ALERT_WEBHOOK_PER_HOUR` (default 6, clamped 1..=30): sliding-hour
//!   cap on sends, on top of the 30-min per-key dedup in [`crate::alerts`].
//!
//! One blocking HTTP/1.1 request per alert with 1 s connect and 2 s I/O
//! timeouts (loopback only, so the cycle cost is bounded). std only.

use crate::alerts::Severity;
use crate::util::ts;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

pub const DEFAULT_PER_HOUR: u32 = 6;
pub const MAX_PER_HOUR: u32 = 30;
const MSG_MAX: usize = 1000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Ntfy,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Ntfy => "ntfy",
        }
    }
}

/// Parsed, validated webhook settings (the raw env strings).
#[derive(Debug, Clone, PartialEq)]
pub struct WebhookConfig {
    pub addr: SocketAddr,
    /// Host text as given (for the `Host:` header and status lines).
    pub host: String,
    pub path: String,
    pub format: Format,
    pub min: Severity,
    pub per_hour: u32,
}

impl WebhookConfig {
    pub fn url(&self) -> String {
        format!("http://{}:{}{}", self.host, self.addr.port(), self.path)
    }
}

fn parse_sev(s: &str) -> Option<Severity> {
    match s.trim().to_ascii_lowercase().as_str() {
        "info" => Some(Severity::Info),
        "warn" | "warning" => Some(Severity::Warn),
        "crit" | "critical" => Some(Severity::Crit),
        _ => None,
    }
}

fn rank(s: Severity) -> u8 {
    match s {
        Severity::Info => 0,
        Severity::Warn => 1,
        Severity::Crit => 2,
    }
}

/// Validate a webhook URL. Only `http://<loopback>[:port]/path`.
pub fn parse_url(url: &str) -> Result<(SocketAddr, String, String), String> {
    let rest = url
        .trim()
        .strip_prefix("http://")
        .ok_or("only http:// is accepted (loopback target; TLS belongs in the relay)")?;
    if rest.contains(['?', '#']) {
        return Err("query/fragment not allowed (no tokens in the URL)".into());
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    if authority.contains('@') {
        return Err("userinfo not allowed (no credentials in the URL)".into());
    }
    if path.chars().any(|c| c.is_control() || c == ' ') {
        return Err("path has spaces or control characters".into());
    }
    let (host, port) = if let Some(h) = authority.strip_prefix('[') {
        let (h, p) = h.split_once(']').ok_or("bad IPv6 literal")?;
        (h.to_string(), p.strip_prefix(':').unwrap_or(""))
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p),
            None => (authority.to_string(), ""),
        }
    };
    let port: u16 = if port.is_empty() {
        80
    } else {
        port.parse().map_err(|_| format!("bad port {port:?}"))?
    };
    if port == 0 {
        return Err("port 0".into());
    }
    let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        host.parse()
            .map_err(|_| format!("host {host:?} is not an IP literal or localhost"))?
    };
    let ip = match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    };
    if !ip.is_loopback() {
        return Err(format!("host {ip} is not loopback; refusing"));
    }
    let host = match ip {
        IpAddr::V6(_) if host != "localhost" => format!("[{host}]"),
        _ => host,
    };
    // Same guard as the built-in client: no LLM / paid endpoint path, no
    // forbidden provider name (slot 424, no paid LLM egress).
    crate::egress::check_path(&path)?;
    Ok((SocketAddr::new(ip, port), host, path))
}

impl WebhookConfig {
    /// `Ok(None)` when not configured; `Err` when configured but rejected.
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Option<Self>, String> {
        let Some(url) = get("NERVE_ALERT_WEBHOOK").filter(|u| !u.trim().is_empty()) else {
            return Ok(None);
        };
        let (addr, host, path) = parse_url(&url)?;
        let format = match get("NERVE_ALERT_WEBHOOK_FORMAT")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "" | "json" => Format::Json,
            "ntfy" => Format::Ntfy,
            o => {
                return Err(format!(
                    "NERVE_ALERT_WEBHOOK_FORMAT {o:?}: use json or ntfy"
                ))
            }
        };
        let min = match get("NERVE_ALERT_WEBHOOK_MIN") {
            None => Severity::Warn,
            Some(s) if s.trim().is_empty() => Severity::Warn,
            Some(s) => parse_sev(&s)
                .ok_or_else(|| format!("NERVE_ALERT_WEBHOOK_MIN {s:?}: use info|warn|crit"))?,
        };
        let per_hour = get("NERVE_ALERT_WEBHOOK_PER_HOUR")
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(DEFAULT_PER_HOUR)
            .clamp(1, MAX_PER_HOUR);
        Ok(Some(WebhookConfig {
            addr,
            host,
            path,
            format,
            min,
            per_hour,
        }))
    }
}

/// Runtime sink with its rate window and counters.
#[derive(Debug)]
pub struct Webhook {
    pub cfg: WebhookConfig,
    window: VecDeque<u64>,
    pub sent: u64,
    pub failed: u64,
    pub rate_limited: u64,
    pub below_min: u64,
    pub last: Option<Value>,
}

/// Outcome of one offer to the sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    BelowMin,
    RateLimited,
    Sent(u16),
    Failed(String),
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl Webhook {
    pub fn new(cfg: WebhookConfig) -> Self {
        Webhook {
            cfg,
            window: VecDeque::new(),
            sent: 0,
            failed: 0,
            rate_limited: 0,
            below_min: 0,
            last: None,
        }
    }

    /// Build the HTTP request bytes (exposed for tests).
    pub fn request(
        &self,
        now: u64,
        sev: Severity,
        component: &str,
        key: &str,
        msg: &str,
    ) -> Vec<u8> {
        let msg = clean(msg, MSG_MAX);
        let component = clean(component, 80);
        let (ctype, extra, body) = match self.cfg.format {
            Format::Json => (
                "application/json",
                String::new(),
                json!({"source": "galion-nerve", "ts": ts(now), "severity": sev.as_str(),
                       "component": component, "key": clean(key, 200), "msg": msg})
                .to_string(),
            ),
            Format::Ntfy => {
                // Header values: ASCII printable only.
                let hv = |s: &str| -> String {
                    s.chars()
                        .filter(|c| c.is_ascii() && !c.is_ascii_control())
                        .take(120)
                        .collect()
                };
                let prio = match sev {
                    Severity::Info => "default",
                    Severity::Warn => "high",
                    Severity::Crit => "urgent",
                };
                (
                    "text/plain; charset=utf-8",
                    format!(
                        "Title: galion-nerve {} {}\r\nPriority: {prio}\r\nTags: galion-nerve,{}\r\n",
                        sev.as_str(),
                        hv(&component),
                        sev.as_str()
                    ),
                    msg,
                )
            }
        };
        let mut r = format!(
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\nUser-Agent: galion-nerve\r\nContent-Type: {ctype}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
            self.cfg.path,
            self.cfg.host,
            self.cfg.addr.port(),
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body.as_bytes());
        r
    }

    /// Offer one alert. Applies the severity floor and the hourly cap, then
    /// sends (blocking, bounded by the timeouts).
    pub fn offer(
        &mut self,
        now: u64,
        sev: Severity,
        component: &str,
        key: &str,
        msg: &str,
    ) -> Outcome {
        if rank(sev) < rank(self.cfg.min) {
            self.below_min += 1;
            return Outcome::BelowMin;
        }
        while self.window.front().is_some_and(|t| *t + 3600 <= now) {
            self.window.pop_front();
        }
        if self.window.len() as u32 >= self.cfg.per_hour {
            self.rate_limited += 1;
            self.last = Some(json!({"ts": ts(now), "key": key, "result": "rate-limited"}));
            return Outcome::RateLimited;
        }
        self.window.push_back(now);
        let req = self.request(now, sev, component, key, msg);
        let out = match send(self.cfg.addr, &req) {
            Ok(code) if (200..300).contains(&code) => Outcome::Sent(code),
            Ok(code) => Outcome::Failed(format!("HTTP {code}")),
            Err(e) => Outcome::Failed(e),
        };
        match &out {
            Outcome::Sent(c) => {
                self.sent += 1;
                self.last = Some(json!({"ts": ts(now), "key": key, "result": "ok", "http": c}));
            }
            Outcome::Failed(e) => {
                self.failed += 1;
                self.last =
                    Some(json!({"ts": ts(now), "key": key, "result": "failed", "error": e}));
            }
            _ => {}
        }
        out
    }

    pub fn status(&self) -> String {
        let last = self
            .last
            .as_ref()
            .and_then(|l| l["result"].as_str())
            .unwrap_or("none yet");
        format!(
            "webhook {} {} (min {}, {}/h): sent {}, failed {}, rate-limited {}; last {last}",
            self.cfg.format.as_str(),
            self.cfg.url(),
            self.cfg.min.as_str(),
            self.cfg.per_hour,
            self.sent,
            self.failed,
            self.rate_limited
        )
    }

    pub fn to_json(&self) -> Value {
        json!({
            "url": self.cfg.url(), "format": self.cfg.format.as_str(),
            "min": self.cfg.min.as_str(), "per_hour": self.cfg.per_hour,
            "in_window": self.window.len(),
            "sent": self.sent, "failed": self.failed, "rate_limited": self.rate_limited,
            "below_min": self.below_min, "last": self.last,
        })
    }
}

fn send(addr: SocketAddr, req: &[u8]) -> Result<u16, String> {
    // Defence in depth: never leave the host even if cfg was built by hand
    // (shared nerve/cortex egress guard, no paid LLM egress; slot 424).
    crate::egress::check_addr(&addr)?;
    let mut s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("connect: {}", e.kind()))?;
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
    s.write_all(req)
        .map_err(|e| format!("write: {}", e.kind()))?;
    let mut buf = [0u8; 256];
    let mut got = Vec::new();
    while !got.windows(2).any(|w| w == b"\r\n") && got.len() < 256 {
        let n = s
            .read(&mut buf)
            .map_err(|e| format!("read: {}", e.kind()))?;
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
    }
    let line = String::from_utf8_lossy(&got);
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .filter(|_| line.starts_with("HTTP/1."))
        .ok_or_else(|| "bad status line".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{Ipv6Addr, TcpListener};

    fn env(kv: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = kv
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn url_rules() {
        let ok = |u| parse_url(u).unwrap();
        assert_eq!(ok("http://127.0.0.1:2586/galion").0.port(), 2586);
        assert_eq!(ok("http://127.0.0.1:2586").2, "/");
        assert_eq!(
            ok("http://localhost:9/x").0.ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(
            ok("http://[::1]:9/x").0.ip(),
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        );
        assert_eq!(
            ok("http://[::ffff:127.0.0.1]:9/x").0.ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(ok("http://127.9.9.9/x").0.port(), 80);
        for bad in [
            "https://127.0.0.1/x",
            "http://10.0.0.1/x",
            "http://0.0.0.0:80/x",
            "http://ntfy.sh/topic",
            "http://[::ffff:8.8.8.8]/x",
            "http://user:pw@127.0.0.1/x",
            "http://127.0.0.1/x?auth=abc",
            "http://127.0.0.1/x#frag",
            "http://127.0.0.1:0/x",
            "http://127.0.0.1:99999/x",
            "http://127.0.0.1/a b",
            "ftp://127.0.0.1/",
        ] {
            assert!(parse_url(bad).is_err(), "{bad} accepted");
        }
    }

    #[test]
    fn config_defaults_and_errors() {
        assert_eq!(WebhookConfig::from_env(&env(&[])).unwrap(), None);
        assert_eq!(
            WebhookConfig::from_env(&env(&[("NERVE_ALERT_WEBHOOK", " ")])).unwrap(),
            None
        );
        let c = WebhookConfig::from_env(&env(&[("NERVE_ALERT_WEBHOOK", "http://127.0.0.1:1/a")]))
            .unwrap()
            .unwrap();
        assert_eq!(
            (c.format, c.min, c.per_hour),
            (Format::Json, Severity::Warn, 6)
        );
        let c = WebhookConfig::from_env(&env(&[
            ("NERVE_ALERT_WEBHOOK", "http://127.0.0.1:1/a"),
            ("NERVE_ALERT_WEBHOOK_FORMAT", "NTFY"),
            ("NERVE_ALERT_WEBHOOK_MIN", "crit"),
            ("NERVE_ALERT_WEBHOOK_PER_HOUR", "999"),
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            (c.format, c.min, c.per_hour),
            (Format::Ntfy, Severity::Crit, 30)
        );
        for (k, v) in [
            ("NERVE_ALERT_WEBHOOK_FORMAT", "xml"),
            ("NERVE_ALERT_WEBHOOK_MIN", "loud"),
        ] {
            assert!(WebhookConfig::from_env(&env(&[
                ("NERVE_ALERT_WEBHOOK", "http://127.0.0.1:1/a"),
                (k, v)
            ]))
            .is_err());
        }
        assert!(
            WebhookConfig::from_env(&env(&[("NERVE_ALERT_WEBHOOK", "http://1.2.3.4/a")])).is_err()
        );
    }

    fn sink(port: u16, format: Format, per_hour: u32) -> Webhook {
        let (addr, host, path) = parse_url(&format!("http://127.0.0.1:{port}/hook")).unwrap();
        Webhook::new(WebhookConfig {
            addr,
            host,
            path,
            format,
            min: Severity::Warn,
            per_hour,
        })
    }

    #[test]
    fn request_shapes() {
        let w = sink(9, Format::Json, 6);
        let r = String::from_utf8(w.request(0, Severity::Crit, "tuner", "k", "a\nb\"c")).unwrap();
        let (head, body) = r.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("POST /hook HTTP/1.1\r\nHost: 127.0.0.1:9\r\n"));
        assert!(head.contains(&format!("Content-Length: {}", body.len())));
        let v: Value = serde_json::from_str(body).unwrap();
        assert_eq!(v["severity"], "crit");
        assert_eq!(v["msg"], "ab\"c");
        assert!(v["ts"].as_str().unwrap().contains("+0"));
        let w = sink(9, Format::Ntfy, 6);
        let r = String::from_utf8(w.request(0, Severity::Warn, "t\r\nX: y", "k", "down")).unwrap();
        let (head, body) = r.split_once("\r\n\r\n").unwrap();
        assert!(head.contains("Title: galion-nerve warn tX: y\r\n"));
        assert!(!head.contains("\r\nX: y"), "header injection: {head}");
        assert!(head.contains("Priority: high"));
        assert_eq!(body, "down");
        let long = "x".repeat(5000);
        let r = w.request(0, Severity::Warn, "c", "k", &long);
        assert!(r.len() < MSG_MAX + 400);
    }

    #[test]
    fn floor_rate_and_failures() {
        // Closed port: bind then drop to get a free one.
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut w = sink(port, Format::Json, 2);
        assert_eq!(
            w.offer(100, Severity::Info, "c", "k", "m"),
            Outcome::BelowMin
        );
        assert!(matches!(
            w.offer(100, Severity::Warn, "c", "k", "m"),
            Outcome::Failed(_)
        ));
        assert!(matches!(
            w.offer(101, Severity::Crit, "c", "k", "m"),
            Outcome::Failed(_)
        ));
        assert_eq!(
            w.offer(102, Severity::Crit, "c", "k", "m"),
            Outcome::RateLimited
        );
        // Window slides after an hour.
        assert!(matches!(
            w.offer(3700, Severity::Crit, "c", "k", "m"),
            Outcome::Failed(_)
        ));
        assert_eq!(
            (w.failed, w.rate_limited, w.below_min, w.sent),
            (3, 1, 1, 0)
        );
        assert!(w.status().contains("failed 3"));
    }

    #[test]
    fn sends_to_capturing_listener() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut got = Vec::new();
            let mut b = [0u8; 1024];
            while !got.ends_with(b"}") {
                let n = s.read(&mut b).unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&b[..n]);
            }
            s.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8_lossy(&got).to_string()
        });
        let mut w = sink(port, Format::Json, 6);
        assert_eq!(
            w.offer(5, Severity::Crit, "nerve", "k9", "hello"),
            Outcome::Sent(204)
        );
        let got = h.join().unwrap();
        assert!(got.contains("\"key\":\"k9\""), "{got}");
        assert_eq!(w.to_json()["sent"], 1);
    }
}
