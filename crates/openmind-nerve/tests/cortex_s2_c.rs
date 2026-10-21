// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Fixture/replay tests for the S2 sensors `proc-owner` (T2.8) and
//! `surface` (T2.9). No host reads, no network beyond loopback.
//!
//! - `fixtures/cortex-s2-c/proc-net-tcp*.txt` + `fd-map.txt` + `passwd.txt`
//!   (a VPS-like socket table) give the golden `proc-owner.expected.json`.
//! - `tls12-flight-galion-app.hex` (a real TLS 1.2 server flight captured
//!   once from the box) parses to the real leaf `notAfter`.
//! - Replaying `fixtures/cortex-replay` through the binary with both
//!   sensors configured keeps the pre-trait golden: host-reading sensors do
//!   nothing when not live.

use openmind_nerve::sensors::proc_owner::{self, Owned, Policy};
use openmind_nerve::sensors::surface::{self, Limits, Outcome, Target, Tls};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[allow(dead_code)] // only `canon` and `drop_edge_hosts` are used here
mod support;

fn fx() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(name: &str) -> String {
    std::fs::read_to_string(fx().join("cortex-s2-c").join(name)).unwrap()
}

fn data_lines(s: &str) -> impl Iterator<Item = &str> {
    s.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
}

fn proc_snapshot() -> Vec<Owned> {
    let mut rows = proc_owner::parse_proc_net_tcp(&read("proc-net-tcp.txt"));
    rows.extend(proc_owner::parse_proc_net_tcp(&read("proc-net-tcp6.txt")));
    let map = read("fd-map.txt");
    rows.into_iter()
        .map(|row| {
            let hit = data_lines(&map)
                .map(|l| l.splitn(4, ' ').collect::<Vec<_>>())
                .find(|f| f[0].parse::<u64>().ok() == Some(row.inode));
            match hit {
                Some(f) => Owned {
                    uid: f[2].parse().unwrap(),
                    pid: f[1].parse().ok(),
                    exe: Some(f[3].to_string()),
                    row,
                },
                None => Owned {
                    uid: row.uid,
                    pid: None,
                    exe: None,
                    row,
                },
            }
        })
        .collect()
}

#[test]
fn proc_owner_fixture_matches_golden() {
    let socks = proc_snapshot();
    assert_eq!(
        socks.len(),
        6,
        "four LISTEN v4 + two v6 (ESTABLISHED skipped)"
    );
    let policy = Policy {
        users: vec!["root".into(), "galion".into()],
        ports: [(8664u16, "galion".to_string())].into(),
    };
    let names = proc_owner::parse_passwd(&read("passwd.txt"));
    let got: Value = proc_owner::judge(&socks, &policy, &names)
        .iter()
        .map(|f| f.to_json())
        .collect::<Vec<_>>()
        .into();
    let want: Value = serde_json::from_str(&read("proc-owner.expected.json")).unwrap();
    assert_eq!(got, want, "{}", serde_json::to_string_pretty(&got).unwrap());
    // B33 events carry no user names, pids or paths.
    for f in proc_owner::judge(&socks, &policy, &names) {
        let ev = f.to_nerve_event(1_792_530_000).to_string();
        assert!(
            !ev.contains("slavik") && !ev.contains("/home") && !ev.contains("5150"),
            "{ev}"
        );
    }
}

#[test]
fn real_tls12_flight_fixture_gives_the_real_expiry() {
    let hex: String = data_lines(&read("tls12-flight-galion-app.hex")).collect();
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    let der = surface::read_leaf_cert(&mut &bytes[..]).expect("leaf certificate");
    let not_after = surface::cert_not_after(&der).expect("notAfter");
    assert_eq!(not_after, 1_793_467_688, "2026-10-31 18:28:08+01:00");
    // At 2026-10-20 23:00+02:00 that is 10 days: quiet at the default 7-day
    // warning, a warn at 14 days, an error at a 11-day critical line.
    let now = 1_792_530_000;
    let t = [Target::parse(1, "https://galion.app").unwrap()];
    let round = [Outcome {
        n: 1,
        dns: Ok(1),
        tls: Some(Tls::NotAfter(not_after)),
        http: None,
        proxied: false,
    }];
    assert!(surface::judge(&t, &round, now, &Limits::default()).is_empty());
    let l14 = Limits {
        warn_days: 14,
        ..Limits::default()
    };
    let f = surface::judge(&t, &round, now, &l14);
    assert_eq!(
        f[0].title,
        "1 of 1 public target(s) have a TLS certificate expiring soon: #1 (10 d)"
    );
    assert!(
        f[0].evidence[0].ends_with("notAfter 2026-10-31T18:28:08+01:00"),
        "{:?}",
        f[0].evidence
    );
    let l11 = Limits {
        warn_days: 14,
        crit_days: 11,
        ..Limits::default()
    };
    assert_eq!(
        surface::judge(&t, &round, now, &l11)[0].sev,
        openmind_nerve::incident::Sev::Error
    );
}

#[test]
fn replay_with_both_sensors_on_keeps_the_golden() {
    let dir = std::env::temp_dir().join(format!("nerve-s2c-replay-{}", std::process::id()));
    let o = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
        .env_clear()
        .env("NERVE_DATA_DIR", &dir)
        .env("NERVE_PROC_OWNER_USERS", "nobody-at-all")
        .env(
            "NERVE_SURFACE_TARGETS",
            "https://never-contacted.invalid,http://127.0.0.1:1/",
        )
        .arg("replay")
        .arg(fx().join("cortex-replay"))
        .stderr(Stdio::null())
        .output()
        .expect("run replay");
    assert_eq!(o.status.code(), Some(1), "failing verdict exits 1");
    let mut got: Value = serde_json::from_slice(&o.stdout).unwrap();
    support::canon(&mut got);
    // T2.12 additive `edge-host:*` streams postdate the golden (cortex-merge-all).
    support::drop_edge_hosts(&mut got);
    let mut want: Value = serde_json::from_str(
        &std::fs::read_to_string(fx().join("cortex-replay.expected.json")).unwrap(),
    )
    .unwrap();
    support::canon(&mut want);
    assert_eq!(got, want, "replay must not read the host or the network");
    let _ = std::fs::remove_dir_all(&dir);
}
