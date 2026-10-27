// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! VPS-sized fake tree (moved unchanged from real_perf.rs, swarm/backup-perf):
//! 4 DBs ~46 MB incl. a held-open WAL, 1845 files.tar members. No real data.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// xorshift64*: deterministic, no deps.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const WORDS: &[&str] = &[
    "galion",
    "hermes",
    "memory",
    "the",
    "and",
    "of",
    "to",
    "a",
    "in",
    "is",
    "that",
    "for",
    "agent",
    "session",
    "user",
    "assistant",
    "tool",
    "result",
    "error",
    "ok",
    "state",
    "model",
    "route",
    "nerve",
    "cortex",
    "backup",
    "tunnel",
    "health",
    "score",
    "event",
    "sensor",
    "plan",
    "cutover",
    "deploy",
    "rust",
    "binary",
    "port",
    "request",
    "reply",
    "summary",
    "level",
    "info",
    "warn",
    "time",
    "zagreb",
    "pyramid",
    "offload",
    "scene",
    "block",
    "vector",
    "query",
    "search",
];

/// Text with roughly natural-language compressibility (random word order
/// plus a sprinkle of hex ids, which is what memory/offload notes look like).
fn text(r: &mut Rng, len: usize) -> Vec<u8> {
    let mut s = Vec::with_capacity(len + 32);
    while s.len() < len {
        match r.below(40) {
            0 => s.extend_from_slice(format!("{:016x}", r.next()).as_bytes()),
            1 => s.push(b'\n'),
            _ => s.extend_from_slice(WORDS[r.below(WORDS.len() as u64) as usize].as_bytes()),
        }
        s.push(b' ');
    }
    s.truncate(len);
    s
}

/// VPS size quantiles of the 1773 plain files (p0 0, p50 737, p90 7734,
/// p99 34907); log-linear between the points, top file absorbs the rest so
/// the sum is the VPS's 6 275 896 B.
fn sizes(n: usize, total: u64) -> Vec<u64> {
    let pts: [(f64, f64); 5] = [
        (0.0, 20.0),
        (0.5, 737.0),
        (0.9, 7734.0),
        (0.99, 34907.0),
        (1.0, 90000.0),
    ];
    let mut v: Vec<u64> = (0..n)
        .map(|i| {
            let u = (i as f64 + 0.5) / n as f64;
            let k = pts.windows(2).position(|w| u <= w[1].0).unwrap_or(3);
            let ((u0, s0), (u1, s1)) = (pts[k], pts[k + 1]);
            let t = (u - u0) / (u1 - u0);
            (s0.ln() + t * (s1.ln() - s0.ln())).exp() as u64
        })
        .collect();
    v[0] = 0; // VPS min is 0
    let sum: u64 = v.iter().sum();
    let last = n - 1;
    v[last] = (v[last] + total).saturating_sub(sum);
    v
}

fn w(root: &Path, rel: &str, data: &[u8], mode: u32) {
    let p = root.join(rel.trim_start_matches('/'));
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, data).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
}

/// A DB filled to ~`bytes`; `blob` = f32-vector rows (incompressible),
/// else chat-history-like text rows. Returns the open connection when
/// `keep_wal` (WAL survives only while a connection is open, like on the VPS
/// where the memory gateway holds it).
fn make_db(
    p: &Path,
    bytes: u64,
    blob: bool,
    keep_wal: u64,
    r: &mut Rng,
) -> Option<rusqlite::Connection> {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut c = rusqlite::Connection::open(p).unwrap();
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
         CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY, session TEXT, role TEXT, content TEXT, ts INTEGER);
         CREATE TABLE IF NOT EXISTS vectors(id INTEGER PRIMARY KEY, ref TEXT, v BLOB);
         CREATE INDEX IF NOT EXISTS m_session ON messages(session);",
    )
    .unwrap();
    let fill = |c: &mut rusqlite::Connection, target: u64, r: &mut Rng| {
        let tx = c.transaction().unwrap();
        let mut done = 0u64;
        while done < target {
            if blob {
                let v: Vec<u8> = (0..384 * 4).map(|_| r.next() as u8).collect();
                tx.execute(
                    "INSERT INTO vectors(ref, v) VALUES (?1, ?2)",
                    rusqlite::params![format!("ref-{:x}", r.next()), v],
                )
                .unwrap();
                done += 1600;
            } else {
                let len = 200 + r.below(1800) as usize;
                let body = String::from_utf8(text(r, len)).unwrap();
                tx.execute(
                    "INSERT INTO messages(session, role, content, ts) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        format!("s-{}", r.below(400)),
                        if r.below(2) == 0 { "user" } else { "assistant" },
                        body,
                        1_760_000_000 + r.below(10_000_000) as i64
                    ],
                )
                .unwrap();
                done += len as u64 + 60;
            }
        }
        tx.commit().unwrap();
    };
    fill(&mut c, bytes, r);
    c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    if keep_wal == 0 {
        return None;
    }
    fill(&mut c, keep_wal, r); // stays in -wal (autocheckpoint off)
    Some(c)
}

