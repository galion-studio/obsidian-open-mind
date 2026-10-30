// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Hermetic checks of the S3.1 supervised loop (`gaia heal-tick --loop`)
//! with an injected tick: budgets, jitter, back-off, panics, stop file and
//! token-dir bookkeeping. Nothing is started; the nerve address is a closed
//! loopback port, so the loop's own started/stopped events are dropped. The
//! real loop against real dummies and the real nerve is real_gaia_loop.rs.

use gaia::heal_loop::{self, next_sleep, next_token, Jitter, LoopCfg, Stop, EXIT_ERRORS};
use gaia::live_heal::{HealCfg, HealTick};
use gaia_core::Json;
use gaia_live::heal::TestTarget;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gaia-heal-loop-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn closed_port() -> std::net::SocketAddr {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

/// Slot 436: the last port of the heal test range in force (default 19999;
/// `GAIA_TEST_PORTS` with `GALION_REAL=1` moves it into a slot block).
fn edge_port() -> u16 {
    *gaia_live::heal::test_ports()
        .unwrap_or_else(|e| panic!("{e}"))
        .end()
}

fn cfg(dir: &std::path::Path) -> LoopCfg {
    let port = edge_port();
    LoopCfg {
        tick: HealCfg {
            target: TestTarget {
                name: "gaia-dummy".into(),
                port,
                health_path: "/health".into(),
                argv: vec!["/bin/true".into(), "--port".into(), port.to_string()],
            },
            allowed: port..=port,
            nerve: closed_port(),
            state_dir: dir.join("state"),
            hades_config: dir.join("hades.toml"),
            token_file: None,
            wait: Duration::from_millis(200),
        },
        interval: Duration::from_millis(100),
        jitter: Duration::ZERO,
        max_ticks: Some(3),
        max_runtime: None,
        max_restarts: None,
        max_errors: 5,
        stop_file: None,
        token_dir: None,
        seed: Some(42),
    }
}

fn tick(outcome: &str, exit: u8, code: Option<&str>, spawned: bool) -> HealTick {
    HealTick {
        exit,
        report: Json::obj().with("outcome", outcome).with("detail", "fake"),
        approval_code: code.map(str::to_string),
        spawned,
    }
}

fn lines(out: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(out)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn sv(j: &Json) -> serde_json::Value {
    serde_json::from_str(&j.compact()).unwrap()
}

#[test]
fn stops_at_max_ticks_with_one_line_per_tick() {
    let d = tmp("ticks");
    let c = cfg(&d);
    let mut out = Vec::new();
    let mut n = 0;
    let r = heal_loop::run_with(&c, &mut out, &mut |_| {
        n += 1;
        Ok(tick("healthy", 0, None, false))
    });
    assert_eq!((r.exit, r.stop, n), (0, Stop::MaxTicks, 3));
    let l = lines(&out);
    assert_eq!(l.len(), 3);
    assert_eq!(l[2]["tick"], 3);
    assert_eq!(l[0]["outcome"], "healthy");
    assert!(
        l[0]["at"].as_str().unwrap().ends_with("+02:00")
            || l[0]["at"].as_str().unwrap().ends_with("+01:00")
    );
    assert_eq!(sv(&r.summary)["ticks"].as_u64(), Some(3));
    // Two sleeps between three ticks, each exactly the interval (no jitter).
    assert_eq!(sv(&r.summary)["sleeps_ms"], serde_json::json!([100, 100]));
}

#[test]
fn jitter_stays_inside_interval_plus_jitter_and_is_seeded() {
    let mut c = cfg(&tmp("jitter"));
    c.interval = Duration::from_millis(1000);
    c.jitter = Duration::from_millis(500);
    let mut a = Jitter::new(Some(7));
    let mut b = Jitter::new(Some(7));
    let mut other = Jitter::new(Some(8));
    let sa: Vec<u128> = (0..200)
        .map(|_| next_sleep(&c, 0, &mut a).as_millis())
        .collect();
    let sb: Vec<u128> = (0..200)
        .map(|_| next_sleep(&c, 0, &mut b).as_millis())
        .collect();
    let so: Vec<u128> = (0..200)
        .map(|_| next_sleep(&c, 0, &mut other).as_millis())
        .collect();
    assert_eq!(sa, sb, "same seed, same sleeps");
    assert_ne!(sa, so, "other seed, other sleeps");
    assert!(sa.iter().all(|s| (1000..=1500).contains(s)), "{sa:?}");
    let (lo, hi) = (sa.iter().min().unwrap(), sa.iter().max().unwrap());
    assert!(*lo < 1100 && *hi > 1400, "spread {lo}..{hi}");
    let distinct: std::collections::BTreeSet<_> = sa.iter().collect();
    assert!(distinct.len() > 100, "{}", distinct.len());
    // Unseeded jitter comes from the OS: two generators differ.
    let mut x = Jitter::new(None);
    let mut y = Jitter::new(None);
    assert_ne!(x.next_u64(), y.next_u64());
}

#[test]
fn backoff_doubles_and_is_capped() {
    let mut c = cfg(&tmp("backoff"));
    c.interval = Duration::from_secs(10);
    let mut r = Jitter::new(Some(1));
    let s: Vec<u64> = (0..8)
        .map(|e| next_sleep(&c, e, &mut r).as_secs())
        .collect();
    assert_eq!(s, vec![10, 10, 20, 40, 70, 70, 70, 70]);
}

#[test]
fn consecutive_errors_end_the_loop_with_exit_7_and_ok_resets_them() {
    let d = tmp("errors");
    let mut c = cfg(&d);
    c.max_ticks = Some(50);
    c.max_errors = 3;
    let mut out = Vec::new();
    let mut n = 0;
    // err, err, ok, err, err, err -> stop at tick 6 (the ok reset the count).
    let r = heal_loop::run_with(&c, &mut out, &mut |_| {
        n += 1;
        if n == 3 {
            Ok(tick("healthy", 0, None, false))
        } else {
            Err("nerve at 127.0.0.1:1 is not healthy".into())
        }
    });
    assert_eq!((r.exit, r.stop, n), (EXIT_ERRORS, Stop::Errors, 6));
    assert_eq!(sv(&r.summary)["errors_total"].as_u64(), Some(5));
    let l = lines(&out);
    assert_eq!(l[5]["consecutive_errors"], 3);
    assert_eq!(l[2]["outcome"], "healthy");
}

#[test]
fn a_panicking_tick_is_supervised_not_fatal() {
    let d = tmp("panic");
    let mut c = cfg(&d);
    c.max_ticks = Some(3);
    let mut out = Vec::new();
    let mut n = 0;
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = heal_loop::run_with(&c, &mut out, &mut |_| {
        n += 1;
        if n == 2 {
            panic!("boom in tick 2");
        }
        Ok(tick("healthy", 0, None, false))
    });
    std::panic::set_hook(prev);
    assert_eq!((r.exit, r.stop, n), (0, Stop::MaxTicks, 3));
    assert_eq!(sv(&r.summary)["panics"].as_u64(), Some(1));
    let l = lines(&out);
    assert!(l[1]["error"].as_str().unwrap().contains("boom in tick 2"));
    assert_eq!(l[2]["outcome"], "healthy");
}

#[test]
fn restart_budget_stops_the_loop() {
    let d = tmp("restarts");
    let mut c = cfg(&d);
    c.max_ticks = Some(100);
    c.max_restarts = Some(2);
    let mut out = Vec::new();
    let mut n = 0;
    let r = heal_loop::run_with(&c, &mut out, &mut |_| {
        n += 1;
        Ok(tick("restarted", 0, Some("approved"), n % 2 == 0))
    });
    assert_eq!((r.stop, n), (Stop::MaxRestarts, 4));
    assert_eq!(sv(&r.summary)["restarts"].as_u64(), Some(2));
}

#[test]
fn runtime_budget_stops_the_loop() {
    let d = tmp("runtime");
    let mut c = cfg(&d);
    c.max_ticks = None;
    c.max_runtime = Some(Duration::from_secs(1));
    c.interval = Duration::from_millis(300);
    let t0 = Instant::now();
    let r = heal_loop::run_with(&c, &mut Vec::new(), &mut |_| {
        Ok(tick("healthy", 0, None, false))
    });
    let el = t0.elapsed();
    assert_eq!(r.stop, Stop::MaxRuntime);
    assert!(
        el >= Duration::from_secs(1) && el < Duration::from_millis(1600),
        "{el:?}"
    );
    let t = sv(&r.summary)["ticks"].as_u64().unwrap();
    assert!((3..=5).contains(&t), "{t}");
}

#[test]
fn stop_file_ends_a_sleeping_loop_within_a_slice() {
    let d = tmp("stopfile");
    let mut c = cfg(&d);
    c.max_ticks = None;
    c.interval = Duration::from_secs(30);
    let stop = d.join("STOP");
    c.stop_file = Some(stop.clone());
    let s2 = stop.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        std::fs::write(s2, b"").unwrap();
        Instant::now()
    });
    let r = heal_loop::run_with(&c, &mut Vec::new(), &mut |_| {
        Ok(tick("healthy", 0, None, false))
    });
    let ended = Instant::now();
    let written = h.join().unwrap();
    assert_eq!(r.stop, Stop::StopFile);
    assert!(ended.duration_since(written) < Duration::from_millis(350));
    assert_eq!(sv(&r.summary)["ticks"].as_u64(), Some(1));
    // A present stop file means not even one tick.
    let r = heal_loop::run_with(&c, &mut Vec::new(), &mut |_| panic!("must not tick"));
    assert_eq!(
        (r.stop, sv(&r.summary)["ticks"].as_u64()),
        (Stop::StopFile, Some(0))
    );
}

