// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Size-headroom guard for the release set (wave-4 slot 368, D-348-4).
//!
//! `scripts/size-budget.sh` and `real_release_repro` fail a binary only once
//! it is OVER its cap (2 MiB, hermes-gateway 2.5 MiB). This guard warns
//! earlier: every release binary whose headroom (cap - bytes) is under
//! `GALION_HEADROOM_WARN_KIB` KiB (default 128, the slot 348 nerve target)
//! gets a `WARN` line, so the next growth is visible before it breaks the
//! budget.
//!
//! - Always-run tests check the rules on synthetic ELF files (no build).
//! - `release_bins_headroom` is gated: `GALION_REAL=1` and
//!   `GALION_RELEASE_BINS_DIR=<dir>` holding the release bytes (for example
//!   `<work>/out/<triple>/full` from `real_release_repro`). It reads the real
//!   files, prints one row per binary and fails on a missing, unstripped or
//!   over-budget binary. Warnings fail too only with
//!   `GALION_HEADROOM_STRICT=1` (owner decision D-368-2: warn vs gate).
//!
//! Read-only: no process is started and nothing is written.

mod common;

use common::{ReleaseBin, RELEASE_BINS};
use std::fs;
use std::path::{Path, PathBuf};

/// Default warning threshold in KiB (slot 348 real_size_headroom target).
const DEFAULT_WARN_KIB: u64 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Ok,
    /// Within the cap but with less headroom than the warning threshold.
    Warn,
    /// Over the cap, not stripped, or not readable as ELF64.
    Fail,
}

#[derive(Debug)]
struct Row {
    bin: &'static str,
    bytes: u64,
    budget: u64,
    /// Signed: negative when over the cap.
    headroom: i64,
    stripped: bool,
    verdict: Verdict,
    note: String,
}

fn warn_bytes_from(value: Option<&str>) -> Result<u64, String> {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(DEFAULT_WARN_KIB * 1024),
        Some(v) => v
            .parse::<u64>()
            .map(|k| k * 1024)
            .map_err(|_| format!("GALION_HEADROOM_WARN_KIB is not a whole number: {v:?}")),
    }
}

fn verdict(bytes: u64, budget: u64, stripped: bool, warn_bytes: u64) -> Verdict {
    if !stripped || bytes > budget {
        Verdict::Fail
    } else if budget - bytes < warn_bytes {
        Verdict::Warn
    } else {
        Verdict::Ok
    }
}

/// `Some(true)` when the ELF64 little-endian file has a `.symtab` section,
/// `Some(false)` when it has none, `None` when it is not a readable ELF64 LE
/// file. Bounds-checked (a truncated file gives `None`, never a panic).
fn has_symtab(b: &[u8]) -> Option<bool> {
    let u16_at = |o: usize| Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize);
    let u32_at = |o: usize| Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize);
    let u64_at = |o: usize| {
        let v = u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?);
        usize::try_from(v).ok()
    };
    if b.len() < 64 || &b[..4] != b"\x7fELF" || b[4] != 2 || b[5] != 1 {
        return None;
    }
    let (shoff, shentsize, shnum, shstrndx) =
        (u64_at(0x28)?, u16_at(0x3a)?, u16_at(0x3c)?, u16_at(0x3e)?);
    if shoff == 0 || shnum == 0 {
        return Some(false);
    }
    let sh = |i: usize| shoff.checked_add(i.checked_mul(shentsize)?);
    let strtab = u64_at(sh(shstrndx)?.checked_add(0x18)?)?;
    for i in 0..shnum {
        let name = strtab.checked_add(u32_at(sh(i)?)?)?;
        let rest = b.get(name..)?;
        let end = rest.iter().position(|c| *c == 0)?;
        if &rest[..end] == b".symtab" {
            return Some(true);
        }
    }
    Some(false)
}

fn check_dir(dir: &Path, bins: &[ReleaseBin], warn_bytes: u64) -> Vec<Row> {
    let mut rows = Vec::new();
    for r in bins {
        // console-static ships two binaries; check each name once.
        if rows.iter().any(|x: &Row| x.bin == r.bin) {
            continue;
        }
        let path = dir.join(r.bin);
        let (bytes, stripped, note) = match fs::read(&path) {
            Err(e) => (0, false, format!("missing: {e}")),
            Ok(b) => match has_symtab(&b) {
                None => (b.len() as u64, false, "not an ELF64 LE file".to_string()),
                Some(sym) => (b.len() as u64, !sym, String::new()),
            },
        };
        let v = if note.is_empty() {
            verdict(bytes, r.budget, stripped, warn_bytes)
        } else {
            Verdict::Fail
        };
        let mut note = note;
        if note.is_empty() && bytes > r.budget {
            note = format!("over budget by {} B", bytes - r.budget);
        } else if note.is_empty() && !stripped {
            note = "not stripped (.symtab present)".to_string();
        }
        rows.push(Row {
            bin: r.bin,
            bytes,
            budget: r.budget,
            headroom: r.budget as i64 - bytes as i64,
            stripped,
            verdict: v,
            note,
        });
    }
    rows
}

