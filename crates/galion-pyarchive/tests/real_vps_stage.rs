// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life VPS check of the two staged owner options for galion-desk
//! (wave-4 slot 372; D-345-1 fold the Rust ExecStart into the base unit,
//! D-345-2 reword the drop-in rollback note). Gated: GALION_REAL=1 plus
//! GALION_PYARCHIVE_SSH=user@host and GALION_PYARCHIVE_BIN=<static x86_64
//! musl release build>.
//!
//! Read-only for the live system: everything happens in a fresh
//! `/var/tmp/unitstage-372-<ts>/` (mode 0700). Nothing under /etc is written,
//! no daemon-reload, no restart. Steps:
//! 1. `systemd-analyze verify` (offline unit loader, does not touch PID 1's
//!    state) on renamed copies: live base + live drop-in, folded base alone,
//!    live base + reworded drop-in, and two negative controls (a missing
//!    ExecStart in a base unit and in a drop-in) that must FAIL, which proves
//!    verify really reads both the file and drop-ins next to it.
//! 2. `galion-pyarchive plan --root <copy>` on a copy of the live systemd and
//!    cron trees + the 5 Phase A files, in three states (live, folded,
//!    reworded), asserting the overridden-base / stale-note results.
//! 3. `systemctl show` of the live unit before/after + sha256 of the live unit
//!    files before/after must be identical. The throwaway folder is removed.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const STAGED_BASE: &str = include_str!("../../../deploy/staged/galion-desk/galion-desk.service");
const STAGED_DROPIN: &str =
    include_str!("../../../deploy/staged/galion-desk/rust-desk-hermes.conf");

