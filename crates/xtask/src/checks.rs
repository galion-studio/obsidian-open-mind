// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Repository text gates, ported 1:1 from the inline bash that used to live in
// .github/workflows/self-eval.yml (no-python, cjk-scan, zagreb-timestamps,
// license). Annotations use the GitHub `::error::` / `::warning::` syntax, which
// prints as plain text locally.

use crate::run::{tracked_files, Res};
use std::path::Path;

// ---------------------------------------------------------------- no-python

/// True for paths git's `*.py` / `*.pyc` / `*.pyw` pathspecs match.
pub fn is_python_path(p: &str) -> bool {
    p.ends_with(".py") || p.ends_with(".pyc") || p.ends_with(".pyw")
}

/// True when the first line is a shebang naming python (`^#!.*python`).
pub fn has_python_shebang(content: &[u8]) -> bool {
    let first = content.split(|b| *b == b'\n').next().unwrap_or(&[]);
    first.starts_with(b"#!") && first.windows(6).any(|w| w == b"python")
}

pub fn no_python(root: &Path) -> Res {
    let files = tracked_files(root)?;
    let mut bad = 0usize;
    for f in files.iter().filter(|f| is_python_path(f)) {
        println!("::error::Python files are not allowed in this repository: {f}");
        bad += 1;
    }
    for f in &files {
        let Ok(bytes) = read_head(&root.join(f), 512) else {
            continue; // deleted in the work tree, a directory (submodule), ...
        };
        if has_python_shebang(&bytes) {
            println!("::error file={f}::python shebang");
            bad += 1;
        }
    }
    if bad == 0 {
        println!("ok: no Python files, no python shebangs");
        Ok(())
    } else {
        Err(format!("{bad} Python finding(s)"))
    }
}

fn read_head(p: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::with_capacity(n);
    std::fs::File::open(p)?
        .take(n as u64)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

// ---------------------------------------------------------------------- cjk

/// The ranges the old `grep -P` scan used: CJK radicals, CJK symbols and
/// punctuation, Hiragana, Katakana, CJK ext. A, CJK unified, Hangul
/// syllables, CJK compatibility ideographs.
const CJK: [(u32, u32); 7] = [
    (0x2E80, 0x2EFF),
    (0x3000, 0x303F),
    (0x3040, 0x30FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xAC00, 0xD7AF),
    (0xF900, 0xFAFF),
];

pub fn is_cjk(c: char) -> bool {
    let u = c as u32;
    CJK.iter().any(|(lo, hi)| (*lo..=*hi).contains(&u))
}

/// grep -I: a file with a NUL byte in its first 32 KiB is binary.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(32 * 1024).any(|b| *b == 0)
}

/// (line number, line) of every line holding a CJK character.
pub fn cjk_lines(text: &str) -> Vec<(usize, &str)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| l.chars().any(is_cjk))
        .map(|(i, l)| (i + 1, l))
        .collect()
}

pub fn cjk(root: &Path) -> Res {
    let mut hits = 0usize;
    for f in tracked_files(root)? {
        let Ok(bytes) = std::fs::read(root.join(&f)) else {
            continue;
        };
        if is_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in cjk_lines(&text) {
            println!("{f}:{n}:{line}");
            hits += 1;
        }
    }
    if hits == 0 {
        println!("ok: no CJK characters");
        Ok(())
    } else {
        println!("::error::Stray CJK characters found. Remove them.");
        Err(format!("{hits} line(s) with CJK characters"))
    }
}

// ---------------------------------------------------------------- zagreb-ts

fn digits(b: &[u8], i: usize, n: usize) -> bool {
    b.len() >= i + n && b[i..i + n].iter().all(u8::is_ascii_digit)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Matches the old ERE `[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}(:[0-9]{2})?(\.[0-9]+)?Z\b`
/// anywhere in `line`.
pub fn has_utc_stamp(line: &str) -> bool {
    let b = line.as_bytes();
    (0..b.len()).any(|s| {
        if !(digits(b, s, 4)
            && b.get(s + 4) == Some(&b'-')
            && digits(b, s + 5, 2)
            && b.get(s + 7) == Some(&b'-')
            && digits(b, s + 8, 2)
            && b.get(s + 10) == Some(&b'T')
            && digits(b, s + 11, 2)
            && b.get(s + 13) == Some(&b':')
            && digits(b, s + 14, 2))
        {
            return false;
        }
        // Optional groups, tried in every combination like a backtracking ERE.
        let mut ends = vec![s + 16];
        if b.get(s + 16) == Some(&b':') && digits(b, s + 17, 2) {
            ends.push(s + 19);
        }
        let mut cands = Vec::new();
        for e in ends {
            cands.push(e);
            if b.get(e) == Some(&b'.') {
                let mut k = e + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                    cands.push(k);
                }
            }
        }
        cands
            .into_iter()
            .any(|e| b.get(e) == Some(&b'Z') && b.get(e + 1).is_none_or(|n| !is_word(*n)))
    })
}