fn print_rows(dir: &Path, rows: &[Row], warn_bytes: u64) {
    println!(
        "size_headroom_guard dir {} warn under {} KiB",
        dir.display(),
        warn_bytes / 1024
    );
    println!(
        "{:<22} {:>9} {:>9} {:>9} {:>8} status",
        "binary", "bytes", "budget", "headroom", "stripped"
    );
    for r in rows {
        let status = match r.verdict {
            Verdict::Ok => "ok".to_string(),
            Verdict::Warn => format!(
                "WARN headroom {} KiB < {} KiB",
                r.headroom / 1024,
                warn_bytes / 1024
            ),
            Verdict::Fail => format!("FAIL {}", r.note),
        };
        println!(
            "{:<22} {:>9} {:>9} {:>9} {:>8} {status}",
            r.bin, r.bytes, r.budget, r.headroom, r.stripped
        );
    }
}

#[test]
fn release_bins_headroom() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1 and GALION_RELEASE_BINS_DIR=<dir>");
        return;
    }
    let Ok(dir) = std::env::var("GALION_RELEASE_BINS_DIR") else {
        panic!("GALION_REAL=1 needs GALION_RELEASE_BINS_DIR=<dir with the release binaries>");
    };
    let warn_bytes = warn_bytes_from(std::env::var("GALION_HEADROOM_WARN_KIB").ok().as_deref())
        .unwrap_or_else(|e| panic!("{e}"));
    let strict = std::env::var("GALION_HEADROOM_STRICT").ok().as_deref() == Some("1");
    let dir = PathBuf::from(dir);
    let rows = check_dir(&dir, &RELEASE_BINS, warn_bytes);
    print_rows(&dir, &rows, warn_bytes);
    let fails: Vec<&str> = rows
        .iter()
        .filter(|r| r.verdict == Verdict::Fail)
        .map(|r| r.bin)
        .collect();
    let warns: Vec<&str> = rows
        .iter()
        .filter(|r| r.verdict == Verdict::Warn)
        .map(|r| r.bin)
        .collect();
    println!(
        "size_headroom_guard: {} binaries, {} ok, {} warn {:?}, {} fail {:?}, strict {strict}",
        rows.len(),
        rows.len() - warns.len() - fails.len(),
        warns.len(),
        warns,
        fails.len(),
        fails
    );
    assert!(fails.is_empty(), "size budget broken: {fails:?}");
    assert!(
        !strict || warns.is_empty(),
        "GALION_HEADROOM_STRICT=1: low headroom: {warns:?}"
    );
}

// ---- always-run checks of the rules on synthetic files ----

/// Minimal ELF64 LE file of exactly `len` bytes. With `symtab`, it has a
/// section header table naming `.shstrtab` and `.symtab`; without, no sections.
fn fake_elf(len: usize, symtab: bool) -> Vec<u8> {
    let mut b = vec![0u8; len.max(512)];
    b[..4].copy_from_slice(b"\x7fELF");
    b[4] = 2; // ELF64
    b[5] = 1; // little-endian
    if symtab {
        let strtab = 256usize;
        let names = b"\0.shstrtab\0.symtab\0";
        b[strtab..strtab + names.len()].copy_from_slice(names);
        let shoff = 320usize;
        b[0x28..0x30].copy_from_slice(&(shoff as u64).to_le_bytes());
        b[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        b[0x3c..0x3e].copy_from_slice(&3u16.to_le_bytes());
        b[0x3e..0x40].copy_from_slice(&1u16.to_le_bytes());
        // section 1 = .shstrtab (name offset 1), its sh_offset -> strtab
        b[shoff + 64..shoff + 68].copy_from_slice(&1u32.to_le_bytes());
        b[shoff + 64 + 0x18..shoff + 64 + 0x20].copy_from_slice(&(strtab as u64).to_le_bytes());
        // section 2 = .symtab (name offset 11)
        b[shoff + 128..shoff + 132].copy_from_slice(&11u32.to_le_bytes());
    }
    b.truncate(len.max(512));
    b
}

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "galion-headroom-guard-{tag}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("mkdir tmp");
    d
}

