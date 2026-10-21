// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the launch gates (Rust port of the old
//! `scripts/real/launch-gates.sh`, slot 15). No network service is started;
//! the "real binary" is the public-export pipeline (`scripts/public-export.sh`)
//! and the third-party scanners it calls, run against the real repository.
//!
//! * A. Export HEAD for real and run every gate. Expect 0 FAIL.
//! * B. `cargo check --workspace --locked --offline` on the EXPORTED tree
//!   (the public tree builds without contrib/, stack/kms/, dist/bin/ ...).
//!   `LG_SKIP_BUILD=1` skips it.
//! * C. Canary run: a throw-away clone gets planted violations (a generated
//!   fake AWS-shaped key, a GPL path crate, an ELF binary, a GPL SPDX header,
//!   the contrib/ export rule removed, a new `*.redacted` file). Gates 2, 3,
//!   5, 6, 7 and 8 must FAIL and the `*.redacted` canary must stay out.
//!
//! Gated: runs only with `GALION_REAL=1`. Tools come from `$GALION_TOOLS_BIN`
//! or `PATH`. `LG_ALLOW_MISSING_TOOLS=1` passes `--allow-missing-tools`: a
//! missing scanner is then a SKIP and its canary check is reported as
//! skipped, not passed. Output under `$LG_OUT` (default: a scratch dir).
//!
//! ```text
//! GALION_REAL=1 GALION_TOOLS_BIN=/path/to/tools/bin \
//!   cargo test --release -p repo-check --test real_launch_gates -- --nocapture
//! ```

use galion_realtest::realrun::{self, Checks, Scratch};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `| N name | RESULT | detail |` rows of a public-export REPORT.md.
fn gate_rows(report: &Path) -> Vec<(String, String)> {
    std::fs::read_to_string(report)
        .unwrap_or_default()
        .lines()
        .filter(|l| {
            l.strip_prefix("| ")
                .and_then(|r| r.chars().next())
                .is_some_and(|ch| ch.is_ascii_digit())
        })
        .filter_map(|l| {
            let cols: Vec<&str> = l.split('|').map(str::trim).collect();
            (cols.len() > 3).then(|| (cols[1].to_string(), cols[2].to_string()))
        })
        .collect()
}

fn print_rows(rows: &[(String, String)], report: &Path) {
    let text = std::fs::read_to_string(report).unwrap_or_default();
    for (name, _) in rows {
        if let Some(l) = text.lines().find(|l| l.starts_with(&format!("| {name} |"))) {
            println!("     {l}");
        }
    }
}

/// Random chars from `alphabet`, read from /dev/urandom. Never printed.
fn random_from(alphabet: &[u8], n: usize) -> String {
    let mut f = std::fs::File::open("/dev/urandom").expect("/dev/urandom");
    let mut out = String::new();
    let mut b = [0u8; 1];
    while out.len() < n {
        f.read_exact(&mut b).unwrap();
        let i = usize::from(b[0]);
        // rejection sampling keeps the distribution uniform
        if i < 256 - 256 % alphabet.len() {
            out.push(char::from(alphabet[i % alphabet.len()]));
        }
    }
    out
}

