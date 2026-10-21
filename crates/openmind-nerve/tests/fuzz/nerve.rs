// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.1 targets (wave-4 slot 212) for the live-build parsers on
//! aa77864: `POST /signal` body, `POST /event` body, the HTTP request reader
//! `server::read_req` (segmentation invariance), journald JSON, cloudflared
//! Prometheus metrics, cloudflared / zerolog log lines and access-log lines,
//! plus generative round trips and a deep-JSON check on the server's
//! 256 KiB connection-thread stack.

use super::{ensure, fnv, Gen, Rng, Target};
use openmind_nerve::access;
use openmind_nerve::hub::{parse_signal, Hub, MAX_BODY};
use openmind_nerve::logscan;
use openmind_nerve::probe::{self, JLine};
use openmind_nerve::server;
use serde_json::{json, Value};
use std::io::Read;

/// 2026-10-21 01:00:00 +02:00
pub const NOW: u64 = 1_792_537_200;

const FIX_JOURNAL: &str = include_str!("../fixtures/cortex-replay/journal.jsonl");
const FIX_ACCESS: &str = include_str!("../fixtures/cortex-replay/access-tuner.log");
const UNITS: &[&str] = &[
    "cloudflared.service",
    "ssh.service",
    "galion-nerve.service",
];
const KINDS: &[&str] = &["heartbeat", "metric", "event", "error"];

/// Extra seed files (real VPS captures) from `CORTEX_FUZZ_SEEDS=<dir>`:
/// `journal.jsonl`, `cloudflared-metrics.txt`, `cloudflared-log.txt`,
/// `access.log`, `signals.jsonl`, `events.jsonl`. Large files are chunked.
fn extra(file: &str, n: usize) -> Vec<String> {
    let Ok(dir) = std::env::var("CORTEX_FUZZ_SEEDS") else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join(file)) else {
        return vec![];
    };
    chunks(&text, n)
}

/// `text` as a whole (when short) plus chunks of `n` lines (at most 64).
pub fn chunks(text: &str, n: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= n {
        return vec![text.to_string()];
    }
    lines.chunks(n).take(64).map(|c| c.join("\n")).collect()
}

fn seeds(base: &[&str], file: &str, n: usize) -> Vec<String> {
    let mut v: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    v.extend(extra(file, n));
    v
}

