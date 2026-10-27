// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 269 real test (GALION_REAL=1 + GALION_VPS_SSH=<ssh host alias>):
//! READ-ONLY drift check of the live galion-backup unit set against the repo
//! mirror in deploy/. Runs only fixed read commands over ssh
//! (`systemctl cat`, `systemctl show -p`, `sha256sum`, `journalctl`); it never
//! starts, stops, reloads or writes anything on the VPS.
//!
//! `GALION_REAL=1 GALION_VPS_SSH=slavik-vps cargo test --release -p galion-backup --test real_unit_live -- --nocapture`

mod units;

use std::process::Command;
use units::{effective, effective_list, parse, split_cat};

const BASE: &str = include_str!("../deploy/galion-backup.service");
const DROPIN: &str = include_str!("../deploy/galion-backup.service.d/90-deploy-t273u.conf");
const TIMER: &str = include_str!("../deploy/galion-backup.timer");
const RUST_EXEC: &str = "/opt/galion/rust/bin/galion-backup-98466ab run";
const LIVE_BIN: &str = "/opt/galion/rust/bin/galion-backup-98466ab";
const LIVE_SHA: &str = "fb26d673cfe6c615bf7272c81b0b076d92f4c05249bc5b8d40440b18fc1a2163";
const SH_SHA: &str = "0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4";

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

fn body(cat: &[(String, String)], path: &str) -> String {
    cat.iter()
        .find(|(p, _)| p == path)
        .map(|(_, b)| b.clone())
        .unwrap_or_else(|| panic!("live `systemctl cat` has no {path}"))
}

#[test]
fn real_live_unit_matches_repo() {
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

    let cat = split_cat(&ssh(
        &host,
        "systemctl cat galion-backup.service galion-backup.timer",
    ));
    let paths: Vec<&str> = cat.iter().map(|(p, _)| p.as_str()).collect();
    say(&format!("live unit files: {paths:?}"));
    let want = [
        "/etc/systemd/system/galion-backup.service",
        "/etc/systemd/system/galion-backup.service.d/90-deploy-t273u.conf",
        "/etc/systemd/system/galion-backup.service.d/offsite.conf",
        "/etc/systemd/system/galion-backup.timer",
    ];
    check(
        "live file set = base + 90-deploy-t273u + offsite + timer",
        paths == want,
        format!("{paths:?}"),
    );

    let lb = body(&cat, want[0]);
    let ld = body(&cat, want[1]);
    let lo = body(&cat, want[2]);
    let lt = body(&cat, want[3]);
    check(
        "base unit settings = repo mirror",
        parse(&lb) == parse(BASE),
        format!("{:?}", parse(&lb)),
    );
    check(
        "drop-in settings = repo mirror",
        parse(&ld) == parse(DROPIN),
        format!("{:?}", parse(&ld)),
    );
    check(
        "timer settings = repo mirror",
        parse(&lt) == parse(TIMER),
        format!("{:?}", parse(&lt)),
    );
    let files = [lb.as_str(), ld.as_str(), lo.as_str()];
    let ex = effective_list(&files, "Service", "ExecStart");
    check(
        "effective ExecStart = Rust ELF run (single)",
        ex == vec![RUST_EXEC],
        format!("{ex:?}"),
    );
    check(
        "offsite hooks still present",
        effective_list(&files, "Service", "ExecStartPost").len() == 1
            && effective_list(&files, "Service", "ExecStopPost").len() == 1,
        String::new(),
    );
    check(
        "UMask 0077",
        effective(&files, "Service", "UMask").as_deref() == Some("0077"),
        String::new(),
    );

    // systemd's own view of what it will exec (merged by systemd, not us).
    let show = ssh(&host, "systemctl show galion-backup.service -p ExecStart -p Result -p ExecMainStatus -p Type -p FragmentPath -p DropInPaths");
    let sd_exec = show
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .unwrap_or("")
        .to_string();
    check(
        "systemd ExecStart path = Rust ELF, argv = [.., run]",
        sd_exec.contains(&format!("path={LIVE_BIN} ;"))
            && sd_exec.contains(&format!("argv[]={LIVE_BIN} run ;")),
        sd_exec.clone(),
    );
    check(
        "Type=oneshot",
        show.lines().any(|l| l == "Type=oneshot"),
        show.clone(),
    );
    check(
        "last Result=success",
        show.lines().any(|l| l == "Result=success"),
        show.clone(),
    );
    check(
        "last ExecMainStatus=0",
        show.lines().any(|l| l == "ExecMainStatus=0"),
        show.clone(),
    );

    let timer = ssh(
        &host,
        "systemctl show galion-backup.timer -p ActiveState -p UnitFileState",
    );
    check(
        "timer active + enabled",
        timer.lines().any(|l| l == "ActiveState=active")
            && timer.lines().any(|l| l == "UnitFileState=enabled"),
        timer.clone(),
    );

    let sums = ssh(&host, "sha256sum /opt/galion/rust/bin/galion-backup-98466ab /opt/galion/rust/bin/galion-backup.sh");
    check(
        "live ELF sha256 = recorded t273u artifact",
        sums.contains(&format!("{LIVE_SHA}  {LIVE_BIN}")),
        sums.clone(),
    );
    check(
        "rollback script still present, sha = parity oracle",
        sums.contains(SH_SHA),
        sums.clone(),
    );

    let j = ssh(
        &host,
        "journalctl -u galion-backup.service -n 200 --no-pager -o short-iso",
    );
    let last_rust_ok = j
        .lines()
        .rfind(|l| l.contains("galion-backup-98466ab[") && l.contains("msg=\"ok\""))
        .map(|l| l.to_string());
    say(&format!(
        "last Rust run line: {}",
        last_rust_ok.as_deref().unwrap_or("<none>")
    ));
    check(
        "journal has a Rust `ok` run",
        last_rust_ok.is_some(),
        String::new(),
    );
    let n_rust = j
        .lines()
        .filter(|l| l.contains("galion-backup-98466ab[") && l.contains("msg=\"ok\""))
        .count();
    let n_err = j
        .lines()
        .filter(|l| l.contains("galion-backup-98466ab[") && l.contains("level=error"))
        .count();
    say(&format!(
        "journal tail: rust ok runs={n_rust} rust error lines={n_err}"
    ));
    check("no Rust error lines", n_err == 0, String::new());

    say(&format!("RESULT: {fails} failed"));
    assert_eq!(fails, 0);
}
