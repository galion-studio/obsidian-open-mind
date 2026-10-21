// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The cortex parser targets (T5.2). Each mutation target names the
//! parser(s) it drives and the properties it checks beyond "no panic";
//! each generator is a round-trip property (render known values the way
//! the kernel / journald / cloudflared would, parse, compare).

use super::{ensure, fnv, Gen, Rng, Target};
use openmind_nerve::cortex::{Cortex, Finding, Input};
use openmind_nerve::logscan::{self, LogInsights};
use openmind_nerve::probe::{self, JLine};
use openmind_nerve::sensors::connrate;
use openmind_nerve::sensors::edgerate::{self, EdgeKind};
use openmind_nerve::sensors::event_feed::{self, EventFeed, MAX_KEYS};
use openmind_nerve::sensors::proc_owner;
use openmind_nerve::sensors::{Sensor, Sink};
use openmind_nerve::{server, util};
use std::collections::BTreeSet;
use std::net::{Ipv4Addr, Ipv6Addr};

// 2026-10-21 01:00:00 +02:00
pub const NOW: u64 = 1_792_537_200;

const FIX_SS: &str = include_str!("../fixtures/cortex-replay/ss.txt");
const FIX_JOURNAL: &str = include_str!("../fixtures/cortex-replay/journal.jsonl");
const FIX_TCP0: &str = include_str!("../fixtures/conn-rate/tcp-0.txt");
const FIX_TCP1: &str = include_str!("../fixtures/conn-rate/tcp-1.txt");
const FIX_TCP2: &str = include_str!("../fixtures/conn-rate/tcp-2.txt");
const FIX_SSP: &str = include_str!("../../../tunnel-lint/tests/fixtures/vps-ss-20261020.txt");
const FIX_INGRESS: &str =
    include_str!("../../../tunnel-lint/tests/fixtures/vps-config-20261020.yml");

/// Extra seed files (real VPS captures) from `CORTEX_FUZZ_SEEDS=<dir>`:
/// `ss-ltn.txt`, `proc-net-tcp.txt`, `proc-net-tcp6.txt`, `journal.jsonl`,
/// `ingress.yml`, `events.jsonl`. Large files are cut into chunks.
fn extra(file: &str) -> Vec<String> {
    let Ok(dir) = std::env::var("CORTEX_FUZZ_SEEDS") else {
        return vec![];
    };
    let Ok(text) = std::fs::read_to_string(std::path::Path::new(&dir).join(file)) else {
        return vec![];
    };
    chunks(&text, 40)
}

/// `text` as whole (when short) plus chunks of `n` lines (at most 64).
pub fn chunks(text: &str, n: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= n {
        return vec![text.to_string()];
    }
    lines.chunks(n).take(64).map(|c| c.join("\n")).collect()
}

fn seeds(base: &[&str], file: &str) -> Vec<String> {
    let mut v: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    v.extend(extra(file));
    v
}

// ---------------------------------------------------------------- ss / proc

fn check_ss(text: &str) -> Result<u64, String> {
    let ls = probe::parse_ss(text);
    let mut sorted = ls.clone();
    sorted.sort();
    sorted.dedup();
    ensure(sorted == ls, || "parse_ss output not sorted+deduped".into())?;
    for l in &ls {
        let lo = l.addr.starts_with("127.") || l.addr == "::1" || l.addr == "localhost";
        ensure(l.loopback == lo, || {
            format!("loopback flag wrong for {l:?}")
        })?;
        ensure(!l.addr.contains('%') && !l.addr.starts_with('['), || {
            format!("addr not normalised: {l:?}")
        })?;
    }
    // Differential: tunnel-lint's SsSnapshot over lines both accept
    // (line starts with the exact token `LISTEN`).
    let common: String = text
        .lines()
        .filter(|l| l.starts_with("LISTEN") && l.split_whitespace().next() == Some("LISTEN"))
        .map(|l| format!("{l}\n"))
        .collect();
    let a: BTreeSet<(String, u16)> = probe::parse_ss(&common)
        .into_iter()
        .map(|l| (l.addr, l.port))
        .collect();
    let b: BTreeSet<(String, u16)> = tunnel_lint::SsSnapshot::parse(&common)
        .entries
        .into_iter()
        .map(|(a, p, _)| (a, p))
        .collect();
    ensure(a == b, || {
        format!(
            "probe::parse_ss vs tunnel_lint::SsSnapshot differ: only probe {:?}, only lint {:?}",
            a.difference(&b).take(3).collect::<Vec<_>>(),
            b.difference(&a).take(3).collect::<Vec<_>>()
        )
    })?;
    Ok(fnv(&format!(
        "{}:{}",
        ls.len().min(8),
        ls.iter().filter(|l| l.loopback).count().min(8)
    )))
}

pub fn target_ss() -> Target {
    Target {
        name: "ss-ltn",
        seeds: seeds(
            &[
                FIX_SS,
                FIX_SSP,
                "LISTEN 0 4096 [::ffff:127.0.0.1]:8670 [::]:*\nLISTEN 0 128 [fe80::1%eth0]:22 [::]:*\nLISTEN 0 5 *:443 *:*",
            ],
            "ss-ltn.txt",
        ),
        dict: vec!["LISTEN", "ESTAB", "127.0.0.1:", "[::1]:", "0.0.0.0:", "*:", "%lo", "users:((\"x\",pid=1,fd=3))", "0.0.0.0:*"],
        check: check_ss,
    }
}