fn installed(s: &str) -> String {
    s.lines()
        .filter(|l| !l.starts_with("# SPDX-") && !l.starts_with("# STAGED"))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn ssh_in(target: &str, cmd: &str, stdin: &str) -> Output {
    let key = format!("{}/.ssh/id_ed25519_vps", std::env::var("HOME").unwrap());
    let mut c = Command::new("ssh")
        .args([
            "-i",
            &key,
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=20",
            target,
            cmd,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    c.wait_with_output().unwrap()
}

fn ssh(target: &str, cmd: &str) -> Output {
    ssh_in(target, cmd, "")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr)
}

const LIVE_PROBE: &str = "systemctl show -p ExecStart -p DropInPaths -p Environment -p MainPID \
     -p NRestarts -p ActiveState galion-desk.service; sha256sum /etc/systemd/system/galion-desk.service \
     /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf";

#[test]
fn real_vps_staged_unit_options() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        return;
    }
    let (Ok(t), Ok(bin)) = (
        std::env::var("GALION_PYARCHIVE_SSH"),
        std::env::var("GALION_PYARCHIVE_BIN"),
    ) else {
        eprintln!("skip: GALION_PYARCHIVE_SSH / GALION_PYARCHIVE_BIN unset");
        return;
    };
    let before = ssh(&t, LIVE_PROBE);
    assert!(before.status.success(), "{}", out(&before));
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let d = format!("/var/tmp/unitstage-372-{ts}");
    let mk = ssh(&t, &format!("mkdir -m 0700 {d} {d}/u {d}/tree"));
    assert!(mk.status.success(), "{}", out(&mk));

    // ---- 1. systemd-analyze verify on renamed copies ----
    let u = format!("{d}/u");
    let setup = format!(
        "set -e; cd {u}; \
         cp /etc/systemd/system/galion-desk.service gd372live.service; \
         mkdir gd372live.service.d; cp /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf gd372live.service.d/; \
         cp /etc/systemd/system/galion-desk.service gd372reword.service; mkdir gd372reword.service.d; \
         cp /etc/systemd/system/galion-desk.service gd372baddrop.service; mkdir gd372baddrop.service.d; \
         printf '[Service]\\nExecStart=\\nExecStart=/opt/galion/rust/bin/galion-desk-missing-372\\n' > gd372baddrop.service.d/x.conf; \
         cat > gd372fold.service"
    );
    let s = ssh_in(&t, &setup, &installed(STAGED_BASE));
    assert!(s.status.success(), "{}", out(&s));
    let s = ssh_in(
        &t,
        &format!("cat > {u}/gd372reword.service.d/rust-desk-hermes.conf"),
        &installed(STAGED_DROPIN),
    );
    assert!(s.status.success(), "{}", out(&s));
    let s = ssh_in(
        &t,
        &format!("sed 's#/opt/galion/rust/bin/galion-desk#/opt/galion/bin/galion-desk-missing-372#' {u}/gd372fold.service > {u}/gd372badbase.service"),
        "",
    );
    assert!(s.status.success(), "{}", out(&s));
    let verify = |name: &str| -> (i32, String) {
        let o = ssh(
            &t,
            &format!("cd {u} && systemd-analyze verify --man=no ./{name} 2>&1"),
        );
        let txt = out(&o).replace(&u, "<dir>");
        eprintln!("verify {name}: exit {:?}\n{txt}", o.status.code());
        (o.status.code().unwrap_or(-1), txt)
    };
    let (live_c, _) = verify("gd372live.service");
    let (fold_c, fold_t) = verify("gd372fold.service");
    let (reword_c, _) = verify("gd372reword.service");
    let (bb_c, bb_t) = verify("gd372badbase.service");
    let (bd_c, bd_t) = verify("gd372baddrop.service");
    assert_eq!(live_c, 0, "live copy must verify");
    assert_eq!(fold_c, 0, "folded base must verify: {fold_t}");
    assert_eq!(reword_c, 0, "reworded drop-in must verify");
    assert_ne!(bb_c, 0, "negative control base must fail: {bb_t}");
    assert!(bb_t.contains("galion-desk-missing-372"), "{bb_t}");
    assert_ne!(bd_c, 0, "negative control drop-in must fail: {bd_t}");
    assert!(bd_t.contains("galion-desk-missing-372"), "{bd_t}");

    // ---- 2. galion-pyarchive plan on a copy of the live trees ----
    let tree = format!("{d}/tree");
    let key = format!("{}/.ssh/id_ed25519_vps", std::env::var("HOME").unwrap());
    let cp = Command::new("scp")
        .args(["-q", "-i", &key, "-o", "IdentitiesOnly=yes", &bin])
        .arg(format!("{t}:{d}/galion-pyarchive"))
        .status()
        .unwrap();
    assert!(cp.success());
    let copy = format!(
        "set -e; cd /; for p in etc/systemd/system etc/systemd/user run/systemd/system usr/local/lib/systemd \
         usr/lib/systemd/system usr/lib/systemd/user etc/crontab etc/anacrontab etc/cron.d etc/cron.hourly \
         etc/cron.daily etc/cron.weekly etc/cron.monthly var/spool/cron etc/hostname \
         opt/galion/bin/desk-hermes.py opt/galion/bin/galion-desk opt/galion/bin/pw.py \
         opt/galion/bin/__pycache__/pw.cpython-312.pyc opt/galion/bin/replicate-picture.py; do \
         [ -e \"$p\" ] || [ -L \"$p\" ] || continue; cp -a --parents \"$p\" {tree}/; done; \
         [ -L lib ] && ln -s usr/lib {tree}/lib || true; du -sh {tree} | cut -f1"
    );
    let c = ssh(&t, &copy);
    assert!(c.status.success(), "{}", out(&c));
    eprintln!("tree copy size {}", out(&c).trim());
    let plan = |tag: &str| -> serde_json::Value {
        let o = ssh(
            &t,
            &format!("nice -n 10 {d}/galion-pyarchive plan --json --no-procs --root {tree}"),
        );
        let j: serde_json::Value = serde_json::from_slice(&o.stdout)
            .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)));
        let ob = j["overridden_base"].as_array().unwrap();
        eprintln!(
            "plan {tag}: exit {:?} verdict {} files {} units {} active {} overridden_base {} stale_notes {}",
            o.status.code(),
            j["verdict"],
            j["files"].as_array().unwrap().len(),
            j["scanned"]["units"],
            j["counts"]["active"],
            ob.len(),
            ob.iter()
                .map(|o| o["stale_rollback_notes"].as_array().unwrap().len())
                .sum::<usize>()
        );
        j
    };
    let unit = format!("{tree}/etc/systemd/system/galion-desk.service");
    let drop = format!("{tree}/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf");
    let live = plan("live");
    assert_eq!(live["verdict"], "PASS");
    assert_eq!(live["counts"]["active"], 0);
    let ob = live["overridden_base"].as_array().unwrap();
    assert_eq!(ob.len(), 1);
    assert_eq!(ob[0]["stale_rollback_notes"].as_array().unwrap().len(), 1);

    let s = ssh_in(&t, &format!("cat > {drop}"), &installed(STAGED_DROPIN));
    assert!(s.status.success());
    let rw = plan("reword D-345-2");
    assert_eq!(rw["verdict"], "PASS");
    let ob = rw["overridden_base"].as_array().unwrap();
    assert_eq!(ob.len(), 1, "base still names the old ELF");
    assert!(ob[0]["stale_rollback_notes"].as_array().unwrap().is_empty());

    let s = ssh_in(
        &t,
        &format!("cat > {unit} && mv {drop} {d}/u/retired-dropin.conf && rmdir {tree}/etc/systemd/system/galion-desk.service.d"),
        &installed(STAGED_BASE),
    );
    assert!(s.status.success(), "{}", out(&s));
    let fold = plan("fold D-345-1");
    assert_eq!(fold["verdict"], "PASS");
    assert_eq!(fold["counts"]["active"], 0);
    assert!(fold["overridden_base"].as_array().unwrap().is_empty());

    // ---- 3. live untouched, clean up our own folder only ----
    let after = ssh(&t, LIVE_PROBE);
    eprintln!("live before/after:\n{}", out(&after));
    assert_eq!(before.stdout, after.stdout, "live galion-desk changed");
    assert!(d.starts_with("/var/tmp/unitstage-372-"));
    let rm = ssh(&t, &format!("rm -rf -- {d} && ! test -e {d}"));
    assert!(rm.status.success(), "{}", out(&rm));
}