#[test]
fn token_dir_hands_out_tokens_in_order_and_retires_used_ones() {
    let d = tmp("tokens");
    let td = d.join("tokens");
    std::fs::create_dir_all(&td).unwrap();
    for n in ["a.tok", "b.tok", "c.tok", "d.tok", "notes.txt"] {
        std::fs::write(td.join(n), "TOKEN-BODY-NEVER-PRINTED").unwrap();
    }
    assert_eq!(next_token(&td), Some(td.join("a.tok")));
    let mut c = cfg(&d);
    c.max_ticks = Some(5);
    c.token_dir = Some(td.clone());
    let mut seen = Vec::new();
    let mut n = 0;
    let codes = [
        Some("approved"),
        Some("no-token"),
        None,
        Some("replayed"),
        Some("approved"),
    ];
    let mut out = Vec::new();
    let _ = heal_loop::run_with(&c, &mut out, &mut |hc| {
        seen.push(
            hc.token_file
                .clone()
                .map(|p| p.file_name().unwrap().to_string_lossy().to_string()),
        );
        let code = codes[n];
        n += 1;
        Ok(tick("x", 0, code, code == Some("approved")))
    });
    // a spent; b kept twice (no-token, capped/healthy), then refused; c spent.
    assert_eq!(
        seen,
        ["a.tok", "b.tok", "b.tok", "b.tok", "c.tok"].map(|s| Some(s.to_string()))
    );
    let mut left: Vec<String> = std::fs::read_dir(&td)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "a.tok.spent",
            "b.tok.refused",
            "c.tok.spent",
            "d.tok",
            "notes.txt"
        ]
    );
    let l = lines(&out);
    assert_eq!(l[0]["token_moved"], "spent");
    assert_eq!(l[3]["token_moved"], "refused");
    assert!(l[1]["token_moved"].is_null());
    // The token file name is reported, its content never.
    assert!(!String::from_utf8_lossy(&out).contains("TOKEN-BODY"));
}