fn check_proc_net(text: &str) -> Result<u64, String> {
    let v4 = probe::parse_proc_net_tcp(text, false);
    let v6 = probe::parse_proc_net_tcp(text, true);
    let own = proc_owner::parse_proc_net_tcp(text);
    // Every row proc_owner accepts, probe accepts too (same LISTEN filter,
    // probe is laxer about the field count).
    let p4: BTreeSet<u16> = v4.iter().map(|l| l.port).collect();
    let p6: BTreeSet<u16> = v6.iter().map(|l| l.port).collect();
    for r in &own {
        let ok = if r.addr.contains(':') {
            p6.contains(&r.port)
        } else {
            p4.contains(&r.port)
        };
        ensure(ok, || {
            format!("proc_owner row {}:{} missing from probe", r.addr, r.port)
        })?;
        let ip_lo = r
            .addr
            .parse::<std::net::IpAddr>()
            .map(|ip| match ip {
                std::net::IpAddr::V4(a) => a.is_loopback(),
                std::net::IpAddr::V6(a) => {
                    a.is_loopback() || a.to_ipv4_mapped().is_some_and(|m| m.is_loopback())
                }
            })
            .map_err(|_| format!("proc_owner addr not an IP: {}", r.addr))?;
        ensure(ip_lo == r.loopback, || {
            format!("proc_owner loopback flag wrong: {}", r.addr)
        })?;
    }
    // conn-rate: watch the first few ports seen; counts stay consistent.
    let mut watch: Vec<u16> = p4.iter().chain(p6.iter()).copied().take(4).collect();
    watch.extend([8420, 8670]);
    let t = connrate::parse_tables(text, &watch);
    let lines = text.lines().count() as u32;
    ensure(
        t.keys().copied().collect::<BTreeSet<_>>() == watch.iter().copied().collect(),
        || "parse_tables keys != watch".into(),
    )?;
    let mut conns = 0usize;
    for (p, c) in &t {
        ensure(c.estab + c.time_wait <= c.conns.len() as u32, || {
            format!("port {p}: estab+time_wait > identities")
        })?;
        conns += c.conns.len();
    }
    ensure(conns as u32 <= lines, || {
        "more identities than lines".into()
    })?;
    Ok(fnv(&format!(
        "{}:{}:{}:{}",
        v4.len().min(6),
        v6.len().min(6),
        own.len().min(6),
        conns.min(6)
    )))
}

pub fn target_proc_net() -> Target {
    let mut s = seeds(&[FIX_TCP0, FIX_TCP1, FIX_TCP2], "proc-net-tcp.txt");
    s.extend(extra("proc-net-tcp6.txt"));
    s.push("  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000001000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000   997        0 12345 1 0000000000000000 100 0 0 10 0".into());
    s.push("  sl  local_address remote_address st\n   0: 0000000\u{e9}00000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000   997        0 12345 1".into());
    Target {
        name: "proc-net-tcp",
        seeds: s,
        dict: vec![
            "0A",
            "01",
            "06",
            "0100007F:",
            "00000000:",
            "0000000000000000FFFF00000100007F:",
            "   0: ",
            " 0 ",
            "ffffffff",
        ],
        check: check_proc_net,
    }
}

// ---------------------------------------------------------------- journald

const UNITS: &[&str] = &["cloudflared.service", "ssh.service", "galion-nerve.service"];

fn check_journal(text: &str) -> Result<u64, String> {
    let all = probe::parse_journal_lines(text, &[]);
    let some = probe::parse_journal_lines(text, UNITS);
    let n = text.lines().count();
    ensure(all.len() <= n && some.len() <= n, || {
        "more records than lines".into()
    })?;
    for l in all.iter().chain(&some) {
        ensure(l.msg.chars().count() <= 2000, || {
            "msg over 2000 chars".into()
        })?;
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
    }
    let ins = logscan::insights(&all);
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
    Target {
        name: "journald-json",
        seeds: seeds(
            &[
                &chunks(FIX_JOURNAL, 20)[0],
                r#"{"UNIT":"ssh.service","_SYSTEMD_UNIT":"init.scope","PRIORITY":"3","MESSAGE":"ssh.service: Failed with result 'exit-code'."}"#,
                r#"{"_SYSTEMD_UNIT":"cloudflared.service","PRIORITY":"6","MESSAGE":[1,2,3]}"#,
            ],
            "journal.jsonl",
        ),
        dict: vec![
            "\"MESSAGE\":",
            "\"UNIT\":",
            "\"_SYSTEMD_UNIT\":",
            "\"PRIORITY\":\"",
            "\"__REALTIME_TIMESTAMP\":\"",
            "cloudflared.service",
            "ssh.service",
            "\\u0000",
            "\\ud800",
            "\\n",
            "null",
            "[]",
            "{}",
            "1e999",
            "Request failed",
            "connection refused",
            "dial tcp ",
            "dest=https://",
            " ERR ",
            "panicked",
        ],
        check: check_journal,
    }
}

// ---------------------------------------------------------------- cloudflared

