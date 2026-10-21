// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Journal text insights for the cortex: what the raw log lines say beyond
//! error counts.
//!
//! - cloudflared refused origin dials (`dial tcp [::1]:4001: connect:
//!   connection refused`): which origin ports are dead and which public
//!   hostnames route to them. These are the tunnel's "bleeding" routes.
//! - scanner / exploit probes (`/.env`, `/.git/config`, `phpinfo.php`, ...)
//!   that a service logged, e.g. the Vite dev server behind console.
//! - dev servers running in production (Vite messages).
//!
//! Pure functions over [`JLine`]s, so the live loop and `replay` share them.

use crate::probe::JLine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Path fragments that only scanners and exploit kits ask for.
pub const SCAN_PATTERNS: &[&str] = &[
    "/.env",
    "/.git/",
    "/.git",
    "phpinfo",
    "wp-login",
    "wp-admin",
    "xmlrpc.php",
    "/etc/passwd",
    "../",
    "/.aws/",
    "/.ssh/",
    "/server-status",
    "/cgi-bin/",
    "/vendor/phpunit",
    "/actuator",
    "/.ds_store",
    "/config.json",
    "/.htaccess",
    "/boaform",
];

/// Is this request path a scanner probe?
pub fn is_scan_path(p: &str) -> bool {
    let l = p.to_ascii_lowercase();
    SCAN_PATTERNS.iter().any(|s| l.contains(s))
}

/// One dead origin port behind the tunnel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeadOrigin {
    pub port: u16,
    pub service: String,
    /// Public hostnames whose requests failed on this port.
    pub hosts: BTreeSet<String>,
    /// Failed requests (cloudflared "Request failed" lines).
    pub failed_requests: u32,
    /// Refused dials (all lines naming the port).
    pub refusals: u32,
    pub first: u64,
    pub last: u64,
}

/// A scanner path seen in a unit's log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanHit {
    pub unit: String,
    pub path: String,
    pub count: u32,
    pub last: u64,
}

/// Everything the cortex reads out of one journal window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogInsights {
    pub dead_origins: BTreeMap<u16, DeadOrigin>,
    /// Timestamps of cloudflared "Request failed" lines (edge requests that
    /// got an error page).
    pub edge_failures: Vec<u64>,
    pub scans: BTreeMap<(String, String), ScanHit>,
    /// Units that print Vite dev-server messages.
    pub dev_servers: BTreeSet<String>,
    pub window_first: u64,
    pub window_last: u64,
    pub lines: usize,
}

fn after<'a>(msg: &'a str, key: &str) -> Option<&'a str> {
    let i = msg.find(key)?;
    let rest = &msg[i + key.len()..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '"')
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Port of a refused dial: `dial tcp [::1]:4001: connect: connection refused`
/// or `dial tcp 127.0.0.1:3011: connect: connection refused`.
pub fn refused_port(msg: &str) -> Option<u16> {
    if !msg.contains("connection refused") {
        return None;
    }
    let d = after(msg, "dial tcp ")?;
    let d = d.trim_end_matches(':');
    d.rsplit_once(':')?.1.parse().ok()
}

/// Host part of `dest=https://galion.studio/path`.
pub fn dest_host(msg: &str) -> Option<String> {
    let d = after(msg, "dest=")?;
    let d = d
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("wss://")
        .trim_start_matches("ws://");
    let h = d.split(['/', '?']).next()?;
    (!h.is_empty()).then(|| h.chars().take(100).collect())
}

