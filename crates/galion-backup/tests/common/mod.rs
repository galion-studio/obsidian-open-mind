// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! A fake VPS tree in the shape galion-backup.sh backs up. No real data:
//! every "secret" is a placeholder string.
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

pub mod vps_tree;

pub fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("galion-backup-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn w(root: &Path, rel: &str, data: &[u8], mode: u32) {
    let p = root.join(rel.trim_start_matches('/'));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, data).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
}

/// A WAL-mode SQLite DB with `rows` rows, left with a live -wal (a reader
/// connection is not kept; the WAL is checkpointed lazily by SQLite).
pub fn make_db(p: &Path, rows: usize) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(p).unwrap();
    c.execute_batch(
        "PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS t(id INTEGER PRIMARY KEY, v TEXT); CREATE TABLE IF NOT EXISTS u(x);",
    )
    .unwrap();
    for i in 0..rows {
        c.execute(
            "INSERT INTO t(v) VALUES (?1)",
            [format!("row-{i}-{}", "x".repeat(i % 50))],
        )
        .unwrap();
    }
}

pub fn build_tree(root: &Path) {
    // SQLite DBs (all four).
    make_db(
        &root.join("var/lib/memory-tencentdb/instances/galion/vectors.db"),
        300,
    );
    make_db(&root.join("var/lib/memory-tencentdb/vectors.db"), 20);
    make_db(&root.join("var/lib/hermes-rs/rs-state.db"), 100);
    make_db(&root.join("opt/galion/hermes/state.db"), 50);
    // Plain trees around them.
    w(
        root,
        "/var/lib/memory-tencentdb/pyramid/l2/a.md",
        b"memo a\n",
        0o644,
    );
    w(
        root,
        "/var/lib/memory-tencentdb/pyramid/l2/b.md",
        b"memo b\n",
        0o640,
    );
    w(
        root,
        "/var/lib/memory-tencentdb/.backup/old.json",
        b"excluded\n",
        0o600,
    );
    w(
        root,
        "/var/lib/memory-tencentdb/keep.db.bak",
        b"kept: not *.db\n",
        0o600,
    );
    w(
        root,
        "/var/lib/memory-tencentdb/dir.db/inside.txt",
        b"excluded via dir\n",
        0o600,
    );
    let long = format!(
        "/var/lib/memory-tencentdb/{}/{}.md",
        "deep-directory-name-".repeat(4),
        "a-very-long-file-name-".repeat(4)
    );
    w(root, &long, b"long path member\n", 0o644);
    w(
        root,
        "/var/lib/hermes-rs/config.toml",
        b"[hermes]\nx = 1\n",
        0o600,
    );
    w(root, "/var/lib/hermes-rs/empty", b"", 0o600);
    let big: Vec<u8> = (0..200_000u32).map(|i| (i * 31 % 251) as u8).collect();
    w(root, "/var/lib/hermes-rs/blob.bin", &big, 0o600);
    symlink("config.toml", root.join("var/lib/hermes-rs/config-link")).unwrap();
    symlink(
        "/nonexistent/target",
        root.join("var/lib/hermes-rs/dangling"),
    )
    .unwrap();
    fs::hard_link(
        root.join("var/lib/hermes-rs/config.toml"),
        root.join("var/lib/hermes-rs/config-hard"),
    )
    .unwrap();
    let _ = Command::new("mkfifo")
        .arg(root.join("var/lib/hermes-rs/fifo"))
        .status();
    w(
        root,
        "/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml",
        b"tunnel: placeholder\n",
        0o644,
    );
    w(
        root,
        "/root/.cloudflared/cert.pem",
        b"PLACEHOLDER-NOT-A-CERT\n",
        0o600,
    );
    w(root, "/opt/galion/hermes/.env", b"PLACEHOLDER=1\n", 0o600);
    w(
        root,
        "/opt/galion/hermes/config.yaml",
        b"model: placeholder\n",
        0o644,
    );
    w(root, "/etc/openmind-memory.env", b"PLACEHOLDER=2\n", 0o600);
    // /etc/memory-gateway.env deliberately missing (skipped like [ -e ]).
    w(
        root,
        "/root/galion-secrets/admin-device.json",
        b"{\"placeholder\":true}\n",
        0o600,
    );
    for u in [
        "hermes-rs",
        "openmind-memory",
        "link-watch-rs",
        "cloudflared",
        "galion-health",
    ] {
        w(
            root,
            &format!("/etc/systemd/system/{u}.service"),
            format!("[Unit]\nDescription={u}\n").as_bytes(),
            0o644,
        );
    }
    for n in [
        "dns-backup-b.json",
        "dns-backup-A.json",
        "dns-backup-a.json",
    ] {
        w(
            root,
            &format!("/root/galion-switch-20261018-2140/{n}"),
            b"{}\n",
            0o600,
        );
    }
    w(
        root,
        "/root/galion-switch-20261018-2140/other.json",
        b"{}\n",
        0o600,
    );
    symlink(
        "missing.json",
        root.join("root/galion-switch-20261018-2140/dns-backup-broken.json"),
    )
    .unwrap();
}

pub fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o7777
}
