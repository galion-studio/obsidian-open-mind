// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Slot 384 (console-realtests-rust-routes): every `/api/...` path the seven
//! console real tests read must have a row in `console_routes::MAP`, so a new
//! Node-era read cannot slip in without a Rust owner or a "retired" entry.
//! Plain file scan, no network; runs in the normal test gate.

use galion_realtest::console_routes::{lookup, MAP};
use std::path::Path;

const REAL_TESTS: &[&str] = &[
    "../console-ui/tests/real_port1.rs",
    "../console-ui/tests/real_console_ui.rs",
    "../console-static/tests/real_port2.rs",
    "../console-static/tests/real_port3.rs",
    "../console-static/tests/real_port4.rs",
    "../console-static/tests/real_port5.rs",
    "../console-static/tests/real_route31.rs",
];

/// Quoted string literals that start with `/api/` (the bare prefix excluded).
fn api_literals(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("\"/api/") {
        let tail = &rest[i + 1..];
        let end = tail.find('"').unwrap_or(tail.len());
        let lit = &tail[..end];
        if lit != "/api/" {
            out.push(lit.to_string());
        }
        rest = &tail[end..];
    }
    out
}

#[test]
fn literal_scan_works() {
    let v = api_literals(r#"get(a, "/api/v1/x?name={n}"); "/api/"; "/api/mesh""#);
    assert_eq!(v, ["/api/v1/x?name={n}", "/api/mesh"]);
}

#[test]
fn every_api_read_in_the_seven_real_tests_is_mapped() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut seen = 0;
    let mut missing = Vec::new();
    for f in REAL_TESTS {
        let src = std::fs::read_to_string(root.join(f)).unwrap_or_else(|e| panic!("{f}: {e}"));
        for lit in api_literals(&src) {
            seen += 1;
            if lookup(&lit).is_none() {
                missing.push(format!("{f}: {lit}"));
            }
        }
    }
    assert!(seen > 30, "scan found only {seen} /api/ literals");
    assert!(missing.is_empty(), "unmapped Node-era reads: {missing:?}");
    assert!(MAP.len() >= 32);
}
