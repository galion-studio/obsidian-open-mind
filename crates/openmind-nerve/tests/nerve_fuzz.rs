// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.1 (wave-4 slot 212): fuzz/property targets for the live nerve
//! parsers (aa77864 lineage): `POST /signal` body, `POST /event` body, the
//! HTTP request reader, journald JSON, cloudflared metrics and log lines,
//! access-log lines; generative round trips; deep JSON on the server's
//! connection-thread stack. Harness: `tests/fuzz/mod.rs` (the cortex T5.2
//! layout), targets: `tests/fuzz/nerve.rs`.
//!
//! `cargo test -p openmind-nerve --test nerve_fuzz` runs a short fixed-seed
//! budget per target. 60 s smoke per target:
//! `CORTEX_FUZZ_SECS=60 cargo test --release -p openmind-nerve --test nerve_fuzz -- --ignored --nocapture --test-threads 1 nerve_fuzz_all_summary`
//! (also `CORTEX_FUZZ_ITERS`, `CORTEX_FUZZ_SEED`, `CORTEX_FUZZ_SEEDS=<dir>`,
//! `CORTEX_FUZZ_CRASH_DIR`). See docs/nerve/fuzzing.md.

mod fuzz;

use fuzz::nerve::{self, gens, targets};
use fuzz::{budget, run_gen, run_target, seed, Report};

fn finish(r: &Report) {
    eprintln!("{}", r.line());
    for (k, m, p) in &r.failures {
        eprintln!("  {k}: {m} (input: {p:?})");
    }
    assert!(
        r.ok(),
        "{}: {} panic(s), {} violation(s)",
        r.name,
        r.panics,
        r.violations
    );
}

fn mutation(name: &str, iters: u64) {
    let t = targets()
        .into_iter()
        .find(|t| t.name == name)
        .expect("target");
    finish(&run_target(&t, budget(iters), seed()));
}

fn generative(name: &str, iters: u64) {
    let g = gens().into_iter().find(|g| g.name == name).expect("gen");
    finish(&run_gen(&g, budget(iters), seed()));
}

#[test]
fn fuzz_signal_body() {
    mutation("signal-body", 4_000);
}
#[test]
fn fuzz_event_body() {
    mutation("event-body", 4_000);
}
#[test]
fn fuzz_http_read_req() {
    mutation("http-read-req", 1_500);
}
#[test]
fn fuzz_journald_json() {
    mutation("journald-json", 1_500);
}
#[test]
fn fuzz_cf_metrics() {
    mutation("cf-metrics", 4_000);
}
#[test]
fn fuzz_cf_log_lines() {
    mutation("cf-log-lines", 3_000);
}
#[test]
fn fuzz_access_line() {
    mutation("access-line", 4_000);
}
#[test]
fn prop_signal_roundtrip() {
    generative("signal-roundtrip", 2_000);
}
#[test]
fn prop_metrics_roundtrip() {
    generative("metrics-roundtrip", 2_000);
}
#[test]
fn prop_http_roundtrip() {
    generative("http-roundtrip", 300);
}
#[test]
fn prop_stamp_roundtrip() {
    generative("stamp-roundtrip", 5_000);
}
#[test]
fn prop_deep_json_stack() {
    generative("deep-json-stack", 300);
}

/// Every target and generator once with one summary table (the shape the
/// recorded box runs use; `--ignored` with `CORTEX_FUZZ_SECS=60`).
#[test]
#[ignore]
fn nerve_fuzz_all_summary() {
    let s = seed();
    let mut reports = vec![];
    for t in targets() {
        reports.push(run_target(&t, budget(20_000), s));
        eprintln!("{}", reports.last().unwrap().line());
    }
    for g in gens() {
        reports.push(run_gen(&g, budget(5_000), s));
        eprintln!("{}", reports.last().unwrap().line());
    }
    let execs: u64 = reports.iter().map(|r| r.execs).sum();
    let bad: Vec<&Report> = reports.iter().filter(|r| !r.ok()).collect();
    eprintln!(
        "TOTAL execs {execs}, targets {}, failing {}",
        reports.len(),
        bad.len()
    );
    for r in &bad {
        for (k, m, p) in &r.failures {
            eprintln!("  {} {k}: {m} (input: {p:?})", r.name);
        }
    }
    assert!(bad.is_empty());
}