fn dns_ok(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

fn check_cloudflared(text: &str) -> Result<u64, String> {
    let mut shape = String::new();
    for msg in text.lines() {
        let e = edgerate::parse(msg);
        if let Some((h, _)) = &e {
            ensure(dns_ok(h), || format!("edgerate host not a DNS name: {h:?}"))?;
        }
        let d = logscan::dest_host(msg);
        if let Some(h) = &d {
            ensure(
                h.chars().count() <= 100 && !h.contains('/') && !h.contains('?') && !h.is_empty(),
                || format!("dest_host bad: {h:?}"),
            )?;
        }
        let r = logscan::refused_port(msg);
        if r.is_some() {
            ensure(msg.contains("connection refused"), || {
                "refused_port without refusal".into()
            })?;
        }
        let sp = logscan::scan_paths(msg);
        let mut s2 = sp.clone();
        s2.sort();
        s2.dedup();
        ensure(s2 == sp, || "scan_paths not sorted+deduped".into())?;
        for p in &sp {
            ensure(p.starts_with('/') && p.chars().count() <= 120, || {
                format!("scan path bad: {p:?}")
            })?;
        }
        if let Some(h) = edgerate::host_of(msg) {
            ensure(dns_ok(&h) && h.contains('.'), || {
                format!("host_of bad: {h:?}")
            })?;
        }
        let c = probe::classify(6, msg);
        shape.push_str(&format!(
            "{}{}{}{}{}|",
            e.map(|x| x.1 == EdgeKind::Failed)
                .map_or(0, |f| 1 + f as u8),
            d.is_some() as u8,
            r.is_some() as u8,
            sp.len().min(2),
            c
        ));
    }
    let jl: Vec<JLine> = text
        .lines()
        .enumerate()
        .map(|(i, m)| JLine {
            t: NOW - 600 + i as u64,
            unit: "cloudflared.service".into(),
            prio: 6,
            msg: m.to_string(),
        })
        .collect();
    let ins = logscan::insights(&jl);
    ensure(ins.edge_failures.len() <= jl.len(), || {
        "more edge failures than lines".into()
    })?;
    Ok(fnv(&shape))
}

pub fn target_cloudflared() -> Target {
    let mut s: Vec<String> = FIX_JOURNAL
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["MESSAGE"].as_str().map(str::to_string))
        .take(30)
        .collect();
    s.extend([
        "2026-10-21T07:12:01Z ERR  error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:4001: connect: connection refused\" connIndex=1 event=1 ingressRule=18 originService=http://localhost:4001".to_string(),
        "2026-10-21T07:12:01Z ERR Request failed error=\"Unable to reach the origin service: dial tcp 127.0.0.1:3011: connect: connection refused\" connIndex=2 dest=https://Galion.App/wp-login.php?x=1 event=0 ip=198.51.100.7 type=http".to_string(),
        "2026-10-21T07:12:02Z DBG GET https://one.galion.app/.env HTTP/1.1 connIndex=0 content-length=0 event=1 headers={\"Accept\":[\"*/*\"]} host=one.galion.app ingressRule=3 originService=http://localhost:8091 path=/.env".to_string(),
    ]);
    // Real cloudflared messages from the VPS capture, when given.
    for chunk in extra("journal.jsonl") {
        for l in chunk.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
                if v["_SYSTEMD_UNIT"] == "cloudflared.service" {
                    if let Some(m) = v["MESSAGE"].as_str() {
                        if s.len() < 400 {
                            s.push(m.to_string());
                        }
                    }
                }
            }
        }
    }
    Target {
        name: "cloudflared-msg",
        seeds: s,
        dict: vec![
            "dest=",
            " dest=",
            "dest=https://",
            "dest=http://",
            "dest=wss://",
            "Request failed",
            "connection refused",
            "dial tcp ",
            "[::1]:",
            "127.0.0.1:",
            " DBG ",
            "DBG",
            " GET ",
            "HTTP/1.1",
            "https://",
            "@",
            ":443",
            "/wp-login.php",
            "/.env",
            "/.git/config",
            " ERR ",
            "..",
            "xn--",
            "A.B",
            "[::]",
        ],
        check: check_cloudflared,
    }
}

// ---------------------------------------------------------------- /event JSONL

fn feed_cycle(f: &mut EventFeed, now: u64) -> Cortex {
    let mut cx = Cortex::default();
    let logs = LogInsights::default();
    let i = Input::new(now, &[], &logs);
    let mut sink = Sink::new(&mut cx, "event-feed");
    f.sense(&i, &mut sink);
    cx
}

fn id_ok(f: &Finding) -> bool {
    !f.id.is_empty()
        && f.id.len() <= 200
        && f.id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-:".contains(c))
}

