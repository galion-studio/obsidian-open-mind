// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Slot 304 (D-269-2): the `size_kb=` log field is the archive's apparent
//! length in KiB (rounded up), not `du -k` allocated blocks, which read 1 on
//! the VPS ZFS dataset right after the rename.
//!
//! * `fake_tree_size_kb_matches_length` runs in plain `cargo test`: a real
//!   backup of the fake tree, then `size_kb` == `ceil(len / 1024)` ==
//!   `du -k --apparent-size` (GNU coreutils on the box).
//! * `real_vps_size_kb_matches_apparent` (GALION_REAL=1 +
//!   GALION_VPS_SSH=<alias>) is READ-ONLY: `stat`, `du`, `df -T` and
//!   `journalctl` on the newest live archive. It never starts or writes
//!   anything on the VPS.
//!
//! `GALION_REAL=1 GALION_VPS_SSH=slavik-vps cargo test --release -p galion-backup --test real_size_kb -- --nocapture`

mod common;

use common::{build_tree, tmp};
use galion_backup::backup::{self, du_kb, size_kb, Opts};
use std::process::Command;

fn say(m: &str) {
    println!("[{}] {m}", backup::now_iso());
}

#[test]
fn fake_tree_size_kb_matches_length() {
    let t = tmp("sizekb");
    let root = t.join("root");
    let dest = t.join("dest");
    build_tree(&root);
    let o = Opts {
        root: root.clone(),
        dest: dest.clone(),
        keep: 12,
        minfree_kb: Some(1),
        gzip_level: 6,
    };
    assert_eq!(backup::run(&o), 0);
    let arcs = backup::archives(&dest);
    assert_eq!(arcs.len(), 1);
    let a = &arcs[0];
    let len = std::fs::metadata(a).unwrap().len();
    let k = size_kb(a).unwrap();
    assert_eq!(k, len.div_ceil(1024));
    let du = Command::new("du")
        .args(["-k", "--apparent-size"])
        .arg(a)
        .output()
        .unwrap();
    assert!(du.status.success());
    let du_app: u64 = String::from_utf8_lossy(&du.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(k, du_app, "size_kb == du -k --apparent-size");
    say(&format!(
        "fake tree: len {len} B size_kb {k} du_kb(alloc) {} du --apparent-size {du_app}",
        du_kb(a).unwrap()
    ));
    let _ = std::fs::remove_dir_all(&t);
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
fn real_vps_size_kb_matches_apparent() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped (set GALION_REAL=1)");
        return;
    }
    let Ok(host) = std::env::var("GALION_VPS_SSH") else {
        eprintln!("skipped (set GALION_VPS_SSH=<ssh host alias>)");
        return;
    };
    // Fixed read-only commands; the archive name is matched against the
    // script's own pattern, so no user input reaches the shell.
    let out = ssh(
        &host,
        "f=$(ls -t /var/backups/galion/galion-*.tar.gz | head -1); \
         echo \"name $(basename \"$f\")\"; \
         echo \"len $(stat -c %s \"$f\")\"; \
         echo \"alloc $(du -k \"$f\" | cut -f1)\"; \
         echo \"apparent $(du -k --apparent-size \"$f\" | cut -f1)\"; \
         echo \"fstype $(df -T /var/backups/galion | awk 'NR==2{print $2}')\"; \
         echo \"logged $(journalctl -u galion-backup.service --no-pager -o cat --since -2days \
           | grep \"archive=$(basename \"$f\") \" | grep -o 'size_kb=[0-9]*' | tail -1)\"",
    );
    let get = |k: &str| -> String {
        out.lines()
            .find_map(|l| l.strip_prefix(&format!("{k} ")))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let len: u64 = get("len").parse().expect("len");
    let apparent: u64 = get("apparent").parse().expect("apparent");
    let fixed = len.div_ceil(1024);
    say(&format!(
        "VPS newest {} on {}: len {len} B, du -k (alloc) {}, du -k --apparent-size {apparent}, \
         live (pre-fix) logged {}, fixed size_kb would log {fixed}",
        get("name"),
        get("fstype"),
        get("alloc"),
        get("logged")
    ));
    assert!(len > 0, "archive not empty");
    assert_eq!(fixed, apparent, "fixed size_kb == du -k --apparent-size");
    println!("RESULT: 0 failed");
}
