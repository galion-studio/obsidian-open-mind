// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.4: fake-clock replay over the 2026-10-25 fall-back
//! (02:59:59 CEST +02:00 -> 02:00:00 CET +01:00, unix 1_792_890_000).
//!
//! Drives `Engine::cycle_with` every 30 s from 2026-10-24T23:00:00+02:00 to
//! 2026-10-25T04:00:00+01:00 (6 h of real time, 5 h on the wall clock) with
//! a healthy VPS-shaped stack: steady memory with noise, no restarts, steady
//! tunnel and access-log traffic whose log stamps carry the local offset
//! (+0200, then +0100). Checks:
//!
//! - self-eval / state stamps switch offset exactly at the fold and parse
//!   back to the cycle instant (strictly increasing, never `Z`);
//! - the 2 h windows (240-sample history, 720 x 10 s access buckets) span
//!   real seconds across the fold;
//! - restart, memory-trend, error-rate and rate (DDoS) findings give no false
//!   alarm, and the score does not move;
//! - every timestamped self-eval file survives (the pre-fix name had no
//!   offset and the CET hour overwrote the CEST one);
//! - positive controls: a real leak, a real restart and a real flood inside
//!   the repeated hour are still found, with windows measured in real time.

use openmind_nerve::access::{parse_line, Hit};
use openmind_nerve::collect::Collected;
use openmind_nerve::config::Config;
use openmind_nerve::engine::Engine;
use openmind_nerve::probe::JLine;
use openmind_nerve::state::Sample;
use openmind_nerve::util::{parse_rfc3339, ts};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 2026-10-25T03:00:00+02:00 == 2026-10-25T02:00:00+01:00.
const FOLD: u64 = 1_792_890_000;
/// 2026-10-24T23:00:00+02:00.
const T0: u64 = FOLD - 4 * 3600;
/// 2026-10-25T04:00:00+01:00.
const T_END: u64 = FOLD + 2 * 3600;
const STEP: u64 = 30;

const SERVICES: &[(&str, &str)] = &[
    ("openmind-memory", "openmind-memory.service"),
    ("hermes-rs", "hermes-rs.service"),
    ("link-watch-rs", "link-watch-rs.service"),
    ("galion-desk", "galion-desk.service"),
    ("tuner", "tuner.service"),
    ("galion-one", "galion-one.service"),
    ("console", "console.service"),
    ("galion-health", "galion-health.service"),
];

/// What goes wrong in a scenario (all `None`: a quiet night).
#[derive(Default, Clone, Copy)]
struct Faults {
    /// hermes-rs memory grows 3 % per cycle from this instant.
    leak_from: Option<u64>,
    /// tuner restarts (new pid, NRestarts + 1) at this instant.
    restart_at: Option<u64>,
    /// tuner access log floods (40x) for 120 s from this instant.
    flood_from: Option<u64>,
}

/// Per-cycle view kept for the assertions.
struct Cycle {
    t: u64,
    pass: bool,
    errors: u64,
    report_ts: String,
    state_ts: String,
    score: u64,
    finding_ids: BTreeSet<String>,
    incidents: usize,
    ddos: Vec<(String, &'static str)>,
    restarts_1h: Vec<(String, usize)>,
    reasons: Vec<String>,
    wrote_report: bool,
}

/// Deterministic noise in [0, m).
fn noise(t: u64, salt: u64, m: u64) -> u64 {
    (t.wrapping_mul(2_654_435_761).wrapping_add(salt * 40_503) >> 7) % m
}

/// `2026-10-25T02:30:00+01:00` -> `25/Oct/2026:02:30:00 +0100` (nginx CLF).
fn clf(t: u64) -> String {
    let s = ts(t);
    let mon = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][s[5..7].parse::<usize>().unwrap() - 1];
    format!(
        "{}/{mon}/{}:{} {}",
        &s[8..10],
        &s[0..4],
        &s[11..19],
        s[19..].replace(':', "")
    )
}

