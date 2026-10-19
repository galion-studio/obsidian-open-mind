// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Fixture file check. This test does not pass `--scan` and opens no socket.

use openmind_discovery::{discover, read_fixture_file, static_seeds, Scan, APPROVAL};
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openmind-peer.mdns")
}

#[test]
fn fixture_file_folds_one_peer_and_does_not_scan() {
    let path = fixture_path();
    let text = read_fixture_file(path.to_str().unwrap()).unwrap();
    let report = discover(Some(&text), Scan::Off).unwrap();
    assert!(!report.scan.requested);
    assert_eq!(report.scan.packets_sent, 0);
    assert_eq!(report.candidates.len(), static_seeds().len() + 1);
    let peer = report.candidates.last().unwrap();
    assert_eq!(peer.id, "desk-1");
    assert_eq!(peer.instance, "desk._openmind._tcp.local.");
    assert_eq!(peer.service_type, "_openmind._tcp.local.");
    assert_eq!(peer.host, "desk.local.");
    assert_eq!(peer.port, Some(7787));
    assert_eq!(
        peer.addrs,
        vec!["192.0.2.10".to_string(), "2001:db8::10".to_string()]
    );
    assert_eq!(peer.txt.get("arch").map(String::as_str), Some("x86_64"));
    assert_eq!(peer.txt.get("name").map(String::as_str), Some("Desk"));
    assert_eq!(peer.approval, APPROVAL);
    assert!(report.candidates.iter().all(|row| row.approval == APPROVAL));
}
