// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Byte-level proof for D-312-4 / D-325-2 (GALION_REAL=1, otherwise skipped).
//!
//! Builds the 14 release binaries (13 packages; gaia-cortex with
//! `llm-advisor`) as one `-p` set, then every package alone in its own fresh
//! target directory, plus partial sets (the slot 312
//! `-p openmind-nerve -p hermes-gateway` pair, gaia-cortex + openmind-nerve)
//! and the console release recipe `-p console-static --bin
//! galion-console-serve`, all as static musl release builds with the same
//! provenance env, and requires identical SHA-256. Every full-set binary must
//! also be stripped (no `.symtab`) and within its RELEASE_BINS size budget
//! (the caps of scripts/size-budget.sh).
//!
//! Env:
//! - `GALION_REPRO_DIR`    work dir (default `$CARGO_TARGET_DIR-repro-work`
//!   or `/tmp/release-repro-work`); bins are kept in `<dir>/out`.
//! - `GALION_REPRO_TARGET` target triple (default x86_64-unknown-linux-musl).
//! - `GALION_REPRO_JOBS`   cargo `-j` (default 2).
//! - `GALION_REPRO_PKGS`   comma list to limit the solo builds.
//! - `GALION_REPRO_KEEP=1` keep the target dirs (default: removed after use).
//!
//! No network (`--offline --locked`), no ports, no live system touched.

mod common;

use common::{cargo, p_args, release_packages, workspace_root, ReleaseBin, RELEASE_BINS};
use galion_common::sha256;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const COMMIT: &str = "release-bins-add";
const BUILD_TIME: &str = "2026-10-28T01:30:00+01:00";
const SOURCE_DATE_EPOCH: &str = "1793147400";

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

/// `cargo build --release` of `pkgs` (+ `extra` args) into `target_dir`; returns seconds.
fn build(pkgs: &[&str], extra: &[&str], target_dir: &Path, triple: &str, jobs: &str) -> f64 {
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
        .args(p_args(pkgs))
        .args(extra);
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

/// True when the ELF file has a `.symtab` section (not stripped). Reads the
/// ELF64 little-endian section headers directly (no readelf needed).
fn has_symtab(bytes: &[u8]) -> bool {
    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as usize;
    let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap()) as usize;
    assert!(
        bytes.len() > 64 && &bytes[..4] == b"\x7fELF",
        "not an ELF file"
    );
    assert_eq!(bytes[4], 2, "not ELF64");
    assert_eq!(bytes[5], 1, "not little-endian");
    let (shoff, shentsize, shnum, shstrndx) =
        (u64_at(0x28), u16_at(0x3a), u16_at(0x3c), u16_at(0x3e));
    if shoff == 0 || shnum == 0 {
        return false;
    }
    let sh = |i: usize| shoff + i * shentsize;
    let strtab = u64_at(sh(shstrndx) + 0x18);
    (0..shnum).any(|i| {
        let name = strtab + u32_at(sh(i));
        let end = bytes[name..].iter().position(|b| *b == 0).unwrap_or(0);
        &bytes[name..name + end] == b".symtab"
    })
}

/// One named build of `pkgs` (+ `extra`) whose `bins` must equal the full set.
fn compare(
    label: &str,
    pkgs: &[&str],
    extra: &[&str],
    bins: &[&'static str],
    ctx: &Ctx,
    acc: &mut Acc,
) {
    let slug: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = ctx.work.join(&slug);
    fresh(&dir);
    let secs = build(pkgs, extra, &dir, &ctx.triple, &ctx.jobs);
    for bin in bins {
        let p = bin_path(&dir, &ctx.triple, bin);
        keep(&p, &ctx.out, &slug, bin);
        let got = sha_of(&p);
        let f = &acc.full[bin];
        let same = got == *f;
        acc.rows.push(format!(
            "{label:<44} {bin:<22} {:>8} B {} full {:>8} B {} {} ({secs:.1} s)",
            got.1,
            &got.0[..16],
            f.1,
            &f.0[..16],
            if same { "IDENTICAL" } else { "DIFFER" }
        ));
        if !same {
            acc.differ.push(format!("{label} {bin}"));
        }
    }
    if !ctx.keep_dirs {
        fresh(&dir);
    }
}

struct Acc {
    full: BTreeMap<&'static str, (String, u64)>,
    rows: Vec<String>,
    differ: Vec<String>,
}

/// (label, packages, extra cargo args, binaries to compare)
type Set = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
);

