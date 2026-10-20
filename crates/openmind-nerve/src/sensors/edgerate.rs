// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Per-hostname edge request rate (cortex plan T2.12).
//!
//! cloudflared's own counters (`total_requests`) feed one `edge-<tunnel>`
//! stream with no hostname. This module splits the edge by public hostname
//! from the cloudflared journal lines the cycle already collected
//! ([`Input::journal`] / `Collected::lines`), so a flood against one
//! hostname is seen even while the tunnel total looks ordinary.
//!
//! Two line shapes are counted (cloudflared `proxy/logger.go`,
//! `connection` request errors):
//! - `ERR Request failed error="..." ... dest=https://<host>/<path> ...`:
//!   one failed request (logged at every log level, so always available);
//! - `DBG GET https://<host>/<path> HTTP/1.1 ... host=<host> ...`: one
//!   request (only with `--loglevel debug`). While debug request lines are
//!   seen, a `Request failed` line adds an error to its host but no second
//!   request.
//!
//! At the VPS's info log level the journal holds failed requests only, so
//! the streams measure the per-hostname *failed* request rate (error share
//! 100 %). That is still the flood signal that matters for dead origins;
//! full per-host rates need cloudflared `--loglevel debug`, which is a
//! cloudflared reconfiguration (owner decision, see docs/cortex/edge-rate.md).
//!
//! Wiring: [`EdgeHosts::feed`] (called from `engine.rs` while the detector is
//! locked) records the lines into [`crate::access::Detector`] streams named
//! `edge-host:<host>` (counter-only, no client IPs kept), so the access.rs
//! median/MAD detector judges each host. The [`EdgeRate`] sensor turns
//! elevated/flooding host streams into one `edge-host-rate` ddos finding and
//! one signal under the cause `ddos:edge-cloudflared`, the same cause as the
//! tunnel counter stream, so a flood seen both ways is one incident. The
//! built-in `ddos` sensor still lists the host streams but leaves their
//! signals to this sensor (no double counting).
//!
//! Settings: `NERVE_EDGE_HOSTS=0` switches feeding and the sensor off
//! (as does `NERVE_SENSORS_OFF=edge-rate`); `NERVE_EDGE_HOSTS_MAX` (default
//! 32, 1..=256) caps distinct hostname streams, later hostnames share
//! `edge-host:other` (hostnames are request-controlled under wildcard DNS).
//! Hostnames stay in evidence and the loopback stream list; titles and B33
//! events carry counts only.

use super::{Env, Sensor, Sink};
use crate::access::{DdosVerdict, Detector};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use crate::probe::JLine;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

/// Sensor name (registry key, `NERVE_SENSORS_OFF` key).
pub const NAME: &str = "edge-rate";
/// Stream name prefix of per-hostname streams.
pub const PREFIX: &str = "edge-host:";
/// Shared stream for hostnames past the cap.
pub const OTHER: &str = "edge-host:other";
/// Component the host streams belong to.
pub const COMPONENT: &str = "cloudflared";
/// Incident cause (same as the tunnel counter stream `edge-cloudflared`).
pub const CAUSE: &str = "ddos:edge-cloudflared";
const DEFAULT_MAX_HOSTS: usize = 32;
/// After the last debug request line, `Request failed` lines count as
/// errors only for this long.
const DEBUG_HOLD_S: u64 = 600;
const METHODS: [&str; 9] = [
    "GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS", "PATCH", "CONNECT", "TRACE",
];

/// What one cloudflared line says about one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// A proxied request (debug request line).
    Request,
    /// A request that failed at the origin (`Request failed ... dest=`).
    Failed,
}

/// Lower-case hostname of a URL (`https://Host:443/p?q`), or None when it
/// is not a plain DNS name (IP literals, junk, no dot, too long).
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
    let auth = auth.rsplit('@').next().unwrap_or(auth);
    if auth.starts_with('[') {
        return None;
    }
    let host = auth.split(':').next().unwrap_or("");
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && !host.starts_with('.')
        && !host.contains("..")
        && host
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && host.bytes().any(|b| b.is_ascii_lowercase());
    ok.then_some(host)
}