fn zagreb_skipped(f: &str) -> bool {
    f.ends_with(".lock") || f.starts_with(".github/workflows/")
}

pub fn zagreb_ts(root: &Path) -> Res {
    let mut hits = 0usize;
    for f in tracked_files(root)?.iter().filter(|f| !zagreb_skipped(f)) {
        let Ok(bytes) = std::fs::read(root.join(f)) else {
            continue;
        };
        if is_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if has_utc_stamp(line) {
                println!(
                    "::warning::UTC-style timestamp (use Europe/Zagreb with offset): {f}:{}:{line}",
                    i + 1
                );
                hits += 1;
            }
        }
    }
    if hits == 0 {
        println!("ok: no Z-suffixed timestamps");
    } else {
        println!("zagreb-ts: {hits} warning(s) (warning only, not a failure)");
    }
    Ok(())
}

// ------------------------------------------------------------------ license

pub const COPYRIGHT_LINE: &str = "Copyright (c) 3000 Galion / galion-studio";

/// Checks on the root LICENSE text and the workspace Cargo.toml.
pub fn license_text_problems(license: &str, cargo_toml: &str) -> Vec<&'static str> {
    let mut p = Vec::new();
    if license.lines().next() != Some("MIT License") {
        p.push("LICENSE is not the MIT text");
    }
    if !license.lines().any(|l| l == COPYRIGHT_LINE) {
        p.push("LICENSE copyright line changed");
    }
    if !license.contains("Permission is hereby granted, free of charge") {
        p.push("MIT grant text missing");
    }
    let mit = cargo_toml.lines().any(|l| {
        l.strip_prefix("license")
            .map(|r| r.trim_start().strip_prefix('=').map(str::trim) == Some("\"MIT\""))
            .unwrap_or(false)
    });
    if !mit {
        p.push("Cargo.toml [workspace.package] license is not MIT");
    }
    p
}

