// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.6 replay: the `ingress-lint` sensor over real VPS data captured
//! read-only on 2026-10-21 08:12-08:13 +02:00 (fixtures/ingress-lint):
//! a redacted copy of the live cloudflared config, `ss -ltn`, and the
//! per-port refused-dial counts of the last hour of the cloudflared journal.
//! Deterministic: the sensor gets the config as text and the sockets as the
//! nerve's listener list; nothing on this box is read or dialled.

use openmind_nerve::cortex::{Cortex, Input};
use openmind_nerve::incident::Sev;
use openmind_nerve::logscan::{self, LogInsights};
use openmind_nerve::probe::{parse_ss, JLine};
use openmind_nerve::sensors::ingress_lint::{IngressLint, NAME};
use openmind_nerve::sensors::Sensors;
use openmind_schema::NerveEvent;
use std::path::PathBuf;
use std::time::Instant;

/// 2026-10-21 08:12:04 +02:00.
const NOW: u64 = 1_792_563_124;

fn fx(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/ingress-lint")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Journal lines rebuilt from the per-port counts (same message shapes the
/// cloudflared journal has; no hostnames or client addresses).
fn journal() -> Vec<JLine> {
    let mut v = vec![];
    for l in fx("vps-refused-20261021.txt").lines() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        let c: Vec<u64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
        let (port, total, failed) = (c[0], c[1], c[2]);
        for k in 0..total {
            let msg = if k < failed {
                format!("ERR  Request failed error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp 127.0.0.1:{port}: connect: connection refused\" originService=http://127.0.0.1:{port}")
            } else {
                format!("ERR  error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp 127.0.0.1:{port}: connect: connection refused\" originService=http://127.0.0.1:{port}")
            };
            v.push(JLine {
                t: NOW - 3600 + k % 3600,
                unit: "cloudflared.service".into(),
                prio: 3,
                msg,
            });
        }
    }
    v
}

fn run(logs: &LogInsights) -> (Cortex, u64) {
    let ls = parse_ss(&fx("vps-ss-20261021.txt"));
    let mut reg = Sensors::empty();
    reg.add(IngressLint::from_text(&fx("vps-config-20261021.yml")));
    let mut i = Input::new(NOW, &[], logs);
    i.listeners = Some(&ls);
    let t = Instant::now();
    let cx = reg.run(&i);
    (cx, t.elapsed().as_micros() as u64)
}

fn closed_ports(cx: &Cortex) -> Vec<u16> {
    let mut v: Vec<u16> = cx
        .bleeding
        .iter()
        .filter_map(|f| f.id.strip_prefix("ingress-closed-")?.parse().ok())
        .collect();
    v.sort_unstable();
    v
}

const CLOSED: [u16; 21] = [
    3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333, 4000, 4001, 4003, 4004, 4020, 4042, 4050, 4060,
    4444, 5530, 7681, 8000, 8889,
];
const WITH_TRAFFIC: [u16; 12] = [
    3004, 3005, 3010, 3011, 3020, 4000, 4001, 4003, 4020, 4042, 4060, 4444,
];

#[test]
fn replay_live_config_closed_ports_match_tunnel_lint() {
    let logs = LogInsights::default();
    let (cx, us) = run(&logs);
    let got = closed_ports(&cx);
    assert_eq!(got, CLOSED);
    // Cross-check: the tunnel-lint CLI path (SsSnapshot) on the same capture.
    let rules = tunnel_lint::parse(&fx("vps-config-20261021.yml")).unwrap();
    let snap = tunnel_lint::SsSnapshot::parse(&fx("vps-ss-20261021.txt"));
    let rep = tunnel_lint::lint(&rules, &snap);
    assert_eq!(rep.closed_ports(), got);
    assert_eq!(rules.len(), 39);
    let routes: usize = cx
        .bleeding
        .iter()
        .filter(|f| f.id.starts_with("ingress-closed-"))
        .map(|f| {
            f.evidence
                .iter()
                .filter(|e| e.starts_with("rule #"))
                .count()
        })
        .sum();
    assert_eq!(routes, rep.closed().count());
    assert_eq!(routes, 33);
    // Without journal evidence every closed port is a warning.
    assert!(cx
        .bleeding
        .iter()
        .filter(|f| f.id.starts_with("ingress-closed-"))
        .all(|f| f.sev == Sev::Warn));
    // Shape: the live config is well formed (catch-all last, no dupes).
    let ids: Vec<&str> = cx.findings().map(|f| f.id.as_str()).collect();
    for bad in [
        "ingress-no-catch-all",
        "ingress-invalid-origin",
        "ingress-shadowed",
        "ingress-config",
    ] {
        assert!(!ids.contains(&bad), "{bad} in {ids:?}");
    }
    assert!(cx.findings().all(|f| f.source == NAME));
    eprintln!(
        "replay: 39 rules, {routes} closed routes on {} ports, sense {us} us",
        got.len()
    );
}