struct Ctx {
    work: PathBuf,
    out: PathBuf,
    triple: String,
    jobs: String,
    keep_dirs: bool,
}

fn bins_of(pkg: &str) -> Vec<&'static str> {
    RELEASE_BINS
        .iter()
        .filter(|r| r.pkg == pkg)
        .map(|r| r.bin)
        .collect()
}

#[test]
fn solo_and_partial_release_builds_are_byte_identical_to_the_full_set() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("real_release_repro: skipped (set GALION_REAL=1)");
        return;
    }
    let triple = env_or("GALION_REPRO_TARGET", "x86_64-unknown-linux-musl");
    let only: Option<Vec<String>> = std::env::var("GALION_REPRO_PKGS")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let work = work_dir();
    let ctx = Ctx {
        out: work.join("out").join(&triple),
        work,
        triple,
        jobs: env_or("GALION_REPRO_JOBS", "2"),
        keep_dirs: std::env::var("GALION_REPRO_KEEP").ok().as_deref() == Some("1"),
    };
    fs::create_dir_all(&ctx.out).expect("mkdir work");

    // 1. Full release set (13 packages, 14 binaries).
    let all = release_packages();
    let full_dir = ctx.work.join("full");
    fresh(&full_dir);
    let secs = build(&all, &[], &full_dir, &ctx.triple, &ctx.jobs);
    eprintln!(
        "full set: {} packages / {} binaries built in {secs:.1} s ({})",
        all.len(),
        RELEASE_BINS.len(),
        p_args(&all).join(" ")
    );
    let mut full: BTreeMap<&'static str, (String, u64)> = BTreeMap::new();
    let mut over = Vec::new();
    for ReleaseBin { bin, budget, .. } in RELEASE_BINS {
        let p = bin_path(&full_dir, &ctx.triple, bin);
        keep(&p, &ctx.out, "full", bin);
        let bytes = fs::read(&p).expect("read full bin");
        let stripped = !has_symtab(&bytes);
        let len = bytes.len() as u64;
        let ok = stripped && len <= budget;
        eprintln!(
            "size {bin:<22} {len:>8} B budget {budget:>8} B stripped {stripped} {}",
            if ok { "ok" } else { "FAIL" }
        );
        if !ok {
            over.push(format!(
                "{bin} {len} B (budget {budget}, stripped {stripped})"
            ));
        }
        full.insert(bin, sha_of(&p));
    }
    if !ctx.keep_dirs {
        fresh(&full_dir);
    }

    // 2. Every package alone, each in its own fresh target dir.
    let mut acc = Acc {
        full,
        rows: Vec::new(),
        differ: Vec::new(),
    };
    for pkg in &all {
        if let Some(list) = &only {
            if !list.iter().any(|x| x == pkg) {
                continue;
            }
        }
        let label = format!("solo -p {pkg}");
        compare(&label, &[pkg], &[], &bins_of(pkg), &ctx, &mut acc);
    }

    // 3. Partial sets and the per-binary console recipe.
    if only.is_none() {
        let sets: [Set; 3] = [
            (
                "pair -p openmind-nerve -p hermes-gateway",
                &["openmind-nerve", "hermes-gateway"],
                &[],
                &["openmind-nerve", "hermes-gateway"],
            ),
            (
                "pair -p gaia-cortex -p openmind-nerve",
                &["gaia-cortex", "openmind-nerve"],
                &[],
                &["gaia-cortex", "openmind-nerve"],
            ),
            (
                "bin -p console-static --bin galion-console-serve",
                &["console-static"],
                &["--bin", "galion-console-serve"],
                &["galion-console-serve"],
            ),
        ];
        for (label, pkgs, extra, bins) in sets {
            compare(label, pkgs, extra, bins, &ctx, &mut acc);
        }
    }

    let mut sums = String::new();
    for (bin, (sha, len)) in &acc.full {
        sums.push_str(&format!("{sha}  {bin}  {len}\n"));
    }
    fs::write(ctx.out.join("SHA256SUMS-full"), &sums).expect("write sums");
    for r in &acc.rows {
        eprintln!("{r}");
    }
    eprintln!(
        "real_release_repro {}: {} compared, {} differ, {} over budget/unstripped; bins in {}",
        ctx.triple,
        acc.rows.len(),
        acc.differ.len(),
        over.len(),
        ctx.out.display()
    );
    assert!(
        acc.differ.is_empty(),
        "not byte-identical: {:?}",
        acc.differ
    );
    assert!(over.is_empty(), "size budget: {over:?}");
}
