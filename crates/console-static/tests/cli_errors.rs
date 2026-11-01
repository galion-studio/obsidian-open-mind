// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! console-cli-error-short: a bad flag prints one stderr line and exits 2;
//! `--help` still prints the full usage on stdout and exits 0.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_galion-console-serve")
}

#[test]
fn bad_flag_is_one_stderr_line_exit_2() {
    for argv in [
        &["--local-api", "--phone", "--status-dir", "relative/dir"][..],
        &["--no-such-flag"][..],
        &["--bind", "not-an-address"][..],
    ] {
        let out = Command::new(bin()).args(argv).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{argv:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(err.lines().count(), 1, "{argv:?}: {err}");
        assert!(err.starts_with("galion-console-static: "), "{err}");
        assert!(err.contains("--help"), "{err}");
        assert!(out.stdout.is_empty(), "{argv:?}");
    }
}

#[test]
fn help_still_prints_the_whole_usage() {
    let out = Command::new(bin()).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("usage: galion-console-static"), "{text}");
    assert!(text.lines().count() > 50, "{}", text.lines().count());
}