/// Scanner paths named in a message (quoted request ids, plain tokens).
pub fn scan_paths(msg: &str) -> Vec<String> {
    let mut v: Vec<String> = msg
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
        .map(|t| t.trim_end_matches(['.', ';', ')']))
        .filter(|t| t.starts_with('/') && t.len() > 1 && is_scan_path(t))
        .map(|t| t.chars().take(120).collect())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Read one window of journal lines.
pub fn insights(lines: &[JLine]) -> LogInsights {
    let mut li = LogInsights {
        lines: lines.len(),
        ..Default::default()
    };
    for l in lines {
        if l.t > 0 {
            if li.window_first == 0 || l.t < li.window_first {
                li.window_first = l.t;
            }
            li.window_last = li.window_last.max(l.t);
        }
        let m = l.msg.as_str();
        if let Some(port) = refused_port(m) {
            let d = li.dead_origins.entry(port).or_insert_with(|| DeadOrigin {
                port,
                first: l.t,
                ..Default::default()
            });
            d.refusals += 1;
            d.first = if d.first == 0 { l.t } else { d.first.min(l.t) };
            d.last = d.last.max(l.t);
            if let Some(s) = after(m, "originService=") {
                d.service = s.chars().take(80).collect();
            }
            if m.contains("Request failed") {
                d.failed_requests += 1;
                if let Some(h) = dest_host(m) {
                    if d.hosts.len() < 32 {
                        d.hosts.insert(h);
                    }
                }
            }
        }
        if m.contains("Request failed") {
            li.edge_failures.push(l.t);
        }
        if m.contains("Vite") || m.contains("vite:") {
            li.dev_servers.insert(l.unit.clone());
        }
        for p in scan_paths(m) {
            let key = (l.unit.clone(), p.clone());
            if !li.scans.contains_key(&key) && li.scans.len() >= 256 {
                continue;
            }
            let h = li.scans.entry(key).or_insert_with(|| ScanHit {
                unit: l.unit.clone(),
                path: p,
                ..Default::default()
            });
            h.count += 1;
            h.last = h.last.max(l.t);
        }
    }
    // A refusal logged only on the "error=" line (no Request failed line)
    // still counts as one failed request.
    for d in li.dead_origins.values_mut() {
        if d.failed_requests == 0 {
            d.failed_requests = d.refusals;
        }
    }
    li
}

impl LogInsights {
    /// Fold a newer window into this one (bounded).
    pub fn merge(&mut self, o: LogInsights) {
        for (p, d) in o.dead_origins {
            let e = self.dead_origins.entry(p).or_insert_with(|| DeadOrigin {
                port: p,
                first: d.first,
                ..Default::default()
            });
            e.refusals += d.refusals;
            e.failed_requests += d.failed_requests;
            e.last = e.last.max(d.last);
            if e.service.is_empty() {
                e.service = d.service.clone();
            }
            for h in d.hosts {
                if e.hosts.len() < 32 {
                    e.hosts.insert(h);
                }
            }
        }
        self.edge_failures.extend(o.edge_failures);
        for (k, s) in o.scans {
            if !self.scans.contains_key(&k) && self.scans.len() >= 256 {
                continue;
            }
            let e = self.scans.entry(k).or_insert_with(|| ScanHit {
                unit: s.unit.clone(),
                path: s.path.clone(),
                ..Default::default()
            });
            e.count += s.count;
            e.last = e.last.max(s.last);
        }
        self.dev_servers.extend(o.dev_servers);
        if self.window_first == 0 || (o.window_first > 0 && o.window_first < self.window_first) {
            self.window_first = o.window_first;
        }
        self.window_last = self.window_last.max(o.window_last);
        self.lines += o.lines;
    }

    /// Drop everything older than `cutoff` (sliding window).
    pub fn expire(&mut self, cutoff: u64) {
        self.edge_failures.retain(|t| *t >= cutoff);
        self.dead_origins.retain(|_, d| d.last >= cutoff);
        self.scans.retain(|_, s| s.last >= cutoff);
        if self.window_first < cutoff {
            self.window_first = cutoff;
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "window": {"first": if self.window_first > 0 { Value::from(crate::util::ts(self.window_first)) } else { Value::Null },
                       "last": if self.window_last > 0 { Value::from(crate::util::ts(self.window_last)) } else { Value::Null }},
            "dead_origins": self.dead_origins.values().map(|d| json!({
                "port": d.port, "service": d.service, "hosts": d.hosts, "failed_requests": d.failed_requests,
                "refusals": d.refusals, "last": crate::util::ts(d.last)})).collect::<Vec<_>>(),
            "edge_failures": self.edge_failures.len(),
            "scans": self.scans.values().map(|s| json!({"unit": s.unit, "path": s.path, "count": s.count})).collect::<Vec<_>>(),
            "dev_servers": self.dev_servers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jl(t: u64, unit: &str, msg: &str) -> JLine {
        JLine {
            t,
            unit: unit.into(),
            prio: 6,
            msg: msg.into(),
        }
    }

    #[test]
    fn cloudflared_dead_origins_and_hosts() {
        let lines = vec![
            jl(100, "cloudflared.service", "2026-10-20T17:04:34Z ERR  error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:4001: connect: connection refused\" connIndex=1 event=1 ingressRule=18 originService=http://localhost:4001"),
            jl(100, "cloudflared.service", "2026-10-20T17:04:34Z ERR Request failed error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:4001: connect: connection refused\" connIndex=1 dest=https://galion.studio/ event=0 ip=198.51.100.13 type=http"), // repo-check: allow (UTC input fixture in cloudflared log format)
            jl(101, "cloudflared.service", "2026-10-20T17:04:35Z ERR Request failed error=\"x: dial tcp 127.0.0.1:3011: connect: connection refused\" dest=https://merlinelement.com/a?b=c ip=203.0.113.4 type=http"), // repo-check: allow (UTC input fixture in cloudflared log format)
            jl(102, "cloudflared.service", "2026-10-20T17:04:36Z INF Registered tunnel connection"),
        ];
        let li = insights(&lines);
        assert_eq!(li.dead_origins.len(), 2);
        let d = &li.dead_origins[&4001];
        assert_eq!((d.refusals, d.failed_requests), (2, 1));
        assert_eq!(d.service, "http://localhost:4001");
        assert!(d.hosts.contains("galion.studio"));
        assert!(li.dead_origins[&3011].hosts.contains("merlinelement.com"));
        assert_eq!(li.edge_failures.len(), 2);
        assert_eq!((li.window_first, li.window_last), (100, 102));
    }

    #[test]
    fn vite_scanner_paths() {
        let lines = vec![
            jl(5, "console.service", "The request id \"/opt/galion/console-ide/.git/config\" is outside of Vite serving allow list."),
            jl(6, "console.service", "Instead of /public/.env, use /.env."),
            jl(7, "console.service", "Instead of /public/phpinfo.php, use /phpinfo.php."),
            jl(8, "tuner.service", "GET /index.html 200"),
        ];
        let li = insights(&lines);
        assert!(li.dev_servers.contains("console.service"));
        let paths: Vec<&str> = li.scans.values().map(|s| s.path.as_str()).collect();
        assert!(paths.contains(&"/.env"));
        assert!(paths.contains(&"/public/.env"));
        assert!(paths.contains(&"/phpinfo.php"));
        assert!(paths.contains(&"/opt/galion/console-ide/.git/config"));
        assert!(!paths.iter().any(|p| p.contains("index")));
    }

    #[test]
    fn merge_and_expire() {
        let a = insights(&[jl(10, "c", "dial tcp [::1]:9: connect: connection refused")]);
        let mut b = insights(&[jl(
            20,
            "c",
            "Request failed dial tcp [::1]:9: connect: connection refused dest=https://x.y/",
        )]);
        b.merge(a);
        assert_eq!(b.dead_origins[&9].refusals, 2);
        b.expire(15);
        assert_eq!(b.edge_failures.len(), 1);
        assert_eq!(b.dead_origins.len(), 1);
        b.expire(30);
        assert!(b.dead_origins.is_empty());
        assert_eq!(
            refused_port("dial tcp [::1]:4001: connect: connection reset"),
            None
        );
    }
}
