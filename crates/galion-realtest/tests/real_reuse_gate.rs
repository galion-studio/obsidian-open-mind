// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 267 (reuse-gate-fix): regression guard for publish blocker B10,
//! "REUSE gate 7 fails (7 files)" (integration-5 0d68f46: bench-banking
//! axes.rs + bench-scoreboard lib.rs hold SPDX header lines inside string
//! fixtures, and 5 vendored candle-core binary test fixtures had no licence
//! info). main-staging 7c95348b fixed them with REUSE.toml overrides and
//! `.license` sidecars; these tests keep it fixed.
//!
//! * `reuse_b10_static` always runs: reads REUSE.toml, the sidecars and
//!   LICENSES/ (no external tool).
//! * `real_reuse_lint_head` runs only with `GALION_REAL=1`. It runs the real
//!   REUSE linter `comply` (Rust, MIT OR Apache-2.0; `$GALION_TOOLS_BIN`
//!   first, then `$PATH`) over `git archive HEAD` exactly like gate 7 of
//!   scripts/public-export.sh, expects 0 problems, then runs two negative
//!   controls: (a) a tiny tree with one header-less file must fail, and
//!   (b) the HEAD tree with the B10 fix undone (one sidecar deleted, the
//!   bench-banking override removed) must fail on exactly those files.
//!   The trees live under the system temp dir, not under target/ (comply
//!   0.1.1 matches .gitignore patterns against the absolute path).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The five vendored candle-core binary fixtures that need `.license` sidecars.
const SIDECARS: [&str; 5] = [
    "vendor/candle-core/tests/fortran_tensor_3d.pth",
    "vendor/candle-core/tests/test.npy",
    "vendor/candle-core/tests/test.npz",
    "vendor/candle-core/tests/test.pt",
    "vendor/candle-core/tests/test_with_key.pt",
];

/// The two source files whose string fixtures contain SPDX header lines.
const OVERRIDES: [&str; 2] = [
    "crates/bench-banking/src/axes.rs",
    "crates/bench-scoreboard/src/lib.rs",
];

/// Tag names built at run time so this file holds no literal SPDX tag beyond
/// its own header (the linter would read them as this file's licensing).
fn tag(name: &str) -> String {
    format!("SPDX-{name}:")
}

/// The `[[annotations]]` blocks of REUSE.toml as raw text chunks.
fn annotation_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if line.trim() == "[[annotations]]" {
            if let Some(b) = cur.take() {
                out.push(b);
            }
            cur = Some(String::new());
        } else if let Some(b) = cur.as_mut() {
            b.push_str(line);
            b.push('\n');
        }
    }
    out.extend(cur);
    out
}

/// Licence identifiers named in an SPDX expression (`MIT OR Apache-2.0`).
fn ids(expr: &str) -> Vec<String> {
    expr.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .filter(|w| !w.is_empty() && !matches!(*w, "OR" | "AND" | "WITH"))
        .map(|w| w.trim_matches('"').to_string())
        .collect()
}

#[test]
fn reuse_b10_static() {
    let r = root();
    let toml = std::fs::read_to_string(r.join("REUSE.toml")).expect("REUSE.toml");
    let blocks = annotation_blocks(&toml);
    assert!(!blocks.is_empty(), "REUSE.toml has no [[annotations]]");

    // 1. the string-fixture files are in an override block, so the SPDX
    //    lines inside their strings are not read as their licensing.
    for f in OVERRIDES {
        assert!(r.join(f).is_file(), "{f} missing");
        let quoted = format!("\"{f}\"");
        let hit = blocks
            .iter()
            .any(|b| b.contains(&quoted) && b.contains("precedence = \"override\""));
        assert!(
            hit,
            "{f} is not in a precedence=override block of REUSE.toml"
        );
    }

    // 2. every binary fixture has a sidecar with both tags and licence texts.
    let lic_tag = tag("License-Identifier");
    let cr_tag = tag("FileCopyrightText");
    for f in SIDECARS {
        assert!(r.join(f).is_file(), "{f} missing");
        let side = format!("{f}.license");
        let text = std::fs::read_to_string(r.join(&side)).unwrap_or_else(|e| panic!("{side}: {e}"));
        let lic = text
            .lines()
            .find_map(|l| l.trim().strip_prefix(lic_tag.as_str()))
            .unwrap_or_else(|| panic!("{side}: no licence identifier"));
        assert!(
            text.lines().any(|l| l.trim().starts_with(cr_tag.as_str())),
            "{side}: no copyright line"
        );
        for id in ids(lic.trim()) {
            assert!(
                r.join("LICENSES").join(format!("{id}.txt")).is_file(),
                "{side}: LICENSES/{id}.txt missing"
            );
        }
    }

    // 3. every licence named in REUSE.toml has its text in LICENSES/.
    let key = format!("SPDX-{}", "License-Identifier");
    for line in toml.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix(key.as_str()) {
            let v = v.trim_start().trim_start_matches('=').trim();
            for id in ids(v) {
                assert!(
                    r.join("LICENSES").join(format!("{id}.txt")).is_file(),
                    "REUSE.toml names {id} but LICENSES/{id}.txt is missing"
                );
            }
        }
    }
}