/// Parse one cloudflared journal message into (hostname, kind).
pub fn parse(msg: &str) -> Option<(String, EdgeKind)> {
    if msg.contains("Request failed") {
        // The quoted error text comes first; take the last `dest=` field.
        let i = msg.rfind(" dest=")?;
        let v = msg[i + 6..].split_whitespace().next()?;
        return host_of(v.trim_matches('"')).map(|h| (h, EdgeKind::Failed));
    }
    let mut it = msg.split_whitespace();
    let mut tok = it.next()?;
    if tok != "DBG" {
        // Leading cloudflared UTC timestamp token.
        tok = it.next()?;
    }
    if tok != "DBG" {
        return None;
    }
    let method = it.next()?;
    if !METHODS.contains(&method) {
        return None;
    }
    let url = it.next()?;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return None;
    }
    if !it.next().is_some_and(|p| p.starts_with("HTTP/")) {
        return None;
    }
    host_of(url).map(|h| (h, EdgeKind::Request))
}

fn line_hash(l: &JLine) -> u64 {
    let mut h = DefaultHasher::new();
    (l.t, &l.unit, &l.msg).hash(&mut h);
    h.finish()
}

/// Feeder state kept by the engine across cycles.
#[derive(Debug, Clone)]
pub struct EdgeHosts {
    /// Cap on distinct `edge-host:<host>` streams.
    pub max_hosts: usize,
    hosts: BTreeSet<String>,
    debug_last: u64,
    /// Newest line second fed, and the hashes of lines fed at that second
    /// (journalctl `--since` is inclusive: boundary lines arrive twice).
    edge_t: u64,
    edge_seen: Vec<u64>,
    /// Lines counted since start / hostnames folded into `other`.
    pub counted: u64,
    pub folded: u64,
}

impl Default for EdgeHosts {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_HOSTS)
    }
}

impl EdgeHosts {
    pub fn new(max_hosts: usize) -> Self {
        EdgeHosts {
            max_hosts: max_hosts.clamp(1, 256),
            hosts: BTreeSet::new(),
            debug_last: 0,
            edge_t: 0,
            edge_seen: vec![],
            counted: 0,
            folded: 0,
        }
    }

    /// `NERVE_EDGE_HOSTS=0|off|false` -> None; `NERVE_EDGE_HOSTS_MAX` caps.
    pub fn from_env(env: Env) -> Option<Self> {
        if matches!(
            env("NERVE_EDGE_HOSTS").as_deref().map(str::trim),
            Some("0" | "off" | "false" | "no")
        ) {
            return None;
        }
        let max = env("NERVE_EDGE_HOSTS_MAX")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_HOSTS);
        Some(Self::new(max))
    }

    /// Distinct hostnames with their own stream.
    pub fn hosts(&self) -> usize {
        self.hosts.len()
    }

    fn stream_name(&mut self, host: String) -> String {
        if self.hosts.contains(&host) {
            return format!("{PREFIX}{host}");
        }
        if self.hosts.len() < self.max_hosts {
            let n = format!("{PREFIX}{host}");
            self.hosts.insert(host);
            return n;
        }
        self.folded += 1;
        OTHER.to_string()
    }

    /// Record this cycle's journal lines into per-host detector streams.
    /// Lines without a timestamp count at `now`. Returns lines counted.
    pub fn feed(&mut self, lines: &[JLine], now: u64, det: &mut Detector) -> usize {
        let mut parsed: Vec<(u64, String, EdgeKind, u64)> = lines
            .iter()
            .filter_map(|l| {
                let (h, k) = parse(&l.msg)?;
                let t = if l.t == 0 { now } else { l.t };
                Some((t, h, k, line_hash(l)))
            })
            .collect();
        if parsed.is_empty() {
            return 0;
        }
        parsed.sort_by_key(|p| p.0);
        for p in parsed.iter().filter(|p| p.2 == EdgeKind::Request) {
            self.debug_last = self.debug_last.max(p.0);
        }
        let (old_t, old_seen) = (self.edge_t, std::mem::take(&mut self.edge_seen));
        let mut n = 0;
        for (t, host, kind, hash) in parsed {
            if t < old_t || (t == old_t && old_seen.contains(&hash)) {
                continue;
            }
            if t > self.edge_t {
                self.edge_t = t;
                self.edge_seen.clear();
            }
            if t == self.edge_t {
                self.edge_seen.push(hash);
            }
            let debug = self.debug_last > 0
                && t <= self.debug_last + DEBUG_HOLD_S
                && t + DEBUG_HOLD_S >= self.debug_last;
            let name = self.stream_name(host);
            let s = det.stream(&name, Some(COMPONENT));
            match (kind, debug) {
                (EdgeKind::Request, _) => s.record_count(t, 1, 0),
                (EdgeKind::Failed, true) => s.record_count(t, 0, 1),
                (EdgeKind::Failed, false) => s.record_count(t, 1, 1),
            }
            n += 1;
        }
        if self.edge_t == old_t {
            // No newer second this cycle: keep the old boundary hashes too.
            let mut seen = old_seen;
            seen.append(&mut self.edge_seen);
            self.edge_seen = seen;
        }
        self.counted += n as u64;
        n
    }
}