pub fn license(root: &Path) -> Res {
    let lic = std::fs::read_to_string(root.join("LICENSE"))
        .map_err(|_| "::error::LICENSE missing".to_string())?;
    let toml = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    let problems = license_text_problems(&lic, &toml);
    for p in &problems {
        println!("::error::{p}");
    }
    let mut copies = 0usize;
    let mut bad = problems.len();
    for dir in ["crates", "edge"] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        let mut paths: Vec<_> = rd.flatten().map(|e| e.path().join("LICENSE")).collect();
        paths.sort();
        for l in paths.iter().filter(|p| p.exists()) {
            copies += 1;
            if std::fs::read(l).ok().as_deref() != Some(lic.as_bytes()) {
                let rel = l.strip_prefix(root).unwrap_or(l);
                println!("::error file={}::differs from root LICENSE", rel.display());
                bad += 1;
            }
        }
    }
    if bad == 0 {
        println!("ok: MIT ({copies} crate LICENSE copies identical)");
        Ok(())
    } else {
        Err(format!("{bad} licence problem(s)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_paths_and_shebangs() {
        assert!(is_python_path("a/b/c.py"));
        assert!(is_python_path("x.pyc"));
        assert!(is_python_path("x.pyw"));
        assert!(!is_python_path("x.pyx"));
        assert!(!is_python_path("python.md"));
        assert!(has_python_shebang(b"#!/usr/bin/env python3\nprint()"));
        assert!(has_python_shebang(b"#!/usr/bin/python"));
        assert!(!has_python_shebang(b"#!/bin/sh\n# python"));
        assert!(!has_python_shebang(b"# python\n"));
        assert!(!has_python_shebang(b""));
    }

    #[test]
    fn cjk_ranges() {
        assert!(is_cjk('\u{4E2D}')); // Han
        assert!(is_cjk('\u{3042}')); // Hiragana
        assert!(is_cjk('\u{30AB}')); // Katakana
        assert!(is_cjk('\u{D55C}')); // Hangul
        assert!(is_cjk('\u{3001}')); // ideographic comma
        assert!(!is_cjk('č'));
        assert!(!is_cjk('ž'));
        assert!(!is_cjk('—'));
        let text = "ok\nbad \u{4E2D}\nok";
        assert_eq!(cjk_lines(text), vec![(2, "bad \u{4E2D}")]);
        assert!(is_binary(b"a\0b"));
        assert!(!is_binary("Zagreb ščćžđ".as_bytes()));
    }

    #[test]
    fn utc_stamp_matcher() {
        assert!(has_utc_stamp("at 2026-10-21T00:55\u{5A} done"));
        assert!(has_utc_stamp("2026-10-21T00:55:01\u{5A}"));
        assert!(has_utc_stamp("x=2026-10-21T00:55:01.123\u{5A},"));
        assert!(has_utc_stamp("2026-10-21T00:55:01.1\u{5A}"));
        assert!(!has_utc_stamp("2026-10-21T00:55:01+02:00"));
        assert!(!has_utc_stamp("2026-10-21T00:55:01\u{5A}ulu"));
        assert!(!has_utc_stamp("2026-10-21 00:55\u{5A}"));
        assert!(!has_utc_stamp("2026-10-21T00:55:0\u{5A}"));
        assert!(!has_utc_stamp(""));
        assert!(zagreb_skipped("Cargo.lock"));
        assert!(zagreb_skipped(".github/workflows/x.yml"));
        assert!(!zagreb_skipped("docs/x.md"));
    }

    #[test]
    fn license_text() {
        let lic = format!("MIT License\n\n{COPYRIGHT_LINE}\n\nPermission is hereby granted, free of charge, to any person\n");
        let toml = "[workspace.package]\nedition = \"2021\"\nlicense = \"MIT\"\n";
        assert!(license_text_problems(&lic, toml).is_empty());
        assert_eq!(
            license_text_problems(&lic, "license = \"Apache-2.0\"\n"),
            vec!["Cargo.toml [workspace.package] license is not MIT"]
        );
        let changed = lic.replace("3000", "2999");
        assert_eq!(
            license_text_problems(&changed, toml),
            vec!["LICENSE copyright line changed"]
        );
        assert_eq!(license_text_problems("", toml).len(), 3);
    }
}

/// The gates against a throw-away git repository with planted findings.
#[cfg(test)]
mod repo_tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let st = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    fn scratch(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        for (p, body) in files {
            let path = dir.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        git(&dir, &["add", "-A"]);
        dir
    }

    #[test]
    fn gates_catch_planted_findings() {
        let lic = format!(
            "MIT License\n\n{COPYRIGHT_LINE}\n\nPermission is hereby granted, free of charge.\n"
        );
        let toml = "[workspace.package]\nlicense = \"MIT\"\n";
        let clean = scratch(
            "clean",
            &[
                ("LICENSE", &lic),
                ("Cargo.toml", toml),
                ("crates/a/LICENSE", &lic),
                ("run.sh", "#!/bin/sh\necho python\n"),
                ("doc.md", "2026-10-21T03:00:00+02:00 Zagreb ščćžđ\n"),
            ],
        );
        assert!(no_python(&clean).is_ok());
        assert!(cjk(&clean).is_ok());
        assert!(license(&clean).is_ok());
        assert!(zagreb_ts(&clean).is_ok());

        let dirty = scratch(
            "dirty",
            &[
                ("LICENSE", &lic),
                ("Cargo.toml", toml),
                ("crates/a/LICENSE", "MIT License\nsomething else\n"),
                ("tool", "#!/usr/bin/env python3\n"),
                ("x/y.py", "print(1)\n"),
                ("doc.md", "line\n\u{6F22}\n2026-10-21T01:00:00\u{5A}\n"),
            ],
        );
        assert_eq!(no_python(&dirty), Err("2 Python finding(s)".into()));
        assert_eq!(cjk(&dirty), Err("1 line(s) with CJK characters".into()));
        assert_eq!(license(&dirty), Err("1 licence problem(s)".into()));
        // Warning-only gate: reports, never fails.
        assert!(zagreb_ts(&dirty).is_ok());
        let _ = std::fs::remove_dir_all(clean);
        let _ = std::fs::remove_dir_all(dirty);
    }
}
