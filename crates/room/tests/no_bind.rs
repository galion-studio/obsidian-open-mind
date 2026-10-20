// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! No flags and `--fixture` must not bind a port or open a socket.
//! The child runs in an empty network namespace, so a regression cannot listen
//! on the host.

use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("room-{name}-{}-{nanos}", std::process::id()))
}

fn assert_no_network(trace: &str) {
    for call in [
        "socket(",
        "bind(",
        "listen(",
        "connect(",
        "accept(",
        "accept4(",
        "socketpair(",
        "sendto(",
        "recvfrom(",
    ] {
        assert!(!trace.contains(call), "{call} appeared:\n{trace}");
    }
}

/// This test needs `unshare --net` (a network namespace) and `strace`. Hosts
/// without them (CI containers, the build box) skip it with a message.
fn net_tools_missing() -> bool {
    let works = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let unshare = works("unshare", &["--net", "--", "true"]);
    let strace = works("strace", &["-V"]);
    if unshare && strace {
        return false;
    }
    println!(
        "SKIP room no_bind: needs `unshare --net` (ok={unshare}) and `strace` (ok={strace}) on this machine"
    );
    true
}

fn run_room(args: &[&str]) -> (i32, String, String, String) {
    let bin = env!("CARGO_BIN_EXE_room");
    let trace_path = scratch("trace");
    let out_path = scratch("out");
    let err_path = scratch("err");
    let out_file = File::create(&out_path).expect("stdout");
    let err_file = File::create(&err_path).expect("stderr");
    let mut child = Command::new("unshare")
        .arg("--net")
        .arg("--")
        .arg("strace")
        .arg("-f")
        .arg("-e")
        .arg("trace=network")
        .arg("-o")
        .arg(&trace_path)
        .arg(bin)
        .args(args)
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .expect("unshare");
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(1),
            Ok(None) if start.elapsed() > Duration::from_secs(90) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("room stayed up with {args:?}; it must not listen");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(err) => panic!("wait: {err}"),
        }
    };
    let stdout = std::fs::read_to_string(&out_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    let trace = std::fs::read_to_string(&trace_path).unwrap_or_default();
    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);
    let _ = std::fs::remove_file(&trace_path);
    (code, stdout, stderr, trace)
}

#[test]
fn binary_no_flags_and_fixture_do_not_bind_or_open_a_socket() {
    if net_tools_missing() {
        return;
    }
    let (code, stdout, stderr, trace) = run_room(&[]);
    assert_eq!(code, 0, "stderr={stderr} trace={trace}");
    assert!(stdout.starts_with("{\"objects\":["));
    assert!(stdout.contains("\"name\":\"room\""));
    assert_no_network(&trace);

    let dir = scratch("fixture");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("note.md"), "# Desk note\n").expect("note");
    let path = dir.to_string_lossy().into_owned();
    let (code, stdout, stderr, trace) = run_room(&["--fixture", &path]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, 0, "stderr={stderr} trace={trace}");
    assert!(stdout.contains("Desk note"));
    assert!(stdout.contains("\"kind\":\"doc\""));
    assert_no_network(&trace);
}
