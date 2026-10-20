// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! LAN discovery stub.
//!
//! Candidates come from a static seed list and, when a fixture is supplied,
//! from an mDNS-SD style record parser. Nothing in the default path opens a
//! socket. [`Scan::Requested`] is the only path that calls a scanner, and the
//! built-in scanner sends no packets. No candidate is approved: this crate
//! does not join a device.

mod cli;
mod parse;
mod scan;
mod seed;

pub use cli::{cli, read_fixture_file, CliError, CliOut, USAGE};
pub use parse::{fold_records, parse_mdns_sd, ParseError, RData, Record};
pub use scan::{active_scan, ScanOutcome};
pub use seed::static_seeds;

use serde::Serialize;
use std::collections::BTreeMap;

/// Approval string written on every candidate. This stub has no approve path.
pub const APPROVAL: &str = "pending";

/// Where a candidate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Row from the built-in seed list. Not a joined device.
    Seed,
    /// Folded out of mDNS-SD style records.
    Mdns,
}

/// One discovery candidate. `approval` is always [`APPROVAL`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub id: String,
    pub instance: String,
    pub service_type: String,
    pub host: String,
    pub port: Option<u16>,
    pub addrs: Vec<String>,
    pub txt: BTreeMap<String, String>,
    pub origin: Origin,
    pub approval: String,
}

/// Whether the caller asked for the active scan.
///
/// [`Scan::Off`] is the default. It does not call the scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scan {
    /// Do not call the scanner.
    Off,
    /// Call the scanner once. The built-in scanner sends no packets.
    Requested,
}

/// What the report records about the scan gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanReport {
    pub requested: bool,
    pub packets_sent: u32,
    pub note: String,
}

impl ScanReport {
    fn off() -> Self {
        Self {
            requested: false,
            packets_sent: 0,
            note: "off".to_owned(),
        }
    }

    fn from_outcome(outcome: ScanOutcome) -> Self {
        Self {
            requested: true,
            packets_sent: outcome.packets_sent,
            note: outcome.note.to_owned(),
        }
    }
}

/// Seed rows plus any candidates folded from `fixture`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub scan: ScanReport,
    pub candidates: Vec<Candidate>,
}

/// Build a report. `Scan::Off` does not call `scanner`.
///
/// A fixture parse error returns before the scanner runs.
pub fn discover_with<F>(fixture: Option<&str>, scan: Scan, scanner: F) -> Result<Report, ParseError>
where
    F: FnOnce() -> ScanOutcome,
{
    let mut candidates = static_seeds();
    if let Some(text) = fixture {
        let records = parse_mdns_sd(text)?;
        candidates.extend(fold_records(&records));
    }
    let scan_report = match scan {
        Scan::Off => ScanReport::off(),
        Scan::Requested => ScanReport::from_outcome(scanner()),
    };
    Ok(Report {
        scan: scan_report,
        candidates,
    })
}

