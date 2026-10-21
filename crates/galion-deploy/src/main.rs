// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! galion-deploy binary: see the crate docs (src/lib.rs) and README.md.

/// `--check`: parse the deploy env exactly as a deploy would and check the
/// backup dir and data-backup script exist. Never locks, installs, restarts
/// or writes.
fn check() -> Result<String, String> {
    use galion_common::flags::{line, path_exists};
    let mut r = String::new();
    galion_deploy::deploy::manager_from_env()?;
    let sm = std::env::var("GALION_DEPLOY_SM").unwrap_or_default();
    line(
        &mut r,
        "manager",
        if sm.is_empty() {
            "systemd"
        } else {
            sm.as_str()
        },
    );
    let cfg = galion_deploy::deploy::Config::from_env("check", "-", None)?;
    line(
        &mut r,
        "version_timeout",
        cfg.version_timeout
            .map_or("off".to_string(), |d| format!("{} ms", d.as_millis())),
    );
    line(
        &mut r,
        "health",
        format!("{} tries x {} ms", cfg.tries, cfg.interval.as_millis()),
    );
    let mut ok = path_exists(&mut r, "backup_dir", &cfg.backup_dir, true);
    if cfg.skip_data_backup {
        line(&mut r, "data_backup", "skipped (SKIP_DATA_BACKUP=1)");
    } else {
        ok &= path_exists(&mut r, "data_backup", &cfg.data_backup, false);
    }
    if ok {
        Ok(r)
    } else {
        Err(format!("{r}backup dir or data-backup script missing"))
    }
}

fn main() {
    galion_common::flags::early(
        "galion-deploy",
        env!("CARGO_PKG_VERSION"),
        galion_deploy::deploy::HELP,
        check,
    );
    galion_deploy::sys::set_umask(0o077);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("backup") => galion_deploy::backup::main_backup(&args[1..]),
        Some("backup-verify") => galion_deploy::backup::main_verify(&args[1..]),
        _ => {
            galion_deploy::sys::install_handlers();
            galion_deploy::deploy::main(&args)
        }
    };
    std::process::exit(code);
}