fn check_event(text: &str) -> Result<u64, String> {
    let mut f = EventFeed::new(vec![], 3600, None);
    let mut nonempty = 0u64;
    for l in text.lines() {
        if !l.trim().is_empty() {
            nonempty += 1;
        }
        f.ingest_line(l, NOW);
        // The POST /event body parser sees the same bytes.
        let _ = server::event_from(l);
    }
    let st = f.stats;
    ensure(st.lines == nonempty, || {
        format!("stats.lines {} != {nonempty}", st.lines)
    })?;
    ensure(st.bad + st.skipped + st.old <= st.lines, || {
        "bad+skipped+old > lines".into()
    })?;
    let cx = feed_cycle(&mut f, NOW);
    let n = cx.findings().count();
    ensure(n <= MAX_KEYS, || format!("{n} findings > MAX_KEYS"))?;
    for x in cx.findings() {
        ensure(id_ok(x), || format!("finding id not clean: {:?}", x.id))?;
        ensure(!x.title.chars().any(char::is_control), || {
            format!("control char in title {:?}", x.title)
        })?;
        ensure(x.title.chars().count() <= 400, || "title too long".into())?;
        ensure(!x.component.chars().any(char::is_control), || {
            "control char in component".into()
        })?;
        for e in &x.evidence {
            ensure(!e.chars().any(char::is_control), || {
                format!("control char in evidence {e:?}")
            })?;
        }
    }
    // The B33 projection of the same state stays valid JSON.
    let j = cx.to_json().to_string();
    ensure(
        serde_json::from_str::<serde_json::Value>(&j).is_ok(),
        || "cortex json invalid".into(),
    )?;
    // A later cycle past the window clears everything.
    let cx2 = feed_cycle(&mut f, NOW + 3600 + 61);
    ensure(cx2.findings().count() == 0, || {
        "findings survived past the window".into()
    })?;
    Ok(fnv(&format!(
        "{}:{}:{}:{}:{}",
        st.bad.min(3),
        st.old.min(3),
        n.min(6),
        cx.signals.len().min(3),
        cx.findings().map(|x| x.sev as u8 as u64).max().unwrap_or(9)
    )))
}

fn ev(t: u64, src: &str, kind: &str, sev: &str, data: &str) -> String {
    format!(
        r#"{{"ts":"{}","source":"{src}","kind":"{kind}","severity":"{sev}","data":{data}}}"#,
        zagreb_time::from_unix(t as i64)
    )
}