/// Sensor `edge-rate`: hostname streams that run hot become one finding.
#[derive(Debug, Clone, Default)]
pub struct EdgeRate;

impl EdgeRate {
    /// Off with `NERVE_EDGE_HOSTS=0` (the engine then feeds nothing either).
    pub fn from_env(env: Env) -> Option<Self> {
        EdgeHosts::from_env(env).map(|_| EdgeRate)
    }
}

impl Sensor for EdgeRate {
    fn name(&self) -> &'static str {
        NAME
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        let mut hot: Vec<&DdosVerdict> = i
            .ddos
            .iter()
            .filter(|v| v.stream.starts_with(PREFIX) && v.level != "normal")
            .collect();
        if hot.is_empty() {
            return;
        }
        hot.sort_by(|a, b| b.rps.total_cmp(&a.rps).then(a.stream.cmp(&b.stream)));
        let floods = hot.iter().filter(|v| v.level == "flood").count();
        let sev = if floods > 0 { Sev::Critical } else { Sev::Warn };
        let worst = hot[0];
        let title = format!(
            "{} public hostname(s) {} at the edge: worst {:.1} req/s over 60 s vs baseline {:.2} req/s, {:.0}% failed at the origin",
            hot.len(),
            if floods > 0 {
                "under a request flood"
            } else {
                "with an elevated request rate"
            },
            worst.rps,
            worst.baseline_rps,
            worst.error_share * 100.0
        );
        let mut ev: Vec<String> = hot
            .iter()
            .take(8)
            .map(|v| format!("[{}] {}", v.level, v.explain()))
            .collect();
        if hot.len() > 8 {
            ev.push(format!("... and {} more hostname stream(s)", hot.len() - 8));
        }
        if hot.iter().all(|v| v.error_share >= 0.999) {
            ev.push("counted from cloudflared `Request failed ... dest=` journal lines only: at log level info cloudflared does not log successful requests, so this is the failed-request rate per hostname".into());
        }
        out.finding(
            Finding::new(FindingKind::Ddos, "edge-host-rate", sev, COMPONENT, title.clone())
                .with_evidence(ev)
                .with_fix("look up the hostname(s) in the evidence; if the traffic is hostile, rate-limit or challenge that hostname at Cloudflare (WAF / rate-limiting rule: owner approval); if its origin is dead, start it or remove the route (cloudflared config + DNS: owner approval). The nerve never changes the edge.")
                .with_cause(CAUSE),
        );
        out.signal(Signal::new(CAUSE, COMPONENT, sev, title));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;

    fn jl(t: u64, msg: &str) -> JLine {
        JLine {
            t,
            unit: "cloudflared.service".into(),
            prio: 3,
            msg: msg.into(),
        }
    }