fn samples(t: u64, f: &Faults, k: u64) -> std::collections::HashMap<String, Sample> {
    let mut m = std::collections::HashMap::new();
    for (i, (name, _unit)) in SERVICES.iter().enumerate() {
        let i = i as u64;
        let mut mem = 40_000 + i * 5_000 + noise(t, i, 600);
        let (mut pid, mut nr, mut enter) = (1000 + i as u32, 0u32, 5_000_000u64 + i);
        if *name == "hermes-rs" {
            if let Some(l) = f.leak_from.filter(|l| t >= *l) {
                let n = (t - l) / STEP;
                mem = (mem as f64 * 1.03f64.powi(n as i32)) as u64;
            }
        }
        if *name == "tuner" && f.restart_at.is_some_and(|r| t >= r) {
            pid += 50_000;
            nr += 1;
            enter += 999;
        }
        m.insert(
            name.to_string(),
            Sample {
                t,
                present: true,
                unit_active: Some("active".into()),
                unit_sub: Some("running".into()),
                unit_result: Some("success".into()),
                file_state: Some("enabled".into()),
                http_code: if *name == "link-watch-rs" {
                    None
                } else {
                    Some(200)
                },
                latency_ms: Some(3 + noise(t, i + 20, 4)),
                mem_kb: Some(mem),
                rss_kb: Some(mem),
                cpu_nsec: Some(k * 50_000_000 + i),
                pid,
                n_restarts: nr,
                active_enter: enter,
                extra: json!({}),
                ..Default::default()
            },
        );
    }
    // link-watch-rs has no port: no HTTP code expected.
    for (name, req) in [("cloudflared", 40u64), ("tuner-quick", 2)] {
        m.insert(
            name.to_string(),
            Sample {
                t,
                present: true,
                unit_active: Some("active".into()),
                unit_sub: Some("running".into()),
                pid: 777 + req as u32,
                active_enter: 4_000_000 + req,
                mem_kb: Some(30_000 + noise(t, req, 300)),
                tunnel_conns: Some(4),
                tunnel_requests: Some(10_000 + k * req + noise(t, 9, 3)),
                tunnel_req_errors: Some(100),
                extra: json!({}),
                ..Default::default()
            },
        );
    }
    m.insert(
        "galion-one-mouth".into(),
        Sample {
            t,
            present: true,
            http_code: Some(200),
            latency_ms: Some(5 + noise(t, 30, 3)),
            extra: json!({}),
            ..Default::default()
        },
    );
    // T2.3 (slot 200) observe-only services, healthy as on the VPS.
    for (i, name) in ["openmind-queue", "tanga-proto", "galion-nerve-shadow"]
        .iter()
        .enumerate()
    {
        let i = 40 + i as u64;
        m.insert(
            name.to_string(),
            Sample {
                t,
                present: true,
                unit_active: Some("active".into()),
                unit_sub: Some("running".into()),
                http_code: Some(200),
                latency_ms: Some(1 + noise(t, i, 2)),
                mem_kb: Some(4_000 + noise(t, i, 50)),
                pid: 3000 + i as u32,
                active_enter: 6_000_000 + i,
                extra: json!({}),
                ..Default::default()
            },
        );
    }
    // galion-backup.timer: waiting, last run 6 h-ish ago at most (wall clock).
    m.insert(
        "galion-backup".into(),
        Sample {
            t,
            present: true,
            unit_active: Some("active".into()),
            unit_sub: Some("waiting".into()),
            extra: json!({"last_run": {"unit": "galion-backup.service", "active": "inactive",
                "result": "success", "exit_status": 0, "finished_unix": t - t % 21_600}}),
            ..Default::default()
        },
    );
    // Not deployed on the VPS: absent, as the live probes report them.
    for name in ["voice-worker", "gaia", "hades-gate"] {
        m.insert(
            name.to_string(),
            Sample {
                t,
                present: false,
                extra: json!({}),
                ..Default::default()
            },
        );
    }
    m.insert(
        "lxc-host".into(),
        Sample {
            t,
            present: true,
            disk_total_kb: Some(100_000_000),
            disk_avail_kb: Some(60_000_000 + noise(t, 3, 2_000)),
            ram_total_kb: Some(16_000_000),
            ram_avail_kb: Some(9_000_000 + noise(t, 4, 50_000)),
            load1: Some(0.4),
            extra: json!({}),
            ..Default::default()
        },
    );
    m.insert(
        "galion-nerve".into(),
        Sample {
            t,
            present: true,
            mem_kb: Some(3_100 + noise(t, 5, 40)),
            rss_kb: Some(3_100 + noise(t, 5, 40)),
            extra: json!({}),
            ..Default::default()
        },
    );
    m
}