fn no_ctl(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

// ---------------------------------------------------------------- /signal

fn check_signal(text: &str) -> Result<u64, String> {
    match parse_signal(text, NOW) {
        Ok(s) => {
            ensure(text.len() <= MAX_BODY, || "accepted a body over MAX_BODY".into())?;
            ensure(s.t == NOW, || "ts not assigned by the nerve".into())?;
            ensure(
                !s.source.is_empty()
                    && s.source.len() <= 64
                    && s.source
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
                || format!("source {:?} outside [A-Za-z0-9-_.]{{1,64}}", s.source),
            )?;
            ensure(KINDS.contains(&s.kind.as_str()), || {
                format!("kind {:?}", s.kind)
            })?;
            ensure(s.name.chars().count() <= 64 && no_ctl(&s.name), || {
                "name over 64 chars or with control chars".into()
            })?;
            ensure(s.msg.chars().count() <= 300 && no_ctl(&s.msg), || {
                "msg over 300 chars or with control chars".into()
            })?;
            ensure(s.value.is_none_or(f64::is_finite), || {
                "non-finite value".into()
            })?;
            let back = s.to_json().to_string();
            ensure(serde_json::from_str::<Value>(&back).is_ok(), || {
                "to_json not JSON".into()
            })?;
            let mut h = Hub::default();
            ensure(h.ingest(s.clone()).is_ok(), || "fresh hub refused".into())?;
            ensure(h.sources[&s.source].total == 1, || "ingest not counted".into())?;
            Ok(fnv(&format!(
                "ok:{}:{}:{}:{}",
                s.kind,
                s.value.is_some(),
                s.name.is_empty(),
                s.msg.is_empty()
            )))
        }
        Err(e) => Ok(fnv(&format!("err:{}", e.split(':').next().unwrap_or("")))),
    }
}

pub fn target_signal() -> Target {
    Target {
        name: "signal-body",
        seeds: seeds(
            &[
                r#"{"source":"hermes-rs","kind":"heartbeat"}"#,
                r#"{"source":"galion-one","kind":"metric","name":"rss_kb","value":51234.5}"#,
                r#"{"source":"gaia.live","kind":"error","name":"probe","msg":"connect refused :8002"}"#,
                r#"{"source":"x","kind":"event","name":"","value":null,"msg":"\u0007bell"}"#,
            ],
            "signals.jsonl",
            1,
        ),
        dict: vec![
            "\"source\":",
            "\"kind\":",
            "\"name\":",
            "\"value\":",
            "\"msg\":",
            "\"heartbeat\"",
            "\"metric\"",
            "\"error\"",
            "\"event\"",
            "1e308",
            "1e309",
            "-0",
            "NaN",
            "\\u0000",
            "\\ud800",
            "\\n",
            "null",
            "[]",
            "{}",
            "true",
        ],
        check: check_signal,
    }
}

// ---------------------------------------------------------------- /event

fn check_event(text: &str) -> Result<u64, String> {
    match server::event_from(text) {
        Ok(e) => {
            for (k, f) in [
                ("source", e.source()),
                ("kind", e.kind()),
                ("severity", e.severity()),
            ] {
                ensure(!f.is_empty() && f.len() <= 128, || {
                    format!("{k} empty or over 128 bytes")
                })?;
            }
            let d = e.data_json();
            ensure(serde_json::from_str::<Value>(&d).is_ok(), || {
                "data_json not JSON".into()
            })?;
            let v: Value = serde_json::from_str(text).map_err(|x| x.to_string())?;
            let sent = v.get("data").cloned().unwrap_or(Value::Null);
            let got: Value = serde_json::from_str(&d).map_err(|x| x.to_string())?;
            ensure(json_close(&sent, &got), || {
                format!("data not carried: sent {sent} got {got}")
            })?;
            Ok(fnv(&format!(
                "ok:{}:{}",
                e.severity().len().min(8),
                d.len().min(4)
            )))
        }
        Err(e) => Ok(fnv(&format!("err:{}", e.split(':').next().unwrap_or("")))),
    }
}

pub fn target_event() -> Target {
    Target {
        name: "event-body",
        seeds: seeds(
            &[
                r#"{"source":"gaia","kind":"service.probe","severity":"info","data":{"port":8002,"ok":false}}"#,
                r#"{"source":"gaia-live","kind":"restart","severity":"error"}"#,
                r#"{"source":"s","kind":"k","severity":"warn","data":[1,"two",null,{"x":[]}]}"#,
            ],
            "events.jsonl",
            1,
        ),
        dict: vec![
            "\"source\":",
            "\"kind\":",
            "\"severity\":",
            "\"data\":",
            "\"error\"",
            "\"crit\"",
            "[[[[[[[[",
            "]]]]]]]]",
            "{\"a\":",
            "1e999",
            "\\u0000",
            "\\ud800",
            "null",
            "\"\"",
        ],
        check: check_event,
    }
}

// ---------------------------------------------------------------- HTTP read_req

/// A reader that hands out at most `step` bytes per `read`, like a TCP
/// stream that delivers the request in segments of that size.
struct Seg<'a> {
    data: &'a [u8],
    pos: usize,
    step: usize,
}

impl Read for Seg<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.step.min(buf.len()).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

type Outcome = Result<(String, String, String, String), u16>;

fn read_with(b: &[u8], step: usize) -> Outcome {
    let mut r = Seg {
        data: b,
        pos: 0,
        step,
    };
    server::read_req(&mut r)
        .map(|q| (q.method, q.path, q.query, q.body))
        .map_err(|(c, _)| c)
}

