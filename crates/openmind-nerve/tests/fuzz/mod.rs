// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! A tiny std-only fuzz/property harness for the cortex parsers (cortex
//! plan T5.2). No new dependencies, no libFuzzer, no C/C++/asm: a seeded
//! SplitMix64 generator, a handful of structure-unaware mutators (byte flip,
//! insert/delete/duplicate ranges, dictionary tokens, multi-byte UTF-8,
//! splices between seeds), a corpus that keeps inputs whose output
//! signature is new (a cheap "coverage" proxy), and `catch_unwind` around
//! every input so a panic is counted and its input saved instead of
//! aborting the run.
//!
//! Two kinds of targets:
//! - [`Target`]: a mutation target over text inputs. `check` returns
//!   `Ok(signature)` or `Err(property violation)`; a panic is a crash.
//! - [`Gen`]: a generative round-trip property driven by the RNG.
//!
//! Budgets: `CORTEX_FUZZ_ITERS` (per target, default per call site) or
//! `CORTEX_FUZZ_SECS` (time box per target, wins when set), seed
//! `CORTEX_FUZZ_SEED` (default fixed, so `cargo test` is reproducible).

#![allow(dead_code)]

pub mod targets;

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// SplitMix64 (public-domain algorithm by Sebastiano Vigna).
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `0..n` (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.next() % den < num
    }
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

/// Tokens every text target gets on top of its own dictionary.
pub const COMMON_DICT: &[&str] = &[
    "\n",
    "\r\n",
    "\r",
    "\t",
    " ",
    "  ",
    ":",
    "::",
    "=",
    "\"",
    "'",
    "\\",
    "/",
    "?",
    "#",
    "[",
    "]",
    "{",
    "}",
    ",",
    "-",
    "- ",
    "%",
    "\0",
    "0",
    "+",
    "-1",
    "65535",
    "65536",
    "4294967296",
    "18446744073709551616",
    "é",
    "€",
    "𝄞",
    "\u{2028}",
    "\u{feff}",
    "\u{fffd}",
    "\u{7f}",
];

/// Mutate `input` into a new string (always valid UTF-8: byte-level edits
/// are repaired with `from_utf8_lossy`, which itself injects U+FFFD).
pub fn mutate(rng: &mut Rng, input: &str, dict: &[&str], corpus: &[String]) -> String {
    let mut b = input.as_bytes().to_vec();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        let len = b.len();
        match rng.below(11) {
            0 if len > 0 => {
                let i = rng.below(len);
                b[i] ^= 1 << rng.below(8);
            }
            1 if len > 0 => {
                let i = rng.below(len);
                b[i] = rng.next() as u8;
            }
            2 => {
                let i = rng.below(len + 1);
                b.insert(i, rng.next() as u8);
            }
            3 if len > 0 => {
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(16));
                b.drain(i..i + n);
            }
            4 if len > 0 => {
                // Duplicate a range (long lines, repeated fields).
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(64));
                let chunk = b[i..i + n].to_vec();
                let cap = if rng.chance(1, 16) { 200 } else { 4 };
                let reps = 1 + rng.below(cap);
                let at = rng.below(b.len() + 1);
                for _ in 0..reps {
                    b.splice(at..at, chunk.iter().copied());
                }
            }
            5 | 6 => {
                let tok = if rng.chance(1, 2) || dict.is_empty() {
                    rng.pick(COMMON_DICT)
                } else {
                    rng.pick(dict)
                };
                let i = rng.below(len + 1);
                b.splice(i..i, tok.bytes());
            }
            7 if len > 0 => {
                // Replace a short run with a dictionary token.
                let i = rng.below(len);
                let n = 1 + rng.below((len - i).min(8));
                let tok = if dict.is_empty() {
                    rng.pick(COMMON_DICT)
                } else {
                    rng.pick(dict)
                };
                b.splice(i..i + n, tok.bytes());
            }
            8 if !corpus.is_empty() => {
                // Splice: our prefix + another input's suffix.
                let o = rng.pick(corpus).as_bytes();
                let cut_a = rng.below(len + 1);
                let cut_b = rng.below(o.len() + 1);
                b.truncate(cut_a);
                b.extend_from_slice(&o[cut_b..]);
            }
            9 if len > 0 => {
                b.truncate(rng.below(len));
            }
            10 if len > 0 => {
                // Change a digit run (ports, counts, hex fields).
                let i = rng.below(len);
                if b[i].is_ascii_hexdigit() {
                    b[i] = *rng.pick(b"0123456789ABCDEFabcdef");
                }
            }
            _ => {}
        }
        if b.len() > 256 * 1024 {
            b.truncate(256 * 1024);
        }
    }
    String::from_utf8_lossy(&b).into_owned()
}

/// A mutation target.
pub struct Target {
    pub name: &'static str,
    pub seeds: Vec<String>,
    pub dict: Vec<&'static str>,
    pub check: fn(&str) -> Result<u64, String>,
}

/// A generative round-trip property.
pub struct Gen {
    pub name: &'static str,
    pub run: fn(&mut Rng) -> Result<u64, String>,
}

/// Counts from one target run.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub name: String,
    pub execs: u64,
    pub distinct: usize,
    pub corpus: usize,
    pub panics: u64,
    pub violations: u64,
    pub elapsed: Duration,
    /// First few failures: (kind, message, saved input path).
    pub failures: Vec<(String, String, Option<PathBuf>)>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.panics == 0 && self.violations == 0
    }
    pub fn line(&self) -> String {
        format!(
            "{:<18} execs {:>9}  distinct {:>6}  corpus {:>4}  panics {}  violations {}  {:.1}s ({:.0}/s)",
            self.name,
            self.execs,
            self.distinct,
            self.corpus,
            self.panics,
            self.violations,
            self.elapsed.as_secs_f64(),
            self.execs as f64 / self.elapsed.as_secs_f64().max(1e-9)
        )
    }
}

