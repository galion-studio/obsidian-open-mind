// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 353 size-budget-gateway-fix: config/release/size-budget.tsv is the
//! one release size budget, and scripts/size-budget.sh enforces it.
//!
//! Always run (no build, no network): the budget policy (200 MB
//! default since t416u 2026-10-29, was 2 MiB; no row above it, every release binary has a row) and the script against
//! synthetic ELF fixtures in a temp dir, including the negative controls:
//! the slot 339 `hermes-gateway-124a212` size (2,198,288 B, over the old
//! 2 MiB) now passes, 200 MB + 1 B FAILs; one byte over FAILs; a
//! `.symtab` FAILs; a tighter per-binary row FAILs; a bad table FAILs.
//! The fixture tests skip (with a message) when bash or readelf is missing.
//!
//! Gated (`GALION_REAL=1`): `SIZE_BUDGET_REAL_DIR=<dir>` must pass (the real
//! 14-bin x86_64 musl release set) and `SIZE_BUDGET_NEG_DIR=<dir>` (a real
//! set with a known-oversize binary) must FAIL naming `SIZE_BUDGET_NEG_BIN`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Per-binary budget: 200 MB (owner t416u, 2026-10-29 CET +01:00; was 2 MiB).
const BUDGET: u64 = 200 * 1024 * 1024;
/// The old 2 MiB budget (slots 329-353), kept for the record.
const OLD_MIB2: u64 = 2 * 1024 * 1024;

/// The release artifacts shipped by slots 312/339 (bin names without tags).
const RELEASE_BINS: [&str; 14] = [
    "desk-hermes",
    "gaia-cortex",
    "galion-backup",
    "galion-console-serve",
    "galion-console-static",
    "galion-desk",
    "galion-health",
    "galion-one",
    "hermes-gateway",
    "link-watch",
    "openmind-memory",
    "openmind-nerve",
    "openmind-queue",
    "tuner",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn table() -> Vec<(String, u64)> {
    let text = std::fs::read_to_string(root().join("config/release/size-budget.tsv"))
        .expect("config/release/size-budget.tsv");
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        assert_eq!(f.len(), 2, "bad row: {line}");
        rows.push((f[0].to_string(), f[1].parse::<u64>().expect("bytes")));
    }
    rows
}

#[test]
fn budget_table_policy() {
    let rows = table();
    let mut seen = std::collections::BTreeSet::new();
    for (k, _) in &rows {
        assert!(seen.insert(k.clone()), "duplicate row {k}");
    }
    let default = rows
        .iter()
        .find(|(k, _)| k == "default")
        .map(|r| r.1)
        .expect("default row");
    assert_eq!(
        default, BUDGET,
        "default budget must be 200 MB (209715200 B)"
    );
    for (k, v) in &rows {
        assert!(
            *v <= default,
            "{k}: {v} B is above the 200 MB default (exceptions need an owner decision)"
        );
        assert!(*v > 0, "{k}: zero budget");
    }
    for b in RELEASE_BINS {
        assert!(seen.contains(b), "release binary {b} has no row");
    }
    let gw = rows.iter().find(|(k, _)| k == "hermes-gateway").unwrap().1;
    assert!(gw <= BUDGET, "hermes-gateway budget {gw} > 200 MB");
}

