// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 394 / F-380-1: prove LogInsights merge+expire no longer inflates
//! edge failed-requests/min by ~37x. Gated `GALION_REAL=1`.
//!
//! A. Synthetic engine replay on the box (ports unused — pure logscan).
//! B. Optional read-only VPS journal sample (`GALION_REAL_VPS=1`, ssh
//!    `slavik-vps`): scrub hostnames, replay through insights once vs a
//!    multi-cycle merge+expire, compare rates. No addresses committed.

use openmind_nerve::logscan::{insights, LogInsights};
use openmind_nerve::probe::JLine;
use std::process::Command;

const WINDOW: u64 = 1800;

fn refused_line(t: u64, port: u16) -> JLine {
    JLine {
        t,
        unit: "cloudflared.service".into(),
        prio: 6,
        msg: format!(
            "ERR Request failed error=\"dial tcp [::1]:{port}: connect: connection refused\" dest=https://ex.test/"
        ),
    }
}

/// Engine-shaped accumulator: merge new lines since last_scan, expire window.
fn engine_replay(all: &[JLine], heartbeat: u64, end: u64) -> LogInsights {
    let mut acc = LogInsights::default();
    let start = all.iter().map(|l| l.t).min().unwrap_or(0);
    let mut last = start.saturating_sub(1);
    // Always cover `end` even when the sample is shorter than one heartbeat.
    let mut now = (last + heartbeat).min(end);
    if now <= last {
        now = end;
    }
    loop {
        let chunk: Vec<JLine> = all
            .iter()
            .filter(|l| l.t > last && l.t <= now)
            .cloned()
            .collect();
        if !chunk.is_empty() {
            acc.merge(insights(&chunk));
        }
        acc.expire(now.saturating_sub(WINDOW));
        if now >= end {
            break;
        }
        last = now;
        now = (now + heartbeat).min(end);
    }
    acc
}

fn per_min(li: &LogInsights) -> f64 {
    let failed: u32 = li.dead_origins.values().map(|d| d.failed_requests).sum();
    let span = li.window_last.saturating_sub(li.window_first).max(60);
    failed as f64 * 60.0 / span as f64
}

#[test]
fn real_edge_overcount_engine_replay() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    // 12 h, one failure every 2 s on :3011 → true 30/min.
    let end = 12 * 3600u64;
    let mut lines = Vec::with_capacity((end / 2) as usize);
    let mut t = 0u64;
    while t < end {
        if t.is_multiple_of(2) {
            lines.push(refused_line(t, 3011));
        }
        t += 1;
    }
    let acc = engine_replay(&lines, 900, end);
    let d = acc
        .dead_origins
        .get(&3011)
        .expect("port 3011 present after replay");
    let expect = (WINDOW / 2) as u32;
    assert_eq!(
        d.failed_requests, expect,
        "in-window failed_requests {} != {expect}",
        d.failed_requests
    );
    let rate = per_min(&acc);
    assert!(
        (rate - 30.0).abs() < 2.0,
        "engine replay rate {rate} failed/min (want ~30); window=[{},{}]",
        acc.window_first,
        acc.window_last
    );
    // Single-pass of the same in-window slice must agree.
    let slice: Vec<JLine> = lines
        .into_iter()
        .filter(|l| l.t >= end.saturating_sub(WINDOW))
        .collect();
    let once = insights(&slice);
    assert_eq!(
        once.dead_origins[&3011].failed_requests, d.failed_requests,
        "single-pass vs engine-replay disagree"
    );
    println!(
        "PASS real_edge_overcount_engine_replay: failed_requests={} rate={rate:.2}/min window_s={}",
        d.failed_requests,
        acc.window_last.saturating_sub(acc.window_first)
    );
}

#[test]
fn real_edge_overcount_vps_journal_replay() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    if std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL_VPS=1 for live journal leg");
        return;
    }
    // Read-only: last ~40 minutes of cloudflared "Request failed" lines.
    // Scrub dest= hostnames so nothing identifying is kept in memory longer
    // than this process (and never written to the repo).
    let out = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "slavik-vps",
            "journalctl -u cloudflared.service --since '40 min ago' -o cat --no-pager 2>/dev/null | grep -F 'Request failed' | head -n 5000",
        ])
        .output()
        .expect("ssh slavik-vps");
    if !out.status.success() {
        panic!(
            "ssh journalctl failed: status {:?} stderr {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut lines: Vec<JLine> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        // Spread across the 40 min window so expire math is meaningful.
        let t = now.saturating_sub(40 * 60) + (i as u64);
        // Scrub dest=https://… → dest=https://ex.test/
        let scrubbed = if let Some(idx) = line.find("dest=") {
            let rest = &line[idx + 5..];
            let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
            format!("{}dest=https://ex.test/{}", &line[..idx], &rest[end..])
        } else {
            line.to_string()
        };
        if !scrubbed.contains("connection refused") {
            continue;
        }
        lines.push(JLine {
            t,
            unit: "cloudflared.service".into(),
            prio: 6,
            msg: scrubbed,
        });
    }
    assert!(
        !lines.is_empty(),
        "no refused Request-failed lines in last 40 min (edge quiet?)"
    );
    let end = lines.iter().map(|l| l.t).max().unwrap();
    let start = lines.iter().map(|l| l.t).min().unwrap();
    let once = insights(&lines);
    let eng = engine_replay(&lines, 900, end);
    let once_n: u32 = once.dead_origins.values().map(|d| d.failed_requests).sum();
    let eng_n: u32 = eng.dead_origins.values().map(|d| d.failed_requests).sum();
    // Engine replay over a span ≤ WINDOW must match single-pass (no lifetime
    // inflation). If the sample is shorter than WINDOW, counts must be equal.
    if end.saturating_sub(start) <= WINDOW {
        assert_eq!(
            eng_n, once_n,
            "engine replay inflated {eng_n} vs single-pass {once_n} on short sample"
        );
    } else {
        // Longer sample: engine must be ≤ single-pass of full sample and
        // ≈ single-pass of the last WINDOW.
        let slice: Vec<JLine> = lines
            .iter()
            .filter(|l| l.t >= end.saturating_sub(WINDOW))
            .cloned()
            .collect();
        let win = insights(&slice);
        let win_n: u32 = win.dead_origins.values().map(|d| d.failed_requests).sum();
        assert_eq!(eng_n, win_n, "engine {eng_n} != window single-pass {win_n}");
    }
    let rate = per_min(&eng);
    println!(
        "PASS real_edge_overcount_vps_journal_replay: lines={} once_failed={once_n} eng_failed={eng_n} rate={rate:.2}/min (hosts scrubbed, not stored)",
        lines.len()
    );
}
