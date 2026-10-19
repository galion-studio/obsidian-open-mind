// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Binary checks for `gaia`. No daemon is spawned; each call exits.
//! Traced commands do not spawn systemctl and do not open a socket.

use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

fn socket_fds() -> Vec<String> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir("/proc/self/fd").expect("fd table");
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let text = target.to_string_lossy();
        if let Some(rest) = text.strip_prefix("socket:") {
            found.push(rest.to_owned());
        }
    }
    found.sort();
    found
}

fn production_source() -> String {
    let lib = include_str!("../src/lib.rs");
    let prod = lib.split("#[cfg(test)]").next().expect("production lib");
    let main = include_str!("../src/main.rs");
    format!("{prod}\n{main}")
}

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("gaia-{name}-{}-{nanos}", std::process::id()))
}

fn write_systemctl_trap(dir: &Path, log: &Path) {
    std::fs::create_dir_all(dir).expect("trap dir");
    let path = dir.join("systemctl");
    let body = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$0\" >> '{}'\nexit 99\n",
        log.display()
    );
    std::fs::write(&path, body).expect("trap script");
    let mut perm = std::fs::metadata(&path).expect("trap meta").permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&path, perm).expect("trap mode");
}

fn assert_trace_clean(trace: &str, args: &[&str]) {
    assert!(
        !trace.contains("systemctl"),
        "spawned systemctl for {args:?}:\n{trace}"
    );
    for call in [
        "socket(",
        "bind(",
        "listen(",
        "connect(",
        "accept(",
        "accept4(",
        "socketpair(",
    ] {
        assert!(!trace.contains(call), "{call} for {args:?}:\n{trace}");
    }
}

fn trace_gaia(
    args: &[&str],
    override_env: Option<&str>,
    trap_dir: &Path,
    trace_path: &Path,
) -> (i32, String, String, String) {
    let out_path = scratch("out");
    let err_path = scratch("err");
    let out_file = File::create(&out_path).expect("stdout file");
    let err_file = File::create(&err_path).expect("stderr file");
    let path_env = {
        let old = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", trap_dir.display(), old)
    };
    let mut cmd = Command::new("/usr/bin/unshare");
    cmd.arg("--net")
        .arg("--")
        .arg("/usr/bin/strace")
        .arg("-f")
        .arg("-e")
        .arg("trace=network,process")
        .arg("-o")
        .arg(trace_path)
        .arg(BIN)
        .args(args)
        .env("PATH", path_env)
        .env_remove("GAIA_OVERRIDE")
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file));
    if let Some(value) = override_env {
        cmd.env("GAIA_OVERRIDE", value);
    }
    let status = cmd.status().expect("unshare strace gaia");
    let code = status.code().unwrap_or(1);
    let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    let trace = std::fs::read_to_string(trace_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);
    let _ = std::fs::remove_file(trace_path);
    (code, stdout, stderr, trace)
}

/// Library calls and the `gaia` binary do not spawn systemctl and do not open a socket.
#[test]
fn gaia_does_not_spawn_systemctl_or_open_a_socket() {
    let src = production_source();
    assert!(
        !src.contains("systemctl"),
        "production source names systemctl"
    );
    for token in [
        "std::process::Command",
        "Command::new",
        "Command::spawn",
        ".spawn(",
        "std::net",
        "TcpListener",
        "TcpStream",
        "UdpSocket",
        "UnixListener",
        "UnixStream",
    ] {
        assert!(!src.contains(token), "{token} in production source");
    }

    let sockets_before = socket_fds();
    let stamp = "2026-10-19T16:39:32+02:00";
    let library: &[(&[&str], Option<&str>)] = &[
        (&["status"], None),
        (&["tick"], None),
        (&["simulate-faro"], None),
        (&["simulate-faro", "--full-power"], None),
        (&["--master-override"], None),
        (&["status", "--master-override"], Some("1")),
        (&["master-override"], Some("1")),
    ];
    for (args, env) in library {
        let outcome = gaia_core::run(args, *env, stamp);
        assert!(matches!(outcome.code, 0 | 2));
    }
    let now = gaia_core::now_zagreb();
    assert_eq!(now.len(), 25, "{now}");
    assert_eq!(gaia_core::tick(gaia_core::Core::IDLE).step, 1);
    assert!(gaia_core::simulate_faro(true).contains("network: none"));
    assert_eq!(socket_fds(), sockets_before, "library path opened a socket");

    let trap_dir = scratch("trap");
    let log = trap_dir.join("spawned");
    let _ = std::fs::remove_dir_all(&trap_dir);
    write_systemctl_trap(&trap_dir, &log);

    let cli: &[(&[&str], Option<&str>, i32, &str)] = &[
        (&["status"], None, 0, "idle"),
        (&["tick"], None, 0, "step=1"),
        (&["simulate-faro"], None, 0, "dry-run"),
        (&["simulate-faro", "--full-power"], None, 0, "unchanged"),
        (&["--master-override"], None, 2, "refused:"),
        (
            &["--master-override"],
            Some("1"),
            2,
            "refused: not approved",
        ),
        (&["master-override"], Some("1"), 2, "refused: not approved"),
    ];
    for (args, env, code, needle) in cli {
        let trace_path = scratch("trace");
        let (got, stdout, stderr, trace) = trace_gaia(args, *env, &trap_dir, &trace_path);
        assert_eq!(
            got, *code,
            "args={args:?} stdout={stdout} stderr={stderr} trace={trace}"
        );
        assert!(
            stdout.contains(needle),
            "args={args:?} stdout={stdout} stderr={stderr}"
        );
        assert_trace_clean(&trace, args);
        assert!(
            !log.exists(),
            "systemctl trap ran: {}",
            std::fs::read_to_string(&log).unwrap_or_default()
        );
    }

    let _ = std::fs::remove_dir_all(&trap_dir);
    assert_eq!(socket_fds(), sockets_before, "cli path left a socket");
}
