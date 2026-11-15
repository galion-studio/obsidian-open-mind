// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Timed runs for health-body and model-list parsing. No socket and no GPU.
//! `cargo bench` ignores `#[test]`, so the timed command is
//! `cargo test --profile bench --bench cpu`.

use openmind_llama::{LlamaCppProvider, ServerKind};
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
    let row = LlamaCppProvider::new("laptop-4060", "localhost:8080", ServerKind::LlamaServer)
        .expect("row");
    let health = include_str!("../fixtures/health_ok.json");
    let models = include_str!("../fixtures/v1_models.json");
    report("parse_health", 40_000, || {
        let parsed = row.parse_health(black_box(health)).expect("health");
        black_box(parsed.is_ready());
    });
    report("parse_models", 10_000, || {
        let parsed = row.parse_models(black_box(models)).expect("models");
        black_box(parsed.data.len());
    });
}
