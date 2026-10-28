// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Discovery path checks. The default path opens no socket; a requested scan
//! reports the packets its scanner sent.

use openmind_discovery::{
    cli, discover, discover_with, read_fixture_file, static_seeds, CliOut, Scan, ScanOutcome,
    APPROVAL,
};
use std::io;
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

#[test]
fn default_path_opens_no_socket_and_a_requested_scan_goes_through_the_scanner() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scan_src = std::fs::read_to_string(root.join("src/scan.rs")).expect("scan.rs");
    // The gate itself names no socket type; it delegates to mdns::browse.
    for token in [
        "UdpSocket",
        "TcpStream",
        "TcpListener",
        "UnixStream",
        "std::net",
    ] {
        assert!(!scan_src.contains(token), "{token} in scan.rs");
    }
    assert!(scan_src.contains("browse(&BrowseOpts::default())"));

    let before = fd_targets();
    let before_sockets = socket_fds(&before);
    assert!(!before.is_empty(), "proc fd table was not readable");

    let off = discover(None, Scan::Off).expect("off");
    assert!(!off.scan.requested);
    assert_eq!(off.scan.packets_sent, 0);
    assert_eq!(off.candidates, static_seeds());

    let out = cli(Vec::<&str>::new(), |_| -> io::Result<String> {
        panic!("fixture read")
    })
    .expect("cli");
    match out {
        CliOut::Report(value) => {
            assert_eq!(value["scan"]["requested"], false);
            assert_eq!(value["scan"]["packets_sent"], 0);
        }
        CliOut::Help => panic!("expected a report"),
    }

    // A requested scan reports whatever the scanner sent (the real one sends
    // a query plus one repeat); here a fake scanner keeps the test offline.
    let text = read_fixture_file(fixture_path().to_str().expect("path")).expect("fixture");
    let with_fixture = discover_with(Some(&text), Scan::Requested, || ScanOutcome {
        packets_sent: 2,
        note: "fake".into(),
        candidates: Vec::new(),
    })
    .expect("fixture scan");
    assert!(with_fixture.scan.requested);
    assert_eq!(with_fixture.scan.packets_sent, 2);
    assert_eq!(with_fixture.candidates.len(), static_seeds().len() + 1);

    let after = fd_targets();
    assert_eq!(socket_fds(&after), before_sockets);
}

fn socket_fds(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| target.starts_with("socket:"))
        .cloned()
        .collect()
}

fn fd_targets() -> Vec<String> {
    let mut names = Vec::new();
    let dir = std::fs::read_dir("/proc/self/fd").expect("fd");
    for entry in dir.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            names.push(target.to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}
