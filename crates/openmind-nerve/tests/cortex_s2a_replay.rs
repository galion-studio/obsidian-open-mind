// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Replay cases for the S2 sensors `sshd` (T2.1) and `rustsec` (T2.4).
//!
//! - `fixtures/cortex-sshd` through the real replay pipeline
//!   (`replay_dir`, `NERVE_SSHD=1`): crit ssh flood, two vulnerability
//!   warns, counts only, B33 events with source `auth.log`.
//! - The same journal cycle by cycle through the registry: the flood
//!   appears, then clears after the 300 s window.
//! - `fixtures/rustsec/Cargo.lock` against the embedded offline snapshot
//!   (no network): smallvec/time advisories; `Cargo.lock.fixed` clears them.
//! - The workspace's own Cargo.lock (printed, not asserted: it moves).

use openmind_nerve::config::Config;
use openmind_nerve::cortex::Input;
use openmind_nerve::incident::Sev;
use openmind_nerve::logscan::LogInsights;
use openmind_nerve::probe::parse_journal_lines;
use openmind_nerve::replay::replay_dir;
use openmind_nerve::sensors::{rustsec, sshd, Sensors};
use openmind_schema::NerveEvent;
use std::path::PathBuf;

const T: u64 = 1_792_531_800; // 2026-10-20 23:30:00 +02:00

