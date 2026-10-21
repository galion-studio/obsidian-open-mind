// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Std-only mutation-fuzz harness for openmind-queue (no fuzz framework,
//! no new crates), in the same style as `galion-guard/tests/fuzz_parser.rs`
//! and the cortex t52 harness: a SplitMix64 PRNG, byte/token mutators and
//! a budget loop.
//!
//! - Plain `cargo test` runs a short pass with a fixed seed per target, so
//!   CI is deterministic.
//! - `GALION_FUZZ_SECS=N` runs every target for N seconds with a
//!   time-based seed; the seed is printed so a failure reproduces with
//!   `GALION_FUZZ_SEED=<seed>` (and the same `GALION_FUZZ_SECS`, or none).
//! - A failing case panics with the target name, seed, iteration and the
//!   input (escaped), so it can be turned into a regression test.

#![allow(dead_code)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// SplitMix64: tiny, full-period, good enough to drive mutations.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, one_in: u64) -> bool {
        self.next().is_multiple_of(one_in.max(1))
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
    /// A u64 biased towards the edges (0, 1, small, MAX, MAX-1, 2^k).
    pub fn edgy_u64(&mut self) -> u64 {
        match self.below(8) {
            0 => 0,
            1 => 1,
            2 => u64::MAX,
            3 => u64::MAX - self.below(3) as u64,
            4 => 1u64 << self.below(64),
            5 => self.below(10) as u64,
            6 => u64::MAX / 2 + self.below(3) as u64,
            _ => self.next(),
        }
    }
    pub fn edgy_u32(&mut self) -> u32 {
        match self.below(6) {
            0 => 0,
            1 => 1,
            2 => u32::MAX,
            3 => u32::MAX - self.below(3) as u32,
            4 => self.below(8) as u32,
            _ => self.next() as u32,
        }
    }
}

/// Byte-level mutation with optional dictionary tokens.
pub fn mutate(r: &mut Rng, input: &[u8], tokens: &[&[u8]]) -> Vec<u8> {
    let mut b = input.to_vec();
    let rounds = 1 + r.below(4);
    for _ in 0..rounds {
        match r.below(10) {
            0 if !b.is_empty() => {
                let i = r.below(b.len());
                b[i] ^= 1 << r.below(8);
            }
            1 if !b.is_empty() => {
                let i = r.below(b.len());
                b[i] = *r.pick(&[0u8, 0xff, b'\n', b'\r', b' ', b':', b'"', b'{', b'}', 0x80]);
            }
            2 if !b.is_empty() => {
                let i = r.below(b.len());
                let n = 1 + r.below(16.min(b.len() - i));
                b.drain(i..i + n);
            }
            3 if !tokens.is_empty() => {
                let i = r.below(b.len() + 1);
                let t = *r.pick(tokens);
                b.splice(i..i, t.iter().copied());
            }
            4 if !b.is_empty() => {
                // duplicate a chunk
                let i = r.below(b.len());
                let n = 1 + r.below(32.min(b.len() - i));
                let chunk: Vec<u8> = b[i..i + n].to_vec();
                let at = r.below(b.len() + 1);
                b.splice(at..at, chunk);
            }
            5 if !b.is_empty() => {
                let n = r.below(b.len());
                b.truncate(n);
            }
            6 => {
                let i = r.below(b.len() + 1);
                b.insert(i, r.next() as u8);
            }
            7 if !b.is_empty() => {
                // replace a run of ASCII digits with an edge number
                let starts: Vec<usize> = (0..b.len())
                    .filter(|&i| b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_digit()))
                    .collect();
                if !starts.is_empty() {
                    let s = *r.pick(&starts);
                    let mut e = s;
                    while e < b.len() && b[e].is_ascii_digit() {
                        e += 1;
                    }
                    let n = match r.below(5) {
                        0 => u64::MAX.to_string(),
                        1 => "18446744073709551616".to_string(),
                        2 => "-1".to_string(),
                        3 => "0".to_string(),
                        _ => r.edgy_u64().to_string(),
                    };
                    b.splice(s..e, n.into_bytes());
                }
            }
            8 if !tokens.is_empty() && !b.is_empty() => {
                // overwrite with a token
                let t = *r.pick(tokens);
                let i = r.below(b.len());
                let end = (i + t.len()).min(b.len());
                b.splice(i..end, t.iter().copied());
            }
            _ => {}
        }
    }
    if b.len() > 64 * 1024 {
        b.truncate(64 * 1024);
    }
    b
}

/// Escape bytes for a failure message.
pub fn show(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b.iter().take(4096) {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'"' => s.push_str("\\\""),
            0x20..=0x7e => s.push(c as char),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    if b.len() > 4096 {
        s.push_str("...");
    }
    s
}

/// How long / how often to run.
pub struct Budget {
    pub seed: u64,
    pub secs: Option<u64>,
    pub iters: u64,
}

pub fn budget(name: &str, default_seed: u64, default_iters: u64) -> Budget {
    let secs = std::env::var("GALION_FUZZ_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&s| s > 0);
    let seed = std::env::var("GALION_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            if secs.is_some() {
                std::time::SystemTime::now() // repo-check: allow (PRNG seed, not a time)
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(default_seed)
                    ^ fxhash(name)
            } else {
                default_seed
            }
        });
    eprintln!("fuzz {name}: seed {seed} secs {secs:?} iters {default_iters}");
    Budget {
        seed,
        secs,
        iters: default_iters,
    }
}

fn fxhash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Run `case` until the budget is spent. `case` gets the rng and returns
/// `Err(reason)` on a property violation; a panic is also a failure. The
/// input that failed is whatever `case` last stored via `last`.
pub fn run<F>(name: &str, default_seed: u64, default_iters: u64, mut case: F) -> u64
where
    F: FnMut(&mut Rng, &mut Vec<u8>) -> Result<(), String>,
{
    let b = budget(name, default_seed, default_iters);
    let mut r = Rng(b.seed);
    let start = Instant::now();
    let mut n = 0u64;
    let mut last = Vec::new();
    loop {
        match b.secs {
            Some(s) if start.elapsed() >= Duration::from_secs(s) => break,
            None if n >= b.iters => break,
            _ => {}
        }
        n += 1;
        last.clear();
        let res = catch_unwind(AssertUnwindSafe(|| case(&mut r, &mut last)));
        let why = match res {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => format!("property failed: {e}"),
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                format!("panic: {msg}")
            }
        };
        panic!(
            "fuzz {name} FIND: {why}\n  seed {} iteration {n}\n  input \"{}\"",
            b.seed,
            show(&last)
        );
    }
    eprintln!(
        "fuzz {name}: {n} cases in {} ms, no finds",
        start.elapsed().as_millis()
    );
    n
}
