// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Rust port of `scripts/real/docs-sync.sh` (deleted on
//! swarm/realscript-port-1). Real-life check for the docs: builds the real
//! `repo-check` binary (release) and runs it, then the repo's own checks
//! `scripts/spdx-scan.sh`, `scripts/self-check.sh` (light;
//! `SELF_CHECK_FULL=1` for full), `scripts/no-python.sh` and
//! `scripts/status-percent.sh` (those four are still shell scripts outside
//! `scripts/real/`; their Rust ports are tracked in
//! /workspace/swarm/plans/non-rust-inventory.md, not here). Fails unless
//! every one is green, repo-check reports 0 warnings, status-percent prints
//! exactly the numbers in docs/STATUS.md, and the STATUS.md swarm table
//! lists every real-test branch. Read-only on the tree; starts no server.
//!
//! Run: `GALION_REAL=1 cargo test -p galion-realtest --test real_sh_docs_sync -- --nocapture`

use galion_realtest::script::*;
use std::process::Command;

fn sh(rel: &str) -> std::process::Output {
    let mut c = Command::new(repo_root().join(rel));
    c.current_dir(repo_root());
    run(c)
}

/// The fenced block in STATUS.md that starts at `status-percent: items`.
fn want_block(status: &str) -> String {
    let mut on = false;
    let mut out = Vec::new();
    for l in status.lines() {
        if l.starts_with("status-percent: items") {
            on = true;
        }
        if on {
            if l.starts_with("```") {
                break;
            }
            out.push(l);
        }
    }
    out.join("\n")
}

#[test]
fn real_sh_docs_sync() {
    if !enabled("docs-sync") {
        return;
    }
    let root = repo_root();
    let mut ck = Checks::new();
    say("docs-sync");
    build(&["-p", "repo-check"]);
    let rc_bin = need_bin(&release_bin("repo-check"));

    let mut c = Command::new(&rc_bin);
    c.current_dir(&root);
    let o = run(c);
    let text = out(&o);
    let summary = text.lines().last().unwrap_or("").to_string();
    say(&format!("repo-check exit {}: {summary}", code(&o)));
    if !ck.ok(
        "repo-check clean",
        summary.ends_with(" 0 error(s), 0 warning(s) -> PASS"),
        &summary,
    ) {
        println!("{text}");
    }
    ck.eq("repo-check exit", "0", code(&o).to_string());

    let o = sh("scripts/spdx-scan.sh");
    let sp = out(&o);
    let missing = sp
        .lines()
        .find_map(|l| l.strip_prefix("spdx-scan: missing "))
        .unwrap_or("?")
        .trim()
        .to_string();
    let scanned = sp
        .lines()
        .find_map(|l| l.strip_prefix("spdx-scan: scanned "))
        .unwrap_or("?")
        .trim()
        .to_string();
    say(&format!("spdx-scan: {scanned} scanned, {missing} missing"));
    if !ck.eq("spdx-scan missing", "0", &missing) {
        println!("{sp}");
    }

    let o = sh("scripts/self-check.sh");
    let log = format!("{}{}", out(&o), err(&o));
    if !ck.ok(
        "self-check",
        o.status.success(),
        log.lines().last().unwrap_or(""),
    ) {
        println!("{log}");
    }

    let o = sh("scripts/no-python.sh");
    if !ck.ok("no-python", o.status.success(), "") {
        println!("{}{}", out(&o), err(&o));
    }

    let o = sh("scripts/status-percent.sh");
    let got = out(&o).trim_end().to_string();
    ck.ok("status-percent ran", o.status.success(), "");
    let status = read(&root.join("docs/STATUS.md"));
    let want = want_block(&status);
    println!("{got}");
    if !ck.eq("status-percent matches docs/STATUS.md", &want, &got) {
        println!("STATUS.md block:\n{want}");
    }

    for b in [
        "gaia-land",
        "hades-approval",
        "gaia-live",
        "survival-heal",
        "one-probe",
        "voice-mouth",
        "tuner-cutover",
        "room",
        "memory-real",
        "nerve",
        "lan-discovery",
        "poseidon",
        "ucpu",
        "real-harness",
        "launch-gates",
        "common",
        "hermes-tools",
        "desk-linkwatch",
        "sandbox",
        "edge-boot",
        "docs-sync",
    ] {
        let needle = format!("`swarm/{b}`");
        if !status.contains(&needle) {
            ck.ok(&format!("STATUS.md lists swarm/{b}"), false, "missing");
        }
    }
    ck.ok("STATUS.md lists all 21 real-test branches", true, "");
    let rows = status
        .lines()
        .filter(|l| {
            l.strip_prefix("| ")
                .map(|r| {
                    let n = r.chars().take_while(|c| c.is_ascii_digit()).count();
                    r[n..].starts_with(" | `swarm/")
                })
                .unwrap_or(false)
        })
        .count();
    say(&format!("STATUS.md swarm table: {rows} branches"));
    ck.finish("docs-sync");
}

#[test]
fn status_block_extraction() {
    let s = "x\n```\nstatus-percent: items 1\ndone: 2\n```\nrest";
    assert_eq!(want_block(s), "status-percent: items 1\ndone: 2");
}