fn fx() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn sshd_fixture_through_replay() {
    // Engine::new reads the process env; every test in this file agrees.
    std::env::set_var("NERVE_SSHD", "1");
    std::env::set_var("NERVE_SSHD_AUTH_LOG", "0");
    let data = std::env::temp_dir().join(format!("nerve-s2a-replay-{}", std::process::id()));
    let d = data.display().to_string();
    let cfg = Config::from_env(&|k| (k == "NERVE_DATA_DIR").then(|| d.clone())).unwrap();
    let out = replay_dir(&fx().join("cortex-sshd"), cfg).unwrap();
    let e = &out.engine;
    assert!(e.sensors.names().contains(&"sshd"));
    let cx = &e.cortex;
    let f = &cx.ddos_findings[0];
    assert_eq!(
        (f.id.as_str(), f.sev, f.source.as_str()),
        ("sshd-credential-flood", Sev::Critical, "auth.log")
    );
    assert_eq!(
        f.title,
        "ssh credential flood: 400 failed password(s) from 50 source(s) in 300 s (rule failed_password)"
    );
    let v: Vec<(&str, Sev)> = cx
        .vulnerabilities
        .iter()
        .filter(|f| f.source == "auth.log")
        .map(|f| (f.id.as_str(), f.sev))
        .collect();
    assert_eq!(
        v,
        [
            ("sshd-accepted-publickey-other", Sev::Warn),
            ("sshd-invalid-user", Sev::Warn)
        ]
    );
    assert!(f.evidence[0].contains("accepted_publickey_root=600"));
    assert!(cx.signals.iter().any(|s| s.cause == "ssh-flood"));
    let evs: Vec<NerveEvent> = cx
        .nerve_events(T)
        .into_iter()
        .filter(|e| e["source"] == "auth.log")
        .map(|e| serde_json::from_value(e).unwrap())
        .collect();
    let mut kinds: Vec<(&str, &str)> = evs
        .iter()
        .map(|e| (e.kind.as_str(), e.severity.as_str()))
        .collect();
    kinds.sort();
    assert_eq!(kinds, [("ddos", "crit"), ("vulnerability", "warn")]);
    assert!(evs.iter().all(|e| e.ts == "2026-10-20T23:30:00+02:00"));
    let all = serde_json::to_string(&e.shared.lock().unwrap().report).unwrap();
    for leak in [
        "198.51.100",
        "203.0.113",
        "192.0.2",
        "fixturedeployer",
        "user1",
    ] {
        assert!(
            !all.contains(leak),
            "{leak} leaked into the report: {}",
            &all[all.find(leak).unwrap().saturating_sub(120)..]
        );
    }
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn sshd_fixture_cycle_by_cycle_appears_and_clears() {
    let text = std::fs::read_to_string(fx().join("cortex-sshd/journal.jsonl")).unwrap();
    let lines = parse_journal_lines(&text, &[]);
    let mut s = Sensors::empty();
    s.add(sshd::SshAuth::new(vec!["ssh.service".into()], None));
    let logs = LogInsights::default();
    let mut sev = vec![];
    let mut prev = 0;
    for now in [T - 2000, T - 200, T - 100, T, T + 200, T + 301] {
        let win: Vec<_> = lines
            .iter()
            .filter(|l| l.t > prev && l.t <= now)
            .cloned()
            .collect();
        prev = now;
        let mut i = Input::new(now, &[], &logs);
        i.journal = &win;
        let cx = s.run(&i);
        sev.push(cx.ddos_findings.first().map(|f| f.sev));
    }
    // T-2000: the old burst alone (100 fails, 20 sources) is high; from T-200 the
    // in-window flood (>= 100 fails, 50 sources) is crit; at T+301 the window is empty.
    let e = Some(Sev::Error);
    let c = Some(Sev::Critical);
    assert_eq!(sev, [e, c, c, c, c, None]);
}

fn rustsec_live(lock: &std::path::Path) -> openmind_nerve::cortex::Cortex {
    let mut s = Sensors::empty();
    s.add(rustsec::RustSec::new(
        lock.to_path_buf(),
        "demo-app",
        None,
        3600,
    ));
    let logs = LogInsights::default();
    let mut i = Input::new(T, &[], &logs);
    i.live = true;
    s.run(&i)
}

#[test]
fn rustsec_fixture_lock_against_offline_snapshot() {
    let cx = rustsec_live(&fx().join("rustsec/Cargo.lock"));
    let got: Vec<(&str, Sev, &str)> = cx
        .vulnerabilities
        .iter()
        .map(|f| (f.id.as_str(), f.sev, f.title.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            (
                "rustsec-smallvec-1.6.0",
                Sev::Critical,
                "smallvec 1.6.0 in Cargo.lock: 1 RustSec advisory (RUSTSEC-2021-0003)"
            ),
            (
                "rustsec-time-0.1.45",
                Sev::Error,
                "time 0.1.45 in Cargo.lock: 1 RustSec advisory (RUSTSEC-2020-0071)"
            ),
        ]
    );
    assert!(cx.vulnerabilities.iter().all(|f| f.component == "demo-app"));
    let ev = cx.nerve_events(T);
    assert_eq!(ev.len(), 1);
    let ev: NerveEvent = serde_json::from_value(ev[0].clone()).unwrap();
    assert_eq!(
        (ev.source.as_str(), ev.severity.as_str()),
        ("rustsec", "crit")
    );
    let fixed = rustsec_live(&fx().join("rustsec/Cargo.lock.fixed"));
    assert!(
        fixed.vulnerabilities.is_empty(),
        "{:?}",
        fixed.vulnerabilities
    );
    // Not live (replay): nothing, no read.
    let mut s = Sensors::empty();
    s.add(rustsec::RustSec::new(
        fx().join("rustsec/Cargo.lock"),
        "x",
        None,
        1,
    ));
    let logs = LogInsights::default();
    assert!(s.run(&Input::new(T, &[], &logs)).vulnerabilities.is_empty());
}

#[test]
fn rustsec_workspace_lock_report() {
    let lock = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let cx = rustsec_live(&lock);
    let db = rustsec::Db::parse(rustsec::SNAPSHOT);
    let pkgs = rustsec::lock_packages(&std::fs::read_to_string(&lock).unwrap());
    eprintln!(
        "workspace Cargo.lock: {} crates.io packages vs {} advisories (snapshot {}): {} finding(s)",
        pkgs.len(),
        db.count,
        &db.commit[..12],
        cx.vulnerabilities.len()
    );
    for f in &cx.vulnerabilities {
        eprintln!("  {} {}: {}", f.sev.as_str(), f.id, f.title);
    }
    assert!(pkgs.len() > 50);
}
