// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life daemon test (GALION_REAL=1): the real `galion-guard` binary,
//! file source, dry-run firewall, driven only through its CLI and control
//! socket. The namespace + real nftables run is tests/real_netns.rs.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-guard");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn cli(cfg: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(BIN)
        .args(args)
        .arg("--config")
        .arg(cfg)
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

fn status(cfg: &Path) -> serde_json::Value {
    let (_, out) = cli(cfg, &["status", "--json"]);
    serde_json::from_str(out.trim()).unwrap_or(serde_json::Value::Null)
}

fn start(cfg: &Path, sock: &Path, log: &Path) -> Child {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    let mut c = Command::new(BIN)
        .args(["run", "--config"])
        .arg(cfg)
        .stdout(Stdio::null())
        .stderr(f)
        .spawn()
        .unwrap();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if sock.exists() && status(cfg).is_object() {
            return c;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = c.kill();
    let _ = c.wait();
    panic!("daemon did not start");
}

fn wait_for(cfg: &Path, f: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
    let t = Instant::now();
    loop {
        let s = status(cfg);
        if f(&s) || t.elapsed() > Duration::from_secs(5) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn ts() -> String {
    common::stamp(galion_guard::state::now_secs())
}

#[test]
fn real_daemon_cli_roundtrip() {
    if !real() {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let dir: PathBuf =
        std::env::temp_dir().join(format!("guard-real-daemon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("guard.toml");
    let sock = dir.join("run/ctl.sock");
    let log = dir.join("auth.log");
    let dlog = dir.join("daemon.log");
    std::fs::write(&log, "").unwrap();
    std::fs::write(
        &cfg,
        format!(
            "[guard]\nbackend = \"dry-run\"\ndry_run_log = \"{d}/fw.nft\"\nstate = \"{d}/state.json\"\nsocket = \"{s}\"\nignoreself = false\n[sshd]\nsource = \"file\"\nlogpath = \"{l}\"\npoll_ms = 100\nbantime = \"30s\"\nignoreip = [\"198.51.100.0/24\"]\nports = [19382]\n",
            d = dir.display(),
            s = sock.display(),
            l = log.display()
        ),
    )
    .unwrap();
    let mut d = start(&cfg, &sock, &dlog);
    let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    for i in 0..5 {
        writeln!(
            f,
            "{} box sshd[{}]: Invalid user x{i} from 203.0.113.77 port 4{i}",
            ts(),
            300 + i
        )
        .unwrap();
    }
    f.flush().unwrap();
    let s = wait_for(&cfg, |s| s["currently_banned"] == 1);
    assert_eq!(s["bans"][0]["ip"], "203.0.113.77", "{s}");
    assert!(std::fs::read_to_string(dir.join("fw.nft"))
        .unwrap()
        .contains("add element inet galion-guard addr4 { 203.0.113.77 timeout"));
    // Manual ban / protected refusals.
    assert!(cli(&cfg, &["ban", "203.0.113.78", "1m"]).0);
    assert!(!cli(&cfg, &["ban", "127.0.0.1"]).0);
    assert!(!cli(&cfg, &["ban", "198.51.100.9"]).0);
    assert_eq!(status(&cfg)["currently_banned"], 2);
    // Restart: both bans come back.
    assert!(cli(&cfg, &["stop"]).0);
    let _ = d.wait();
    d = start(&cfg, &sock, &dlog);
    let s = status(&cfg);
    assert_eq!(s["currently_banned"], 2, "{s}");
    // CLI unban.
    let (ok, out) = cli(&cfg, &["unban", "203.0.113.77"]);
    assert!(ok && out.contains("unbanned"), "{out}");
    assert_eq!(status(&cfg)["currently_banned"], 1);
    let (ok, text) = cli(&cfg, &["status"]);
    assert!(ok && text.contains("Currently banned:\t1"), "{text}");
    assert!(cli(&cfg, &["unban", "--all"]).0);
    assert_eq!(status(&cfg)["currently_banned"], 0);
    assert!(cli(&cfg, &["stop"]).0);
    let _ = d.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
