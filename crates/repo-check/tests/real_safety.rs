// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-binary test of the `safety` check (slot 229). Gated: GALION_REAL=1.
//!
//! 1. The release/debug `repo-check` binary scans this repository with
//!    `--only safety` and must report 0 findings (every unsafe documented).
//! 2. A throwaway tree with one undocumented block: default run warns and
//!    exits 0 (warn-first), `--error safety` exits 1, `--json` names the
//!    file and line, and adding the comment makes it clean again.

use std::path::{Path, PathBuf};
use std::process::Command;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_repo-check")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

fn run(args: &[&str]) -> (i32, String) {
    let o = Command::new(bin())
        .args(args)
        .output()
        .expect("run repo-check");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

#[test]
fn real_tree_has_no_undocumented_unsafe() {
    if !real() {
        eprintln!("GALION_REAL!=1; skipping");
        return;
    }
    let root = repo_root();
    let (code, out) = run(&["--root", root.to_str().unwrap(), "--only", "safety"]);
    let last = out.lines().last().unwrap_or("");
    eprintln!("{last}");
    assert_eq!(code, 0, "{out}");
    assert!(
        last.contains("0 error(s), 0 warning(s) -> PASS"),
        "undocumented unsafe in tree:\n{out}"
    );
    // The edge node and the sandbox really contain unsafe code (so 0 means
    // documented, not unscanned).
    for f in [
        "edge/openmind-node/src/sandbox.rs",
        "edge/openmind-node/src/seccomp.rs",
        "crates/openmind-sandbox/src/lib.rs",
    ] {
        let t = std::fs::read_to_string(root.join(f)).unwrap();
        assert!(t.contains(concat!("un", "safe {")), "{f}");
    }
}

#[test]
fn real_planted_block_warns_then_errors_then_clears() {
    if !real() {
        eprintln!("GALION_REAL!=1; skipping");
        return;
    }
    let d = std::env::temp_dir().join(format!("repo-check-safety-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("edge/x/src")).unwrap();
    std::fs::create_dir_all(d.join("vendor/y/src")).unwrap();
    std::fs::write(
        d.join("LICENSE"),
        "MIT License\n\nCopyright (c) 3000 Galion / galion-studio\n\nPermission is hereby granted, free of charge, to any person\n",
    )
    .unwrap();
    let kw = concat!("un", "safe");
    let hdr = "// SPDX-License-Identifier: MIT\n";
    let bad = format!("{hdr}fn f() -> i32 {{\n    {kw} {{ libc_getpid() }}\n}}\n");
    std::fs::write(d.join("edge/x/src/lib.rs"), &bad).unwrap();
    // vendored code is out of scope
    std::fs::write(d.join("vendor/y/src/lib.rs"), &bad).unwrap();
    let root = d.to_str().unwrap();

    let (code, out) = run(&["--root", root, "--only", "safety"]);
    eprintln!("{out}");
    assert_eq!(code, 0, "warn-first must not fail: {out}");
    assert!(out.contains("warn  safety   edge/x/src/lib.rs:3:"), "{out}");
    assert!(!out.contains("vendor/"), "{out}");
    assert!(out.contains("0 error(s), 1 warning(s) -> PASS"), "{out}");

    let (code, out) = run(&["--root", root, "--only", "safety", "--error", "safety"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("1 error(s), 0 warning(s) -> FAIL"), "{out}");

    let (code, js) = run(&["--root", root, "--only", "safety", "--json"]);
    assert_eq!(code, 0);
    assert!(
        js.contains(
            "\"check\":\"safety\",\"level\":\"warn\",\"file\":\"edge/x/src/lib.rs\",\"line\":3"
        ),
        "{js}"
    );
    assert!(js.contains("+0"), "Zagreb offset in ts: {js}");

    let good = bad.replace(
        &format!("    {kw} {{"),
        &format!("    // SAFETY: getpid has no preconditions.\n    {kw} {{"),
    );
    std::fs::write(d.join("edge/x/src/lib.rs"), good).unwrap();
    let (code, out) = run(&["--root", root, "--only", "safety", "--error", "safety"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("0 error(s), 0 warning(s) -> PASS"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}