#[test]
fn replay_journal_escalates_ports_with_failing_requests() {
    let lines = journal();
    let logs = logscan::insights(&lines);
    let mut busy: Vec<u16> = logs.dead_origins.keys().copied().collect();
    busy.sort_unstable();
    assert_eq!(busy, WITH_TRAFFIC);
    let (cx, _) = run(&logs);
    let mut err = vec![];
    let mut warn = vec![];
    for f in cx
        .bleeding
        .iter()
        .filter(|f| f.id.starts_with("ingress-closed-"))
    {
        let p: u16 = f.id["ingress-closed-".len()..].parse().unwrap();
        match f.sev {
            Sev::Error => err.push(p),
            Sev::Warn => warn.push(p),
            s => panic!("{p}: {s:?}"),
        }
    }
    err.sort_unstable();
    warn.sort_unstable();
    assert_eq!(err, WITH_TRAFFIC);
    assert_eq!(warn, [3015, 3080, 3333, 4004, 4050, 5530, 7681, 8000, 8889]);
    let f = cx
        .bleeding
        .iter()
        .find(|f| f.id == "ingress-closed-4001")
        .unwrap();
    assert!(
        f.evidence
            .iter()
            .any(|e| e.contains("1040 refused dial(s), 520 failed request(s)")),
        "{:?}",
        f.evidence
    );
    // Error findings sort first.
    assert_eq!(cx.bleeding[0].sev, Sev::Error);
    eprintln!(
        "journal: {} lines, error ports {err:?}, warn ports {warn:?}",
        lines.len()
    );
}

#[test]
fn replay_events_are_b33_and_leak_nothing() {
    let logs = logscan::insights(&journal());
    let (cx, _) = run(&logs);
    let ev = cx.nerve_events(NOW);
    assert_eq!(ev.len(), 1, "one event per (source, kind): {ev:?}");
    let e: NerveEvent = serde_json::from_value(ev[0].clone()).expect("B33");
    assert_eq!(e.source, NAME);
    let txt = serde_json::to_string(&ev).unwrap();
    for leak in [
        "galion.app",
        "slaviknomad",
        ".com",
        "127.0.0.1",
        "REDACTED",
        "credentials",
    ] {
        assert!(!txt.contains(leak), "{leak} leaked into events: {txt}");
    }
    assert_eq!(ev[0]["severity"], "high");
    assert_eq!(ev[0]["ts"], "2026-10-21T08:12:04+02:00");
    // Titles carry ports and counts only.
    for f in cx.findings() {
        assert!(
            !f.title.contains("galion") && !f.title.contains(".com"),
            "{}",
            f.title
        );
    }
    // The loopback API JSON never carries the redacted credentials keys.
    let api = serde_json::to_string(&cx.to_json()).unwrap();
    assert!(!api.contains("credentials-file") && !api.contains("REDACTED"));
}

#[test]
fn path_sensor_reads_nothing_in_replay() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ingress-lint");
    let p = dir.join("vps-config-20261021.yml");
    let env = |k: &str| (k == "NERVE_INGRESS_LINT_CONFIG").then(|| p.display().to_string());
    let mut reg = Sensors::from_env(&env);
    assert_eq!(reg.names().last(), Some(&NAME));
    let ls = parse_ss(&fx("vps-ss-20261021.txt"));
    let logs = LogInsights::default();
    let mut i = Input::new(NOW, &[], &logs);
    i.listeners = Some(&ls);
    let cx = reg.run(&i);
    assert!(
        !cx.findings().any(|f| f.source == NAME),
        "not live: the configured file is not read"
    );
    // Off by default: no env, no sensor.
    assert!(!Sensors::from_env(&|_| None).names().contains(&NAME));
}
