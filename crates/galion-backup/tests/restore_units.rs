// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 326, offline and always on: the STAGED restore-test unit + timer
//! (deploy/staged/, NOT installed) run the Rust subcommand, are sandboxed,
//! and the timer never lands inside a backup window.

mod units;

use units::parse;

const SVC: &str = include_str!("../deploy/staged/galion-restore-test.service");
const TIMER: &str = include_str!("../deploy/staged/galion-restore-test.timer");
const BACKUP_TIMER: &str = include_str!("../deploy/galion-backup.timer");

fn get<'a>(v: &'a [(String, String, String)], sec: &str, key: &str) -> Vec<&'a str> {
    v.iter()
        .filter(|(s, k, _)| s == sec && k == key)
        .map(|(_, _, x)| x.as_str())
        .collect()
}

#[test]
fn staged_service_runs_rust_restore_test_sandboxed() {
    assert!(SVC.contains("NOT INSTALLED"));
    let u = parse(SVC);
    let exec = get(&u, "Service", "ExecStart");
    assert_eq!(exec.len(), 1);
    let (bin, args) = exec[0].split_once(' ').unwrap();
    assert!(
        bin.starts_with("/opt/galion/rust/bin/galion-backup-"),
        "{bin}"
    );
    assert_eq!(args, "restore-test", "newest archive, no shell script");
    assert!(!exec[0].contains(".sh"));
    for (k, v) in [
        ("Type", "oneshot"),
        ("UMask", "0077"),
        ("PrivateTmp", "yes"),
        ("PrivateNetwork", "yes"),
        ("NoNewPrivileges", "yes"),
        ("ProtectSystem", "strict"),
        ("ProtectHome", "yes"),
        ("Nice", "19"),
        ("IOSchedulingClass", "idle"),
    ] {
        assert_eq!(get(&u, "Service", k), vec![v], "{k}");
    }
    // ProtectSystem=strict makes / read-only: the subcommand only writes the
    // temp dir, which must be under /tmp (PrivateTmp), never under DEST.
    assert!(get(&u, "Service", "Environment").is_empty());
    assert!(get(&u, "Service", "ReadWritePaths").is_empty());
    assert_eq!(get(&u, "Unit", "After"), vec!["galion-backup.service"]);
}

fn hm(v: &str) -> (u32, u32) {
    // "*-*-* HH:MM:SS"
    let t = v.split_whitespace().nth(1).unwrap();
    let mut p = t.split(':');
    (
        p.next().unwrap().parse().unwrap(),
        p.next().unwrap().parse().unwrap(),
    )
}

#[test]
fn staged_timer_avoids_backup_runs() {
    assert!(TIMER.contains("NOT INSTALLED"));
    let t = parse(TIMER);
    let cal = get(&t, "Timer", "OnCalendar");
    assert_eq!(cal.len(), 1);
    let (h, m) = hm(cal[0]);
    let delay: u32 = get(&t, "Timer", "RandomizedDelaySec")[0]
        .trim_end_matches("min")
        .parse()
        .unwrap();
    assert_eq!(get(&t, "Timer", "Persistent"), vec!["true"]);
    assert_eq!(get(&t, "Install", "WantedBy"), vec!["timers.target"]);
    // galion-backup.timer: *-*-* 00/6:17:00 + up to 5 min.
    let b = parse(BACKUP_TIMER);
    assert_eq!(get(&b, "Timer", "OnCalendar"), vec!["*-*-* 00/6:17:00"]);
    let start = h * 60 + m;
    let end = start + delay + 15; // + generous run time (~1.5 s measured)
    for bh in (0..24).step_by(6) {
        let bs = bh * 60 + 17;
        let be = bs + 5 + 10; // backup window incl. its delay and run (~10 s measured)
        assert!(
            end <= bs || start >= be,
            "restore test {h:02}:{m:02} overlaps backup {bh:02}:17"
        );
    }
}

/// Slot 361: the staged unit uses the strict-checksum build, and the staged
/// .sh swap is documented with its gate and rollback (nothing installed).
#[test]
fn staged_swap_uses_strict_build_and_has_rollback() {
    const SWAP: &str = include_str!("../../../docs/ops/restore-checksum-swap.md");
    assert!(SVC.contains("/opt/galion/rust/bin/galion-backup-dd89bea4 restore-test"));
    assert!(!SVC.contains("galion-backup-2595481"));
    for must in [
        "NOT INSTALLED",
        "D-361-1",
        "galion-backup-dd89bea4",
        "ln -s galion-backup-dd89bea4 /opt/galion/rust/bin/galion-restore-test.sh",
        "Rollback",
        "galion-restore-test.sh.pre361",
        "30f58ba4a8e3d8b7b230fdbb6144858193164284d8c79ff3d35f5f178dbc51c6",
    ] {
        assert!(SWAP.contains(must), "swap doc lacks {must:?}");
    }
}
