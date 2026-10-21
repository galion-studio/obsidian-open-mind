// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! `galion-backup run` against a fake tree (library entry, root = temp dir),
//! checked with the system GNU tar / sha256sum / sqlite reader. Runs in
//! plain `cargo test`; the bash-vs-Rust parity run is tests/real_parity.rs.

mod common;

use common::{build_tree, mode, tmp};
use galion_backup::backup::{self, Opts};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;

// run() sets the process umask and uses second-resolution stamps: serialize.
static SERIAL: Mutex<()> = Mutex::new(());

fn opts(root: &Path, dest: &Path) -> Opts {
    Opts {
        root: root.to_path_buf(),
        dest: dest.to_path_buf(),
        keep: 12,
        minfree_kb: Some(1),
    }
}

fn tar_list(args: &[&str], p: &Path) -> String {
    let o = Command::new("tar").args(args).arg(p).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn full_backup_contents_modes_and_manifest() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let t = tmp("full");
    let root = t.join("root");
    let dest = t.join("dest");
    build_tree(&root);
    assert_eq!(backup::run(&opts(&root, &dest)), 0);

    assert_eq!(mode(&dest), 0o700, "DEST is 0700");
    let arcs = backup::archives(&dest);
    assert_eq!(arcs.len(), 1);
    let a = &arcs[0];
    assert_eq!(mode(a), 0o600, "archive is 0600");
    let leftovers: Vec<_> = fs::read_dir(&dest)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".work-") || n.ends_with(".part"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "work dir / part removed: {leftovers:?}"
    );
    assert_eq!(mode(&dest.join(".lock")), 0o600);

    // Outer archive layout.
    let top = a
        .file_name()
        .unwrap()
        .to_string_lossy()
        .trim_end_matches(".tar.gz")
        .to_string();
    let list = tar_list(&["-tzf"], a);
    for m in [
        "/",
        "/SHA256SUMS",
        "/db/",
        "/db/MAP",
        "/files/",
        "/files/files.tar",
    ] {
        assert!(
            list.lines().any(|l| l == format!("{top}{m}")),
            "{m} in\n{list}"
        );
    }
    for n in [
        "memory-vectors.db",
        "memory-root-vectors.db",
        "hermes-rs-state.db",
        "hermes-legacy-state.db",
    ] {
        assert!(list.contains(&format!("{top}/db/{n}")), "{n}");
    }
    let x = t.join("x");
    fs::create_dir_all(&x).unwrap();
    assert!(Command::new("tar")
        .arg("-xzf")
        .arg(a)
        .arg("-C")
        .arg(&x)
        .status()
        .unwrap()
        .success());
    let s = x.join(&top);
    assert_eq!(mode(&s), 0o700);
    assert_eq!(mode(&s.join("files/files.tar")), 0o600);

    // MAP in script order.
    let map = fs::read_to_string(s.join("db/MAP")).unwrap();
    assert_eq!(
        map,
        "/var/lib/memory-tencentdb/instances/galion/vectors.db memory-vectors.db\n\
         /var/lib/memory-tencentdb/vectors.db memory-root-vectors.db\n\
         /var/lib/hermes-rs/rs-state.db hermes-rs-state.db\n\
         /opt/galion/hermes/state.db hermes-legacy-state.db\n"
    );
    // Snapshots are complete, consistent single files (journal_mode=delete).
    let c = rusqlite::Connection::open(s.join("db/memory-vectors.db")).unwrap();
    let n: i64 = c
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 300);
    let jm: String = c
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(jm, "delete");

    // SHA256SUMS: C-locale order, verifies with coreutils.
    let sums = fs::read_to_string(s.join("SHA256SUMS")).unwrap();
    let names: Vec<&str> = sums.lines().map(|l| &l[66..]).collect();
    assert_eq!(
        names,
        [
            "./db/MAP",
            "./db/hermes-legacy-state.db",
            "./db/hermes-rs-state.db",
            "./db/memory-root-vectors.db",
            "./db/memory-vectors.db",
            "./files/files.tar"
        ]
    );
    let ok = Command::new("sha256sum")
        .args(["-c", "--quiet", "SHA256SUMS"])
        .current_dir(&s)
        .status()
        .unwrap();
    assert!(ok.success());

    // files.tar: GNU tar reads it; excludes, links, long names, order.
    let ft = s.join("files/files.tar");
    assert_eq!(fs::metadata(&ft).unwrap().len() % 10240, 0, "record padded");
    let inner = tar_list(&["-tvf"], &ft);
    let names = tar_list(&["-tf"], &ft);
    for bad in [
        ".db\n",
        ".db-wal",
        ".db-shm",
        "/.backup",
        "dir.db",
        "memory-gateway",
        "other.json",
        "broken",
    ] {
        assert!(!names.contains(bad), "{bad} must be excluded:\n{names}");
    }
    for good in [
        "var/lib/memory-tencentdb/\n",
        "var/lib/memory-tencentdb/keep.db.bak",
        "var/lib/hermes-rs/blob.bin",
        "var/lib/hermes-rs/empty",
        "var/lib/hermes-rs/fifo",
        "opt/galion/hermes/.env",
        "etc/systemd/system/galion-health.service",
        "root/galion-secrets/admin-device.json",
        "a-very-long-file-name-a-very-long-file-name-a-very-long-file-name-a-very-long-file-name-.md",
    ] {
        assert!(names.contains(good), "{good} missing:\n{names}");
    }
    assert!(
        inner.contains("var/lib/hermes-rs/config-link -> config.toml"),
        "{inner}"
    );
    assert!(inner.contains("var/lib/hermes-rs/dangling -> /nonexistent/target"));
    assert!(
        inner.contains("var/lib/hermes-rs/config-hard link to var/lib/hermes-rs/config.toml")
            || inner
                .contains("var/lib/hermes-rs/config.toml link to var/lib/hermes-rs/config-hard"),
        "{inner}"
    );
    // Operand order = FILES order, then sorted dns-backup-*.json (C locale: A < a < b).
    let tops: Vec<&str> = names
        .lines()
        .filter(|l| l.starts_with("root/galion-switch"))
        .collect();
    assert_eq!(
        tops,
        [
            "root/galion-switch-20261018-2140/dns-backup-A.json",
            "root/galion-switch-20261018-2140/dns-backup-a.json",
            "root/galion-switch-20261018-2140/dns-backup-b.json"
        ]
    );
    let first = names.lines().next().unwrap();
    assert_eq!(first, "var/lib/memory-tencentdb/");
    // Extract and compare bytes + modes with the source tree.
    let fx = t.join("fx");
    fs::create_dir_all(&fx).unwrap();
    assert!(Command::new("tar")
        .arg("-xpf")
        .arg(&ft)
        .arg("-C")
        .arg(&fx)
        .status()
        .unwrap()
        .success());
    for rel in [
        "var/lib/hermes-rs/blob.bin",
        "opt/galion/hermes/.env",
        "var/lib/memory-tencentdb/pyramid/l2/b.md",
    ] {
        assert_eq!(
            fs::read(fx.join(rel)).unwrap(),
            fs::read(root.join(rel)).unwrap(),
            "{rel}"
        );
        assert_eq!(mode(&fx.join(rel)), mode(&root.join(rel)), "{rel} mode");
        assert_eq!(
            fs::metadata(fx.join(rel)).unwrap().mtime(),
            fs::metadata(root.join(rel)).unwrap().mtime()
        );
    }
    let _ = fs::remove_dir_all(&t);
}