    /// cloudflared writes its own UTC timestamp into MESSAGE (input, never output).
    const CF_TS: &str = concat!("2026-10-20T22:35:21", "Z");

    // Shape of the live VPS lines (2026-10-21 01:10 CEST probe), addresses replaced.
    fn failed(t: u64, host: &str) -> JLine {
        jl(t, &format!("{CF_TS} ERR Request failed error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:4001: connect: connection refused\" connIndex=1 dest=https://{host}/wp-login.php event=0 ip=198.51.100.13 type=http"))
    }

    fn dbg(t: u64, host: &str) -> JLine {
        jl(t, &format!("{CF_TS} DBG GET https://{host}/ HTTP/1.1 connIndex=0 content-length=0 event=1 headers={{\"Accept\":[\"*/*\"]}} host={host} ingressRule=3 originService=http://localhost:8091 path=/"))
    }

    #[test]
    fn parses_hosts_and_kinds() {
        assert_eq!(
            parse(&failed(1, "Galion.Studio").msg),
            Some(("galion.studio".into(), EdgeKind::Failed))
        );
        assert_eq!(
            parse(&dbg(1, "one.galion.app").msg),
            Some(("one.galion.app".into(), EdgeKind::Request))
        );
        // The paired `ERR  error=...` line (no dest) and the response line do not count.
        assert_eq!(parse(&format!("{CF_TS} ERR  error=\"dial tcp [::1]:4001: connect: connection refused\" connIndex=1 event=1 ingressRule=34 originService=http://localhost:4001")), None);
        assert_eq!(
            parse(&format!(
                "{CF_TS} DBG 200 OK connIndex=0 content-length=5 event=1"
            )),
            None
        );
        assert_eq!(
            parse(&format!(
                "{CF_TS} INF Registered tunnel connection connIndex=0"
            )),
            None
        );
        assert_eq!(
            host_of("https://user@Ex.Test.:443/a?b"),
            Some("ex.test".into())
        );
        for bad in [
            "https://[::1]/",
            "https://203.0.113.4/",
            "https://localhost/",
            "https://a..b/",
            "https://ex%2e.test/",
            "",
        ] {
            assert_eq!(host_of(bad), None, "{bad}");
        }
        assert_eq!(host_of(&format!("https://{}.test/", "a".repeat(260))), None);
    }

    #[test]
    fn feeds_counter_streams_and_dedups_boundary_lines() {
        let mut d = Detector::default();
        let mut e = EdgeHosts::new(32);
        let a = [
            failed(100, "a.test"),
            failed(100, "b.test"),
            failed(101, "a.test"),
        ];
        assert_eq!(e.feed(&a, 200, &mut d), 3);
        // Next window starts at the inclusive boundary second 101: the
        // repeated line is skipped, a new line in the same second counts.
        let mut again = failed(101, "a.test");
        assert_eq!(e.feed(std::slice::from_ref(&again), 210, &mut d), 0);
        again.msg.push_str(" x=1");
        assert_eq!(e.feed(&[again, failed(105, "b.test")], 210, &mut d), 2);
        assert_eq!(d.streams["edge-host:a.test"].total, 3);
        assert_eq!(d.streams["edge-host:b.test"].total, 2);
        assert_eq!(
            d.streams["edge-host:a.test"].component.as_deref(),
            Some("cloudflared")
        );
        assert!(!d.streams["edge-host:a.test"].has_ips, "no client IPs kept");
        assert_eq!(e.counted, 5);
    }

    #[test]
    fn debug_mode_counts_requests_once() {
        let mut d = Detector::default();
        let mut e = EdgeHosts::new(32);
        e.feed(
            &[
                dbg(100, "a.test"),
                dbg(101, "a.test"),
                failed(101, "a.test"),
            ],
            200,
            &mut d,
        );
        let v = d.evaluate(110);
        let a = v.iter().find(|v| v.stream == "edge-host:a.test").unwrap();
        assert_eq!(a.requests_60s, 2);
        assert!((a.error_share - 0.5).abs() < 1e-9);
    }

