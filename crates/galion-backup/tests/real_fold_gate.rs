// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 311 real test (GALION_REAL=1 + GALION_VPS_SSH=<ssh host alias>):
//! READ-ONLY. (1) The staged folded base, combined with the LIVE offsite.conf,
//! must resolve to exactly the LIVE effective unit (base + 90-deploy-t273u +
//! offsite). (2) The 12-clean-timer-run gate from the live journal since the
//! t273u deploy. (3) Who still references the retirement targets
//! (galion-backup.sh, the pre-t273u helper ELF). Fixed read commands only:
//! `systemctl cat`, `systemctl show -p`, `journalctl`, `grep -l`.
//! A closed gate is reported, not a failure; drift or an unclean timer run fails.
//!
//! `GALION_REAL=1 GALION_VPS_SSH=slavik-vps cargo test --release -p galion-backup --test real_fold_gate -- --nocapture`

mod units;

use std::process::Command;
use units::gate::{gate, runs, GATE_RUNS};
use units::{effective_list, parse, split_cat};

const FOLDED: &str = include_str!("../deploy/staged/galion-backup.service");
const RUST_EXEC: &str = "/opt/galion/rust/bin/galion-backup-98466ab run";
/// t273u deploy (2026-10-27 18:20 CET +01:00) in the VPS clock (Etc/UTC).
const SINCE_UTC: &str = "2026-10-27 17:00:00";

fn say(m: &str) {
    println!("[{}] {m}", galion_backup::backup::now_iso());
}

fn ssh(host: &str, cmd: &str) -> String {
    let o = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, cmd])
        .output()
        .expect("ssh");
    assert!(
        o.status.success(),
        "ssh `{cmd}` failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn real_fold_neutral_and_gate() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let Ok(host) = std::env::var("GALION_VPS_SSH") else {
        eprintln!("skipped (set GALION_VPS_SSH=<ssh host alias>)");
        return;
    };
    let mut fails = 0;
    let mut check = |what: &str, ok: bool, detail: String| {
        say(&format!(
            "{} {what}{}",
            if ok { "PASS" } else { "FAIL" },
            if ok {
                String::new()
            } else {
                format!(": {detail}")
            }
        ));
        if !ok {
            fails += 1;
        }
    };

    // (1) neutrality against the live files.
    let cat = split_cat(&ssh(&host, "systemctl cat galion-backup.service"));
    let paths: Vec<&str> = cat.iter().map(|(p, _)| p.as_str()).collect();
    say(&format!("live unit files: {paths:?}"));
    let live: Vec<&str> = cat.iter().map(|(_, b)| b.as_str()).collect();
    let offsite: Vec<&str> = cat
        .iter()
        .filter(|(p, _)| p.ends_with("/offsite.conf"))
        .map(|(_, b)| b.as_str())
        .collect();
    check(
        "live still has the t273u drop-in (fold not yet done)",
        paths.iter().any(|p| p.ends_with("/90-deploy-t273u.conf")),
        format!("{paths:?}"),
    );
    check(
        "live offsite.conf present",
        offsite.len() == 1,
        String::new(),
    );
    let mut folded = vec![FOLDED];
    folded.extend(offsite.iter().copied());
    let mut keys: Vec<(String, String)> = Vec::new();
    for f in live.iter().chain(folded.iter()) {
        for (s, k, _) in parse(f) {
            if !keys.contains(&(s.clone(), k.clone())) {
                keys.push((s, k));
            }
        }
    }
    let mut diff = Vec::new();
    for (s, k) in &keys {
        let a = effective_list(&live, s, k);
        let b = effective_list(&folded, s, k);
        if a != b {
            diff.push(format!("{s}.{k}: live={a:?} folded={b:?}"));
        }
    }
    say(&format!("compared {} effective keys", keys.len()));
    check(
        "folded + live offsite == live effective unit (all keys)",
        diff.is_empty(),
        diff.join("; "),
    );
    let show = ssh(&host, "systemctl show galion-backup.service -p ExecStart");
    check(
        "systemd's own ExecStart = folded ExecStart",
        show.contains("argv[]=/opt/galion/rust/bin/galion-backup-98466ab run ;")
            && effective_list(&folded, "Service", "ExecStart") == vec![RUST_EXEC],
        show.trim().to_string(),
    );

    // (2) the gate.
    let j = ssh(
        &host,
        &format!(
            "journalctl -u galion-backup.service --since '{SINCE_UTC}' --no-pager -o short-iso"
        ),
    );
    let rs = runs(&j, "galion-backup-98466ab[");
    for r in &rs {
        say(&format!(
            "run start={} kind={} clean={} (finished={} failed={} rust_ok={} bad_lines={})",
            r.start,
            if r.timer { "timer" } else { "manual" },
            r.clean(),
            r.finished,
            r.failed,
            r.rust_ok,
            r.bad_lines
        ));
    }
    let g = gate(&rs);
    say(&format!(
        "GATE {}: streak {}/{GATE_RUNS} clean timer runs (timer={} manual={} unclean_timer={})",
        if g.open() { "OPEN" } else { "CLOSED" },
        g.streak,
        g.timer_runs,
        g.manual_runs,
        g.unclean_timer_runs
    ));
    check(
        "no unclean timer run since t273u",
        g.unclean_timer_runs == 0,
        format!("{g:?}"),
    );
    check("at least one clean timer run", g.streak >= 1, String::new());
    let next = ssh(
        &host,
        "systemctl show galion-backup.timer -p NextElapseUSecRealtime -p LastTriggerUSec",
    );
    say(&format!("timer: {}", next.trim().replace('\n', " | ")));

    // (3) retirement blockers: who still names the .sh or the helper ELF.
    let refs_sh = ssh(
        &host,
        "grep -rIl --exclude-dir=.git -e galion-backup.sh /etc/systemd/system /etc/cron.d /opt/galion/rust/bin /opt/galion/offsite /usr/local/bin 2>/dev/null || true",
    );
    let refs_elf = ssh(
        &host,
        "grep -rIlE --exclude-dir=.git -e '/opt/galion/rust/bin/galion-backup( |$)' -e 'galion-backup verify' /etc/systemd/system /etc/cron.d /opt/galion/rust/bin /opt/galion/offsite /usr/local/bin 2>/dev/null || true",
    );
    say(&format!(
        "refs to galion-backup.sh: {:?}",
        refs_sh.lines().collect::<Vec<_>>()
    ));
    say(&format!(
        "refs to helper ELF galion-backup (verify): {:?}",
        refs_elf.lines().collect::<Vec<_>>()
    ));

    say(&format!("RESULT: {fails} failed"));
    assert_eq!(fails, 0);
}