const CAP: u64 = 4096;
const BINS: [ReleaseBin; 3] = [
    ReleaseBin {
        pkg: "a",
        bin: "roomy",
        features: &[],
        budget: CAP,
    },
    ReleaseBin {
        pkg: "b",
        bin: "tight",
        features: &[],
        budget: CAP,
    },
    ReleaseBin {
        pkg: "b",
        bin: "tight",
        features: &[],
        budget: CAP,
    },
];

#[test]
fn verdict_boundaries() {
    let w = 1024;
    assert_eq!(verdict(CAP - 2048, CAP, true, w), Verdict::Ok);
    assert_eq!(verdict(CAP - 1024, CAP, true, w), Verdict::Ok); // headroom == threshold
    assert_eq!(verdict(CAP - 1023, CAP, true, w), Verdict::Warn);
    assert_eq!(verdict(CAP, CAP, true, w), Verdict::Warn); // exactly at cap: still within
    assert_eq!(verdict(CAP + 1, CAP, true, w), Verdict::Fail);
    assert_eq!(verdict(10, CAP, false, w), Verdict::Fail); // unstripped
    assert_eq!(verdict(CAP, CAP, true, 0), Verdict::Ok); // threshold 0 = never warn
}

#[test]
fn warn_threshold_env_parsing() {
    assert_eq!(warn_bytes_from(None), Ok(128 * 1024));
    assert_eq!(warn_bytes_from(Some("")), Ok(128 * 1024));
    assert_eq!(warn_bytes_from(Some(" 64 ")), Ok(64 * 1024));
    assert_eq!(warn_bytes_from(Some("0")), Ok(0));
    assert!(warn_bytes_from(Some("12k")).is_err());
    assert!(warn_bytes_from(Some("-1")).is_err());
}

#[test]
fn symtab_detection_on_synthetic_elf() {
    assert_eq!(has_symtab(&fake_elf(600, false)), Some(false));
    assert_eq!(has_symtab(&fake_elf(600, true)), Some(true));
    assert_eq!(has_symtab(b"#!/bin/sh\n"), None);
    let mut trunc = fake_elf(600, true);
    trunc.truncate(330); // section headers cut off: no panic
    assert_eq!(has_symtab(&trunc), None);
}

#[test]
fn check_dir_warns_and_fails() {
    let d = tmp_dir("mixed");
    fs::write(d.join("roomy"), fake_elf(1024, false)).unwrap();
    fs::write(d.join("tight"), fake_elf(CAP as usize - 100, false)).unwrap();
    let rows = check_dir(&d, &BINS, 1024);
    assert_eq!(rows.len(), 2, "duplicate bin name checked once");
    assert_eq!(rows[0].verdict, Verdict::Ok);
    assert_eq!(rows[0].headroom, 3072);
    assert_eq!(rows[1].verdict, Verdict::Warn);
    assert_eq!(rows[1].headroom, 100);

    // negative controls: over the cap, unstripped, missing, not ELF
    fs::write(d.join("tight"), fake_elf(CAP as usize + 1, false)).unwrap();
    let rows = check_dir(&d, &BINS, 1024);
    assert_eq!(rows[1].verdict, Verdict::Fail);
    assert_eq!(rows[1].headroom, -1);
    assert_eq!(rows[1].note, "over budget by 1 B");
    fs::write(d.join("tight"), fake_elf(1024, true)).unwrap();
    let rows = check_dir(&d, &BINS, 1024);
    assert_eq!(rows[1].verdict, Verdict::Fail);
    assert!(rows[1].note.starts_with("not stripped"));
    fs::write(d.join("tight"), b"#!/bin/sh\necho hi\n").unwrap();
    assert_eq!(check_dir(&d, &BINS, 1024)[1].verdict, Verdict::Fail);
    fs::remove_file(d.join("tight")).unwrap();
    let rows = check_dir(&d, &BINS, 1024);
    assert_eq!(rows[1].verdict, Verdict::Fail);
    assert!(rows[1].note.starts_with("missing"));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn release_set_is_covered_once_per_binary() {
    let mut names: Vec<&str> = RELEASE_BINS.iter().map(|r| r.bin).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), RELEASE_BINS.len(), "binary names are unique");
    assert!(RELEASE_BINS.iter().all(|r| r.budget >= 2 * 1024 * 1024));
}