pub fn target_event() -> Target {
    let mut s = vec![
        ev(NOW - 120, "gaia", "service.probe", "warn", r#"{"service":"t1","port":19761,"state":"down","detail":"connect 127.0.0.1:19761 refused","probe_id":"p"}"#),
        ev(NOW - 60, "gaia", "gaia.heal.capped", "error", r#"{"service":"t1","port":19761,"restarts_last_hour":3,"cap":3,"retry_after_s":900,"probe_id":"p"}"#),
        ev(NOW - 50, "gaia", "gaia.heal.refused", "warn", r#"{"service":"t1","stage":"hades","reason":"no approval file"}"#),
        ev(NOW - 40, "gaia", "gaia.heal.applied", "info", r#"{"service":"t1"}"#),
        ev(NOW - 30, "survival", "probe", "warn", r#"{"name":"disk","ok":false,"detail":"/ 97% used"}"#),
        r#"{"ts":"2026-10-21T00:59:00+02:00","source":"survival","kind":"probe","name":"disk","ok":true}"#.to_string(),
        ev(NOW - 20, "event_cortex", "cortex.finding", "critical", r#"{"top":{"id":"x-1","component":"edge","title":"10.0.0.1:443 open to 192.0.2.4"}}"#),
        ev(NOW - 10, "galion-deploy", "nerve-cutover", "info", r#"{"binary":"openmind-nerve-aa77864","slot":180}"#),
        "not json\n[]\n\"str\"\n{}".to_string(),
    ];
    s.push(s[..8].join("\n"));
    s.extend(extra("events.jsonl"));
    Target {
        name: "event-jsonl",
        seeds: s,
        dict: vec![
            "\"ts\":\"",
            "\"source\":\"",
            "\"kind\":\"",
            "\"severity\":\"",
            "\"data\":",
            "gaia.heal.",
            "capped",
            "refused",
            "applied",
            "failed",
            "service.probe",
            "probe",
            "\"ok\":false",
            "\"ok\":true",
            "\"state\":\"up\"",
            "critical",
            "error",
            "warn",
            "info",
            "+02:00",
            "+01:00",
            "Z",
            "\\u0007",
            "\\u001b[31m",
            "\\n",
            "9999-12-31T23:59:60+00:00",
            "2026-10-25T02:30:00+01:00",
            "1970-01-01T00:00:00Z",
            "\"top\":{",
            "\"restarts_last_hour\":",
            "18446744073709551615",
        ],
        check: check_event,
    }
}

fn check_rfc3339(text: &str) -> Result<u64, String> {
    let mut shape = 0u64;
    for l in text.lines() {
        let a = event_feed::parse_rfc3339(l);
        let b = util::parse_rfc3339(l);
        if let Some(x) = a {
            // Everything the strict (event-feed) parser accepts, the lax
            // shared parser accepts with the same value.
            ensure(b == Some(x), || {
                format!("event_feed {a:?} vs util {b:?} for {l:?}")
            })?;
        }
        shape = shape
            .wrapping_mul(7)
            .wrapping_add(a.is_some() as u64 * 2 + b.is_some() as u64);
    }
    Ok(shape % 4096)
}

pub fn target_rfc3339() -> Target {
    Target {
        name: "rfc3339",
        seeds: vec![
            "2026-10-21T01:00:00+02:00\n2026-10-20T23:00:00Z\n2026-10-20T23:00:00.123456Z\n2026-10-25T02:00:00+01:00\n2026-10-25T02:59:59+02:00".into(),
            "2026-10-21 01:00:00+02:00\n2026-10-21t01:00:00-11:30\n1970-01-01T00:00:00Z".into(),
        ],
        dict: vec!["+", "-", "Z", "z", "T", "t", ".", "60", "00", "23", "59", "12", "31", "+14:00", "-12:00"],
        check: check_rfc3339,
    }
}

fn check_http(text: &str) -> Result<u64, String> {
    let mut cur = std::io::Cursor::new(text.as_bytes().to_vec());
    match server::read_req(&mut cur) {
        Ok(r) => {
            ensure(r.body.len() <= 4 * text.len() + 16, || "body grew".into())?;
            ensure(!r.path.contains('?'), || "path keeps query".into())?;
            Ok(fnv(&format!(
                "ok:{}:{}",
                r.method.len().min(8),
                r.body.is_empty()
            )))
        }
        Err((code, _)) => {
            ensure(matches!(code, 400 | 413 | 431), || {
                format!("odd error code {code}")
            })?;
            Ok(code as u64)
        }
    }
}

pub fn target_http() -> Target {
    Target {
        name: "http-event-req",
        seeds: vec![
            format!("POST /event HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 58\r\n\r\n{}", r#"{"source":"gaia","kind":"service.probe","severity":"info"}"#),
            "GET /api/v1/state?x=1 HTTP/1.0\r\n\r\n".into(),
            "POST /signal HTTP/1.1\r\ncontent-length: 2\r\n\r\n{}".into(),
        ],
        dict: vec!["\r\n", "\r\n\r\n", "Content-Length: ", "content-length:", "99999999", "-1", "POST", "GET", "/event", "?"],
        check: check_http,
    }
}

// ---------------------------------------------------------------- ingress YAML

struct Fixed(BTreeSet<u16>);
impl tunnel_lint::Listeners for Fixed {
    fn check(&self, _host: &str, port: u16) -> Option<String> {
        self.0.contains(&port).then(String::new)
    }
}

fn check_ingress(text: &str) -> Result<u64, String> {
    let Ok(rules) = tunnel_lint::parse(text) else {
        return Ok(1);
    };
    let n_lines = text.lines().count();
    for (i, r) in rules.iter().enumerate() {
        ensure(r.index == i + 1, || "rule index not sequential".into())?;
        ensure(r.lines.0 <= r.lines.1 && r.lines.1 < n_lines, || {
            format!(
                "rule {} line range {:?} outside 0..{n_lines}",
                r.index, r.lines
            )
        })?;
        ensure(!r.service.is_empty(), || "empty service".into())?;
        if i > 0 {
            ensure(rules[i - 1].lines.1 < r.lines.0, || {
                "rule line ranges overlap".into()
            })?;
        }
        let _ = tunnel_lint::origin(&r.service);
    }
    // Listeners: even ports open, odd closed.
    let open: BTreeSet<u16> = rules
        .iter()
        .filter_map(|r| match tunnel_lint::origin(&r.service) {
            tunnel_lint::Origin::Local { port, .. } if port % 2 == 0 => Some(port),
            _ => None,
        })
        .collect();
    let rep = tunnel_lint::lint(&rules, &Fixed(open));
    let dead = rep
        .closed()
        .filter(|r| r.rule.hostname.is_some() || r.rule.path.is_some())
        .filter(|r| matches!(r.origin, tunnel_lint::Origin::Local { .. }))
        .count();
    let staged = tunnel_lint::stage(text, &rep);
    match tunnel_lint::parse(&staged) {
        Ok(r2) => ensure(r2.len() == rules.len() - dead, || {
            format!(
                "staged config has {} rules, want {} - {dead}",
                r2.len(),
                rules.len()
            )
        })?,
        Err(e) => ensure(rules.len() == dead, || {
            format!("staged config no longer parses: {e}")
        })?,
    }
    Ok(fnv(&format!(
        "{}:{}:{}",
        rules.len().min(6),
        dead.min(4),
        rep.has_errors()
    )))
}

pub fn target_ingress() -> Target {
    Target {
        name: "ingress-yaml",
        seeds: seeds(
            &[
                FIX_INGRESS,
                "tunnel: x\ningress:\n  - hostname: a.example.test\n    service: http://localhost:8081\n    originRequest:\n      noTLSVerify: true\n      access:\n        - team\n  - hostname: b.example.test\n    path: /api\n    service: http://127.0.0.1:8083 # comment\n  - service: http_status:404\n",
                "ingress:\n- hostname: \"q.example.test\"\n  service: 'tcp://[::1]:22'\n- service: hello_world\n",
            ],
            "ingress.yml",
        ),
        dict: vec![
            "ingress:\n", "  - ", "- ", "hostname: ", "service: ", "path: ", "originRequest:\n", "    ", "http://localhost:",
            "https://127.0.0.1:", "tcp://[::1]:", "unix:/run/x.sock", "http_status:404", " # ", "\"", "'", "\t", "-\n",
        ],
        check: check_ingress,
    }
}

// ---------------------------------------------------------------- generators

fn rand_v6(rng: &mut Rng) -> Ipv6Addr {
    match rng.below(4) {
        0 => Ipv6Addr::LOCALHOST,
        1 => Ipv6Addr::UNSPECIFIED,
        2 => Ipv4Addr::new(127, 0, 0, rng.range(1, 254) as u8).to_ipv6_mapped(),
        _ => Ipv6Addr::from(((rng.next() as u128) << 64) | rng.next() as u128),
    }
}

fn rand_v4(rng: &mut Rng) -> Ipv4Addr {
    match rng.below(4) {
        0 => Ipv4Addr::new(127, 0, 0, 1),
        1 => Ipv4Addr::UNSPECIFIED,
        2 => Ipv4Addr::new(127, 0, 0, 53),
        _ => Ipv4Addr::from(rng.next() as u32),
    }
}

/// Kernel text for one address: 32-bit words in host (little-endian) order.
fn kernel_hex_v4(a: Ipv4Addr) -> String {
    format!("{:08X}", u32::from_le_bytes(a.octets()))
}
fn kernel_hex_v6(a: Ipv6Addr) -> String {
    a.octets()
        .chunks(4)
        .map(|c| format!("{:08X}", u32::from_le_bytes([c[0], c[1], c[2], c[3]])))
        .collect()
}

/// Random sockets rendered as /proc/net/tcp(6); both parsers must return
/// exactly the LISTEN rows, and conn-rate the ESTAB/TIME-WAIT counts.
fn gen_proc_net(rng: &mut Rng) -> Result<u64, String> {
    let v6 = rng.chance(1, 2);
    let n = rng.below(40);
    let mut text = String::from("  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n");
    let mut want: BTreeSet<(String, u16, u32, u64)> = BTreeSet::new();
    let watch = 8420u16;
    let (mut estab, mut tw) = (0u32, 0u32);
    let mut seen_conn: BTreeSet<(String, String)> = BTreeSet::new();
    for i in 0..n {
        let (addr, hex, raddr) = if v6 {
            let a = rand_v6(rng);
            (a.to_string(), kernel_hex_v6(a), kernel_hex_v6(rand_v6(rng)))
        } else {
            let a = rand_v4(rng);
            (a.to_string(), kernel_hex_v4(a), kernel_hex_v4(rand_v4(rng)))
        };
        let port: u16 = if rng.chance(1, 3) {
            watch
        } else {
            rng.range(1, 65535) as u16
        };
        let rport: u16 = rng.range(1, 65535) as u16;
        let st: u8 = *rng.pick(&[0x0A, 0x01, 0x06, 0x08]);
        let uid = rng.below(70000) as u32;
        let inode = rng.next() >> 20;
        let local = format!("{hex}:{port:04X}");
        let remote = format!("{raddr}:{rport:04X}");
        text.push_str(&format!(
            "{i:4}: {local} {remote} {st:02X} 00000000:00000000 00:00000000 00000000 {uid:5} 0 {inode} 1 0000000000000000 100 0 0 10 0\n"
        ));
        if st == 0x0A {
            want.insert((addr, port, uid, inode));
        } else if port == watch
            && rport != watch
            && seen_conn.insert((local.clone(), remote.clone()))
        {
            match st {
                0x01 => estab += 1,
                0x06 => tw += 1,
                _ => {}
            }
        }
    }
    let got: BTreeSet<(String, u16, u32, u64)> = proc_owner::parse_proc_net_tcp(&text)
        .into_iter()
        .map(|r| (r.addr, r.port, r.uid, r.inode))
        .collect();
    ensure(got == want, || {
        format!(
            "proc_owner rows differ: got {} want {}",
            got.len(),
            want.len()
        )
    })?;
    let pl: BTreeSet<u16> = probe::parse_proc_net_tcp(&text, v6)
        .into_iter()
        .map(|l| l.port)
        .collect();
    let wl: BTreeSet<u16> = want.iter().map(|w| w.1).collect();
    ensure(pl == wl, || "probe ports differ".into())?;
    let t = connrate::parse_tables(&text, &[watch]);
    let c = &t[&watch];
    // Lower bound: client-side rows (remote port = watched) may add more.
    ensure(c.estab >= estab && c.time_wait >= tw, || {
        format!(
            "conn-rate estab {}/{estab} tw {}/{tw}",
            c.estab, c.time_wait
        )
    })?;
    Ok((want.len() as u64).min(20))
}

/// Random listeners rendered as `ss -ltn`.
fn gen_ss(rng: &mut Rng) -> Result<u64, String> {
    let mut text =
        String::from("State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess\n");
    let mut want = BTreeSet::new();
    for _ in 0..rng.below(30) {
        let port = rng.range(1, 65535) as u16;
        let (shown, norm) = match rng.below(5) {
            0 => {
                let a = rand_v4(rng);
                (a.to_string(), a.to_string())
            }
            1 => {
                let a = rand_v6(rng);
                (format!("[{a}]"), a.to_string())
            }
            2 => ("*".into(), "*".into()),
            3 => ("[::1]%lo".into(), "::1".into()),
            _ => ("127.0.0.53%lo".into(), "127.0.0.53".into()),
        };
        text.push_str(&format!(
            "LISTEN 0      4096   {shown}:{port}   0.0.0.0:*\n"
        ));
        want.insert((norm, port));
    }
    let got: BTreeSet<(String, u16)> = probe::parse_ss(&text)
        .into_iter()
        .map(|l| (l.addr, l.port))
        .collect();
    ensure(got == want, || {
        format!("parse_ss: got {got:?} want {want:?}")
    })?;
    let lint: BTreeSet<(String, u16)> = tunnel_lint::SsSnapshot::parse(&text)
        .entries
        .into_iter()
        .map(|(a, p, _)| (a, p))
        .collect();
    ensure(lint == want, || {
        format!("SsSnapshot: got {lint:?} want {want:?}")
    })?;
    Ok(want.len() as u64)
}

fn rand_text(rng: &mut Rng, max: usize) -> String {
    const ALPH: &[&str] = &[
        "a", "Z", "0", " ", "\"", "\\", "/", "\n", "\t", "\u{0}", "\u{1b}", "é", "€", "𝄞",
        "\u{2028}", "dest=", ":", "=", "{", "}",
    ];
    (0..rng.below(max)).map(|_| *rng.pick(ALPH)).collect()
}

/// Random journald records (any MESSAGE text) serialised with serde_json.
fn gen_journal(rng: &mut Rng) -> Result<u64, String> {
    let mut out = String::new();
    let mut want = vec![];
    for _ in 0..rng.below(20) {
        let unit = *rng.pick(UNITS);
        let msg = if rng.chance(1, 50) {
            "x".repeat(2100)
        } else {
            rand_text(rng, 80)
        };
        let prio = rng.below(8) as u8;
        let us = rng.range(1_700_000_000, 1_900_000_000) * 1_000_000 + rng.below(1_000_000) as u64;
        let v = serde_json::json!({"_SYSTEMD_UNIT": unit, "PRIORITY": prio.to_string(), "__REALTIME_TIMESTAMP": us.to_string(), "MESSAGE": msg});
        out.push_str(&v.to_string());
        out.push('\n');
        want.push(JLine {
            t: us / 1_000_000,
            unit: unit.into(),
            prio,
            msg: msg.chars().take(2000).collect(),
        });
    }
    let got = probe::parse_journal_lines(&out, UNITS);
    ensure(got == want, || "journal round-trip differs".into())?;
    Ok(want.len() as u64)
}

fn rand_label(rng: &mut Rng) -> String {
    let n = 1 + rng.below(12);
    let mut s: String = (0..n)
        .map(|_| *rng.pick(&['a', 'b', 'Q', 'z', '0', '9', '-']))
        .collect();
    if s.starts_with('-') {
        s.replace_range(0..1, "x");
    }
    s
}

/// cloudflared lines built around a random host/port/path.
fn gen_cloudflared(rng: &mut Rng) -> Result<u64, String> {
    let host = format!(
        "{}.{}.{}",
        rand_label(rng),
        rand_label(rng),
        *rng.pick(&["app", "studio", "test"])
    );
    let port = rng.range(1, 65535) as u16;
    let path = *rng.pick(&["/", "/wp-login.php", "/api/v1/state?x=1", "", "/a%20b"]);
    let scheme = *rng.pick(&["https", "http"]);
    let lo = if rng.chance(1, 2) {
        format!("[::1]:{port}")
    } else {
        format!("127.0.0.1:{port}")
    };
    let failed = format!(
        "2026-10-21T07:12:01Z ERR Request failed error=\"Unable to reach the origin service: dial tcp {lo}: connect: connection refused\" connIndex=1 dest={scheme}://{host}{path} event=0 type=http"
    );
    let lower = host.to_ascii_lowercase();
    ensure(
        edgerate::parse(&failed) == Some((lower.clone(), EdgeKind::Failed)),
        || format!("edgerate failed-line: {failed}"),
    )?;
    ensure(logscan::refused_port(&failed) == Some(port), || {
        format!("refused_port: {failed}")
    })?;
    ensure(
        logscan::dest_host(&failed).as_deref() == Some(host.as_str()),
        || format!("dest_host: {failed}"),
    )?;
    let method = *rng.pick(&["GET", "POST", "HEAD"]);
    let dbg = format!("2026-10-21T07:12:02Z DBG {method} {scheme}://{host}{path} HTTP/1.1 connIndex=0 host={host} ingressRule=3 originService=http://localhost:{port}");
    ensure(
        edgerate::parse(&dbg) == Some((lower, EdgeKind::Request)),
        || format!("edgerate dbg-line: {dbg}"),
    )?;
    Ok(port as u64 % 64)
}

/// Valid /event records at random times inside the window are all
/// counted, none bad, and survive one cycle.
fn gen_event(rng: &mut Rng) -> Result<u64, String> {
    let mut f = EventFeed::new(vec![], 3600, None);
    let n = 1 + rng.below(30);
    let mut svcs = BTreeSet::new();
    for _ in 0..n {
        let t = NOW - rng.below(3000) as u64;
        let svc = format!("s{}", rng.below(5));
        svcs.insert(svc.clone());
        let off = *rng.pick(&[0i64, 3600, 7200, -18000]);
        let ts = {
            let local = t as i64 + off;
            let days = local.div_euclid(86_400);
            let secs = local.rem_euclid(86_400);
            let (y, m, d) = civil(days);
            let sign = if off < 0 { '-' } else { '+' };
            format!(
                "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
                secs / 3600,
                secs / 60 % 60,
                secs % 60,
                off.abs() / 3600,
                off.abs() / 60 % 60
            )
        };
        ensure(event_feed::parse_rfc3339(&ts) == Some(t), || {
            format!("ts round-trip {ts} != {t}")
        })?;
        let line = format!(
            r#"{{"ts":"{ts}","source":"gaia","kind":"service.probe","severity":"warn","data":{{"service":"{svc}","state":"down","detail":"x"}}}}"#
        );
        f.ingest_line(&line, NOW);
    }
    ensure(
        f.stats.lines == n as u64 && f.stats.bad == 0 && f.stats.old == 0,
        || format!("stats {:?}", f.stats),
    )?;
    let cx = feed_cycle(&mut f, NOW);
    ensure(cx.findings().count() == svcs.len(), || {
        format!(
            "{} findings for {} services",
            cx.findings().count(),
            svcs.len()
        )
    })?;
    Ok(svcs.len() as u64)
}

/// Inverse of days_from_civil (H. Hinnant, public-domain description).
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

/// Random instants through zagreb_time and both RFC 3339 parsers.
fn gen_rfc3339(rng: &mut Rng) -> Result<u64, String> {
    let t = rng.range(0, 7_258_118_399); // up to 2199-12-31
    let s = zagreb_time::from_unix(t as i64);
    ensure(event_feed::parse_rfc3339(&s) == Some(t), || {
        format!("event_feed {s} != {t}")
    })?;
    ensure(util::parse_rfc3339(&s) == Some(t), || {
        format!("util {s} != {t}")
    })?;
    Ok(s.ends_with("+02:00") as u64)
}

/// Random ingress configs: the parser returns exactly what was written.
fn gen_ingress(rng: &mut Rng) -> Result<u64, String> {
    let n = 1 + rng.below(12);
    let mut text =
        String::from("tunnel: 00000000-0000-0000-0000-000000000000\ncredentials-file: /dev/null\n");
    if rng.chance(1, 3) {
        text.push_str("originRequest:\n  connectTimeout: 10s\n");
    }
    text.push_str("ingress:\n");
    let ind = *rng.pick(&["", "  ", "    "]);
    let mut want = vec![];
    for i in 0..n {
        let last = i + 1 == n;
        let host = (!last).then(|| format!("{}.example.test", rand_label(rng)));
        let path = (!last && rng.chance(1, 4)).then(|| "/api".to_string());
        let svc = if last {
            "http_status:404".to_string()
        } else {
            format!(
                "{}://{}:{}",
                rng.pick(&["http", "https", "tcp"]),
                rng.pick(&["localhost", "127.0.0.1", "[::1]"]),
                rng.range(1, 65535)
            )
        };
        let q = |s: &str, rng: &mut Rng| match rng.below(3) {
            0 => format!("\"{s}\""),
            1 => format!("'{s}'"),
            _ => s.to_string(),
        };
        let mut first = true;
        let mut item = |k: &str, v: &str, text: &mut String| {
            text.push_str(&format!(
                "{ind}{} {k}: {v}\n",
                if first { "-" } else { " " }
            ));
            first = false;
        };
        if let Some(h) = &host {
            let v = q(h, rng);
            item("hostname", &v, &mut text);
        }
        if let Some(p) = &path {
            let v = q(p, rng);
            item("path", &v, &mut text);
        }
        let v = q(&svc, rng);
        item("service", &v, &mut text);
        if rng.chance(1, 3) {
            text.push_str(&format!("{ind}  originRequest:\n{ind}    noTLSVerify: true\n{ind}    access:\n{ind}      - team\n"));
        }
        if rng.chance(1, 4) {
            text.push_str("\n# comment\n");
        }
        want.push((host, path, svc));
    }
    if rng.chance(1, 2) {
        text.push_str("warp-routing:\n  enabled: false\n");
    }
    let rules = tunnel_lint::parse(&text).map_err(|e| format!("parse failed: {e}\n{text}"))?;
    let got: Vec<_> = rules
        .into_iter()
        .map(|r| (r.hostname, r.path, r.service))
        .collect();
    ensure(got == want, || {
        format!("ingress round-trip differs\n{text}")
    })?;
    Ok(n as u64)
}

pub fn targets() -> Vec<Target> {
    vec![
        target_ss(),
        target_proc_net(),
        target_journal(),
        target_cloudflared(),
        target_event(),
        target_rfc3339(),
        target_http(),
        target_ingress(),
    ]
}

pub fn gens() -> Vec<Gen> {
    vec![
        Gen {
            name: "gen-proc-net",
            run: gen_proc_net,
        },
        Gen {
            name: "gen-ss",
            run: gen_ss,
        },
        Gen {
            name: "gen-journal",
            run: gen_journal,
        },
        Gen {
            name: "gen-cloudflared",
            run: gen_cloudflared,
        },
        Gen {
            name: "gen-event",
            run: gen_event,
        },
        Gen {
            name: "gen-rfc3339",
            run: gen_rfc3339,
        },
        Gen {
            name: "gen-ingress",
            run: gen_ingress,
        },
    ]
}