    #[test]
    fn caps_distinct_hostnames() {
        let mut d = Detector::default();
        let mut e = EdgeHosts::new(2);
        let lines: Vec<JLine> = (0..5)
            .map(|k| failed(100 + k, &format!("h{k}.test")))
            .collect();
        e.feed(&lines, 200, &mut d);
        assert_eq!(e.hosts(), 2);
        assert_eq!(e.folded, 3);
        assert_eq!(d.streams[OTHER].total, 3);
        assert_eq!(d.streams.len(), 3);
    }

    #[test]
    fn env_switches() {
        assert!(EdgeHosts::from_env(&|_| None).is_some());
        assert!(EdgeRate::from_env(&|k| (k == "NERVE_EDGE_HOSTS").then(|| "0".into())).is_none());
        let e =
            EdgeHosts::from_env(&|k| (k == "NERVE_EDGE_HOSTS_MAX").then(|| "5000".into())).unwrap();
        assert_eq!(e.max_hosts, 256);
    }

    /// 25 min quiet failed traffic on two hosts, then 60 s at 40 req/s on one.
    fn flood_streams() -> Vec<DdosVerdict> {
        let mut d = Detector::default();
        let mut e = EdgeHosts::default();
        let t0 = 1_792_500_000u64;
        let mut lines = vec![];
        for k in 0..300u64 {
            lines.push(failed(t0 + k * 5, "quiet.test"));
            lines.push(failed(t0 + k * 5 + 2, "hot.test"));
        }
        let now = t0 + 1500;
        e.feed(&lines, now, &mut d);
        assert!(d.evaluate(now).iter().all(|v| v.level == "normal"));
        let burst: Vec<JLine> = (0..2400u64)
            .map(|k| failed(now + k / 40, "hot.test"))
            .collect();
        e.feed(&burst, now + 60, &mut d);
        d.evaluate(now + 60)
    }

    #[test]
    fn flood_on_one_hostname_is_one_finding_and_one_signal() {
        let v = flood_streams();
        let hot = v.iter().find(|v| v.stream == "edge-host:hot.test").unwrap();
        assert_eq!(hot.level, "flood", "{}", hot.explain());
        assert_eq!(
            v.iter()
                .find(|v| v.stream == "edge-host:quiet.test")
                .unwrap()
                .level,
            "normal"
        );
        let comps = vec![];
        let logs = LogInsights::default();
        let mut i = Input::new(1_792_501_560, &comps, &logs);
        i.ddos = &v;
        let mut s = Sensors::builtin();
        s.add(EdgeRate);
        let cx = s.run(&i);
        assert!(cx.ddos_firing);
        assert_eq!(cx.ddos.len(), 2, "both host streams listed");
        assert_eq!(cx.ddos_findings.len(), 1);
        let f = &cx.ddos_findings[0];
        assert_eq!(
            (f.id.as_str(), f.sev, f.source.as_str()),
            ("edge-host-rate", Sev::Critical, "edge-rate")
        );
        assert!(
            f.title
                .starts_with("1 public hostname(s) under a request flood"),
            "{}",
            f.title
        );
        assert!(!f.title.contains("hot.test"), "no hostnames in titles");
        assert!(f.evidence[0].contains("edge-host:hot.test"));
        // One signal, from edge-rate, under the tunnel's cause; ddos stays quiet for host streams.
        let sig: Vec<(&str, &str)> = cx
            .signals
            .iter()
            .map(|s| (s.cause.as_str(), s.component.as_str()))
            .collect();
        assert_eq!(sig, [(CAUSE, COMPONENT)]);
        let ev = serde_json::to_string(&cx.nerve_events(1_792_501_560)).unwrap();
        assert!(!ev.contains("hot.test"), "{ev}");
    }

    #[test]
    fn quiet_hosts_report_nothing() {
        let comps = vec![];
        let logs = LogInsights::default();
        let i = Input::new(1, &comps, &logs);
        let mut s = Sensors::empty();
        s.add(EdgeRate);
        let cx = s.run(&i);
        assert!(cx.ddos_findings.is_empty() && cx.signals.is_empty());
    }
}
