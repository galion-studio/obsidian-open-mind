// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 269: the repo mirror of the live galion-backup unit set
//! (deploy/galion-backup.service + .service.d/90-deploy-t273u.conf + timer)
//! must resolve to the Rust ELF, and removing the drop-in (the rollback)
//! must resolve back to the bash script. Offline, always on.

mod units;

use units::{effective, effective_list, parse};

const BASE: &str = include_str!("../deploy/galion-backup.service");
const DROPIN: &str = include_str!("../deploy/galion-backup.service.d/90-deploy-t273u.conf");
const TIMER: &str = include_str!("../deploy/galion-backup.timer");
/// The live ws/offsite drop-in body (read-only capture 2026-10-27 18:23 CET
/// +01:00); not owned by this crate, so only quoted here.
const OFFSITE: &str = "[Service]\n\
ExecStartPost=/opt/galion/offsite/galion-offsite-encrypt.sh\n\
ExecStopPost=/opt/galion/offsite/galion-offsite-encrypt.sh --perms-only\n";

pub const RUST_EXEC: &str = "/opt/galion/rust/bin/galion-backup-98466ab run";
pub const SH_EXEC: &str = "/opt/galion/rust/bin/galion-backup.sh";

#[test]
fn live_set_runs_rust_elf_once() {
    // systemd orders drop-ins by file name: 90-deploy-t273u.conf < offsite.conf.
    let files = [BASE, DROPIN, OFFSITE];
    assert_eq!(
        effective_list(&files, "Service", "ExecStart"),
        vec![RUST_EXEC]
    );
    assert_eq!(
        effective(&files, "Service", "Type").as_deref(),
        Some("oneshot")
    );
    assert_eq!(
        effective(&files, "Service", "UMask").as_deref(),
        Some("0077")
    );
    assert_eq!(effective(&files, "Service", "Nice").as_deref(), Some("19"));
    assert_eq!(
        effective(&files, "Service", "IOSchedulingClass").as_deref(),
        Some("idle")
    );
    // offsite hooks survive the ExecStart reset (different keys).
    assert_eq!(effective_list(&files, "Service", "ExecStartPost").len(), 1);
    assert_eq!(effective_list(&files, "Service", "ExecStopPost").len(), 1);
}

#[test]
fn rollback_is_removing_the_dropin() {
    let files = [BASE, OFFSITE];
    assert_eq!(
        effective_list(&files, "Service", "ExecStart"),
        vec![SH_EXEC]
    );
}

#[test]
fn dropin_without_reset_would_be_invalid_for_oneshot() {
    // Negative control: without the empty `ExecStart=` a oneshot would get
    // two ExecStart lines (bash THEN rust = two backups per timer tick).
    let bad = DROPIN.replace("ExecStart=\n", "");
    let files = [BASE, bad.as_str(), OFFSITE];
    assert_eq!(effective_list(&files, "Service", "ExecStart").len(), 2);
}

#[test]
fn dropin_only_touches_execstart() {
    for (s, k, _) in parse(DROPIN) {
        assert_eq!((s.as_str(), k.as_str()), ("Service", "ExecStart"));
    }
    let argv: Vec<&str> = RUST_EXEC.split_whitespace().collect();
    assert_eq!(
        argv.len(),
        2,
        "binary + `run` only (main.rs requires argc == 2)"
    );
    assert_eq!(argv[1], "run");
}

#[test]
fn timer_unchanged() {
    let t = [TIMER];
    assert_eq!(
        effective(&t, "Timer", "OnCalendar").as_deref(),
        Some("*-*-* 00/6:17:00")
    );
    assert_eq!(
        effective(&t, "Timer", "Persistent").as_deref(),
        Some("true")
    );
    assert_eq!(
        effective(&t, "Timer", "RandomizedDelaySec").as_deref(),
        Some("5min")
    );
    assert_eq!(
        effective(&t, "Install", "WantedBy").as_deref(),
        Some("timers.target")
    );
}

#[test]
fn spdx_headers() {
    for f in [BASE, DROPIN, TIMER] {
        assert!(f.starts_with("# SPDX-License-Identifier: MIT\n"));
        assert!(f.contains("Copyright (c) 3000 Galion / galion-studio"));
    }
}