#[test]
fn rotation_keeps_newest_keep() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let t = tmp("rot");
    let root = t.join("root");
    let dest = t.join("dest");
    build_tree(&root);
    fs::create_dir_all(&dest).unwrap();
    // 14 older archives with distinct mtimes, plus a non-matching file.
    for i in 0..14 {
        let p = dest.join(format!("galion-2026010{}-{:06}+0100.tar.gz", i % 9, i));
        fs::write(&p, b"old").unwrap();
        let ts = 1_700_000_000 + i * 3600;
        let st = Command::new("touch")
            .arg("-d")
            .arg(format!("@{ts}"))
            .arg(&p)
            .status()
            .unwrap();
        assert!(st.success());
    }
    fs::write(dest.join("galion-notes.txt"), b"not an archive").unwrap();
    let mut o = opts(&root, &dest);
    o.keep = 3;
    assert_eq!(backup::run(&o), 0);
    let left = backup::newest_first(backup::archives(&dest));
    assert_eq!(left.len(), 3, "{left:?}");
    let names: Vec<String> = left
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // The new one plus the two newest old ones (i = 13, 12).
    assert!(
        names[1].ends_with("-000013+0100.tar.gz") && names[2].ends_with("-000012+0100.tar.gz"),
        "{names:?}"
    );
    assert!(dest.join("galion-notes.txt").exists());
    let _ = fs::remove_dir_all(&t);
}

#[test]
fn low_disk_lock_missing_and_corrupt_db() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let t = tmp("edge");
    let root = t.join("root");
    let dest = t.join("dest");
    build_tree(&root);

    // Low disk: exit 1, nothing written but DEST (0700).
    let mut o = opts(&root, &dest);
    o.minfree_kb = Some(u64::MAX);
    assert_eq!(backup::run(&o), 1);
    assert_eq!(mode(&dest), 0o700);
    assert!(backup::archives(&dest).is_empty());
    o.minfree_kb = None; // non-integer MINFREE: bash took the low-disk branch
    assert_eq!(backup::run(&o), 1);

    // Lock held elsewhere: exit 0, no archive.
    let held = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(dest.join(".lock"))
        .unwrap();
    held.lock().unwrap();
    assert_eq!(backup::run(&opts(&root, &dest)), 0);
    assert!(backup::archives(&dest).is_empty());
    drop(held);

    // A missing DB is a warning, not a failure.
    fs::remove_file(root.join("opt/galion/hermes/state.db")).unwrap();
    assert_eq!(backup::run(&opts(&root, &dest)), 0);
    let a = &backup::archives(&dest)[0];
    let list = tar_list(&["-tzf"], a);
    assert!(!list.contains("hermes-legacy-state.db"));
    assert!(list.contains("hermes-rs-state.db"));
    std::thread::sleep(std::time::Duration::from_millis(1100));

    // A corrupt DB aborts the run (exit 1), no new archive, no leftovers.
    fs::write(
        root.join("var/lib/hermes-rs/rs-state.db"),
        b"this is not sqlite at all, really",
    )
    .unwrap();
    let _ = fs::remove_file(root.join("var/lib/hermes-rs/rs-state.db-wal"));
    let _ = fs::remove_file(root.join("var/lib/hermes-rs/rs-state.db-shm"));
    assert_eq!(backup::run(&opts(&root, &dest)), 1);
    assert_eq!(backup::archives(&dest).len(), 1);
    assert!(!fs::read_dir(&dest)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(".work-")));
    let _ = fs::remove_dir_all(&t);
}