/// Same request bytes, different TCP segmentation, same outcome; and the
/// usual shape checks on a parsed request.
pub fn check_http_bytes(b: &[u8]) -> Result<u64, String> {
    let whole = read_with(b, 1 << 20);
    let mut steps = vec![2048usize, 13];
    if b.len() <= 3000 {
        steps.push(1);
    }
    for st in steps {
        let o = read_with(b, st);
        ensure(o == whole, || {
            format!(
                "segmentation {st} B changes the outcome: {:?} vs one read {:?}",
                o.as_ref().map(|x| (&x.0, &x.1, x.3.len())),
                whole.as_ref().map(|x| (&x.0, &x.1, x.3.len()))
            )
        })?;
    }
    match whole {
        Ok((m, p, _q, body)) => {
            ensure(!p.contains('?'), || "path keeps the query".into())?;
            ensure(body.len() <= 3 * MAX_BODY, || "body over the cap".into())?;
            let head = b.windows(4).position(|w| w == b"\r\n\r\n");
            ensure(head.is_some_and(|h| h + 4 <= 8192), || {
                format!("accepted a head of {:?} bytes (> 8192)", head.map(|h| h + 4))
            })?;
            Ok(fnv(&format!("ok:{}:{}", m.len().min(8), body.is_empty())))
        }
        Err(code) => {
            ensure(matches!(code, 400 | 413 | 431), || {
                format!("odd error code {code}")
            })?;
            Ok(code as u64)
        }
    }
}

fn check_http(text: &str) -> Result<u64, String> {
    check_http_bytes(text.as_bytes())
}

pub fn big_head(total: usize) -> String {
    let start = "POST /signal HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nX-Pad: ";
    let end = "\r\n\r\n";
    let pad = total.saturating_sub(start.len() + end.len());
    format!("{start}{}{end}{{}}", "a".repeat(pad))
}