fn comply() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("GALION_TOOLS_BIN") {
        let p = Path::new(&dir).join("comply");
        if p.is_file() {
            return Some(p);
        }
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("comply"))
            .find(|p| p.is_file())
    })
}

/// Fresh directory under the system temp dir (never under target/).
fn temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let d = std::env::temp_dir().join(format!("reuse267-{label}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

/// `git archive HEAD | tar -x -C dir` (honours export-ignore like gate 1).
fn archive_head(dir: &Path) {
    let mut git = Command::new("git")
        .arg("-C")
        .arg(root())
        .args(["archive", "--format=tar", "HEAD"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("git archive");
    let tar = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dir)
        .stdin(git.stdout.take().expect("pipe"))
        .status()
        .expect("tar");
    assert!(
        git.wait().expect("git wait").success(),
        "git archive failed"
    );
    assert!(tar.success(), "tar -x failed");
}

/// Runs `comply lint [--lines] dir`: (exit ok, stdout+stderr).
fn lint(bin: &Path, dir: &Path, lines: bool) -> (bool, String) {
    let mut c = Command::new(bin);
    c.arg("lint");
    if lines {
        c.arg("--lines");
    }
    let o = c.arg(dir).output().expect("run comply");
    let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&o.stderr));
    (o.status.success(), s)
}

#[test]
fn real_reuse_lint_head() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1 (needs comply and git)");
        return;
    }
    let bin = comply().expect("GALION_REAL=1 but comply not found in $GALION_TOOLS_BIN or $PATH");

    // Positive: HEAD tree, same as public-export gate 7.
    let head = temp_dir("head");
    archive_head(&head);
    let (ok, out) = lint(&bin, &head, true);
    eprintln!(
        "comply lint --lines HEAD: ok={ok} problems={}",
        out.lines().count()
    );
    assert!(
        ok && out.trim().is_empty(),
        "REUSE problems on HEAD:\n{out}"
    );
    let (ok, summary) = lint(&bin, &head, false);
    let total = summary
        .lines()
        .find(|l| l.contains(" total"))
        .unwrap_or("")
        .trim()
        .to_string();
    eprintln!("comply lint HEAD: {total}");
    assert!(
        ok && total.contains(" 0 failed"),
        "summary not clean:\n{summary}"
    );

    // Negative (a): a header-less file must be reported.
    let neg = temp_dir("neg");
    std::fs::create_dir_all(neg.join("LICENSES")).expect("mkdir");
    std::fs::copy(
        root().join("LICENSES/MIT.txt"),
        neg.join("LICENSES/MIT.txt"),
    )
    .expect("copy MIT");
    let header = format!(
        "// {} MIT\n// {} 3000 Galion / galion-studio\n",
        tag("License-Identifier"),
        tag("FileCopyrightText")
    );
    std::fs::write(neg.join("good.rs"), format!("{header}fn a() {{}}\n")).expect("write");
    std::fs::write(neg.join("bad.rs"), "fn b() {}\n").expect("write");
    let (ok, out) = lint(&bin, &neg, true);
    eprintln!("negative (a): ok={ok}\n{out}");
    assert!(!ok, "comply passed a header-less file");
    assert!(
        out.contains("bad.rs") && !out.contains("good.rs"),
        "wrong files flagged:\n{out}"
    );

    // Negative (b): undo the B10 fix in a copy of HEAD; exactly those fail.
    let side = format!("{}.license", SIDECARS[1]);
    std::fs::remove_file(head.join(&side)).expect("remove sidecar");
    let toml = std::fs::read_to_string(head.join("REUSE.toml")).expect("REUSE.toml");
    let entry = format!("\"{}\",", OVERRIDES[0]);
    assert!(toml.contains(&entry), "override entry not found");
    std::fs::write(head.join("REUSE.toml"), toml.replace(&entry, "")).expect("write REUSE.toml");
    let (ok, out) = lint(&bin, &head, true);
    eprintln!("negative (b): ok={ok}\n{out}");
    assert!(!ok, "comply passed with the B10 fix undone");
    let flagged: std::collections::BTreeSet<String> = out
        .lines()
        .filter_map(|l| l.split(": ").next())
        .filter_map(|p| p.strip_prefix(head.to_str().unwrap_or("")))
        .map(|p| p.trim_start_matches('/').to_string())
        .collect();
    let want: std::collections::BTreeSet<String> =
        [SIDECARS[1].to_string(), OVERRIDES[0].to_string()]
            .into_iter()
            .collect();
    assert_eq!(flagged, want, "unexpected flagged set:\n{out}");

    for d in [head, neg] {
        let _ = std::fs::remove_dir_all(d);
    }
}