pub fn build_vps_sized_tree(root: &Path) -> Vec<rusqlite::Connection> {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut held = Vec::new();
    held.extend(make_db(
        &root.join("opt/galion/hermes/state.db"),
        34_520_000,
        false,
        0,
        &mut r,
    ));
    held.extend(make_db(
        &root.join("var/lib/memory-tencentdb/instances/galion/vectors.db"),
        3_115_000,
        true,
        3_220_000,
        &mut r,
    ));
    held.extend(make_db(
        &root.join("var/lib/hermes-rs/rs-state.db"),
        764_000,
        false,
        0,
        &mut r,
    ));
    held.extend(make_db(
        &root.join("var/lib/memory-tencentdb/vectors.db"),
        86_000,
        true,
        0,
        &mut r,
    ));

    // memory-tencentdb: 48 included dirs + the excluded empty .backup.
    let m = "/var/lib/memory-tencentdb";
    fs::create_dir_all(root.join("var/lib/memory-tencentdb/.backup")).unwrap();
    let mut dirs: Vec<(String, usize)> = vec![
        ("pyramid".into(), 1),
        ("pyramid/l0".into(), 249),
        ("pyramid/l1".into(), 215),
        ("pyramid/l2".into(), 249),
        ("offload".into(), 0),
        ("offload/openmind-release-20261018".into(), 296),
        ("offload/openmind-foundation-20261018".into(), 210),
        ("offload/d90b6a09-0000-4000-8000-000000000000".into(), 86),
        ("offload/api-b8534f161e82fc1d".into(), 68),
        ("offload/api-8097caa43660e0df".into(), 64),
        ("scene_blocks".into(), 69),
        ("instances".into(), 0),
        ("instances/galion".into(), 0),
    ];
    for i in 0..30 {
        // 250 files over 30 smaller offload dirs (974 in offload total).
        dirs.push((
            format!("offload/api-{:016x}", r.next()),
            if i < 10 { 9 } else { 8 },
        ));
    }
    for i in 0..5 {
        dirs.push((format!("misc-{i}"), if i == 0 { 4 } else { 3 }));
    }
    let nfiles: usize = dirs.iter().map(|d| d.1).sum();
    assert_eq!(dirs.len(), 48);
    assert_eq!(nfiles, 1773);
    let sz = sizes(nfiles, 6_275_896);
    let mut k = 0usize;
    for (d, n) in &dirs {
        fs::create_dir_all(root.join(format!("{}/{d}", &m[1..]))).unwrap();
        for j in 0..*n {
            let ext = match k % 61 {
                0 => "jsonl",
                1 if k < 61 * 17 => "mmd",
                2 if k < 61 * 3 => "json",
                3 if k < 61 => "mdu",
                _ => "md",
            };
            let body = text(&mut r, sz[(k * 7919) % nfiles] as usize);
            w(
                root,
                &format!("{m}/{d}/n{j:04}-{:08x}.{ext}", r.next() as u32),
                &body,
                0o644,
            );
            k += 1;
        }
    }
    w(
        root,
        "/var/lib/hermes-rs/config.toml",
        &text(&mut r, 703),
        0o600,
    );
    w(
        root,
        "/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml",
        &text(&mut r, 3319),
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
        &text(&mut r, 2000),
        0o644,
    );
    w(root, "/etc/openmind-memory.env", b"PLACEHOLDER=2\n", 0o600);
    w(root, "/etc/memory-gateway.env", b"PLACEHOLDER=3\n", 0o600);
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
    for i in 0..7 {
        w(
            root,
            &format!("/root/galion-switch-20261018-2140/dns-backup-{i}.json"),
            &text(&mut r, 1500),
            0o600,
        );
    }
    held
}
