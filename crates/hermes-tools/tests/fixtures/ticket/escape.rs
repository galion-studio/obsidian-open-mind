// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Written into the worktree by the mock model, then run by `cargo test`
//! INSIDE the sandbox. Every attempt must fail; the test fails if one works.
//! Placeholders are filled in by the real-life script.

use std::io::Write;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::Command;
use std::time::Duration;

fn report(name: &str, worked: bool, detail: String) -> bool {
    println!(
        "ESCAPE {name}: {} ({detail})",
        if worked { "SUCCEEDED" } else { "BLOCKED" }
    );
    worked
}

#[test]
fn escape_attempts_are_all_blocked() {
    let mut worked = vec![];
    let t = Duration::from_secs(3);
    for (name, addr) in [
        ("net_loopback_mock_model", "127.0.0.1:@MOCK_PORT@"),
        ("net_internet_1.1.1.1:443", "1.1.1.1:443"), // repo-check: allow (egress probe; must be unreachable from the sandbox)
    ] {
        let a: SocketAddr = addr.parse().unwrap();
        let r = TcpStream::connect_timeout(&a, t);
        if report(name, r.is_ok(), format!("{r:?}")) {
            worked.push(name);
        }
    }
    let r = "github.com:443".to_socket_addrs();
    if report("dns_github.com", r.is_ok(), format!("{:?}", r.map(|_| "resolved"))) {
        worked.push("dns");
    }
    let r = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("@CANARY@/pwned.txt")
        .and_then(|mut f| f.write_all(b"escaped"));
    if report("write_outside_worktree", r.is_ok(), format!("{r:?}")) {
        worked.push("write");
    }
    let o = Command::new("rm").args(["-rf", "@CANARY@"]).output();
    let gone = !std::path::Path::new("@CANARY@/keep.txt").exists();
    if report(
        "rm_-rf_outside_worktree",
        gone,
        format!("{:?}", o.map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())),
    ) {
        worked.push("rm");
    }
    let r = std::fs::read_to_string("@HOST_FILE@");
    if report("read_host_file_outside_worktree", r.is_ok(), format!("{:?}", r.map(|s| s.len()))) {
        worked.push("read");
    }
    let o = Command::new("git")
        .args(["push", "origin", "HEAD:refs/heads/escaped"])
        .output();
    let pushed = o.as_ref().is_ok_and(|o| o.status.success());
    if report(
        "git_push_to_origin",
        pushed,
        format!("{:?}", o.map(|o| String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").to_string())),
    ) {
        worked.push("push");
    }
    println!("INFO pid1_seen_from_sandbox: {:?}", std::fs::read_to_string("/proc/1/comm").map(|s| s.trim().to_string()));
    assert!(worked.is_empty(), "escapes that worked: {worked:?}");
}
