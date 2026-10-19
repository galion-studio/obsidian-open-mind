// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Binary checks for `gaia`. No daemon is spawned; each call exits.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_gaia");

fn run(args: &[&str], override_env: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new(BIN);
    cmd.args(args);
    cmd.env_remove("GAIA_OVERRIDE");
    if let Some(value) = override_env {
        cmd.env("GAIA_OVERRIDE", value);
    }
    cmd.output().expect("gaia exits")
}

fn is_zagreb_rfc3339(stamp: &str) -> bool {
    let b = stamp.as_bytes();
    if b.len() != 25 {
        return false;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    digit(0)
        && digit(1)
        && digit(2)
        && digit(3)
        && b[4] == b'-'
        && digit(5)
        && digit(6)
        && b[7] == b'-'
        && digit(8)
        && digit(9)
        && b[10] == b'T'
        && digit(11)
        && digit(12)
        && b[13] == b':'
        && digit(14)
        && digit(15)
        && b[16] == b':'
        && digit(17)
        && digit(18)
        && b[19] == b'+'
        && digit(20)
        && digit(21)
        && b[22] == b':'
        && digit(23)
        && digit(24)
        && (stamp.ends_with("+01:00") || stamp.ends_with("+02:00"))
}

#[test]
fn status_prints_zagreb_timestamp_and_idle() {
    let out = run(&["status"], None);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.ends_with(" idle\n"), "{text}");
    assert_eq!(text.lines().count(), 1, "{text}");
    let stamp = text.split_whitespace().next().unwrap();
    assert!(is_zagreb_rfc3339(stamp), "{stamp}");
    assert!(!text.contains('Z'), "{text}");
    assert!(out.stderr.is_empty());
}

#[test]
fn tick_is_one_step_and_does_not_loop() {
    let out = run(&["tick"], None);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text, "tick step=1\n");
    let again = run(&["tick"], None);
    assert_eq!(again.stdout, b"tick step=1\n");
    let counted = run(&["tick", "5"], None);
    assert_eq!(counted.status.code(), Some(2));
    assert!(counted.stdout.is_empty());
}

#[test]
fn simulate_faro_is_dry_run_text_only() {
    let out = run(&["simulate-faro"], None);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("dry-run"), "{text}");
    assert!(text.contains("power: unchanged"), "{text}");
    assert!(text.contains("gpio: untouched"), "{text}");
    assert!(text.contains("network: none"), "{text}");
    assert!(!text.contains("systemctl"), "{text}");

    let full = run(&["simulate-faro", "--full-power"], None);
    assert_eq!(full.status.code(), Some(0));
    let full_text = String::from_utf8(full.stdout).unwrap();
    assert!(full_text.contains("power: unchanged"), "{full_text}");
    assert!(full_text.contains("no power change"), "{full_text}");
    assert!(!full_text.contains("systemctl"), "{full_text}");
}

#[test]
fn master_override_prints_refused_and_exits_2() {
    let closed = run(&["--master-override"], None);
    assert_eq!(closed.status.code(), Some(2));
    let closed_text = String::from_utf8(closed.stdout).unwrap();
    assert!(closed_text.starts_with("refused:"), "{closed_text}");
    assert_ne!(closed_text, "refused: not approved\n");

    let open = run(&["--master-override"], Some("1"));
    assert_eq!(open.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(open.stdout).unwrap(),
        "refused: not approved\n"
    );
    assert!(open.stderr.is_empty());

    let with_status = run(&["--master-override", "status"], Some("1"));
    assert_eq!(with_status.status.code(), Some(2));
    let with_text = String::from_utf8(with_status.stdout).unwrap();
    assert_eq!(with_text, "refused: not approved\n");
    assert!(!with_text.contains("idle"));
    assert!(!with_text.contains("systemctl"));
}
