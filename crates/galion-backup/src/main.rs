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
//!   restore-test [archive]       restore the newest (or given) archive into a temp dir and
//!                                verify it (replaces galion-restore-test.sh; env GALION_BACKUP_DEST)
//!
//! Multi-call: invoked as `galion-restore-test` or `galion-restore-test.sh`
//! (a symlink, the staged swap of slot 361) it behaves as `restore-test [archive]`.

use galion_backup::{backup, restore, sqlite};
use std::path::Path;

fn die(m: &str) -> ! {
    eprintln!("galion-backup: {m}");
    std::process::exit(1)
}

fn main() {
    let mut a: Vec<String> = std::env::args().collect();
    if galion_backup::restore::is_restore_test_alias(a.first().map(|s| s.as_str()).unwrap_or("")) {
        a.insert(1, "restore-test".into());
    }
    match a.get(1).map(|s| s.as_str()) {
        Some("run") if a.len() == 2 => {
            let o = backup::Opts::from_env().unwrap_or_else(|e| die(&e));
            backup::install_signal_handlers();
            std::process::exit(backup::run(&o));
        }
        Some("snapshot") if a.len() == 4 => {
            sqlite::snapshot(Path::new(&a[2]), Path::new(&a[3])).unwrap_or_else(|e| die(&e));
            println!("snapshot ok {} -> {}", a[2], a[3]);
        }
        Some("verify") if a.len() == 3 => {
            let (r, t) = sqlite::verify(Path::new(&a[2])).unwrap_or_else(|e| die(&e));
            println!("verify {} integrity={} tables={}", a[2], r, t);
            if r != "ok" {
                std::process::exit(1)
            }
        }
        Some("schema") if a.len() == 3 => {
            for l in sqlite::schema(Path::new(&a[2])).unwrap_or_else(|e| die(&e)) {
                println!("{l}");
            }
        }
        Some("migrate-baseline") if a.len() == 5 => {
            let ver: i64 = a[3].parse().unwrap_or_else(|_| die("bad version"));
            let n = sqlite::migrate_baseline(Path::new(&a[2]), ver, &a[4]).unwrap_or_else(|e| die(&e));
            println!(
                "schema_migrations baseline v{ver} {}",
                if n { "recorded" } else { "already present" }
            );
        }
        Some("restore-test") if a.len() <= 3 => {
            // umask 077 like the .sh: the restored copies include secrets.
            // SAFETY: umask only changes this process's file-creation mask.
            unsafe {
                libc::umask(0o077);
            }
            backup::install_signal_handlers();
            let o = restore::Opts::from_env(a.get(2).map(|s| s.as_str()));
            let mut out = std::io::stdout().lock();
            match restore::run(&o, &mut out) {
                Ok(()) => {}
                Err(restore::Fail::Msg(m)) => die(&format!("restore-test: {m}")),
                Err(restore::Fail::Signal(s)) => {
                    eprintln!("galion-backup: restore-test: interrupted by signal {s}");
                    std::process::exit(128 + s)
                }
            }
        }
        _ => die("usage: run | restore-test [archive] | snapshot <src> <dst> | verify <db> | schema <db> | migrate-baseline <db> <ver> <name>"),
    }
}
