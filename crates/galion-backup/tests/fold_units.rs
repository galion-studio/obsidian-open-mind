// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 311 (D-269-1 prep), offline and always on: the STAGED folded base
//! unit deploy/staged/galion-backup.service must be behaviour-neutral against
//! the live as-built set (base + 90-deploy-t273u.conf + offsite.conf), every
//! intermediate install order must stay single-ExecStart, the post-fold
//! rollback must land on the bash script, and the 12-clean-run gate parser
//! must read the real t273u journal correctly.

mod units;

use std::collections::BTreeMap;
use units::gate::{gate, in_timer_window, runs, utc_hm, Run, GATE_RUNS};
use units::{effective_list, parse};

const BASE: &str = include_str!("../deploy/galion-backup.service");
const DROPIN: &str = include_str!("../deploy/galion-backup.service.d/90-deploy-t273u.conf");
const FOLDED: &str = include_str!("../deploy/staged/galion-backup.service");
const OFFSITE: &str = "[Service]\n\
ExecStartPost=/opt/galion/offsite/galion-offsite-encrypt.sh\n\
ExecStopPost=/opt/galion/offsite/galion-offsite-encrypt.sh --perms-only\n";
const JOURNAL: &str = include_str!("fixtures/journal-t273u-20261027.txt");

const RUST_EXEC: &str = "/opt/galion/rust/bin/galion-backup-98466ab run";
const SH_EXEC: &str = "/opt/galion/rust/bin/galion-backup.sh";
const TAG: &str = "galion-backup-98466ab[";

