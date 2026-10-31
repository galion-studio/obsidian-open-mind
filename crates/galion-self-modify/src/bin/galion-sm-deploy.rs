// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-sm-deploy: the ONLY root step. Refuses unless all four signatures
//! (stack, sandbox test, sandbox dry-run, owner) and every fingerprint
//! verify, the nonce is unused and the approval is not expired.
//!
//!   galion-sm-deploy verify   --entry E [paths...]   read-only, burns nothing (any user)
//!   galion-sm-deploy deploy   --entry E [paths...]   root: burn nonce, backup, install, ONE restart,
//!                                                    health check, auto-rollback, ntfy either way
//!   galion-sm-deploy rollback --entry E [paths...]   root, by a person
//!
//! Paths (defaults match the server layout):
//!   --stack-pub /etc/hermes-self-modify/stack.pub   --sandbox-pub /etc/hermes-self-modify/sandbox-v2.pub
//!   --owner-pub /etc/hermes-self-modify/owner.pub   --ledger /var/lib/galion-sm-deploy/used-nonces
//!   --backup-dir /var/lib/galion-sm-deploy/backups  --bin-dir /opt/galion/rust/bin
//!   --dropin-dir /etc/systemd/system/hermes-rs.service.d  --dropin-name 99j-self-modify-approved.conf
//!   --unit hermes-rs.service  --health 127.0.0.1:8642  --health-path /health  --health-timeout-s 60
//!   --ntfy 127.0.0.1:2586  --systemctl /usr/bin/systemctl  [--line 'Environment=K=V']...
//!
//! Refuses inside a Hermes runtime environment (HERMES_HOME / HERMES_RS_*).
//! Exit: 0 ok, 1 refused / rolled back, 2 usage or I/O.

use galion_self_modify::deploy::{self, DeployCfg, Keys, RealSystem};
use galion_self_modify::{owner, util};
use std::path::PathBuf;

fn die(code: i32, m: &str) -> ! {
    eprintln!("galion-sm-deploy: {m}");
    std::process::exit(code)
}

fn opt(a: &[String], k: &str, d: &str) -> String {
    a.iter()
        .position(|x| x == k)
        .and_then(|i| a.get(i + 1))
        .cloned()
        .unwrap_or_else(|| d.to_string())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if std::env::vars().any(|(k, _)| k == "HERMES_HOME" || k.starts_with("HERMES_RS_")) {
        die(1, "refused: Hermes runtime environment detected");
    }
    let cmd = a.first().cloned().unwrap_or_default();
    let entry = PathBuf::from(opt(&a, "--entry", ""));
    if entry.as_os_str().is_empty() {
        die(2, "missing --entry");
    }
    let p = |k: &str, d: &str| PathBuf::from(opt(&a, k, d));
    let cfg = DeployCfg {
        bin_dir: p("--bin-dir", "/opt/galion/rust/bin"),
        dropin_dir: p("--dropin-dir", "/etc/systemd/system/hermes-rs.service.d"),
        dropin_name: opt(&a, "--dropin-name", "99j-self-modify-approved.conf"),
        unit: opt(&a, "--unit", "hermes-rs.service"),
        ledger: p("--ledger", "/var/lib/galion-sm-deploy/used-nonces"),
        backup_dir: p("--backup-dir", "/var/lib/galion-sm-deploy/backups"),
        extra_lines: a
            .iter()
            .enumerate()
            .filter(|(_, x)| *x == "--line")
            .filter_map(|(i, _)| a.get(i + 1).cloned())
            .collect(),
    };
    let mut sys = RealSystem {
        systemctl: p("--systemctl", "/usr/bin/systemctl"),
        health_addr: opt(&a, "--health", "127.0.0.1:8642"),
        health_path: opt(&a, "--health-path", "/health"),
        health_timeout_s: opt(&a, "--health-timeout-s", "60").parse().unwrap_or(60),
        ntfy_addr: opt(&a, "--ntfy", "127.0.0.1:2586"),
    };
    if cmd == "rollback" {
        if util::euid() != 0 {
            die(1, "refused: rollback runs as root");
        }
        match deploy::rollback(&entry, &cfg, &mut sys, util::now_secs()) {
            Ok(r) => println!("{r}"),
            Err(e) => die(1, &e),
        }
        return;
    }
    let keys = (|| {
        let owner_text = util::read_file(
            &p("--owner-pub", "/etc/hermes-self-modify/owner.pub"),
            64 * 1024,
        )?;
        Ok::<Keys, String>(Keys {
            stack: util::load_pub(&p("--stack-pub", "/etc/hermes-self-modify/stack.pub"))?,
            sandbox: util::load_pub(&p(
                "--sandbox-pub",
                "/etc/hermes-self-modify/sandbox-v2.pub",
            ))?,
            owner: owner::OwnerKey::parse(&String::from_utf8_lossy(&owner_text))?,
        })
    })()
    .unwrap_or_else(|e| die(2, &format!("keys: {e}")));
    match cmd.as_str() {
        "verify" => match deploy::verify_all(&entry, &keys, &cfg.ledger, util::now_secs()) {
            Ok(v) => println!(
                "OK: all 4 signatures verify; id {} artifact {} base {} owner {} nonce {} expires {}",
                v.id,
                v.artifact_sha256,
                v.base_commit,
                v.owner_scheme,
                v.nonce,
                hermes_rs::time::zagreb_rfc3339(v.expires)
            ),
            Err(e) => die(1, &e),
        },
        "deploy" => {
            if util::euid() != 0 {
                die(1, "refused: deploy runs as root");
            }
            match deploy::deploy(&entry, &keys, &cfg, &mut sys, util::now_secs()) {
                Ok(r) => println!("{r}"),
                Err(e) => die(1, &e),
            }
        }
        _ => die(2, "usage: verify | deploy | rollback (see source header)"),
    }
}
