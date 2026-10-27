// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-ccwrap`: a Rust replacement for the generated `cc-<triple>` shell
//! wrappers of `scripts/ucpu-gate.sh`, used by the real UCPU test
//! (`tests/real_sh_ucpu.rs`) to build bundled C code (SQLite) with `zig cc`
//! for foreign musl targets. Test tooling only, never shipped.
//!
//!   CC_<triple>=galion-ccwrap      -> $GALION_ZIG cc -target <zig triple of $TARGET> <args>
//!   AR_<triple>="galion-ccwrap ar" -> $GALION_ZIG ar <args>
//!
//! Rewrites what zig rejects: cc-rs's `--target=<rust triple>` is dropped
//! (the zig triple comes from cargo's `$TARGET`), and `-march=armv7-a` becomes
//! `-mcpu=generic+v7a` (ucpu-gate: allow — zig rewrite of -march=armv7-a, not a host CPU pin). `GALION_ZIG` defaults to `zig` on PATH.

use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

/// Rust target triple -> zig target triple.
pub fn zig_triple(rust: &str) -> Option<&'static str> {
    Some(match rust {
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        "armv7-unknown-linux-musleabihf" => "arm-linux-musleabihf",
        // zig ships no bionic headers: compile against musl (check only).
        "aarch64-linux-android" => "aarch64-linux-musl",
        _ => return None,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zig = std::env::var_os("GALION_ZIG").unwrap_or_else(|| "zig".into());
    let mut cmd = Command::new(zig);
    if args.first().map(String::as_str) == Some("ar") {
        cmd.args(&args);
    } else {
        let rust = std::env::var("TARGET").unwrap_or_default();
        let Some(zt) = zig_triple(&rust) else {
            eprintln!("galion-ccwrap: unsupported or missing $TARGET {rust:?}");
            return ExitCode::from(2);
        };
        cmd.args(["cc", "-target", zt]);
        for a in args {
            match a.as_str() {
                a if a.starts_with("--target=") => {}
                "-march=armv7-a" => {
                    cmd.arg("-mcpu=generic+v7a"); // ucpu-gate: allow — zig rewrite of -march=armv7-a, not a host CPU pin
                }
                _ => {
                    cmd.arg(a);
                }
            }
        }
    }
    let e = cmd.exec();
    eprintln!("galion-ccwrap: exec zig: {e}");
    ExitCode::from(127)
}
