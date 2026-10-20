// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! CLI checks on the test-profile binary (no sockets, always run).

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_galion-wallstreet"))
}

fn fixture(p: &str) -> String {
    format!("{}/fixtures/{p}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn score_cli_is_100_and_exit_zero() {
    let o = bin().args(["score", "--md"]).output().unwrap();
    let s = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{s}");
    assert!(s.contains("B2 wallstreet: 100%"), "{s}");
}

#[test]
fn ticket_cli_reproducible_and_insider_refused() {
    let run = || {
        let o = bin()
            .args(["ticket", &fixture("tickets/scripted-open-close.json")])
            .output()
            .unwrap();
        assert!(o.status.success());
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        v["report"]["fingerprint"].as_str().unwrap().to_string()
    };
    assert_eq!(run(), run());
    let o = bin()
        .args(["ticket", &fixture("tickets/insider-refusal.json")])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3));
}

#[test]
fn screen_cli() {
    assert_eq!(
        bin()
            .args(["screen", "use my real money account"])
            .status()
            .unwrap()
            .code(),
        Some(3)
    );
    assert!(bin()
        .args(["screen", "backtest GLXA"])
        .status()
        .unwrap()
        .success());
}

#[test]
fn no_network_client_in_source() {
    // Paper-only guarantee: the crate never dials out. The only socket API is
    // the loopback HTTP server's TcpListener.
    let dir = format!("{}/src", env!("CARGO_MANIFEST_DIR"));
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let src = std::fs::read_to_string(&p).unwrap();
        for bad in [
            "TcpStream::connect",
            "UdpSocket",
            "reqwest",
            "ureq",
            "hyper::",
        ] {
            assert!(!src.contains(bad), "{} contains {bad}", p.display());
        }
    }
    let toml =
        std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let deps = toml.split("[dependencies]").nth(1).unwrap_or("");
    let deps = deps.split("\n[").next().unwrap_or("");
    let names: Vec<&str> = deps
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim()))
        .collect();
    assert_eq!(names, ["serde", "serde_json", "zagreb-time"]);
}
