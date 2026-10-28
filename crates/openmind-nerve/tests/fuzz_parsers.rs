// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex T5.2: fuzz and property targets for the cortex parsers that had
//! none (ss / proc-net-tcp, journald JSON, cloudflared `dest=` lines, the
//! `/event` JSONL feed and POST body, RFC 3339 stamps, tunnel ingress YAML).
//!
//! `cargo test -p openmind-nerve --test fuzz_parsers` runs a short,
//! reproducible budget per target (fixed seed). Longer box runs:
//! `CORTEX_FUZZ_SECS=60 cargo test --release -p openmind-nerve --test fuzz_parsers -- --nocapture --test-threads 1`
//! (also `CORTEX_FUZZ_ITERS`, `CORTEX_FUZZ_SEED`, `CORTEX_FUZZ_SEEDS=<dir of
//! captures>`). Every target prints one count line; panics and property
//! violations fail the test and save the input under `CORTEX_FUZZ_CRASH_DIR`
//! (default `$TMPDIR/cortex-fuzz-crashes`). See docs/cortex/fuzzing.md.

mod fuzz;

use fuzz::targets::{gens, targets};
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
fn fuzz_ss_ltn() {
    mutation("ss-ltn", 4000);
}
#[test]
fn fuzz_proc_net_tcp() {
    mutation("proc-net-tcp", 3000);
}
#[test]
fn fuzz_journald_json() {
    mutation("journald-json", 3000);
}
#[test]
fn fuzz_cloudflared_msg() {
    mutation("cloudflared-msg", 4000);
}
#[test]
fn fuzz_event_jsonl() {
    mutation("event-jsonl", 3000);
}
#[test]
fn fuzz_rfc3339() {
    mutation("rfc3339", 6000);
}
#[test]
fn fuzz_http_event_req() {
    mutation("http-event-req", 4000);
}
#[test]
fn fuzz_ingress_yaml() {
    mutation("ingress-yaml", 3000);
}
#[test]
fn prop_proc_net_roundtrip() {
    generative("gen-proc-net", 1500);
}
#[test]
fn prop_ss_roundtrip() {
    generative("gen-ss", 2000);
}
#[test]
fn prop_journal_roundtrip() {
    generative("gen-journal", 1500);
}
#[test]
fn prop_cloudflared_roundtrip() {
    generative("gen-cloudflared", 3000);
}
#[test]
fn prop_event_roundtrip() {
    generative("gen-event", 800);
}
#[test]
fn prop_rfc3339_roundtrip() {
    generative("gen-rfc3339", 5000);
}
#[test]
fn prop_ingress_roundtrip() {
    generative("gen-ingress", 2000);
}

/// Every target and generator once, with one summary table: the shape the
/// box runs record (`--ignored`, use with `CORTEX_FUZZ_SECS`).
#[test]
#[ignore]
fn fuzz_all_summary() {
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

/// `ss` prints scoped addresses as `[::1]%lo:53`; the zone must go before
/// the brackets or `::1]` comes out and the listener is "not loopback".
#[test]
fn regress_ss_scoped_v6_loopback() {
    let ls = openmind_nerve::probe::parse_ss(
        "LISTEN 0 4096 [::1]%lo:8670 [::]:*\nLISTEN 0 4096 127.0.0.53%lo:53 0.0.0.0:*\nLISTEN 0 5 [fe80::1]%eth0:546 [::]:*\n",
    );
    let got: Vec<(&str, u16, bool)> = ls
        .iter()
        .map(|l| (l.addr.as_str(), l.port, l.loopback))
        .collect();
    assert_eq!(
        got,
        [
            ("127.0.0.53", 53, true),
            ("::1", 8670, true),
            ("fe80::1", 546, false)
        ]
    );
}

/// A 32-byte /proc/net/tcp6 address with multi-byte UTF-8 used to panic
/// on a char boundary; now the row is skipped.
#[test]
fn regress_proc_net_tcp6_multibyte_no_panic() {
    let text = "  sl  local_address remote_address st\n   0: 0000000\u{e9}00000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 0 0 0 997 0 12345 1\n";
    assert!(openmind_nerve::probe::parse_proc_net_tcp(text, true).is_empty());
}

/// The event-feed stamp parser took signed fields (`+5` as a month).
#[test]
fn regress_rfc3339_no_signed_fields() {
    use openmind_nerve::sensors::event_feed::parse_rfc3339;
    for bad in [
        "2026-10-+5T02:00:00+01:00",
        "2026-10-21T01:+0:00+02:00",
        "2026-10-21T-1:00:00+02:00",
        "2026-10-21T01:00:00++2:00",
    ] {
        assert_eq!(parse_rfc3339(bad), None, "{bad}");
    }
    assert_eq!(
        parse_rfc3339("2026-10-21T01:00:00+02:00"),
        Some(fuzz::targets::NOW)
    );
}

/// A rule list item shallower than the first one used to end the list
/// silently (every later rule unlinted); now it is a parse error.
#[test]
fn regress_ingress_shallower_item_is_an_error() {
    let text = "ingress:\n  - hostname: a.example.test\n    service: http://localhost:8081\n- hostname: b.example.test\n  service: http://localhost:8083\n- service: http_status:404\n";
    let e = tunnel_lint::parse(text).unwrap_err();
    assert_eq!(e.line, 3, "{e}");
    assert!(e.msg.contains("less indented"), "{e}");
}
