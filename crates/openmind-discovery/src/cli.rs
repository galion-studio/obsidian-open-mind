// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Legacy flag form: `openmind-discovery [--fixture PATH] [--scan]`.
//!
//! `--scan` is off unless the operator passes it. Passing it runs one real
//! mDNS browse (at most two query packets). Tests inject their own scanner
//! through [`cli_with`]. The subcommands live in `cmd.rs`.

use crate::{active_scan, discover_with, Scan, ScanOutcome};
use serde_json::Value;
use std::fs;
use std::io;

/// Usage text printed for `--help` and for a bad argument list.
pub const USAGE: &str = "\
openmind-discovery [--fixture PATH] [--scan]
openmind-discovery find     [--registry PATH] [--log PATH] [--timeout-ms N] [--mdns-port P]
                            [--iface IPV4] [--services a,b] [--no-mdns] [--dry-run] [--by NAME]
                            [--probe A.B.C.0/24 [--probe-ports kind:port,..] [--probe-hosts N-M] [--rate N]]
openmind-discovery list     [--registry PATH] [--approval pending|approved|rejected]
openmind-discovery approve  ID [--registry PATH] [--log PATH] [--by NAME] [--note TEXT]
openmind-discovery deny     ID [--registry PATH] [--log PATH] [--by NAME] [--note TEXT]
openmind-discovery join     ID [--registry PATH] [--log PATH] [--by NAME] [--timeout-ms N]
openmind-discovery announce --service TYPE:INSTANCE:PORT[:k=v;k=v] [--service ...]
                            [--host NAME] [--addr IPV4] [--mdns-port P] [--duration-s N] [--ttl S]

Without a subcommand: print seed candidates as JSON. No socket is opened.
  --fixture PATH  add candidates parsed from an mDNS-SD style record file.
  --scan          run one real mDNS browse (2 query packets, 2 s) and add what answers.
find      browse mDNS for _galion _exo _llama-rpc _llamacpp _ollama _openmind (one query plus
          one repeat to 224.0.0.251, bounded wait) and, only with --probe, TCP-probe one local /24.
          Every device becomes a provider row with approval pending. Known rows keep their approval.
list      print provider rows.
approve   mark a row approved (the only way a device is admitted). Logged with a Zagreb timestamp.
deny      mark a row rejected. Logged.
join      approved rows only: one TCP connect to the local endpoint, health up/down. Logged.
announce  answer mDNS for the given services only (binds the mDNS port shared, SO_REUSEPORT),
          2 startup announcements, at most 20 replies per second, then exit after --duration-s.
Defaults: --registry ./discovery-providers.json, --log <registry>.approvals.log, --mdns-port 5353.
";

/// Successful CLI result.
#[derive(Debug, Clone, PartialEq)]
pub enum CliOut {
    Help,
    Report(Value),
}

/// Usage mistakes and fixture failures.
#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Fail(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(msg) | CliError::Fail(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for CliError {}

/// Read a fixture file. Rejects non-files and anything over 64 KiB.
pub fn read_fixture_file(path: &str) -> io::Result<String> {
    const MAX: u64 = 65_536;
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture is not a file",
        ));
    }
    if meta.len() > MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "fixture is larger than 64 KiB",
        ));
    }
    fs::read_to_string(path)
}

/// Parse args and build the JSON report with the real scanner.
///
/// `read` is called only when `--fixture` is present. The mDNS browse runs
/// only when `--scan` is present.
pub fn cli<I, S, F>(args: I, read: F) -> Result<CliOut, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: FnMut(&str) -> io::Result<String>,
{
    cli_with(args, read, active_scan)
}

/// [`cli`] with an injected scanner, called at most once and only for `--scan`.
pub fn cli_with<I, S, F, G>(args: I, mut read: F, scanner: G) -> Result<CliOut, CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    F: FnMut(&str) -> io::Result<String>,
    G: FnOnce() -> ScanOutcome,
{
    let mut fixture_path: Option<String> = None;
    let mut scan = Scan::Off;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "--help" | "-h" => return Ok(CliOut::Help),
            "--scan" => {
                if scan == Scan::Requested {
                    return Err(CliError::Usage("duplicate --scan".into()));
                }
                scan = Scan::Requested;
            }
            "--fixture" => {
                if fixture_path.is_some() {
                    return Err(CliError::Usage("duplicate --fixture".into()));
                }
                let Some(path) = args.next() else {
                    return Err(CliError::Usage("missing path after --fixture".into()));
                };
                let path = path.as_ref();
                if path.is_empty() || path.starts_with('-') {
                    return Err(CliError::Usage("missing path after --fixture".into()));
                }
                fixture_path = Some(path.to_owned());
            }
            other => {
                let shown: String = other.chars().take(64).collect();
                return Err(CliError::Usage(format!("unknown argument: {shown}")));
            }
        }
    }

    let text = if let Some(path) = fixture_path.as_deref() {
        match read(path) {
            Ok(text) => Some(text),
            Err(err) => return Err(CliError::Fail(format!("fixture {path}: {err}"))),
        }
    } else {
        None
    };

    let report = discover_with(text.as_deref(), scan, scanner)
        .map_err(|err| CliError::Fail(err.to_string()))?;
    let value = serde_json::to_value(&report).map_err(|err| CliError::Fail(err.to_string()))?;
    Ok(CliOut::Report(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process;

    #[test]
    fn duplicate_flags_are_usage() {
        let err = cli(["--scan", "--scan"], |_| -> io::Result<String> {
            panic!("read")
        })
        .unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
        let err = cli(
            ["--fixture", "a", "--fixture", "b"],
            |_| -> io::Result<String> { panic!("read") },
        )
        .unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn read_fixture_file_rejects_a_directory_and_a_missing_path() {
        let err = read_fixture_file("/tmp").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        let err = read_fixture_file("/tmp/openmind-discovery-missing-b35").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn read_fixture_file_rejects_oversize() {
        let id = process::id();
        let path = std::env::temp_dir().join(format!("openmind-discovery-b35-big-{id}"));
        let file = fs::File::create(&path).unwrap();
        file.set_len(65_537).unwrap();
        let result = read_fixture_file(path.to_str().unwrap());
        let _ = fs::remove_file(&path);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