/// Every (section, key) -> effective value list after base + drop-ins.
fn resolved(files: &[&str]) -> BTreeMap<(String, String), Vec<String>> {
    let mut keys = Vec::new();
    for f in files {
        for (s, k, _) in parse(f) {
            if !keys.contains(&(s.clone(), k.clone())) {
                keys.push((s, k));
            }
        }
    }
    keys.into_iter()
        .map(|(s, k)| {
            let v = effective_list(files, &s, &k);
            ((s, k), v)
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

#[test]
fn folded_is_behaviour_neutral_vs_live_set() {
    let live = resolved(&[BASE, DROPIN, OFFSITE]);
    let folded = resolved(&[FOLDED, OFFSITE]);
    assert_eq!(folded, live);
    assert_eq!(
        folded[&("Service".into(), "ExecStart".into())],
        vec![RUST_EXEC]
    );
}

#[test]
fn folded_differs_from_base_only_in_execstart() {
    let b: Vec<_> = parse(BASE)
        .into_iter()
        .filter(|(_, k, _)| k != "ExecStart")
        .collect();
    let f: Vec<_> = parse(FOLDED)
        .into_iter()
        .filter(|(_, k, _)| k != "ExecStart")
        .collect();
    assert_eq!(b, f, "same keys, same order, same values");
    let ex: Vec<_> = parse(FOLDED)
        .into_iter()
        .filter(|(_, k, _)| k == "ExecStart")
        .map(|(_, _, v)| v)
        .collect();
    assert_eq!(ex, vec![RUST_EXEC], "exactly one non-empty ExecStart");
    assert!(
        !parse(FOLDED).iter().any(|(_, _, v)| v.contains(".sh")),
        "folded unit must not name the bash script"
    );
}

#[test]
fn every_install_order_keeps_one_rust_or_sh_execstart() {
    // Order A (recommended): write folded base, then rm drop-in.
    // Intermediate = folded base + old drop-in still present.
    let a_mid = effective_list(&[FOLDED, DROPIN, OFFSITE], "Service", "ExecStart");
    assert_eq!(a_mid, vec![RUST_EXEC], "drop-in reset makes it idempotent");
    // Order B: rm drop-in first -> the as-built base runs the .sh (= today's
    // rollback state, safe but one window on bash).
    let b_mid = effective_list(&[BASE, OFFSITE], "Service", "ExecStart");
    assert_eq!(b_mid, vec![SH_EXEC]);
    // End state either way.
    let end = effective_list(&[FOLDED, OFFSITE], "Service", "ExecStart");
    assert_eq!(end, vec![RUST_EXEC]);
}

#[test]
fn post_fold_rollback_is_reinstalling_as_built_base() {
    let rb = effective_list(&[BASE, OFFSITE], "Service", "ExecStart");
    assert_eq!(rb, vec![SH_EXEC]);
}

#[test]
fn folded_header() {
    assert!(FOLDED.starts_with("# SPDX-License-Identifier: MIT\n"));
    assert!(FOLDED.contains("Copyright (c) 3000 Galion / galion-studio"));
    assert!(FOLDED.contains("NOT INSTALLED"));
}

#[test]
fn systemd_analyze_verify_folded() {
    // Real systemd parser (user mode, no root): verify the staged unit with
    // the ELF path swapped for /bin/true (the ELF only exists on the VPS).
    // Negative control: drop Type=oneshot and add a 2nd ExecStart; systemd
    // must reject it, which proves verify really parsed the file.
    let have = std::process::Command::new("systemd-analyze")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !have || !std::path::Path::new("/bin/true").exists() {
        eprintln!("skipped (no systemd-analyze)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("galion-fold-311-{}", std::process::id()));
    let rt = dir.join("rt");
    std::fs::create_dir_all(&rt).unwrap();
    let verify = |name: &str, body: &str| -> (bool, String) {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        let o = std::process::Command::new("systemd-analyze")
            .args(["--user", "verify"])
            .arg(&p)
            .env("XDG_RUNTIME_DIR", &rt)
            .output()
            .unwrap();
        (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    };
    let good = FOLDED.replace("/opt/galion/rust/bin/galion-backup-98466ab", "/bin/true");
    let (ok, err) = verify("galion-backup.service", &good);
    let bad = good.replace("Type=oneshot\n", "").replace(
        "ExecStart=/bin/true run\n",
        "ExecStart=/bin/true run\nExecStart=/bin/true x\n",
    );
    let (bad_ok, bad_err) = verify("galion-backup-bad.service", &bad);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "systemd-analyze verify folded: {err}");
    assert!(!bad_ok, "negative control must fail: {bad_err}");
    assert!(bad_err.contains("more than one ExecStart"), "{bad_err}");
}

#[test]
fn gate_reads_real_t273u_journal() {
    let rs = runs(JOURNAL, TAG);
    assert_eq!(rs.len(), 2, "{rs:#?}");
    // 17:20 UTC = 18:20 CET t273u deploy smoke (manual start).
    assert!(!rs[0].timer && rs[0].clean(), "{:?}", rs[0]);
    // 18:18:58 UTC = 19:18:58 CET first timer tick (18:17 + random delay).
    assert!(rs[1].timer && rs[1].clean(), "{:?}", rs[1]);
    let g = gate(&rs);
    assert_eq!(
        (g.timer_runs, g.manual_runs, g.unclean_timer_runs, g.streak),
        (1, 1, 0, 1)
    );
    assert!(!g.open());
}

fn synth(start: &str, ok: bool, fail: bool) -> String {
    let mut s = format!("{start} ip50 systemd[1]: Starting galion-backup.service - Galion...\n");
    if ok {
        s += &format!("{start} ip50 galion-backup-98466ab[1]: ts=x svc=galion-backup level=info msg=\"ok\" archive=a size_kb=1 kept=12\n");
    } else {
        s += &format!("{start} ip50 galion-backup-98466ab[1]: ts=x svc=galion-backup level=error msg=\"db verify failed\"\n");
    }
    if fail {
        s += &format!("{start} ip50 systemd[1]: galion-backup.service: Main process exited, code=exited, status=1/FAILURE\n");
        s += &format!(
            "{start} ip50 systemd[1]: galion-backup.service: Failed with result 'exit-code'.\n"
        );
        s += &format!(
            "{start} ip50 systemd[1]: Failed to start galion-backup.service - Galion...\n"
        );
    } else {
        s +=
            &format!("{start} ip50 systemd[1]: galion-backup.service: Deactivated successfully.\n");
        s += &format!("{start} ip50 systemd[1]: Finished galion-backup.service - Galion...\n");
    }
    s += &format!("{start} ip50 systemd[1]: galion-backup.service: Consumed 3.1s CPU time.\n");
    s
}

fn ticks(n: usize) -> Vec<String> {
    // Timer ticks from 2026-10-28 00:19 UTC, 4 per day.
    (0..n)
        .map(|i| {
            let day = 28 + i / 4;
            let h = (i % 4) * 6;
            format!("2026-10-{day:02}T{h:02}:19:30+00:00")
        })
        .collect()
}

#[test]
fn gate_opens_at_twelve_consecutive_clean_timer_runs() {
    let mut j = String::from(JOURNAL);
    for t in ticks(GATE_RUNS - 2) {
        j += &synth(&t, true, false);
    }
    let g = gate(&runs(&j, TAG));
    assert_eq!(g.streak, GATE_RUNS - 1);
    assert!(!g.open());
    j += &synth("2026-10-30T12:20:00+00:00", true, false);
    let g = gate(&runs(&j, TAG));
    assert_eq!(g.streak, GATE_RUNS);
    assert!(g.open(), "{g:?}");
}

#[test]
fn gate_resets_on_failed_or_error_run_and_ignores_manual() {
    let t = ticks(16);
    let mut j = String::new();
    for x in &t[..6] {
        j += &synth(x, true, false);
    }
    j += &synth(&t[6], false, true); // failed timer run
    for x in &t[7..15] {
        j += &synth(x, true, false);
    }
    j += &synth("2026-10-30T09:41:00+00:00", true, false); // manual start
    let g = gate(&runs(&j, TAG));
    assert_eq!(
        (g.timer_runs, g.manual_runs, g.unclean_timer_runs, g.streak),
        (15, 1, 1, 8)
    );
    assert!(!g.open());
    // error line but systemd still "Finished" (exit 0) is unclean too.
    let mut j2 = String::new();
    for x in &t[..12] {
        j2 += &synth(x, true, false);
    }
    j2 += &synth(&t[12], false, false);
    let g2 = gate(&runs(&j2, TAG));
    assert_eq!(g2.streak, 0);
    assert_eq!(g2.unclean_timer_runs, 1);
}

#[test]
fn unfinished_run_is_unclean_and_trailing_run_not_counted() {
    let t = ticks(3);
    let mut j = synth(&t[0], true, false);
    j += &format!(
        "{} ip50 systemd[1]: Starting galion-backup.service - G...\n",
        t[1]
    );
    j += &synth(&t[2], true, false);
    j += &format!(
        "{} ip50 systemd[1]: Starting galion-backup.service - G...\n",
        "2026-10-29T00:18:00+00:00"
    );
    let rs = runs(&j, TAG);
    assert_eq!(rs.len(), 3);
    assert!(!rs[1].finished && !rs[1].clean());
    assert_eq!(gate(&rs).streak, 1);
}

#[test]
fn timer_window() {
    assert_eq!(utc_hm("2026-10-27T18:18:58+00:00"), Some((18, 18)));
    assert!(in_timer_window("2026-10-27T18:18:58+00:00"));
    assert!(in_timer_window("2026-10-28T00:17:00+00:00"));
    assert!(in_timer_window("2026-10-28T06:23:59+00:00"));
    assert!(!in_timer_window("2026-10-28T06:24:00+00:00"));
    assert!(!in_timer_window("2026-10-27T17:20:06+00:00"));
    assert!(!in_timer_window("2026-10-28T03:17:00+00:00"));
    // Non-UTC offset: never assume, never count.
    assert!(!in_timer_window("2026-10-28T01:18:00+01:00"));
    let r = Run {
        start: String::new(),
        finished: true,
        failed: false,
        rust_ok: false,
        bad_lines: 0,
        timer: true,
    };
    assert!(!r.clean(), "no Rust ok line = not clean");
}