/// Budget for one target.
#[derive(Debug, Clone, Copy)]
pub enum Budget {
    Iters(u64),
    Secs(u64),
}

/// `CORTEX_FUZZ_SECS` wins; else `CORTEX_FUZZ_ITERS`; else `default_iters`.
pub fn budget(default_iters: u64) -> Budget {
    let env = |k: &str| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    if let Some(s) = env("CORTEX_FUZZ_SECS").filter(|s| *s > 0) {
        return Budget::Secs(s);
    }
    Budget::Iters(env("CORTEX_FUZZ_ITERS").unwrap_or(default_iters))
}

pub fn seed() -> u64 {
    std::env::var("CORTEX_FUZZ_SEED")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0x00C0_47E5_2026_1021)
}

fn crash_dir() -> PathBuf {
    let d = std::env::var("CORTEX_FUZZ_CRASH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("cortex-fuzz-crashes"));
    let _ = std::fs::create_dir_all(&d);
    d
}

fn save(name: &str, n: u64, input: &str) -> Option<PathBuf> {
    let p = crash_dir().join(format!("{name}-{n}.txt"));
    std::fs::write(&p, input).ok().map(|_| p)
}

fn panic_msg(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| e.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

fn quiet_panics<R>(f: impl FnOnce() -> R) -> R {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = f();
    std::panic::set_hook(prev);
    r
}

fn more(b: Budget, start: Instant, execs: u64) -> bool {
    match b {
        Budget::Iters(n) => execs < n,
        Budget::Secs(s) => start.elapsed() < Duration::from_secs(s),
    }
}

const MAX_CORPUS: usize = 512;
const MAX_FAILS_KEPT: usize = 5;

/// Run a mutation target. Seeds are executed first (unmutated), then
/// mutants of corpus entries.
pub fn run_target(t: &Target, b: Budget, seed: u64) -> Report {
    let mut rng = Rng::new(seed ^ fnv(t.name));
    let mut corpus: Vec<String> = t.seeds.clone();
    let mut sigs: HashSet<u64> = HashSet::new();
    let mut r = Report {
        name: t.name.into(),
        ..Report::default()
    };
    let start = Instant::now();
    let mut queue: Vec<String> = t.seeds.clone();
    quiet_panics(|| {
        while more(b, start, r.execs) {
            let input = match queue.pop() {
                Some(s) => s,
                None => {
                    let base = if corpus.is_empty() {
                        String::new()
                    } else {
                        corpus[rng.below(corpus.len())].clone()
                    };
                    mutate(&mut rng, &base, &t.dict, &corpus)
                }
            };
            r.execs += 1;
            match catch_unwind(AssertUnwindSafe(|| (t.check)(&input))) {
                Ok(Ok(sig)) => {
                    if sigs.insert(sig) && corpus.len() < MAX_CORPUS {
                        corpus.push(input);
                    }
                }
                Ok(Err(msg)) => {
                    r.violations += 1;
                    if r.failures.len() < MAX_FAILS_KEPT {
                        let p = save(t.name, r.violations + r.panics, &input);
                        r.failures.push(("violation".into(), msg, p));
                    }
                }
                Err(e) => {
                    r.panics += 1;
                    if r.failures.len() < MAX_FAILS_KEPT {
                        let p = save(t.name, r.violations + r.panics, &input);
                        r.failures.push(("panic".into(), panic_msg(&*e), p));
                    }
                }
            }
        }
    });
    r.elapsed = start.elapsed();
    r.distinct = sigs.len();
    r.corpus = corpus.len();
    r
}

/// Run a generative property.
pub fn run_gen(g: &Gen, b: Budget, seed: u64) -> Report {
    let mut rng = Rng::new(seed ^ fnv(g.name));
    let mut sigs: HashSet<u64> = HashSet::new();
    let mut r = Report {
        name: g.name.into(),
        ..Report::default()
    };
    let start = Instant::now();
    quiet_panics(|| {
        while more(b, start, r.execs) {
            r.execs += 1;
            let state = rng.clone();
            match catch_unwind(AssertUnwindSafe(|| (g.run)(&mut rng))) {
                Ok(Ok(sig)) => {
                    sigs.insert(sig);
                }
                Ok(Err(msg)) => {
                    r.violations += 1;
                    if r.failures.len() < MAX_FAILS_KEPT {
                        r.failures.push(("violation".into(), msg, None));
                    }
                }
                Err(e) => {
                    r.panics += 1;
                    if r.failures.len() < MAX_FAILS_KEPT {
                        r.failures.push(("panic".into(), panic_msg(&*e), None));
                    }
                    // Keep going from a fresh state.
                    rng = state;
                    rng.next();
                }
            }
        }
    });
    r.elapsed = start.elapsed();
    r.distinct = sigs.len();
    r
}

/// FNV-1a, for stable signatures across runs (DefaultHasher is seeded
/// per process only in HashMap, but keep it explicit).
pub fn fnv(s: &str) -> u64 {
    fnv_bytes(s.as_bytes())
}

pub fn fnv_bytes(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Signature of any Debug-able output (bucketed so "same shape" collapses).
pub fn sig_of<T: std::fmt::Debug>(v: &T) -> u64 {
    fnv(&format!("{v:?}"))
}

/// Panic-free assertion helper for property checks.
pub fn ensure(cond: bool, msg: impl FnOnce() -> String) -> Result<(), String> {
    if cond {
        Ok(())
    } else {
        Err(msg())
    }
}
