// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life runs (gated: GALION_REAL=1).
//!
//! * box: the built binary scans the real `/` of the build box (Phase A files
//!   are absent there, so the verdict must be FAIL with 5 missing and 0 active).
//! * VPS (also needs GALION_PYARCHIVE_SSH=user@host and GALION_PYARCHIVE_BIN=
//!   path to a static x86_64 musl release build): copy the binary to a fresh
//!   /tmp/pyarchive-345-<ts>/ (tmpfiles-cleaned), run `plan --json` read-only
//!   against `/`, and assert PASS with 5 files and 0 active references, then
//!   `stat` the Phase A files again to prove nothing moved. Nothing is deleted.

use std::process::Command;

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

#[test]
fn real_box_root_scan() {
    if !real() {
        return;
    }
    let o = Command::new(env!("CARGO_BIN_EXE_galion-pyarchive"))
        .args(["plan", "--json"])
        .output()
        .unwrap();
    let j: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    eprintln!(
        "box: verdict {} missing {} units {} cron {} procs {} active {}",
        j["verdict"],
        j["missing"].as_array().unwrap().len(),
        j["scanned"]["units"],
        j["scanned"]["cron_files"],
        j["scanned"]["processes"],
        j["counts"]["active"]
    );
    for r in j["refs"].as_array().unwrap() {
        eprintln!(
            "box ref: {} {} {} -> {}",
            r["class"], r["kind"], r["source"], r["target"]
        );
    }
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(j["missing"].as_array().unwrap().len(), 5);
    // The box is shared by ~20 agents whose ssh probes may name Phase A paths
    // on their command line (VPS paths, not box files), so only unit/cron
    // references must be 0 here; process hits are printed, not asserted.
    let unit_cron_active = j["refs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["class"] == "active" && r["kind"] != "process")
        .count();
    assert_eq!(unit_cron_active, 0);
}

fn ssh(target: &str, cmd: &str) -> std::process::Output {
    let key = format!("{}/.ssh/id_ed25519_vps", std::env::var("HOME").unwrap());
    Command::new("ssh")
        .args([
            "-i",
            &key,
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "ConnectTimeout=20",
            target,
            cmd,
        ])
        .output()
        .unwrap()
}

#[test]
fn real_vps_plan_read_only() {
    if !real() {
        return;
    }
    let (Ok(target), Ok(bin)) = (
        std::env::var("GALION_PYARCHIVE_SSH"),
        std::env::var("GALION_PYARCHIVE_BIN"),
    ) else {
        eprintln!("skip VPS part: GALION_PYARCHIVE_SSH / GALION_PYARCHIVE_BIN unset");
        return;
    };
    let stat = "stat -c '%n %s %Y %i' /opt/galion/bin/desk-hermes.py /opt/galion/bin/galion-desk \
                /opt/galion/bin/pw.py /opt/galion/bin/__pycache__/pw.cpython-312.pyc \
                /opt/galion/bin/replicate-picture.py";
    let before = ssh(&target, stat);
    assert!(before.status.success());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dir = format!("/tmp/pyarchive-345-{ts}");
    assert!(ssh(&target, &format!("mkdir -m 0700 {dir}"))
        .status
        .success());
    let key = format!("{}/.ssh/id_ed25519_vps", std::env::var("HOME").unwrap());
    let cp = Command::new("scp")
        .args(["-q", "-i", &key, "-o", "IdentitiesOnly=yes", &bin])
        .arg(format!("{target}:{dir}/galion-pyarchive"))
        .status()
        .unwrap();
    assert!(cp.success());
    let o = ssh(
        &target,
        &format!("nice -n 10 {dir}/galion-pyarchive plan --json --out {dir}/plan"),
    );
    let j: serde_json::Value = serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&o.stderr)));
    eprintln!(
        "vps: verdict {} files {} bytes {} units {} cron {} procs {} active {} overridden {} masked {} shadowed {} comment {} unreadable {}",
        j["verdict"],
        j["files"].as_array().unwrap().len(),
        j["total_bytes"],
        j["scanned"]["units"],
        j["scanned"]["cron_files"],
        j["scanned"]["processes"],
        j["counts"]["active"],
        j["counts"]["overridden"],
        j["counts"]["masked"],
        j["counts"]["shadowed"],
        j["counts"]["comment"],
        j["unreadable"].as_array().unwrap().len()
    );
    let v = ssh(
        &target,
        &format!("{dir}/galion-pyarchive verify --manifest {dir}/plan/phase-a.sha256"),
    );
    eprintln!("vps verify exit {:?}", v.status.code());
    let after = ssh(&target, stat);
    assert_eq!(before.stdout, after.stdout, "Phase A files changed");
    assert_eq!(o.status.code(), Some(0), "plan verdict not PASS");
    assert_eq!(v.status.code(), Some(0), "verify not PASS");
    assert_eq!(j["files"].as_array().unwrap().len(), 5);
    assert_eq!(j["counts"]["active"], 0);
    // slot 345: the overridden base galion-desk ExecStart is flagged with an
    // exact safe rollback (restore archive first, then the drop-in).
    let ob = j["overridden_base"].as_array().unwrap();
    for o in ob {
        eprintln!(
            "vps overridden-base: {} {}:{} {}= -> {} by {}:{} notes {}",
            o["unit"],
            o["base_file"],
            o["base_line"],
            o["key"],
            o["target"],
            o["by_file"],
            o["by_line"],
            o["stale_rollback_notes"]
        );
        for s in o["safe_rollback"].as_array().unwrap() {
            eprintln!("  {}", s.as_str().unwrap());
        }
    }
    assert_eq!(ob.len(), 1);
    assert_eq!(ob[0]["unit"], "galion-desk.service");
    assert_eq!(ob[0]["target"], "/opt/galion/bin/galion-desk");
    assert_eq!(
        ob[0]["by_file"],
        "/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf"
    );
    assert!(!ob[0]["stale_rollback_notes"].as_array().unwrap().is_empty());
}