fn export(script: &Path, out: &Path, log: &Path, allow_missing: bool) -> Option<i32> {
    let f = std::fs::File::create(log).unwrap();
    let mut cmd = Command::new(script);
    cmd.arg("--out").arg(out);
    if allow_missing {
        cmd.arg("--allow-missing-tools");
    }
    cmd.stdout(f.try_clone().unwrap())
        .stderr(f)
        .status()
        .expect("run public-export.sh")
        .code()
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=canary", "-c", "user.email=canary@invalid"])
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn export_gates_pass_and_canaries_are_caught() {
    if !realrun::enabled("real_launch_gates") {
        return;
    }
    let root = realrun::repo_root();
    let scratch = Scratch::new("launch-gates");
    let out: PathBuf = std::env::var_os("LG_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.0.clone());
    std::fs::create_dir_all(&out).unwrap();
    let allow_missing = std::env::var("LG_ALLOW_MISSING_TOOLS").as_deref() == Ok("1");
    let mut c = Checks::new();

    realrun::say("A. real export of HEAD");
    let rc = export(
        &root.join("scripts/public-export.sh"),
        &out.join("A"),
        &out.join("A.log"),
        allow_missing,
    );
    let rows = gate_rows(&out.join("A/REPORT.md"));
    print_rows(&rows, &out.join("A/REPORT.md"));
    c.eq("public-export HEAD exit 0", rc, Some(0));
    c.that("report has 10 gate rows", rows.len() >= 10);
    let fails: Vec<&String> = rows
        .iter()
        .filter(|r| r.1 == "FAIL")
        .map(|r| &r.0)
        .collect();
    c.eq("HEAD: 0 FAIL gates", fails, Vec::<&String>::new());
    let skipped: Vec<String> = rows
        .iter()
        .filter(|r| r.1 == "SKIP")
        .map(|r| r.0.clone())
        .collect();
    if !skipped.is_empty() {
        realrun::say(format!("SKIPPED gates (tools missing): {skipped:?}"));
    }

    realrun::say("B. cargo check of the exported tree");
    if std::env::var("LG_SKIP_BUILD").as_deref() == Ok("1") {
        realrun::say("skip (LG_SKIP_BUILD=1)");
    } else {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let log = std::fs::File::create(out.join("B.log")).unwrap();
        let st = Command::new("nice")
            .args([
                "-n",
                "10",
                &cargo,
                "check",
                "--workspace",
                "--locked",
                "--offline",
                "-j",
                "2",
                "-q",
            ])
            .current_dir(out.join("A/tree"))
            .env("CARGO_TARGET_DIR", out.join("B-target"))
            .env("CARGO_INCREMENTAL", "0")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .status()
            .expect("cargo check");
        c.that(
            "exported workspace: cargo check --workspace --locked",
            st.success(),
        );
        let _ = std::fs::remove_dir_all(out.join("B-target"));
    }

    realrun::say("C. canary run");
    let canary = out.join("C-repo");
    let _ = std::fs::remove_dir_all(&canary);
    assert!(Command::new("git")
        .args(["clone", "-q", "--no-hardlinks"])
        .arg(&root)
        .arg(&canary)
        .status()
        .unwrap()
        .success());
    let head = String::from_utf8(
        realrun::run(
            Command::new("git")
                .current_dir(&root)
                .args(["rev-parse", "HEAD"]),
        )
        .stdout,
    )
    .unwrap();
    assert!(git(&canary, &["checkout", "-q", "--detach", head.trim()]));
    // 1. secret: a random AWS-shaped access key id, built at run time so this
    //    file never contains one. The values are never printed.
    let key = format!(
        "AKIA{}",
        random_from(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567", 16)
    );
    let sec = random_from(
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
        40,
    );
    std::fs::write(
        canary.join("docs/canary-leak.txt"),
        format!("aws_access_key_id = {key}\naws_secret_access_key = {sec}\n"),
    )
    .unwrap();
    // 2. GPL dependency inside the workspace. The licence strings are split
    //    so this file itself never carries a GPL SPDX header.
    let gpl = ["GPL", "-3.0-only"].concat();
    let spdx = ["SPDX-License", "-Identifier"].concat();
    std::fs::create_dir_all(canary.join("crates/zz-gpl-canary/src")).unwrap();
    std::fs::write(
        canary.join("crates/zz-gpl-canary/Cargo.toml"),
        format!("[package]\nname = \"zz-gpl-canary\"\nversion = \"0.1.0\"\nedition = \"2021\"\nlicense = \"{gpl}\"\npublish = false\n"),
    )
    .unwrap();
    std::fs::write(
        canary.join("crates/zz-gpl-canary/src/lib.rs"),
        format!("// {spdx}: {gpl}\n// SPDX-FileCopyrightText: 3000 Canary\n"),
    )
    .unwrap();
    // 3. an ELF binary
    std::fs::copy("/bin/true", canary.join("docs/canary-elf")).unwrap();
    // 4. contrib/ export rule removed
    let ga = canary.join(".gitattributes");
    let attrs = std::fs::read_to_string(&ga).unwrap_or_default();
    let kept: Vec<&str> = attrs
        .lines()
        .filter(|l| !l.starts_with("/contrib export-ignore"))
        .collect();
    std::fs::write(&ga, kept.join("\n") + "\n").unwrap();
    // 5. a new redacted inventory file (must stay out via *.redacted)
    std::fs::create_dir_all(canary.join("stack/hermes")).unwrap();
    std::fs::write(
        canary.join("stack/hermes/canary.env.redacted"),
        "TOKEN=REDACTED\n",
    )
    .unwrap();
    c.that(
        "canary commit",
        git(&canary, &["add", "-A", "-f"]) && git(&canary, &["commit", "-q", "-m", "canary"]),
    );
    // cargo deny needs Cargo.lock to know the new member; refresh it offline.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let _ = Command::new(&cargo)
        .args(["generate-lockfile", "--offline", "-q"])
        .current_dir(&canary)
        .status();
    let _ = git(&canary, &["add", "-A"]) && git(&canary, &["commit", "-q", "-m", "lock"]);

    let rc = export(
        &canary.join("scripts/public-export.sh"),
        &out.join("C"),
        &out.join("C.log"),
        allow_missing,
    );
    let rows = gate_rows(&out.join("C/REPORT.md"));
    print_rows(&rows, &out.join("C/REPORT.md"));
    c.that(
        &format!("canary export exits non-zero ({rc:?})"),
        rc != Some(0),
    );
    for g in [
        "2 path policy",
        "3 no binaries",
        "5 secret patterns",
        "6 gitleaks dir",
        "7 REUSE lint",
        "8 cargo deny",
    ] {
        let res = rows
            .iter()
            .find(|r| r.0.starts_with(g))
            .map(|r| r.1.as_str())
            .unwrap_or("missing");
        if res == "SKIP" && allow_missing {
            realrun::say(format!(
                "SKIP canary gate {g}: tool missing (not counted as pass)"
            ));
        } else {
            c.eq(&format!("canary caught by gate {g}"), res, "FAIL");
        }
    }
    c.that(
        "*.redacted canary export-ignored",
        !out.join("C/tree/stack/hermes/canary.env.redacted").exists(),
    );
    if out.join("C/tree/contrib").exists() {
        c.ok("contrib/ appears once its rule is removed (gate 2 is the backstop)");
    }
    let _ = std::fs::remove_dir_all(&canary);
    let _ = std::fs::remove_dir_all(out.join("C/sbom"));
    // The planted key exists only in the throw-away clone and the export
    // tree under `out`; neither is ever committed.
    let _ = std::fs::remove_dir_all(out.join("C/tree/docs"));
    c.finish("real_launch_gates");
}
