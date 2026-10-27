// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! galion-backup <cmd>:
//!   run                          one periodic backup (replaces galion-backup.sh;
//!                                env GALION_BACKUP_DEST / _KEEP / _MINFREE_KB)
//!   snapshot <src.db> <dst.db>   online backup API copy (consistent with live WAL writers)
//!   verify <db>                  PRAGMA integrity_check + table count; exit 1 if not "ok"
//!   schema <db>                  print user_version and schema_migrations rows (read-only)
//!   migrate-baseline <db> <ver> <name>   create schema_migrations if missing, record baseline (idempotent)

use galion_backup::{backup, sqlite};
use std::path::Path;

fn die(m: &str) -> ! {
    eprintln!("galion-backup: {m}");
    std::process::exit(1)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(|s| s.as_str()) {
        Some("run") if a.len() == 2 => {
            let o = backup::Opts::from_env().unwrap_or_else(|e| die(&e));
            // NERVE push (no-op unless NERVE_URL is set): started + heartbeat.
            nerve_client::event(
                "galion-backup",
                "started",
                &format!("run dest={}", o.dest.display()),
            );
            nerve_client::start_heartbeat("galion-backup", 60);
            backup::install_signal_handlers();
            std::process::exit(backup::run(&o));
        }
        Some("snapshot") if a.len() == 4 => {
            nerve_client::event(
                "galion-backup",
                "started",
                &format!("snapshot {} -> {}", a[2], a[3]),
            );
            sqlite::snapshot(Path::new(&a[2]), Path::new(&a[3])).unwrap_or_else(|e| die(&e));
            println!("snapshot ok {} -> {}", a[2], a[3]);
        }
        Some("verify") if a.len() == 3 => {
            nerve_client::event("galion-backup", "started", &format!("verify {}", a[2]));
            let (r, t) = sqlite::verify(Path::new(&a[2])).unwrap_or_else(|e| die(&e));
            println!("verify {} integrity={} tables={}", a[2], r, t);
            if r != "ok" {
                std::process::exit(1)
            }
        }
        Some("schema") if a.len() == 3 => {
            nerve_client::event("galion-backup", "started", &format!("schema {}", a[2]));
            for l in sqlite::schema(Path::new(&a[2])).unwrap_or_else(|e| die(&e)) {
                println!("{l}");
            }
        }
        Some("migrate-baseline") if a.len() == 5 => {
            let ver: i64 = a[3].parse().unwrap_or_else(|_| die("bad version"));
            nerve_client::event(
                "galion-backup",
                "started",
                &format!("migrate-baseline {} v{ver}", a[2]),
            );
            let n = sqlite::migrate_baseline(Path::new(&a[2]), ver, &a[4]).unwrap_or_else(|e| die(&e));
            println!(
                "schema_migrations baseline v{ver} {}",
                if n { "recorded" } else { "already present" }
            );
        }
        _ => die("usage: run | snapshot <src> <dst> | verify <db> | schema <db> | migrate-baseline <db> <ver> <name>"),
    }
}
