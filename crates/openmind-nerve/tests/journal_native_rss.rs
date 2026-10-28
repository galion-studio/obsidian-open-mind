// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Box RSS comparison: `journalctl -f` child vs in-process `sdjournal` follower
//! (feature `journal-native`). Ports 21520-21529.
//!
//! Run:
//!   cargo test -p openmind-nerve --features journal-native --test journal_native_rss -- --nocapture
//!
//! The box has no readable *.journal files; native open of an empty dir idles
//! without a journalctl child. Live VPS baseline (read-only, 2026-10-27 CET):
//! nerve ~7.9 MB + journalctl child ~49.1 MB.

#![cfg(feature = "journal-native")]

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
/// Port block start. Default is this test's original slot range; slot 292
/// (nerve-merge-4) moves the whole block at build time with
/// `REAL_NERVE_PORT_BASE=<port>` (e.g. 21620) so merged re-runs stay inside
/// their own slot range. Offsets inside the block are unchanged.
const fn port_block(default: u16) -> u16 {
    match option_env!("REAL_NERVE_PORT_BASE") {
        None => default,
        Some(s) => {
            let b = s.as_bytes();
            let mut i = 0;
            let mut v: u32 = 0;
            assert!(!b.is_empty(), "REAL_NERVE_PORT_BASE empty");
            while i < b.len() {
                assert!(b[i].is_ascii_digit(), "REAL_NERVE_PORT_BASE not a number");
                v = v * 10 + (b[i] - b'0') as u32;
                assert!(v <= 65_526, "REAL_NERVE_PORT_BASE too large");
                i += 1;
            }
            v as u16
        }
    }
}

const PORT: u16 = port_block(21520);

fn rss_kb(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn http_get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    Some((code, body))
}

fn wait_port(port: u16, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if let Some((c, _)) = http_get(port, "/stats") {
            if c == 200 {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn child_journalctl_pids(ppid: u32) -> Vec<u32> {
    let Ok(out) = Command::new("ps")
        .args(["-o", "pid=,cmd=", "--ppid", &ppid.to_string()])
        .output()
    else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("journalctl"))
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect()
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_nerve(port: u16, data: &std::path::Path, journalctl: &str) -> Kill {
    Kill(
        Command::new(BIN)
            .args([
                "serve",
                "--mode",
                "event",
                "--listen",
                &format!("127.0.0.1:{port}"),
            ])
            .env("NERVE_DATA_DIR", data)
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_EVT_JOURNALCTL", journalctl)
            .env("NERVE_EVT_PIDFD", "0")
            .env("NERVE_EVT_JOURNAL", "1")
            .env("NERVE_EVT_HEARTBEAT", "900")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("nerve"),
    )
}

#[test]
fn journalctl_vs_native_rss() {
    let tmp = std::env::temp_dir().join(format!("nerve-journal-native-rss-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // --- A: standalone journalctl -f RSS (box often has empty journal) ---
    let mut jc = Command::new("journalctl")
        .args(["-f", "-n", "0", "-o", "json", "--no-pager"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn journalctl");
    thread::sleep(Duration::from_millis(800));
    let jc_rss = rss_kb(jc.id()).unwrap_or(0);
    let _ = jc.kill();
    let _ = jc.wait();
    println!("box journalctl -f RSS_KB={jc_rss}");

    // --- B: nerve event mode with replay (no journalctl child; RSS baseline) ---
    let replay = tmp.join("empty.jsonl");
    std::fs::write(&replay, "").unwrap();
    let data_ctl = tmp.join("data-ctl");
    std::fs::create_dir_all(&data_ctl).unwrap();
    let child_ctl = start_nerve(PORT, &data_ctl, &format!("replay:{}", replay.display()));
    assert!(wait_port(PORT, 15), "nerve (replay) did not bind");
    thread::sleep(Duration::from_millis(500));
    let ctl_rss = rss_kb(child_ctl.0.id()).unwrap_or(0);
    let ctl_jc = child_journalctl_pids(child_ctl.0.id());
    println!("nerve replay-mode RSS_KB={ctl_rss} journalctl_children={ctl_jc:?}");
    drop(child_ctl);
    thread::sleep(Duration::from_millis(400));

    // --- C: nerve with native backend (must not spawn journalctl) ---
    let empty_journal = tmp.join("empty-journal");
    std::fs::create_dir_all(&empty_journal).unwrap();
    let native_spec = match openmind_nerve::evt::journal_native::test_journal_dir() {
        Some(p) => format!("native:{}", p.display()),
        None => format!("native:{}", empty_journal.display()),
    };
    let port_n = PORT + 1;
    let data_n = tmp.join("data-native");
    std::fs::create_dir_all(&data_n).unwrap();
    let child_n = start_nerve(port_n, &data_n, &native_spec);
    assert!(wait_port(port_n, 20), "nerve (native) did not bind");
    thread::sleep(Duration::from_secs(2));
    let native_rss = rss_kb(child_n.0.id()).unwrap_or(0);
    let native_jc = child_journalctl_pids(child_n.0.id());
    let st = http_get(port_n, "/stats").expect("stats");
    assert_eq!(st.0, 200, "{}", st.1);
    let stats: Value = serde_json::from_str(&st.1).unwrap_or(Value::Null);
    println!(
        "nerve native-mode RSS_KB={native_rss} journalctl_children={native_jc:?} stats_keys={}",
        stats.as_object().map(|m| m.len()).unwrap_or(0)
    );
    assert!(
        native_jc.is_empty(),
        "native mode must not spawn journalctl; got {native_jc:?}"
    );

    let report = format!(
        "box_journalctl_rss_kb={jc_rss}\n\
         nerve_replay_rss_kb={ctl_rss}\n\
         nerve_native_rss_kb={native_rss}\n\
         native_journalctl_children={}\n\
         native_spec={native_spec}\n\
         vps_live_nerve_rss_kb=8088\n\
         vps_live_journalctl_rss_kb=50264\n",
        native_jc.len()
    );
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/real-tests/nerve-journal-reader-rs.rss.txt");
    if let Some(parent) = out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&out, &report);
    println!("wrote {}", out.display());
    print!("{report}");

    assert!(
        native_rss > 0 && native_rss < 80_000,
        "native RSS_KB unexpected: {native_rss}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
