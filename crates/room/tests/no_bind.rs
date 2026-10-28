// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! No flags and `--fixture` must not bind a port or open a socket.
//!
//! The child runs under `strace -f -e trace=network` inside an empty network
//! namespace, so a regression cannot listen on the host. A positive control
//! runs `--listen` the same way and must show `bind(`/`listen(`, which proves
//! the harness really sees network syscalls.
//!
//! Needs `strace` and a network namespace. `unshare --net` is tried first
//! (root or CAP_SYS_ADMIN); unprivileged hosts fall back to
//! `unshare --user --map-root-user --net` (user namespaces). Hosts with
//! neither skip with a `SKIP` line instead of failing.

use std::fs::File;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NETWORK_CALLS: [&str; 9] = [
    "socket(",
    "bind(",
    "listen(",
    "connect(",
    "accept(",
    "accept4(",
    "socketpair(",
    "sendto(",
    "recvfrom(",
];

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("room-{name}-{}-{nanos}", std::process::id()))
}

fn works(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The `unshare` prefix that gives an empty network namespace here, or `None`.
fn netns_prefix() -> Option<Vec<&'static str>> {
    let candidates: [&[&'static str]; 2] = [
        &["unshare", "--net", "--"],
        &["unshare", "--user", "--map-root-user", "--net", "--"],
    ];
    candidates.into_iter().find_map(|prefix| {
        let mut args = prefix[1..].to_vec();
        args.push("true");
        works(prefix[0], &args).then(|| prefix.to_vec())
    })
}

/// Prefix plus a working strace, or a printed SKIP line and `None`.
fn harness(test: &str) -> Option<Vec<&'static str>> {
    let prefix = netns_prefix();
    let strace = works("strace", &["-V"]);
    match prefix {
        Some(prefix) if strace => {
            println!("room {test}: netns via {:?}", prefix);
            Some(prefix)
        }
        _ => {
            println!(
                "SKIP room {test}: needs a network namespace (ok={}) and `strace` (ok={strace})",
                prefix.is_some()
            );
            None
        }
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
    trace: String,
}

fn run_traced(prefix: &[&str], inner: &[&str], env: &[(&str, &str)]) -> Run {
    let trace_path = scratch("trace");
    let out_path = scratch("out");
    let err_path = scratch("err");
    let mut cmd = Command::new(prefix[0]);
    cmd.args(&prefix[1..])
        .args(["strace", "-f", "-e", "trace=network", "-o"])
        .arg(&trace_path)
        .args(inner)
        .stdout(Stdio::from(File::create(&out_path).expect("stdout")))
        .stderr(Stdio::from(File::create(&err_path).expect("stderr")));
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().expect("spawn unshare");
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) if start.elapsed() > Duration::from_secs(90) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("room stayed up with {inner:?}");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(err) => panic!("wait: {err}"),
        }
    };
    let read = |p: &PathBuf| {
        let text = std::fs::read_to_string(p).unwrap_or_default();
        let _ = std::fs::remove_file(p);
        text
    };
    Run {
        code,
        stdout: read(&out_path),
        stderr: read(&err_path),
        trace: read(&trace_path),
    }
}

fn assert_no_network(trace: &str) {
    for call in NETWORK_CALLS {
        assert!(!trace.contains(call), "{call} appeared:\n{trace}");
    }
}

#[test]
fn binary_no_flags_and_fixture_do_not_bind_or_open_a_socket() {
    let Some(prefix) = harness("no_bind") else {
        return;
    };
    let bin = env!("CARGO_BIN_EXE_room");
    let run = run_traced(&prefix, &[bin], &[]);
    assert_eq!(run.code, 0, "stderr={} trace={}", run.stderr, run.trace);
    assert!(run.stdout.starts_with("{\"objects\":["));
    assert!(run.stdout.contains("\"name\":\"room\""));
    assert_no_network(&run.trace);

    let dir = scratch("fixture");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("note.md"), "# Desk note\n").expect("note");
    let path = dir.to_string_lossy().into_owned();
    let run = run_traced(&prefix, &[bin, "--fixture", &path], &[]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(run.code, 0, "stderr={} trace={}", run.stderr, run.trace);
    assert!(run.stdout.contains("Desk note"));
    assert!(run.stdout.contains("\"kind\":\"doc\""));
    assert_no_network(&run.trace);

    for flag in ["--help", "--version", "--bogus"] {
        let run = run_traced(&prefix, &[bin, flag], &[]);
        assert_ne!(run.code, -1, "{flag}");
        assert_no_network(&run.trace);
    }
}

#[test]
fn positive_control_listen_is_seen_by_strace_and_stays_in_the_namespace() {
    let Some(prefix) = harness("listen_control") else {
        return;
    };
    if !works("timeout", &["--version"]) {
        println!("SKIP room listen_control: needs coreutils `timeout`");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_room");
    let empty = scratch("empty-root");
    let root = empty.to_string_lossy().into_owned();
    // `timeout` kills room after 2 s, so nothing outlives the test. Port 0
    // inside the fresh namespace cannot collide with anything on the host.
    let run = run_traced(
        &prefix,
        &["timeout", "-s", "KILL", "2", bin, "--listen"],
        &[("ROOM_BIND", "127.0.0.1:0"), ("ROOM_ROOT", &root)],
    );
    // GNU timeout exits 124 when the limit fires (Ubuntu 26.04 runners).
    // Older strace builds re-raise the KILL, so the same run is -1 or 137.
    // The listen proof is the trace below, not which of those codes came back.
    assert!(
        matches!(run.code, -1 | 124 | 137),
        "code={} stderr={} trace={}",
        run.code,
        run.stderr,
        run.trace
    );
    assert!(
        run.stderr.contains("room: listening on 127.0.0.1:"),
        "{}",
        run.stderr
    );
    assert!(run.trace.contains("bind("), "{}", run.trace);
    assert!(run.trace.contains("listen("), "{}", run.trace);
}