/// Access-log hits for (t - STEP, t]: ~2 rps with jitter, as CLF lines with
/// the Zagreb offset of each hit's own instant, plus JSON lines with RFC 3339
/// stamps, parsed by the production parser.
fn hits(t: u64, f: &Faults) -> Vec<(String, Option<String>, Hit)> {
    let mut v = vec![];
    for s in (t - STEP + 1)..=t {
        let mut n = 1 + noise(s, 7, 3); // 1..3 per second
        if f.flood_from.is_some_and(|ff| s >= ff && s < ff + 120) {
            n *= 40;
        }
        for j in 0..n {
            let line = if j % 2 == 0 {
                format!(
                    "203.0.113.{} - - [{}] \"GET /api/tune HTTP/1.1\" 200 512 \"-\" \"probe\"", // repo-check: allow (TEST-NET-3 fixture, never dialled)
                    1 + (s + j) % 200,
                    clf(s)
                )
            } else {
                json!({"ts": ts(s), "ip": format!("198.51.100.{}", 1 + (s + j) % 200), "path": "/", "status": 200}) // repo-check: allow (TEST-NET-2 fixture)
                    .to_string()
            };
            // `now` far away: a stamp the parser cannot read would land there.
            let h = parse_line(&line, 1).expect("access line parses");
            assert_eq!(h.t, s, "stamp {line} parsed to {}", h.t);
            v.push(("tuner".to_string(), Some("tuner".to_string()), h));
        }
    }
    v
}

fn journal(t: u64) -> Vec<JLine> {
    (0..3)
        .map(|j| JLine {
            t: t - STEP + 1 + j * 10,
            unit: "cloudflared.service".into(),
            prio: 6,
            msg: format!("{} INF Updated to new configuration version=7", ts(t)),
        })
        .collect()
}

fn data_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-dst-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn engine(dir: &Path) -> Engine {
    let d = dir.display().to_string();
    let cfg = Config::from_env(&|k| match k {
        "NERVE_DATA_DIR" => Some(d.clone()),
        "NERVE_REFLEX" => Some("0".into()),
        _ => None,
    })
    .unwrap();
    let mut e = Engine::new(cfg).unwrap();
    e.reflexes_enabled = false;
    e.live = false;
    e.write_files = true;
    e.alerts.path = None;
    e.alerts.to_journal = false;
    e
}

fn run(tag: &str, f: Faults) -> (Vec<Cycle>, Engine, PathBuf) {
    let dir = data_dir(tag);
    let mut e = engine(&dir);
    let mut out = vec![];
    let mut k = 0u64;
    let mut t = T0;
    while t <= T_END {
        let col = Collected {
            samples: samples(t, &f, k),
            lines: journal(t),
            ..Default::default()
        };
        let wrote = e.cycle_with(col, hits(t, &f), t, Instant::now());
        let sh = e.shared.lock().unwrap();
        let r = &sh.report;
        let ids = r["cortex"]
            .as_object()
            .into_iter()
            .flat_map(|o| o.values())
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|x| x["id"].as_str().map(str::to_string))
            .collect();
        let c = Cycle {
            t,
            pass: r["pass"].as_bool().unwrap_or(false),
            errors: r["errors"].as_u64().unwrap_or(99),
            report_ts: r["ts"].as_str().unwrap_or("").to_string(),
            state_ts: sh.state["ts"].as_str().unwrap_or("").to_string(),
            score: r["score"].as_u64().unwrap_or(0),
            finding_ids: ids,
            incidents: r["incidents"].as_array().map_or(0, Vec::len),
            ddos: e.ddos.iter().map(|d| (d.stream.clone(), d.level)).collect(),
            restarts_1h: e
                .comps
                .iter()
                .map(|c| (c.spec.name.to_string(), c.restarts_1h(t)))
                .collect(),
            reasons: e.comps.iter().flat_map(|c| c.reasons.clone()).collect(),
            wrote_report: wrote,
        };
        drop(sh);
        out.push(c);
        k += 1;
        t += STEP;
    }
    (out, e, dir)
}

