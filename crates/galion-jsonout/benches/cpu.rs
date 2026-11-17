// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Timed runs for `PyFormatter` via `to_json_compat` on a mixed payload.
//! `cargo bench` ignores `#[test]`, so the timed command is
//! `cargo test --profile bench --bench cpu`.

use galion_jsonout::to_json_compat;
use serde_json::json;
use std::hint::black_box;
use std::time::Instant;

const SAMPLES: usize = 5;

fn report(label: &str, iters: u64, mut op: impl FnMut()) {
    for _ in 0..8 {
        op();
    }
    let mut samples = [0u64; SAMPLES];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..iters {
            op();
        }
        *sample = (start.elapsed().as_nanos() as u64) / iters;
    }
    samples.sort_unstable();
    println!(
        "{label} median_ns {} samples {samples:?} iters {iters}",
        samples[SAMPLES / 2]
    );
}

#[test]
fn cpu_hot_paths() {
    let mixed = json!({
        "desk": "link watch",
        "place": "Zagreb",
        "note": "café ready",
        "items": [1, 2, "plain", "naïve"],
        "ok": true
    });
    report("py_formatter_compact", 20_000, || {
        black_box(to_json_compat(black_box(&mixed), false));
    });
    report("py_formatter_spaced", 20_000, || {
        black_box(to_json_compat(black_box(&mixed), true));
    });
}
