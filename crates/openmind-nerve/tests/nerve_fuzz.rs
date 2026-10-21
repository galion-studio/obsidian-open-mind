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

#[test]
fn regress_placeholder_seed_heads() {
    // Heads just under the limit are accepted at every segmentation.
    let ok = nerve::big_head(8192);
    nerve::check_http_bytes(ok.as_bytes()).unwrap();
}