// ------------------------------------------------- regressions (fuzz finds)


/// Find 1 (read_req): a head of 8193..10240 bytes was accepted when it
/// arrived in one 2 KiB-aligned burst and refused (431) when it came in
/// small TCP segments. Now the limit is on the head itself, both ways.
#[test]
fn regress_read_req_head_limit_is_segmentation_independent() {
    use openmind_nerve::server::{read_req, MAX_HEAD};
    for total in [MAX_HEAD - 1, MAX_HEAD, MAX_HEAD + 1, MAX_HEAD + 5, 10_000, 10_240] {
        let raw = nerve::big_head(total);
        nerve::check_http_bytes(raw.as_bytes()).unwrap();
        let r = read_req(&mut raw.as_bytes());
        if total <= MAX_HEAD {
            assert_eq!(r.ok().map(|q| q.body), Some("{}".to_string()), "{total}");
        } else {
            assert_eq!(r.err().map(|e| e.0), Some(431), "{total}");
        }
    }
}

/// Find 2 (cloudflared metrics): `inf`, `1e20` became u64::MAX and `-1` /
/// `NaN` became 0 requests; such values are "unknown" (None) now.
#[test]
fn regress_cloudflared_metrics_reject_non_finite_and_negative() {
    use openmind_nerve::probe::parse_cloudflared_metrics as p;
    for bad in ["+Inf", "inf", "NaN", "-1", "-0.5", "1e20", "18446744073709551616"] {
        let m = p(&format!(
            "cloudflared_tunnel_total_requests {bad}\ncloudflared_tunnel_request_errors {bad}\ncloudflared_tunnel_ha_connections {bad}\n"
        ));
        assert_eq!(
            (m.total_requests, m.request_errors, m.ha_connections),
            (None, None, None),
            "{bad}"
        );
    }
    let m = p("cloudflared_tunnel_total_requests 1.4565376e+07\ncloudflared_tunnel_ha_connections 4\ncloudflared_tunnel_request_errors 0\n");
    assert_eq!(
        (m.total_requests, m.ha_connections, m.request_errors),
        (Some(14_565_376), Some(4), Some(0))
    );
}

/// Find 3 (access JSON lines): `"status":70000` wrapped to 4464 (`as u16`);
/// a status that does not fit is 0 (unknown) now.
#[test]
fn regress_access_status_does_not_wrap() {
    use openmind_nerve::access::parse_line;
    let h = parse_line(r#"{"ts":1792514105,"path":"/","status":70000}"#, 0).unwrap();
    assert_eq!(h.status, 0);
    let h = parse_line(r#"{"ts":1792514105,"path":"/","status":"65536"}"#, 0).unwrap();
    assert_eq!(h.status, 0);
    let h = parse_line(r#"{"ts":1792514105,"path":"/","status":503}"#, 0).unwrap();
    assert_eq!(h.status, 503);
}

/// Deep JSON at serde_json's nesting limit on the server's 256 KiB
/// connection-thread stack: answered, no stack overflow (debug and release).
#[test]
fn regress_deep_json_fits_server_stack() {
    for depth in [1, 64, 126, 127, 128, 129, 200, 4096] {
        for obj in [false, true] {
            let (s, e) = nerve::deep_on_server_stack(depth, obj).unwrap();
            if depth < 120 {
                assert!(s && e, "depth {depth} obj {obj}");
            } else if depth > 130 {
                assert!(!s && !e, "depth {depth} obj {obj} should hit the recursion limit");
            }
        }
    }
}