#[test]
fn parse_cli_reads_loop_flags_and_refuses_bad_ones() {
    let base = |extra: &[&'static str]| {
        let mut v = vec![
            "--loop",
            "--service",
            "gaia-dummy",
            "--port",
            "19601",
            "--allow-ports",
            "19600-19609",
            "--nerve",
            "127.0.0.1:20300",
            "--state",
            "/tmp/s",
            "--hades-config",
            "/tmp/h.toml",
        ];
        v.extend_from_slice(extra);
        v.extend_from_slice(&["--", "/bin/true", "--port", "19601"]);
        v
    };
    let c = heal_loop::parse_cli(&base(&[])).unwrap();
    assert_eq!(c.interval, Duration::from_secs(30));
    assert_eq!(
        (c.jitter, c.max_ticks, c.max_errors),
        (Duration::ZERO, None, 5)
    );
    let c = heal_loop::parse_cli(&base(&[
        "--interval-ms",
        "250",
        "--jitter-ms",
        "100",
        "--max-ticks",
        "9",
        "--max-runtime-s",
        "20",
        "--max-restarts",
        "4",
        "--max-errors",
        "2",
        "--stop-file",
        "/tmp/STOP",
        "--token-dir",
        "/tmp/tok",
        "--seed",
        "11",
    ]))
    .unwrap();
    assert_eq!(c.interval, Duration::from_millis(250));
    assert_eq!(c.jitter, Duration::from_millis(100));
    assert_eq!(
        (c.max_ticks, c.max_restarts, c.max_errors, c.seed),
        (Some(9), Some(4), 2, Some(11))
    );
    assert_eq!(c.max_runtime, Some(Duration::from_secs(20)));
    assert_eq!(c.token_dir, Some(PathBuf::from("/tmp/tok")));
    for bad in [
        &["--interval-ms", "5"][..],
        &["--interval-ms", "x"],
        &["--max-ticks", "0"],
        &["--max-errors", "0"],
        &["--jitter-ms", "999999999"],
        &["--token-dir", "/a", "--token-file", "/b"],
    ] {
        assert!(heal_loop::parse_cli(&base(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn an_out_of_scope_target_stops_before_the_first_tick() {
    let d = tmp("scope");
    let mut c = cfg(&d);
    c.tick.target.port = 8642;
    c.tick.target.argv = vec!["/bin/true".into(), "--port".into(), "8642".into()];
    c.tick.allowed = 1..=65535;
    let mut out = Vec::new();
    let r = heal_loop::run_with(&c, &mut out, &mut |_| panic!("must not tick"));
    assert_eq!((r.stop, r.exit), (Stop::Scope, 4));
    let s = sv(&r.summary);
    assert_eq!(s["ticks"], 0);
    assert!(s["last_error"]
        .as_str()
        .unwrap()
        .contains("outside the test range"));
    assert!(out.is_empty());
}
