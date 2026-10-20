// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Discovery path checks. A requested scan reports zero packets and opens no socket.

use openmind_discovery::{cli, discover, read_fixture_file, static_seeds, CliOut, Scan, APPROVAL};
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
fn scan_path_reports_zero_packets_and_opens_no_socket_when_requested() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scan_src = std::fs::read_to_string(root.join("src/scan.rs")).expect("scan.rs");
    for token in [
        "UdpSocket",
        "TcpStream",
        "TcpListener",
        "UnixStream",
        "std::net",
    ] {
        assert!(!scan_src.contains(token), "{token} in scan.rs");
    }

    let before = fd_targets();
    let before_sockets = socket_fds(&before);
    assert!(!before.is_empty(), "proc fd table was not readable");

    let off = discover(None, Scan::Off).expect("off");
    assert!(!off.scan.requested);
    assert_eq!(off.scan.packets_sent, 0);

    let requested = discover(None, Scan::Requested).expect("scan");
    assert!(requested.scan.requested);
    assert_eq!(requested.scan.packets_sent, 0);
    assert!(requested.scan.note.contains("no packets"));

    let text = read_fixture_file(fixture_path().to_str().expect("path")).expect("fixture");
    let with_fixture = discover(Some(&text), Scan::Requested).expect("fixture scan");
    assert!(with_fixture.scan.requested);
    assert_eq!(with_fixture.scan.packets_sent, 0);
    assert_eq!(with_fixture.candidates.len(), static_seeds().len() + 1);

    let out = cli(["--scan"], |_| -> io::Result<String> {
        panic!("fixture read")
    })
    .expect("cli");
    match out {
        CliOut::Report(value) => {
            assert_eq!(value["scan"]["requested"], true);
            assert_eq!(value["scan"]["packets_sent"], 0);
        }
        CliOut::Help => panic!("expected a report"),
    }

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