/// Unix second for a Zagreb wall-clock stamp with its offset.
fn at(s: &str) -> u64 {
    parse_rfc3339(s).unwrap()
}

#[test]
fn quiet_night_over_the_fold_raises_nothing_and_stamps_switch_offset() {
    let (cy, e, dir) = run("quiet", Faults::default());
    assert_eq!(cy.len() as u64, (T_END - T0) / STEP + 1);
    assert_eq!(cy.len(), 721);

    // 1. Stamps: Zagreb offset by instant, round-trip, strictly increasing.
    let mut prev = 0u64;
    let mut saw_fold_twice = 0;
    for c in &cy {
        assert_eq!(c.report_ts, ts(c.t));
        assert_eq!(c.state_ts, c.report_ts);
        assert_eq!(zagreb_time::require_rfc3339(&c.report_ts), Ok(()));
        let want = if c.t < FOLD { "+02:00" } else { "+01:00" };
        assert!(c.report_ts.ends_with(want), "{} at {}", c.report_ts, c.t);
        let back = parse_rfc3339(&c.report_ts).unwrap();
        assert_eq!(back, c.t);
        assert!(back > prev, "not increasing: {}", c.report_ts);
        prev = back;
        if c.report_ts.starts_with("2026-10-25T02:30:00") {
            saw_fold_twice += 1;
        }
    }
    assert_eq!(
        saw_fold_twice, 2,
        "02:30 must occur once as CEST and once as CET"
    );
    let last_cest = cy.iter().rfind(|c| c.t < FOLD).unwrap();
    let first_cet = cy.iter().find(|c| c.t >= FOLD).unwrap();
    assert_eq!(last_cest.report_ts, "2026-10-25T02:59:30+02:00");
    assert_eq!(first_cet.report_ts, "2026-10-25T02:00:00+01:00");
    assert_eq!(first_cet.t - last_cest.t, STEP);

    // 2. No false alarm after a 2 h warm-up (full 240-sample history),
    //    straight through the repeated hour.
    let warm = T0 + 2 * 3600;
    let base = cy.iter().find(|c| c.t >= warm).unwrap();
    for c in cy.iter().filter(|c| c.t >= warm) {
        assert_eq!(c.incidents, 0, "incident at {}", c.report_ts);
        assert!(
            c.pass && c.errors == 0,
            "self-eval not pass at {}",
            c.report_ts
        );
        assert_eq!(c.score, base.score, "score moved at {}", c.report_ts);
        assert_eq!(
            c.finding_ids, base.finding_ids,
            "findings changed at {}",
            c.report_ts
        );
        for id in &c.finding_ids {
            for bad in [
                "restarts-",
                "memory-trend-",
                "error-rate-",
                "host-pressure",
                "ddos",
                "flood",
            ] {
                assert!(!id.starts_with(bad), "false alarm {id} at {}", c.report_ts);
            }
        }
        for (s, l) in &c.ddos {
            assert_eq!(*l, "normal", "stream {s} {l} at {}", c.report_ts);
        }
        for (n, r) in &c.restarts_1h {
            assert_eq!(*r, 0, "{n} restarts_1h at {}", c.report_ts);
        }
        for r in &c.reasons {
            assert!(
                !(r.contains("restart")
                    || r.contains("memory rising")
                    || r.contains("errors in 5 min")),
                "reason {r} at {}",
                c.report_ts
            );
        }
    }
    assert_eq!(
        base.score, 100,
        "healthy synthetic stack scores 100: {:?}",
        base.finding_ids
    );
    assert!(e.alerts.recent.is_empty(), "alerts: {:?}", e.alerts.recent);

    // 3. The 2 h windows are 2 h of real seconds at the end (they straddle
    //    the fold: 02:00+02:00 .. 04:00+01:00 on the wall clock is 3 h).
    for c in &e.comps {
        if c.hist.len() < 240 {
            continue;
        }
        let span = c.hist.back().unwrap().t - c.hist.front().unwrap().t;
        assert_eq!(span, 239 * STEP, "{}", c.spec.name);
    }
    let host = e.comps.iter().find(|c| c.spec.name == "lxc-host").unwrap();
    assert_eq!(host.hist.len(), 240);
    assert_eq!(
        ts(host.hist.front().unwrap().t),
        "2026-10-25T02:00:30+01:00"
    );
    let st = e.shared.lock().unwrap();
    let tuner = st.detector.streams.get("tuner").expect("tuner stream");
    let v = tuner.window();
    assert_eq!(v.0, 720, "2 h of 10 s buckets kept");
    assert_eq!(v.2 - v.1, 719 * 10, "bucket span is real seconds");
    drop(st);

    // 4. Every timestamped self-eval report is on disk with a unique name.
    let reports = cy.iter().filter(|c| c.wrote_report).count();
    assert_eq!(reports, 1 + 721 / 20);
    let names: Vec<String> = std::fs::read_dir(dir.join("self-eval"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("self-eval-"))
        .collect();
    assert_eq!(names.len(), reports, "a report was overwritten: {names:?}");
    for n in &names {
        let t = openmind_nerve::engine::self_eval_file_instant(n).expect(n);
        assert!(cy.iter().any(|c| c.wrote_report && c.t == t), "{n}");
    }
    // Reports land at xx:x9:30; the repeated hour holds six CEST and six CET
    // reports with the same wall-clock names apart from the offset.
    let fold_hour: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with("self-eval-20261025T02"))
        .collect();
    assert_eq!(fold_hour.len(), 12, "{fold_hour:?}");
    assert!(names
        .iter()
        .any(|n| n == "self-eval-20261025T020930+0200.json"));
    assert!(names
        .iter()
        .any(|n| n == "self-eval-20261025T020930+0100.json"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn leak_inside_the_repeated_hour_is_found_as_fast_as_on_a_normal_night() {
    // Same leak three times: an ordinary hour (control), the CEST pass of
    // 02:45 and the CET pass of 02:45. Detection delay is measured in real
    // seconds and must not depend on the wall clock.
    let starts = [
        at("2026-10-25T01:15:00+02:00"),
        at("2026-10-25T02:45:00+02:00"),
        at("2026-10-25T02:45:00+01:00"),
    ];
    let mut delays = vec![];
    for (n, leak) in starts.iter().enumerate() {
        let (cy, _e, dir) = run(
            &format!("leak{n}"),
            Faults {
                leak_from: Some(*leak),
                ..Default::default()
            },
        );
        assert!(cy
            .iter()
            .filter(|c| c.t < *leak)
            .all(|c| !c.finding_ids.iter().any(|i| i.starts_with("memory-trend-"))));
        let hit = cy
            .iter()
            .find(|c| c.finding_ids.contains("memory-trend-hermes-rs"))
            .expect("leak found");
        let r = hit
            .reasons
            .iter()
            .find(|r| r.contains("memory rising"))
            .unwrap();
        // "memory rising N% over M min": M is unix seconds / 60 and the
        // trend window is 30 min of the same pid.
        let mins: u64 = r
            .split(" over ")
            .nth(1)
            .and_then(|x| x.split(' ').next())
            .and_then(|x| x.parse().ok())
            .unwrap();
        assert!(mins <= 30, "{r}");
        assert!(
            hit.t - leak <= 1800,
            "found after {} s ({})",
            hit.t - leak,
            hit.report_ts
        );
        delays.push(hit.t - leak);
        let _ = std::fs::remove_dir_all(&dir);
    }
    eprintln!("leak detection delay (s): control/CEST/CET = {delays:?}");
    for d in &delays[1..] {
        assert!(d.abs_diff(delays[0]) <= 2 * STEP, "delays {delays:?}");
    }
}
#[test]
fn restart_in_the_cest_half_counts_for_exactly_one_real_hour() {
    let r = at("2026-10-25T02:50:00+02:00");
    let (cy, _e, dir) = run(
        "restart",
        Faults {
            restart_at: Some(r),
            ..Default::default()
        },
    );
    let tuner = |c: &Cycle| c.restarts_1h.iter().find(|x| x.0 == "tuner").unwrap().1;
    for c in &cy {
        let want = usize::from(c.t >= r && c.t <= r + 3600);
        assert_eq!(tuner(c), want, "tuner restarts_1h at {}", c.report_ts);
        let has = c.finding_ids.contains("restarts-tuner");
        assert_eq!(has, want == 1, "restarts-tuner at {}", c.report_ts);
    }
    // One real hour later is 02:50 CET: wall clock says 0 min passed.
    let gone = cy.iter().find(|c| c.t == r + 3600 + STEP).unwrap();
    assert_eq!(gone.report_ts, "2026-10-25T02:50:30+01:00");
    assert_eq!(tuner(gone), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flood_right_after_the_fold_is_flagged_and_clears() {
    let f0 = at("2026-10-25T02:10:00+01:00");
    let (cy, _e, dir) = run(
        "flood",
        Faults {
            flood_from: Some(f0),
            ..Default::default()
        },
    );
    let lvl = |c: &Cycle| {
        c.ddos
            .iter()
            .find(|x| x.0 == "tuner")
            .map(|x| x.1)
            .unwrap_or("none")
    };
    for c in cy.iter().filter(|c| c.t >= T0 + 2 * 3600 && c.t < f0) {
        assert_eq!(lvl(c), "normal", "{}", c.report_ts);
    }
    let fl = cy
        .iter()
        .find(|c| c.t > f0 && lvl(c) == "flood")
        .expect("flood flagged");
    eprintln!(
        "flood flagged at {} ({} s after start)",
        fl.report_ts,
        fl.t - f0
    );
    assert!(fl.t - f0 <= 60, "flagged {} s late", fl.t - f0);
    // 120 s burst + 60 s window: back to normal within ~3 min of real time.
    let calm = cy
        .iter()
        .find(|c| c.t > f0 + 120 && lvl(c) == "normal")
        .expect("flood cleared");
    assert!(calm.t - f0 <= 240, "cleared at {}", calm.report_ts);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn replay_dir_orders_fold_captures_by_instant_not_by_name() {
    // Captures named by wall clock without offset (as `date +%H%M` would):
    // by name 0210, 0220, 0240, 0250; by instant 0240+02, 0250+02, 0210+01,
    // 0220+01.
    let dir = data_dir("replaydir");
    std::fs::create_dir_all(&dir).unwrap();
    let stamps = [
        "2026-10-25T02:40:00+02:00",
        "2026-10-25T02:50:00+02:00",
        "2026-10-25T02:10:00+01:00",
        "2026-10-25T02:20:00+01:00",
    ];
    for s in stamps {
        let comps: Vec<Value> = ["openmind-memory", "lxc-host", "galion-nerve"]
            .iter()
            .map(|n| json!({"name": n, "status": "healthy", "unit_state": "active", "http": 200, "mem_kb": 1000, "extra": {}}))
            .collect();
        let name = format!("state-{}.json", s[11..16].replace(':', ""));
        std::fs::write(
            dir.join(name),
            json!({"ts": s, "components": comps, "probe_errors": []}).to_string(),
        )
        .unwrap();
    }
    let d = dir.join("data").display().to_string();
    let cfg = Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| d.clone())).unwrap();
    let out = openmind_nerve::replay::replay_dir(&dir, cfg).unwrap();
    assert_eq!(out.cycles, 4);
    assert!(
        out.notes[0].contains("2026-10-25T02:40:00+02:00 .. 2026-10-25T02:20:00+01:00"),
        "{}",
        out.notes[0]
    );
    assert_eq!(out.engine.now, at("2026-10-25T02:20:00+01:00"));
    let sh = out.engine.shared.lock().unwrap();
    assert_eq!(sh.report["ts"], "2026-10-25T02:20:00+01:00");
    drop(sh);
    let _ = std::fs::remove_dir_all(&dir);
}
