// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! LAN device discovery for OpenMind.
//!
//! Candidates come from a static seed list, an mDNS-SD style fixture, a real
//! mDNS / DNS-SD browse ([`mdns::browse`]) and an opt-in /24 port probe
//! ([`probe::probe`]). Every found device is stored as an OpenMind provider
//! row in [`Approval::Pending`](openmind_provider::Approval::Pending); only an
//! explicit `approve` admits it ([`admit`]). Nothing in the default path
//! opens a socket: the browse runs only for `find` / `--scan`, the probe only
//! for `--probe NET`, and the responder only for `announce`.

pub mod admit;
mod cli;
mod cmd;
pub mod dns;
pub mod mdns;
mod parse;
pub mod probe;
mod scan;
mod seed;

pub use cli::{cli, cli_with, read_fixture_file, CliError, CliOut, USAGE};
pub use cmd::run;
pub use parse::{fold_records, parse_mdns_sd, ParseError, RData, Record};
pub use scan::{active_scan, ScanOutcome};
pub use seed::static_seeds;

use serde::Serialize;
use std::collections::BTreeMap;

/// Approval string written on every candidate. Candidates are never
/// approved here; approval lives in the provider registry ([`admit`]).
pub const APPROVAL: &str = "pending";

/// Where a candidate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Row from the built-in seed list. Not a joined device.
    Seed,
    /// Folded out of mDNS-SD records (fixture or live browse).
    Mdns,
    /// Found by the opt-in TCP port probe.
    Probe,
}

/// What kind of compute a candidate looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceKind {
    /// A Galion edge node (or a Galion wrapper in front of Exo/Ollama/llama.cpp).
    Galion,
    /// Exo cluster HTTP API (default port 52415).
    Exo,
    /// llama.cpp `rpc-server` (binary protocol, default port 50052).
    LlamaRpc,
    /// llama.cpp `llama-server` HTTP API (default port 8080).
    LlamaCpp,
    /// Ollama HTTP API (default port 11434).
    Ollama,
    /// Another OpenMind peer (default port 7787).
    OpenMind,
}

impl DeviceKind {
    /// Every kind, in display order.
    pub const ALL: [DeviceKind; 6] = [
        DeviceKind::Galion,
        DeviceKind::Exo,
        DeviceKind::LlamaRpc,
        DeviceKind::LlamaCpp,
        DeviceKind::Ollama,
        DeviceKind::OpenMind,
    ];

    /// Stable short name, also the DNS-SD service label without `_`.
    pub const fn as_str(self) -> &'static str {
        match self {
            DeviceKind::Galion => "galion",
            DeviceKind::Exo => "exo",
            DeviceKind::LlamaRpc => "llama-rpc",
            DeviceKind::LlamaCpp => "llamacpp",
            DeviceKind::Ollama => "ollama",
            DeviceKind::OpenMind => "openmind",
        }
    }

    /// Parse a short name (`exo`, `llama-rpc`, ...).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    /// DNS-SD service type, e.g. `_exo._tcp.local.`.
    pub fn service_type(self) -> String {
        format!("_{}._tcp.local.", self.as_str())
    }

    /// Kind for a DNS-SD service type such as `_ollama._tcp.local.`.
    pub fn from_service_type(service_type: &str) -> Option<Self> {
        let label = service_type.strip_prefix('_')?.split('.').next()?;
        if !service_type.contains("._tcp.") {
            return None;
        }
        Self::parse(label)
    }

    /// HTTP path used to fingerprint an open port, `None` for non-HTTP.
    pub const fn fingerprint_path(self) -> Option<&'static str> {
        match self {
            DeviceKind::Exo => Some("/models"),
            DeviceKind::Ollama => Some("/api/version"),
            DeviceKind::LlamaCpp | DeviceKind::Galion | DeviceKind::OpenMind => Some("/health"),
            DeviceKind::LlamaRpc => None,
        }
    }

    /// Endpoint scheme stored in the provider row.
    pub const fn scheme(self) -> &'static str {
        match self {
            DeviceKind::LlamaRpc => "tcp",
            _ => "http",
        }
    }
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

impl Candidate {
    /// The device kind: from the service type, or from the probe's `kind` TXT.
    pub fn kind(&self) -> Option<DeviceKind> {
        DeviceKind::from_service_type(&self.service_type).or_else(|| {
            if self.origin == Origin::Probe {
                self.txt.get("kind").and_then(|k| DeviceKind::parse(k))
            } else {
                None
            }
        })
    }
}

/// Whether the caller asked for the active scan.
///
/// [`Scan::Off`] is the default. It does not call the scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scan {
    /// Do not call the scanner.
    Off,
    /// Call the scanner once (the built-in one is a real mDNS browse).
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
}

/// Seed rows plus any candidates folded from `fixture` and the scan.
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
        Scan::Requested => {
            let outcome = scanner();
            candidates.extend(outcome.candidates);
            ScanReport {
                requested: true,
                packets_sent: outcome.packets_sent,
                note: outcome.note,
            }
        }
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

    fn fake_outcome() -> ScanOutcome {
        let text = "\
_galion._tcp.local. IN PTR lab._galion._tcp.local.
lab._galion._tcp.local. IN SRV 0 0 19111 lab.local.
lab.local. IN A 192.0.2.44
";
        ScanOutcome {
            packets_sent: 2,
            note: "test-scanner".into(),
            candidates: fold_records(&parse_mdns_sd(text).unwrap()),
        }
    }

    #[test]
    fn requested_scan_merges_the_scanner_candidates() {
        let report = discover_with(None, Scan::Requested, fake_outcome).unwrap();
        assert!(report.scan.requested);
        assert_eq!(report.scan.packets_sent, 2);
        assert_eq!(report.scan.note, "test-scanner");
        assert_eq!(report.candidates.len(), static_seeds().len() + 1);
        let found = report.candidates.last().unwrap();
        assert_eq!(found.id, "lab");
        assert_eq!(found.kind(), Some(DeviceKind::Galion));
        assert_eq!(found.approval, APPROVAL);
    }

    #[test]
    fn device_kinds_roundtrip() {
        for kind in DeviceKind::ALL {
            assert_eq!(DeviceKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                DeviceKind::from_service_type(&kind.service_type()),
                Some(kind)
            );
        }
        assert_eq!(DeviceKind::from_service_type("_ipp._tcp.local."), None);
        assert_eq!(DeviceKind::from_service_type("_exo._udp.local."), None);
        assert_eq!(DeviceKind::LlamaRpc.scheme(), "tcp");
        assert_eq!(DeviceKind::LlamaRpc.fingerprint_path(), None);
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
    fn scan_gate_delegates_sockets_to_the_mdns_module() {
        let src = include_str!("scan.rs");
        assert!(src.contains("browse(&BrowseOpts::default())"));
        let src = include_str!("scan.rs");
        assert!(!src.contains("UdpSocket"));
        assert!(!src.contains("TcpStream"));
        assert!(!src.contains("std::net"));
    }

    #[test]
    fn default_cli_does_not_read_a_fixture_or_scan() {
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
    fn scan_flag_calls_the_scanner_once() {
        let mut calls = 0;
        let out = cli_with(
            ["--scan"],
            |_| -> io::Result<String> { panic!("fixture read") },
            || {
                calls += 1;
                fake_outcome()
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        match out {
            CliOut::Report(value) => {
                assert_eq!(value["scan"]["requested"], true);
                assert_eq!(value["scan"]["packets_sent"], 2);
                let rows = value["candidates"].as_array().unwrap();
                assert!(rows
                    .iter()
                    .any(|row| row["id"] == "lab" && row["approval"] == "pending"));
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
