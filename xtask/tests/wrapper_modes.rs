// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The built xtask binary, hard-linked as `galion-cc` / `galion-ar`, execs
//! `$GALION_ZIG` with the rewritten arguments. `/bin/echo` stands in for zig
//! so the exact argv is visible (no C compiler needed for this test).

use std::path::PathBuf;
use std::process::Command;

fn alias(name: &str) -> PathBuf {
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_xtask"));
    let dir = std::env::temp_dir().join(format!("xtask-modes-{}", std::process::id()));
    xtask::install_alias(&exe, &dir, name).unwrap()
}

fn run(name: &str, target: Option<&str>, args: &[&str]) -> (i32, String, String) {
    let mut c = Command::new(alias(name));
    c.args(args)
        .env("GALION_ZIG", "/bin/echo")
        .env_remove("TARGET");
    if let Some(t) = target {
        c.env("TARGET", t);
    }
    let o = c.output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).trim().to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

#[test]
fn cc_mode_armv7_real_cc_rs_args() {
    let (code, out, _) = run(
        "galion-cc",
        Some("armv7-unknown-linux-musleabihf"),
        &[
            "-Oz",
            "--target=arm-unknown-linux-musleabihf",
            "-march=armv7-a",
            "-c",
            "sqlite3.c",
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "cc -target arm-linux-musleabihf -Oz -mcpu=generic+v7a -c sqlite3.c" // ucpu-gate: allow
    );
}

#[test]
fn cc_mode_aarch64_and_android() {
    let (_, out, _) = run(
        "galion-cc",
        Some("aarch64-unknown-linux-musl"),
        &["-E", "x.c"],
    );
    assert_eq!(out, "cc -target aarch64-linux-musl -E x.c");
    let (_, out, _) = run("galion-cc", Some("aarch64-linux-android"), &["-c", "a.c"]);
    assert_eq!(out, "cc -target aarch64-linux-musl -c a.c");
}

#[test]
fn cc_mode_unknown_target_fails() {
    let (code, _, err) = run(
        "galion-cc",
        Some("riscv64gc-unknown-linux-gnu"),
        &["-c", "a.c"],
    );
    assert_eq!(code, 2);
    assert!(err.contains("unsupported target"), "{err}");
    let (code, _, err) = run("galion-cc", None, &["-c", "a.c"]);
    assert_eq!(code, 2, "{err}");
}

#[test]
fn ar_mode_passes_through() {
    let (code, out, _) = run("galion-ar", None, &["cq", "libx.a", "a.o"]);
    assert_eq!(code, 0);
    assert_eq!(out, "ar cq libx.a a.o");
}

#[test]
fn task_mode_rejects_unknown_target() {
    let o = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["cross", "mips-unknown-linux-gnu"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown target"));
    let o = Command::new(env!("CARGO_BIN_EXE_xtask")).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "no args -> usage");
}
