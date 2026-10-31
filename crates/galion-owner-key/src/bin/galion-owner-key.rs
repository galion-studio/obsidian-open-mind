// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-owner-key`: the ROOT helper that installs Slavik's owner key.
//!
//! The console (galion-one) only stages an install request after Slavik
//! created the key on his device, confirmed its fingerprint and signed the
//! activation with it. A person runs, as root:
//!
//! ```text
//! galion-owner-key install --request /var/lib/galion-one/approval/owner-key/install-request.json --dry-run
//! galion-owner-key install --request /var/lib/galion-one/approval/owner-key/install-request.json
//! ```
//!
//! It verifies everything again, and only then writes
//! `/etc/hermes-self-modify/owner.pub` (root:root 0444, atomic, old key kept
//! as `owner.pub.off-<unix>`). Replacing an installed key needs the current
//! key's signature. It never touches /var/lib/galion-gate, never restarts
//! anything and prints only fingerprints.

use galion_owner_key as ok;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  galion-owner-key fingerprint <owner.pub>
  galion-owner-key verify-approval <owner.pub> <approval.json>
  galion-owner-key install --request <install-request.json> [--dest /etc/hermes-self-modify/owner.pub] [--dry-run]";

fn now() -> i64 {
    std::time::SystemTime::now() // repo-check: allow (unix epoch number for ages, ids or temp names; never printed as a timestamp)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn euid() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = s.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

fn read_plain(p: &Path) -> Result<String, String> {
    let m = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !m.file_type().is_file() {
        return Err(format!(
            "{} is not a regular file (symlinks refused)",
            p.display()
        ));
    }
    if m.len() > 64 * 1024 {
        return Err(format!("{} is too large", p.display()));
    }
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("--version") => Ok(format!("galion-owner-key {}", env!("CARGO_PKG_VERSION"))),
        Some("fingerprint") => {
            let p = args.get(1).ok_or(USAGE)?;
            let k = ok::OwnerKey::parse(&read_plain(Path::new(p))?)?;
            Ok(format!(
                "scheme {}\nkey_id {}\nfingerprint {}\nshort {}",
                k.scheme(),
                k.key_id(),
                ok::group4(&k.fingerprint()),
                &k.fingerprint()[..16]
            ))
        }
        Some("verify-approval") => {
            let (kp, ap) = (args.get(1).ok_or(USAGE)?, args.get(2).ok_or(USAGE)?);
            let k = ok::OwnerKey::parse(&read_plain(Path::new(kp))?)?;
            let env: serde_json::Value = serde_json::from_str(&read_plain(Path::new(ap))?)
                .map_err(|_| "approval is not JSON".to_string())?;
            let b = ok::verify_owner_approval(&k, &env, now())?;
            Ok(format!(
                "ok owner signature ({}) for {} artifact {} expires in {} s",
                k.scheme(),
                b["id"].as_str().unwrap_or("?"),
                &b["artifact_sha256"].as_str().unwrap_or("?")[..16],
                b["expires"].as_i64().unwrap_or(0) - now()
            ))
        }
        Some("install") => {
            let mut req = None;
            let mut dest = PathBuf::from(ok::OWNER_PUB);
            let mut dry = false;
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--request" => req = it.next().map(PathBuf::from),
                    "--dest" => dest = it.next().map(PathBuf::from).ok_or(USAGE)?,
                    "--dry-run" => dry = true,
                    _ => return Err(USAGE.into()),
                }
            }
            let req = req.ok_or(USAGE)?;
            let root = euid() == Some(0);
            // The real key file is root-only; a test --dest elsewhere may run unprivileged.
            if !dry && !root && dest == std::path::Path::new(ok::OWNER_PUB) {
                return Err("install must run as root (try --dry-run to check first)".into());
            }
            let v: serde_json::Value = serde_json::from_str(&read_plain(&req)?)
                .map_err(|_| "install request is not JSON".to_string())?;
            let out = ok::install(
                &v,
                &dest,
                now(),
                &ok::InstallOpts {
                    chown_root: root,
                    dry_run: dry,
                },
            )?;
            Ok(match out {
                ok::InstallOutcome::Installed {
                    fingerprint,
                    backup,
                } => format!(
                    "installed {} fingerprint {}{}",
                    dest.display(),
                    ok::group4(&fingerprint),
                    backup
                        .map(|b| format!("\nprevious key kept as {}", b.display()))
                        .unwrap_or_default()
                ),
                ok::InstallOutcome::AlreadyInstalled { fingerprint } => {
                    format!(
                        "already installed, fingerprint {}",
                        ok::group4(&fingerprint)
                    )
                }
                ok::InstallOutcome::WouldInstall {
                    fingerprint,
                    replaces,
                } => format!(
                    "dry run: verified; would install fingerprint {}{}",
                    ok::group4(&fingerprint),
                    replaces
                        .map(|r| format!(" replacing {}", ok::group4(&r)))
                        .unwrap_or_else(|| " (no key installed yet)".into())
                ),
            })
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("galion-owner-key: {e}");
            ExitCode::from(2)
        }
    }
}