/// [`discover_with`] using [`active_scan`] when a scan was requested.
pub fn discover(fixture: Option<&str>, scan: Scan) -> Result<Report, ParseError> {
    discover_with(fixture, scan, active_scan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn default_discover_does_not_call_the_scanner() {
        let report = discover_with(None, Scan::Off, || panic!("scanner called")).unwrap();
        assert!(!report.scan.requested);
        assert_eq!(report.scan.packets_sent, 0);
        assert_eq!(report.scan.note, "off");
        assert_eq!(report.candidates, static_seeds());
        assert!(report.candidates.iter().all(|row| row.approval == APPROVAL));
        assert!(report
            .candidates
            .iter()
            .all(|row| row.origin == Origin::Seed));
    }

    #[test]
    fn bad_fixture_returns_before_the_scanner() {
        let err = discover_with(Some("not a record\n"), Scan::Requested, || {
            panic!("scanner called")
        })
        .unwrap_err();
        assert!(err.line >= 1);
    }

    #[test]
    fn requested_scan_uses_the_stub_and_sends_no_packets() {
        let report = discover(None, Scan::Requested).unwrap();
        assert!(report.scan.requested);
        assert_eq!(report.scan.packets_sent, 0);
        assert!(report.scan.note.contains("no packets"));
        assert_eq!(active_scan().packets_sent, 0);
    }

    #[test]
    fn requested_scan_reports_the_injected_scanner() {
        let report = discover_with(None, Scan::Requested, || ScanOutcome {
            packets_sent: 0,
            note: "test-scanner",
        })
        .unwrap();
        assert!(report.scan.requested);
        assert_eq!(report.scan.packets_sent, 0);
        assert_eq!(report.scan.note, "test-scanner");
    }

    #[test]
    fn fixture_candidates_follow_the_seeds() {
        let text = "\
_openmind._tcp.local. IN PTR desk._openmind._tcp.local.
desk._openmind._tcp.local. IN SRV 0 0 7787 desk.local.
desk.local. IN A 192.0.2.10
";
        let report = discover_with(Some(text), Scan::Off, || panic!("scanner called")).unwrap();
        assert_eq!(report.candidates.len(), static_seeds().len() + 1);
        let last = report.candidates.last().unwrap();
        assert_eq!(last.origin, Origin::Mdns);
        assert_eq!(last.id, "desk");
        assert_eq!(last.port, Some(7787));
        assert_eq!(last.addrs, vec!["192.0.2.10".to_string()]);
        assert_eq!(last.approval, APPROVAL);
        assert!(!report.scan.requested);
    }

    #[test]
    fn scan_source_names_no_socket_type() {
        let src = include_str!("scan.rs");
        assert!(!src.contains("UdpSocket"));
        assert!(!src.contains("TcpStream"));
        assert!(!src.contains("std::net"));
    }

    #[test]
    fn default_cli_does_not_read_a_fixture() {
        let out = cli(Vec::<&str>::new(), |_| -> io::Result<String> {
            panic!("fixture read")
        })
        .unwrap();
        match out {
            CliOut::Report(value) => {
                assert_eq!(value["scan"]["requested"], false);
                assert_eq!(value["scan"]["packets_sent"], 0);
                let rows = value["candidates"].as_array().unwrap();
                assert_eq!(rows.len(), static_seeds().len());
                assert!(rows.iter().all(|row| row["approval"] == "pending"));
                assert!(rows.iter().all(|row| row["origin"] == "seed"));
            }
            CliOut::Help => panic!("expected a report"),
        }
    }

    #[test]
    fn help_does_not_read_or_scan() {
        let out = cli(["--scan", "--help"], |_| -> io::Result<String> {
            panic!("fixture read")
        })
        .unwrap();
        assert!(matches!(out, CliOut::Help));
    }

    #[test]
    fn unknown_argument_is_usage() {
        let err = cli(["--probe"], |_| -> io::Result<String> {
            panic!("fixture read")
        })
        .unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn scan_flag_is_the_stub() {
        let out = cli(["--scan"], |_| -> io::Result<String> {
            panic!("fixture read")
        })
        .unwrap();
        match out {
            CliOut::Report(value) => {
                assert_eq!(value["scan"]["requested"], true);
                assert_eq!(value["scan"]["packets_sent"], 0);
            }
            CliOut::Help => panic!("expected a report"),
        }
    }

    #[test]
    fn fixture_flag_parses_without_scanning() {
        let text = "_exo._tcp.local. IN PTR box._exo._tcp.local.\n";
        let out = cli(["--fixture", "peer.mdns"], |path| {
            assert_eq!(path, "peer.mdns");
            Ok(text.to_string())
        })
        .unwrap();
        match out {
            CliOut::Report(value) => {
                assert_eq!(value["scan"]["requested"], false);
                assert_eq!(value["scan"]["packets_sent"], 0);
                let rows = value["candidates"].as_array().unwrap();
                assert!(rows.iter().any(|row| row["id"] == "box"));
            }
            CliOut::Help => panic!("expected a report"),
        }
    }

    #[test]
    fn missing_fixture_path_is_usage() {
        let err = cli(["--fixture"], |_| -> io::Result<String> {
            panic!("fixture read")
        })
        .unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn fixture_read_error_is_a_failure() {
        let err = cli(["--fixture", "missing.mdns"], |_path| {
            Err(io::Error::new(io::ErrorKind::NotFound, "missing"))
        })
        .unwrap_err();
        match err {
            CliError::Fail(msg) => assert!(msg.contains("missing.mdns")),
            CliError::Usage(_) => panic!("expected a failure"),
        }
    }
}