pub fn target_http() -> Target {
    Target {
        name: "http-read-req",
        seeds: vec![
            format!(
                "POST /event HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 58\r\n\r\n{}",
                r#"{"source":"gaia","kind":"service.probe","severity":"info"}"#
            ),
            "GET /api/v1/state?x=1 HTTP/1.0\r\n\r\n".into(),
            "POST /signal HTTP/1.1\r\ncontent-length: 2\r\n\r\n{}".into(),
            "GET /events?n=5 HTTP/1.1\r\nHost: 127.0.0.1:8670\r\nConnection: close\r\n\r\n".into(),
            big_head(8150),
            big_head(8192),
        ],
        dict: vec![
            "\r\n",
            "\r\n\r\n",
            "Content-Length: ",
            "content-length:",
            "Content-Length: 8192",
            "Content-Length: 8193",
            "99999999",
            "-1",
            "+5",
            "POST",
            "GET",
            "/signal",
            "/event",
            "?",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
        check: check_http,
    }
}

// ---------------------------------------------------------------- journald JSON

fn check_journal(text: &str) -> Result<u64, String> {
    let all = probe::parse_journal_lines(text, &[]);
    let some = probe::parse_journal_lines(text, UNITS);
    let n = text.lines().count();
    ensure(all.len() <= n && some.len() <= n, || {
        "more records than lines".into()
    })?;
    for l in all.iter().chain(&some) {
        ensure(l.msg.chars().count() <= 2000, || "msg over 2000 chars".into())?;
    }
    for l in &some {
        ensure(UNITS.contains(&l.unit.as_str()), || {
            format!("unit {} not in filter", l.unit)
        })?;
    }
    let counts = probe::journal_counts(&some, UNITS);
    let total: u64 = counts.values().map(|c| c.lines as u64).sum();
    ensure(total == some.len() as u64, || {
        "journal_counts lines != records".into()
    })?;
    for c in counts.values() {
        ensure(c.errors + c.warnings <= c.lines, || {
            "errors+warnings > lines".into()
        })?;
        ensure(c.restart_notices <= c.lines, || "restarts > lines".into())?;
    }
    let ins = logscan::insights(&all);
    ensure(ins.lines == all.len(), || "insights.lines".into())?;
    let errs: u64 = counts.values().map(|c| c.errors as u64).sum();
    Ok(fnv(&format!(
        "{}:{}:{}:{}:{}",
        all.len().min(5),
        some.len().min(5),
        errs.min(3),
        ins.dead_origins.len().min(3),
        ins.scans.len().min(3)
    )))
}

pub fn target_journal() -> Target {
    let fx = chunks(FIX_JOURNAL, 20);
    Target {
        name: "journald-json",
        seeds: seeds(
            &[
                &fx[0],
                r#"{"UNIT":"ssh.service","_SYSTEMD_UNIT":"init.scope","PRIORITY":"3","MESSAGE":"ssh.service: Failed with result 'exit-code'."}"#,
                r#"{"_SYSTEMD_UNIT":"cloudflared.service","PRIORITY":"6","MESSAGE":[1,2,3]}"#,
                r#"{"_SYSTEMD_UNIT":"galion-nerve.service","PRIORITY":"4","__REALTIME_TIMESTAMP":"1792537200123456","MESSAGE":"<4>openmind-nerve: GET /events skipped 1 unreadable line(s)"}"#,
            ],
            "journal.jsonl",
            20,
        ),
        dict: vec![
            "\"MESSAGE\":",
            "\"UNIT\":",
            "\"_SYSTEMD_UNIT\":",
            "\"PRIORITY\":\"",
            "\"__REALTIME_TIMESTAMP\":\"",
            "18446744073709551615",
            "cloudflared.service",
            "ssh.service",
            "\\u0000",
            "\\ud800",
            "\\n",
            "null",
            "[]",
            "{}",
            "Request failed",
            "connection refused",
            "dial tcp ",
            "dest=https://",
            " ERR ",
            "panicked",
            "Scheduled restart job",
        ],
        check: check_journal,
    }
}

// ---------------------------------------------------------------- cloudflared metrics

/// The spec: the last line whose first token is the metric name wins; its
/// value counts only when it is a finite, non-negative number that fits a
/// u64 (cloudflared counters and gauges are never negative or infinite).
fn spec_metric(text: &str, key: &str) -> Option<u64> {
    let mut out = None;
    let mut seen = false;
    for l in text.lines() {
        if l.starts_with('#') {
            continue;
        }
        let mut it = l.split_whitespace();
        let (Some(k), Some(v)) = (it.next(), it.next()) else {
            continue;
        };
        if k == key {
            seen = true;
            out = v
                .parse::<f64>()
                .ok()
                .filter(|f| f.is_finite() && *f >= 0.0 && *f < 18_446_744_073_709_551_616.0)
                .map(|f| f as u64);
        }
    }
    let _ = seen;
    out
}

fn check_metrics(text: &str) -> Result<u64, String> {
    let m = probe::parse_cloudflared_metrics(text);
    for (k, got) in [
        ("cloudflared_tunnel_ha_connections", m.ha_connections),
        ("cloudflared_tunnel_total_requests", m.total_requests),
        ("cloudflared_tunnel_request_errors", m.request_errors),
    ] {
        let want = spec_metric(text, k);
        ensure(got == want, || format!("{k}: parsed {got:?}, spec {want:?}"))?;
    }
    Ok(fnv(&format!(
        "{}:{}:{}",
        m.ha_connections.map(|x| x.min(5)).unwrap_or(99),
        m.total_requests.is_some(),
        m.request_errors.is_some()
    )))
}

const METRICS_SEED: &str = "# HELP cloudflared_tunnel_ha_connections Number of active ha connections\n\
# TYPE cloudflared_tunnel_ha_connections gauge\n\
cloudflared_tunnel_ha_connections 4\n\
# HELP cloudflared_tunnel_total_requests Amount of requests proxied through all the tunnels\n\
# TYPE cloudflared_tunnel_total_requests counter\n\
cloudflared_tunnel_total_requests 81920\n\
cloudflared_tunnel_request_errors 48577\n\
cloudflared_tunnel_concurrent_requests_per_tunnel{connection_id=\"0\"} 0\n\
go_memstats_heap_inuse_bytes 1.4565376e+07\n";

pub fn target_metrics() -> Target {
    Target {
        name: "cf-metrics",
        seeds: seeds(
            &[METRICS_SEED, "cloudflared_tunnel_ha_connections 0\n"],
            "cloudflared-metrics.txt",
            200,
        ),
        dict: vec![
            "cloudflared_tunnel_ha_connections ",
            "cloudflared_tunnel_total_requests ",
            "cloudflared_tunnel_request_errors ",
            "+Inf",
            "-Inf",
            "NaN",
            "inf",
            "-1",
            "1e20",
            "1.8446744073709552e19",
            "4.2e+06",
            "{le=\"0.5\"}",
            "# ",
            "\n",
        ],
        check: check_metrics,
    }
}

// ---------------------------------------------------------------- cloudflared / zerolog log lines

fn check_cflog(text: &str) -> Result<u64, String> {
    let mut shape = String::new();
    let mut jl = vec![];
    for (i, msg) in text.lines().enumerate() {
        for p in [0u8, 3, 4, 6, 7] {
            let c = probe::classify(p, msg);
            ensure(c <= 2, || format!("classify {c}"))?;
            if p <= 3 {
                ensure(c == 2, || "prio <= 3 not an error".into())?;
            }
        }
        if let Some(l) = probe::zerolog_level(msg) {
            ensure(msg.split_whitespace().take(2).any(|t| t == l), || {
                format!("zerolog level {l} not a leading token")
            })?;
        }
        if let Some(p) = logscan::refused_port(msg) {
            ensure(
                msg.contains("connection refused")
                    && msg.contains("dial tcp ")
                    && msg.contains(&p.to_string()),
                || format!("refused port {p} not in the line"),
            )?;
            shape.push('r');
        }
        if let Some(h) = logscan::dest_host(msg) {
            ensure(msg.contains("dest="), || "dest_host without dest=".into())?;
            ensure(
                !h.is_empty()
                    && h.chars().count() <= 100
                    && !h.contains(|c: char| c.is_whitespace() || "/?\"".contains(c)),
                || format!("dest host {h:?}"),
            )?;
            ensure(msg.contains(&h), || format!("host {h:?} not in the line"))?;
            shape.push('d');
        }
        let sp = logscan::scan_paths(msg);
        ensure(sp.windows(2).all(|w| w[0] < w[1]), || {
            "scan paths not sorted+dedup".into()
        })?;
        for p in &sp {
            ensure(
                p.starts_with('/') && p.chars().count() <= 120 && msg.contains(p.as_str()),
                || format!("scan path {p:?}"),
            )?;
        }
        if !sp.is_empty() {
            shape.push('s');
        }
        jl.push(JLine {
            t: NOW + i as u64,
            unit: "cloudflared.service".into(),
            prio: 6,
            msg: msg.to_string(),
        });
    }
    let ins = logscan::insights(&jl);
    let failed = jl.iter().filter(|l| l.msg.contains("Request failed")).count();
    ensure(ins.edge_failures.len() == failed, || {
        "edge_failures != Request failed lines".into()
    })?;
    for (port, d) in &ins.dead_origins {
        ensure(*port == d.port && d.refusals >= 1 && d.failed_requests >= 1, || {
            format!("dead origin {port} counts")
        })?;
        ensure(d.hosts.len() <= 32 && d.first <= d.last, || {
            format!("dead origin {port} hosts/first/last")
        })?;
    }
    ensure(ins.scans.len() <= 256, || "scans over 256".into())?;
    shape.truncate(12);
    Ok(fnv(&shape))
}

pub fn target_cflog() -> Target {
    Target {
        name: "cf-log-lines",
        seeds: seeds(
            &[
                "2026-10-20T16:40:00Z ERR Request failed error=\"dial tcp [::1]:4001: connect: connection refused\" connIndex=0 dest=https://one.example.test/api?x=1 event=0 ip=198.41.200.13 type=http",
                "2026-10-20T16:40:01Z ERR  error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp 127.0.0.1:3011: connect: connection refused\" cfRay=8d0000000000-AMS event=1 ingressRule=3 originService=http://localhost:3011",
                "2026-10-20T16:40:02Z WRN Connection terminated error=\"context canceled\" connIndex=1",
                "2026-10-20T16:40:03Z INF Registered tunnel connection connIndex=2 location=fra08 protocol=quic",
                "GET /.env 404 3ms, GET /wp-login.php 404, \"/.git/config\" scanned",
                "thread 'main' panicked at src/main.rs:1:1",
            ],
            "cloudflared-log.txt",
            40,
        ),
        dict: vec![
            "connection refused",
            "dial tcp ",
            "[::1]:",
            "127.0.0.1:",
            ":65535",
            ":65536",
            "dest=",
            "dest=https://",
            "dest=wss://",
            "originService=",
            "Request failed",
            " ERR ",
            " WRN ",
            "FTL",
            "level=error",
            "/.env",
            "/.git/",
            "../",
            "phpinfo",
            "Vite",
        ],
        check: check_cflog,
    }
}

// ---------------------------------------------------------------- access-log lines

fn check_access(text: &str) -> Result<u64, String> {
    let mut sig = String::new();
    for line in text.lines() {
        let Some(h) = access::parse_line(line, NOW) else {
            ensure(line.trim().is_empty() || line.trim().starts_with('{'), || {
                "non-empty CLF line refused".into()
            })?;
            sig.push('-');
            continue;
        };
        ensure(h.ip.chars().count() <= 64 && h.path.chars().count() <= 200, || {
            "ip/path over cap".into()
        })?;
        if h.status != 0 {
            ensure(line.contains(&h.status.to_string()), || {
                format!("status {} not in the line", h.status)
            })?;
        }
        sig.push(if h.status == 0 { '0' } else { 's' });
    }
    sig.truncate(8);
    Ok(fnv(&sig))
}

pub fn target_access() -> Target {
    let fx = chunks(FIX_ACCESS, 5);
    Target {
        name: "access-line",
        seeds: seeds(
            &[
                &fx[0],
                "127.0.0.1 - - [20/Oct/2026:19:04:34 +0200] \"GET /.env HTTP/1.1\" 404 12 \"-\" \"curl/8\"",
                r#"{"time":"2026-10-20T19:06:05+02:00","remote_addr":"10.0.0.7","request":"GET /api HTTP/1.1","status":"503"}"#,
            ],
            "access.log",
            20,
        ),
        dict: vec![
            "\"status\":",
            "\"ts\":",
            "70000",
            "65536",
            "-1",
            "1e3",
            "[20/Oct/2026:19:04:34 +0200]",
            "\"GET / HTTP/1.1\"",
            " 200 ",
        ],
        check: check_access,
    }
}

// ---------------------------------------------------------------- generative

fn rand_word(rng: &mut Rng, alpha: &[u8], max: usize) -> String {
    let n = 1 + rng.below(max);
    (0..n).map(|_| *rng.pick(alpha) as char).collect()
}

const SRC_ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.";

/// A valid signal in, the same fields out (text fields cleaned of control
/// chars and capped exactly as documented).
fn gen_signal(rng: &mut Rng) -> Result<u64, String> {
    let src = rand_word(rng, SRC_ALPHA, 64);
    let kind = *rng.pick(KINDS);
    let name: String = (0..rng.below(90))
        .map(|_| *rng.pick(&['a', 'Z', ' ', '\t', 'é', '\u{7}', '𝄞']))
        .collect();
    let msg: String = (0..rng.below(400))
        .map(|_| *rng.pick(&['m', ' ', '\n', '€', '\u{0}', '"', '\\']))
        .collect();
    let value = match rng.below(3) {
        0 => Value::Null,
        1 => json!(rng.next() as i64 as f64 / 7.0),
        _ => json!(rng.below(1_000_000)),
    };
    let body = json!({"source": src, "kind": kind, "name": name, "msg": msg, "value": value})
        .to_string();
    if body.len() > MAX_BODY {
        return Ok(0);
    }
    let s = parse_signal(&body, NOW).map_err(|e| format!("valid signal refused: {e}"))?;
    let clean = |x: &str, m: usize| -> String {
        x.chars().filter(|c| !c.is_control()).take(m).collect()
    };
    ensure(s.source == src && s.kind == kind, || "source/kind changed".into())?;
    ensure(s.name == clean(&name, 64) && s.msg == clean(&msg, 300), || {
        "name/msg not cleaned as documented".into()
    })?;
    // serde_json without its `float_roundtrip` feature may be 1 ULP off on
    // big floats (owner decision D-212-3); allow that, nothing more.
    let close = match (s.value, value.as_f64()) {
        (Some(a), Some(b)) => a == b || ((a - b).abs() <= b.abs() * 4.0 * f64::EPSILON),
        (a, b) => a == b,
    };
    ensure(close, || {
        format!("value changed: sent {value} got {:?}", s.value)
    })?;
    Ok(fnv(kind))
}

/// Random valid metrics text (comments, other metrics, labels) parses to
/// exactly the generated values.
fn gen_metrics(rng: &mut Rng) -> Result<u64, String> {
    let keys = [
        "cloudflared_tunnel_ha_connections",
        "cloudflared_tunnel_total_requests",
        "cloudflared_tunnel_request_errors",
    ];
    let mut want = [None, None, None];
    let mut text = String::new();
    for _ in 0..rng.below(12) {
        match rng.below(4) {
            0 => text.push_str("# HELP x y z\n# TYPE x counter\n"),
            1 => text.push_str(&format!("go_goroutines{{a=\"b\"}} {}\n", rng.below(999))),
            _ => {
                let i = rng.below(3);
                let v = rng.next() >> 11; // < 2^53: exact in f64
                let shown = if rng.chance(1, 3) {
                    format!("{:e}", v as f64)
                } else {
                    v.to_string()
                };
                text.push_str(&format!("{} {shown}\n", keys[i]));
                want[i] = Some(v);
            }
        }
    }
    let m = probe::parse_cloudflared_metrics(&text);
    let got = [m.ha_connections, m.total_requests, m.request_errors];
    ensure(got == want, || format!("metrics {got:?} != {want:?}\n{text}"))?;
    Ok(fnv(&format!("{:?}", want.map(|x| x.is_some()))))
}

/// One HTTP request built from random valid parts reads back unchanged at
/// any segmentation.
fn gen_http(rng: &mut Rng) -> Result<u64, String> {
    let method = *rng.pick(&["GET", "POST", "HEAD", "PUT"]);
    let path = format!("/{}", rand_word(rng, b"abcdefghij/-_.", 20));
    let query = if rng.chance(1, 2) {
        format!("n={}", rng.below(2000))
    } else {
        String::new()
    };
    let body_len = rng.below(MAX_BODY + 1);
    let body: String = (0..body_len).map(|_| *rng.pick(b"{}\":ab ") as char).collect();
    let mut head = format!(
        "{method} {path}{}{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n",
        if query.is_empty() { "" } else { "?" }
    );
    for _ in 0..rng.below(20) {
        head.push_str(&format!("X-{}: {}\r\n", rand_word(rng, b"abc", 8), rand_word(rng, b"xyz ", 40)));
    }
    head.push_str(&format!("Content-Length: {body_len}\r\n\r\n"));
    let raw = format!("{head}{body}");
    let want = (method.to_string(), path, query, body);
    for st in [1usize << 20, 2048, 97, 13] {
        let got = read_with(raw.as_bytes(), st);
        ensure(got.as_ref() == Ok(&want), || {
            format!("valid request misread at segment {st}: {:?}", got.map(|g| (g.0, g.1, g.3.len())))
        })?;
    }
    Ok(fnv(&format!("{}:{}", want.0, want.3.is_empty())))
}

/// RFC 3339 / CLF stamps of random instants parse back to the instant.
fn gen_stamps(rng: &mut Rng) -> Result<u64, String> {
    let t = 946_684_800 + rng.below(4_102_444_800 - 946_684_800) as u64;
    let off_min = (rng.below(29) as i64 - 14) * 60 + if rng.chance(1, 4) { 30 } else { 0 };
    let local = t as i64 + off_min * 60;
    let (days, sod) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    let (y, m, d) = civil(days);
    let (hh, mm, ss) = (sod / 3600, sod / 60 % 60, sod % 60);
    let sign = if off_min < 0 { '-' } else { '+' };
    let (oh, om) = (off_min.abs() / 60, off_min.abs() % 60);
    let iso = format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}{sign}{oh:02}:{om:02}");
    let got = openmind_nerve::util::parse_rfc3339(&iso);
    ensure(got == Some(t), || format!("{iso} -> {got:?}, want {t}"))?;
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let clf = format!(
        "{d:02}/{}/{y:04}:{hh:02}:{mm:02}:{ss:02} {sign}{oh:02}{om:02}",
        MON[m as usize - 1]
    );
    let got = openmind_nerve::util::parse_clf_time(&clf);
    ensure(got == Some(t), || format!("{clf} -> {got:?}, want {t}"))?;
    Ok(fnv(&format!("{}", off_min.signum())))
}