/// Minimal ELF64 little-endian EXEC header (x86_64, entry 0x401000), padded
/// with zeros to `size` bytes. With `symtab`, a 3-entry section table
/// (null, .shstrtab, .symtab) follows the header so readelf sees `.symtab`.
fn elf(size: u64, symtab: bool) -> Vec<u8> {
    let mut b = vec![0u8; 64];
    b[0..4].copy_from_slice(b"\x7fELF");
    b[4] = 2; // ELFCLASS64
    b[5] = 1; // little endian
    b[6] = 1; // EV_CURRENT
    b[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    b[18..20].copy_from_slice(&0x3eu16.to_le_bytes()); // x86_64
    b[20..24].copy_from_slice(&1u32.to_le_bytes());
    b[24..32].copy_from_slice(&0x401000u64.to_le_bytes()); // entry
    b[52..54].copy_from_slice(&64u16.to_le_bytes()); // ehsize
    b[54..56].copy_from_slice(&56u16.to_le_bytes()); // phentsize
    b[58..60].copy_from_slice(&64u16.to_le_bytes()); // shentsize
    if symtab {
        let names = b"\0.shstrtab\0.symtab\0";
        b.extend_from_slice(names);
        b.resize(128, 0);
        let sh = |name: u32, ty: u32, off: u64, size: u64, link: u32, ent: u64| {
            let mut s = vec![0u8; 64];
            s[0..4].copy_from_slice(&name.to_le_bytes());
            s[4..8].copy_from_slice(&ty.to_le_bytes());
            s[24..32].copy_from_slice(&off.to_le_bytes());
            s[32..40].copy_from_slice(&size.to_le_bytes());
            s[40..44].copy_from_slice(&link.to_le_bytes());
            s[48..56].copy_from_slice(&1u64.to_le_bytes());
            s[56..64].copy_from_slice(&ent.to_le_bytes());
            s
        };
        b.extend(vec![0u8; 64]); // SHN_UNDEF
        b.extend(sh(1, 3, 64, names.len() as u64, 0, 0)); // .shstrtab STRTAB
        b.extend(sh(11, 2, 64, 0, 1, 24)); // .symtab SYMTAB
        b[40..48].copy_from_slice(&128u64.to_le_bytes()); // shoff
        b[60..62].copy_from_slice(&3u16.to_le_bytes()); // shnum
        b[62..64].copy_from_slice(&1u16.to_le_bytes()); // shstrndx
    }
    assert!(b.len() as u64 <= size, "fixture smaller than its header");
    b
}

fn tools_ok() -> bool {
    let ok = |c: &str| {
        Command::new(c)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    if ok("bash") && ok("readelf") {
        true
    } else {
        eprintln!("skip: bash or readelf (binutils) not on PATH");
        false
    }
}

struct Run {
    ok: bool,
    out: String,
}

fn run(args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut c = Command::new("bash");
    c.arg(root().join("scripts/size-budget.sh"))
        .args(args)
        .env_remove("SIZE_BUDGET_FILE");
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c.output().expect("run size-budget.sh");
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    eprintln!("---- size-budget.sh {args:?}\n{out}");
    Run {
        ok: o.status.success(),
        out,
    }
}

/// Fresh fixture dir with `files` = (name, size, symtab).
fn fixture(tag: &str, files: &[(&str, u64, bool)]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("size-budget-353-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (n, s, sym) in files {
        // header, then a sparse tail up to the size (200 MB fixtures cost no disk)
        std::fs::write(d.join(n), elf(*s, *sym)).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(d.join(n))
            .unwrap()
            .set_len(*s)
            .unwrap();
    }
    d
}

/// The row of `name` in the script output: (status, budget, key).
fn row<'a>(out: &'a str, name: &str) -> (&'a str, &'a str, &'a str) {
    let l = out
        .lines()
        .find(|l| l.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("no row for {name}"));
    let f: Vec<&str> = l.split_whitespace().collect();
    assert_eq!(f.len(), 7, "row shape: {l}");
    (f[4], f[5], f[6])
}

#[test]
fn script_passes_full_tagged_set_at_budget() {
    if !tools_ok() {
        return;
    }
    // Every release name, tagged like the slot 339 set, the gateway at
    // exactly 200 MB (the budget is inclusive).
    let names: Vec<String> = RELEASE_BINS
        .iter()
        .map(|b| match *b {
            "gaia-cortex" => "gaia-cortex-77df80f-llm".to_string(),
            "galion-console-serve" => "galion-console-serve-2d846fb".to_string(),
            b => format!("{b}-124a212"),
        })
        .collect();
    let mut files: Vec<(&str, u64, bool)> =
        names.iter().map(|n| (n.as_str(), 4096, false)).collect();
    files[8].1 = BUDGET; // hermes-gateway-124a212
    files.push(("not-in-table", 4096, false));
    std::fs::create_dir_all(std::env::temp_dir()).unwrap();
    let d = fixture("full", &files);
    std::fs::write(d.join("README.txt"), "not an ELF").unwrap();
    std::fs::write(d.join("libx.rlib"), "skip").unwrap();
    let r = run(&["--check-dir", d.to_str().unwrap()], &[]);
    assert!(r.ok, "full set must pass");
    assert!(r.out.contains("size-budget: ok (15 binaries)"));
    assert!(r.out.contains("budget_file ") && r.out.contains("config/release/size-budget.tsv"));
    assert_eq!(
        row(&r.out, "hermes-gateway-124a212"),
        ("ok", "209715200", "hermes-gateway")
    );
    assert_eq!(row(&r.out, "gaia-cortex-77df80f-llm").2, "gaia-cortex");
    assert_eq!(
        row(&r.out, "galion-console-serve-2d846fb").2,
        "galion-console-serve"
    );
    assert_eq!(row(&r.out, "not-in-table").2, "default");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn script_fails_negative_controls() {
    if !tools_ok() {
        return;
    }
    // 1. hermes-gateway: the slot 339 size (2,198,288 B, over the old 2 MiB)
    //    passes under the 200 MB budget (t416u); one byte over 200 MB fails.
    for name in ["hermes-gateway-124a212", "hermes-gateway"] {
        let d = fixture(
            "gw339",
            &[(name, 2_198_288, false), ("tuner-124a212", 4096, false)],
        );
        let r = run(&["--check-dir", d.to_str().unwrap()], &[]);
        assert!(
            r.ok,
            "{name} 2,198,288 B (> old {OLD_MIB2} B) must pass now"
        );
        assert_eq!(row(&r.out, name), ("ok", "209715200", "hermes-gateway"));
        let _ = std::fs::remove_dir_all(&d);
        let d = fixture(
            "gwover",
            &[(name, BUDGET + 1, false), ("tuner-124a212", 4096, false)],
        );
        let r = run(&["--check-dir", d.to_str().unwrap()], &[]);
        assert!(!r.ok, "{name} 200 MB + 1 B must fail");
        assert_eq!(row(&r.out, name), ("FAIL", "209715200", "hermes-gateway"));
        assert_eq!(row(&r.out, "tuner-124a212").0, "ok");
        assert!(r.out.contains("size-budget: FAIL"));
        let _ = std::fs::remove_dir_all(&d);
    }
    // 2. One byte over.
    let d = fixture("over1", &[("openmind-nerve-124a212", BUDGET + 1, false)]);
    let r = run(&["--check-dir", d.to_str().unwrap()], &[]);
    assert!(!r.ok);
    assert_eq!(row(&r.out, "openmind-nerve-124a212").0, "FAIL");
    let _ = std::fs::remove_dir_all(&d);
    // 3. Not stripped (.symtab), small.
    let d = fixture("symtab", &[("tuner", 8192, true)]);
    let r = run(&["--check-dir", d.to_str().unwrap()], &[]);
    assert!(!r.ok, "a .symtab must fail");
    assert!(r.out.contains(" no ") && row(&r.out, "tuner").0 == "FAIL");
    let _ = std::fs::remove_dir_all(&d);
    // 4. A tighter per-binary row in a custom table (flag and env).
    let d = fixture(
        "tight",
        &[("tuner-124a212", 4096, false), ("galion-one", 4096, false)],
    );
    let t = d.join("budget.tsv");
    std::fs::write(&t, "# test\ndefault 209715200\ntuner 4000 # tight\n").unwrap();
    for (args, env) in [
        (
            vec![
                "--check-dir",
                d.to_str().unwrap(),
                "--budget-file",
                t.to_str().unwrap(),
            ],
            vec![],
        ),
        (
            vec!["--check-dir", d.to_str().unwrap()],
            vec![("SIZE_BUDGET_FILE", t.to_str().unwrap())],
        ),
    ] {
        let r = run(&args, &env);
        assert!(!r.ok, "tuner over its 4000 B row must fail");
        assert_eq!(row(&r.out, "tuner-124a212"), ("FAIL", "4000", "tuner"));
        assert_eq!(row(&r.out, "galion-one"), ("ok", "209715200", "default"));
    }
    // 5. Bad tables fail before any check.
    for (tag, body, msg) in [
        ("nodefault", "tuner 4000\n", "no 'default' row"),
        ("dup", "default 1\ndefault 2\n", "duplicate row"),
        ("bytes", "default 2MiB\n", "bad byte count"),
        ("shape", "default 1 2\n", "want '<binary> <max bytes>'"),
        ("glob", "default 209715200\n* 1\n", "bad binary name"),
    ] {
        std::fs::write(&t, body).unwrap();
        let r = run(
            &[
                "--check-dir",
                d.to_str().unwrap(),
                "--budget-file",
                t.to_str().unwrap(),
            ],
            &[],
        );
        assert!(!r.ok, "{tag} must fail");
        assert!(r.out.contains(msg), "{tag}: want {msg:?}");
    }
    let r = run(
        &[
            "--check-dir",
            d.to_str().unwrap(),
            "--budget-file",
            "/nonexistent/x.tsv",
        ],
        &[],
    );
    assert!(!r.ok && r.out.contains("budget file not found"));
    let _ = std::fs::remove_dir_all(&d);
    // 6. Empty dir: no ELF executables.
    let d = fixture("empty", &[]);
    let r = run(&["--check-dir", d.to_str().unwrap()], &[]);
    assert!(!r.ok && r.out.contains("no ELF executables"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn real_release_set() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: GALION_REAL!=1");
        return;
    }
    if !tools_ok() {
        return;
    }
    if let Ok(dir) = std::env::var("SIZE_BUDGET_REAL_DIR") {
        let r = run(&["--check-dir", &dir], &[]);
        assert!(r.ok, "real set {dir} must pass");
        let n = r
            .out
            .lines()
            .filter(|l| l.split_whitespace().nth(4) == Some("ok"))
            .count();
        let want: usize = std::env::var("SIZE_BUDGET_REAL_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(14);
        assert_eq!(n, want, "binaries checked in {dir}");
        assert!(r
            .out
            .contains(&format!("size-budget: ok ({want} binaries)")));
    }
    if let Ok(dir) = std::env::var("SIZE_BUDGET_NEG_DIR") {
        let bin = std::env::var("SIZE_BUDGET_NEG_BIN").expect("SIZE_BUDGET_NEG_BIN");
        let r = run(&["--check-dir", &dir], &[]);
        assert!(!r.ok, "negative control {dir} must fail");
        assert_eq!(row(&r.out, &bin).0, "FAIL");
    }
}
