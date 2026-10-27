// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Byte-level proof for D-312-4 (GALION_REAL=1, otherwise skipped).
//!
//! Builds the 12 release binaries as one `-p` set, then every binary alone in
//! its own fresh target directory, plus one partial pair (the slot 312
//! `-p openmind-nerve -p hermes-gateway` case), all as static musl release
//! builds with the same provenance env, and requires identical SHA-256.
//!
//! Env:
//! - `GALION_REPRO_DIR`    work dir (default `$CARGO_TARGET_DIR/../release-repro-work`
//!   or `/tmp/release-repro-work`); bins are kept in `<dir>/out`.
//! - `GALION_REPRO_TARGET` target triple (default x86_64-unknown-linux-musl).
//! - `GALION_REPRO_JOBS`   cargo `-j` (default 2).
//! - `GALION_REPRO_PKGS`   comma list to limit the solo builds.
//! - `GALION_REPRO_KEEP=1` keep the target dirs (default: removed after use).
//!
//! No network (`--offline --locked`), no ports, no live system touched.

mod common;

use common::{cargo, p_args, workspace_root, RELEASE_BINS};
use galion_common::sha256;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const COMMIT: &str = "release-repro-unify";
const BUILD_TIME: &str = "2026-10-27T23:00:00+01:00";
const SOURCE_DATE_EPOCH: &str = "1793138400";

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn work_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GALION_REPRO_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    match std::env::var("CARGO_TARGET_DIR") {
        Ok(t) if !t.is_empty() => PathBuf::from(format!("{t}-repro-work")),
        _ => PathBuf::from("/tmp/release-repro-work"),
    }
}

/// `cargo build --release` of `pkgs` into `target_dir`; returns seconds.
fn build(pkgs: &[&str], target_dir: &Path, triple: &str, jobs: &str) -> f64 {
    let started = Instant::now();
    let mut cmd = Command::new(cargo());
    cmd.current_dir(workspace_root());
    // Nested cargo: drop the outer test's per-crate CARGO_* / RUSTC_* env so the
    // inner build sees a clean shell environment (CARGO_HOME stays).
    for (k, _) in std::env::vars() {
        let drop_it = (k.starts_with("CARGO_") && k != "CARGO_HOME")
            || k.starts_with("RUSTC")
            || k == "RUSTFLAGS"
            || k == "RUSTDOCFLAGS"
            || k.starts_with("__CARGO");
        if drop_it {
            cmd.env_remove(&k);
        }
    }
    cmd.env("GALION_BUILD_COMMIT", COMMIT)
        .env("GALION_BUILD_TIME", BUILD_TIME)
        .env("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH)
        .env("CARGO_INCREMENTAL", "0")
        .arg("build")
        .args(["--release", "--locked", "--offline", "--quiet", "-j", jobs])
        .args(["--target", triple])
        .arg("--target-dir")
        .arg(target_dir)
        .args(p_args(pkgs));
    let out = cmd.output().expect("run cargo build");
    assert!(
        out.status.success(),
        "cargo build {pkgs:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    started.elapsed().as_secs_f64()
}

fn sha_of(path: &Path) -> (String, u64) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    (sha256::hex(&bytes), bytes.len() as u64)
}

fn bin_path(target_dir: &Path, triple: &str, bin: &str) -> PathBuf {
    target_dir.join(triple).join("release").join(bin)
}

fn keep(path: &Path, out: &Path, label: &str, bin: &str) {
    let dir = out.join(label);
    fs::create_dir_all(&dir).expect("mkdir out");
    fs::copy(path, dir.join(bin)).expect("copy bin");
}

fn fresh(dir: &Path) {
    if dir.exists() {
        fs::remove_dir_all(dir).expect("clear target dir");
    }
}

#[test]
fn solo_and_partial_release_builds_are_byte_identical_to_the_full_set() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("real_release_repro: skipped (set GALION_REAL=1)");
        return;
    }
    let triple = env_or("GALION_REPRO_TARGET", "x86_64-unknown-linux-musl");
    let jobs = env_or("GALION_REPRO_JOBS", "2");
    let keep_dirs = std::env::var("GALION_REPRO_KEEP").ok().as_deref() == Some("1");
    let only: Option<Vec<String>> = std::env::var("GALION_REPRO_PKGS")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let work = work_dir();
    let out = work.join("out").join(&triple);
    fs::create_dir_all(&out).expect("mkdir work");

    // 1. Full release set.
    let all: Vec<&str> = RELEASE_BINS.iter().map(|(p, _)| *p).collect();
    let full_dir = work.join("full");
    fresh(&full_dir);
    let secs = build(&all, &full_dir, &triple, &jobs);
    eprintln!("full set: 12 packages built in {secs:.1} s");
    let mut full: BTreeMap<&str, (String, u64)> = BTreeMap::new();
    for (_, bin) in RELEASE_BINS {
        let p = bin_path(&full_dir, &triple, bin);
        keep(&p, &out, "full", bin);
        full.insert(bin, sha_of(&p));
    }
    if !keep_dirs {
        fresh(&full_dir);
    }

    // 2. Every binary alone, each in its own fresh target dir.
    let mut rows = Vec::new();
    let mut differ = Vec::new();
    for (pkg, bin) in RELEASE_BINS {
        if let Some(list) = &only {
            if !list.iter().any(|x| x == pkg) {
                continue;
            }
        }
        let dir = work.join(format!("solo-{pkg}"));
        fresh(&dir);
        let secs = build(&[pkg], &dir, &triple, &jobs);
        let p = bin_path(&dir, &triple, bin);
        keep(&p, &out, "solo", bin);
        let solo = sha_of(&p);
        let f = &full[bin];
        let same = solo == *f;
        rows.push(format!(
            "solo -p {pkg:<16} {bin:<22} {:>8} B {} full {:>8} B {} {} ({secs:.1} s)",
            solo.1,
            &solo.0[..16],
            f.1,
            &f.0[..16],
            if same { "IDENTICAL" } else { "DIFFER" }
        ));
        if !same {
            differ.push(format!("solo {pkg}"));
        }
        if !keep_dirs {
            fresh(&dir);
        }
    }

    // 3. Partial pair from slot 312.
    if only.is_none() {
        let pair = ["openmind-nerve", "hermes-gateway"];
        let dir = work.join("pair");
        fresh(&dir);
        let secs = build(&pair, &dir, &triple, &jobs);
        for bin in pair {
            let p = bin_path(&dir, &triple, bin);
            keep(&p, &out, "pair", bin);
            let got = sha_of(&p);
            let same = got == full[bin];
            rows.push(format!(
                "pair -p openmind-nerve -p hermes-gateway: {bin:<16} {:>8} B {} {} ({secs:.1} s)",
                got.1,
                &got.0[..16],
                if same { "IDENTICAL" } else { "DIFFER" }
            ));
            if !same {
                differ.push(format!("pair {bin}"));
            }
        }
        if !keep_dirs {
            fresh(&dir);
        }
    }

    let mut sums = String::new();
    for (bin, (sha, len)) in &full {
        sums.push_str(&format!("{sha}  {bin}  {len}\n"));
    }
    fs::write(out.join("SHA256SUMS-full"), &sums).expect("write sums");
    for r in &rows {
        eprintln!("{r}");
    }
    eprintln!(
        "real_release_repro {triple}: {} compared, {} differ; bins in {}",
        rows.len(),
        differ.len(),
        out.display()
    );
    assert!(differ.is_empty(), "not byte-identical: {differ:?}");
}