/// Days since 1970-01-01 to a civil date (inverse of days_from_civil).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Deeply nested JSON (up to and past serde_json's 128 limit) through the
/// /signal and /event parsers on a thread with the server's 256 KiB stack:
/// must answer (Ok or Err), never overflow the stack.
pub fn deep_json(depth: usize, obj: bool) -> String {
    let (open, close) = if obj { ("{\"a\":", "}") } else { ("[", "]") };
    format!(
        "{{\"source\":\"s\",\"kind\":\"event\",\"severity\":\"info\",\"name\":\"n\",\"data\":{}1{}}}",
        open.repeat(depth),
        close.repeat(depth)
    )
}

pub fn deep_on_server_stack(depth: usize, obj: bool) -> Result<(bool, bool), String> {
    let body = deep_json(depth, obj);
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let s = parse_signal(&body, NOW).is_ok();
            let e = server::event_from(&body)
                .map(|e| e.data_json().len())
                .is_ok();
            (s, e)
        })
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "parser thread panicked".to_string())
}

fn gen_deep(rng: &mut Rng) -> Result<u64, String> {
    let depth = 1 + rng.below(160);
    let obj = rng.chance(1, 2);
    let (s, e) = deep_on_server_stack(depth, obj)?;
    // Within serde_json's limit both parsers accept the body.
    if depth < 120 {
        ensure(s && e, || format!("depth {depth} obj {obj} refused: {s} {e}"))?;
    }
    Ok(fnv(&format!("{s}:{e}")))
}

pub fn targets() -> Vec<Target> {
    vec![
        target_signal(),
        target_event(),
        target_http(),
        target_journal(),
        target_metrics(),
        target_cflog(),
        target_access(),
    ]
}

pub fn gens() -> Vec<Gen> {
    vec![
        Gen {
            name: "signal-roundtrip",
            run: gen_signal,
        },
        Gen {
            name: "metrics-roundtrip",
            run: gen_metrics,
        },
        Gen {
            name: "http-roundtrip",
            run: gen_http,
        },
        Gen {
            name: "stamp-roundtrip",
            run: gen_stamps,
        },
        Gen {
            name: "deep-json-stack",
            run: gen_deep,
        },
    ]
}

/// Same JSON value, numbers allowed 4 ULP apart: serde_json without its
/// `float_roundtrip` feature can move a big float by 1 ULP when the value is
/// printed and parsed again, which `event_from` does (D-212-3).
pub fn json_close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x == y
                || match (x.as_f64(), y.as_f64()) {
                    (Some(p), Some(q)) => (p - q).abs() <= p.abs().max(q.abs()) * 4.0 * f64::EPSILON,
                    _ => false,
                }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_close(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| json_close(p, q)))
        }
        _ => a == b,
    }
}
