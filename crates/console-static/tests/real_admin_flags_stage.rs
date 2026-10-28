// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 313, D-303-1 prep): run the release
//! `galion-console-serve` with the EXACT argv of the staged drop-in
//! `deploy/console-rust-serve/console.service.d/60-admin-store.conf`
//! (parsed from the file, only host paths and ports remapped into a temp
//! root), and check over real TCP what the drop-in fixes and how it fails:
//!
//! A. drop-in as staged (store dirs writable = ReadWritePaths): board routes
//!    200, remote host `/api/v1/admin` 200, pair page instead of 503, an
//!    already-paired phone (Node-format store) keeps working, new pairing
//!    writes hashes only to store + mirror (0600, identical).
//! B. drop-in without ReadWritePaths (store + mirror dirs 0500): paired
//!    cookie still verifies, new pairing fails closed 503 "store not
//!    writable", store bytes unchanged, board still 200; since slot 323
//!    (F-313-1 fixed) wrong codes get the same 503 and then lock (429).
//! C. binary swap only (live argv, no admin flags, on the new build):
//!    board 200 but `/api/v1/admin` 503 "admin store not configured".
//! D. mirror dir unwritable only: pairing still 200, mirror stays stale.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_admin_flags_stage -- --nocapture`
//! Ports: 21830-21833 (box only). No VPS contact, no live change.

mod units;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LIVE: &str = include_str!("../../../deploy/console-rust-serve/live/console.service");
const DROP: &str =
    include_str!("../../../deploy/console-rust-serve/console.service.d/60-admin-store.conf");
const REMOTE: &str = "console.galion.app";
const CODE: &str = "SLOT313-STAGE-CODE";
const OLD_TOKEN: &str = "slot313-already-paired-phone-token";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn req(at: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Resp {
    let Ok(mut s) = TcpStream::connect(at) else {
        return Resp {
            code: 0,
            head: String::new(),
            body: String::new(),
        };
    };
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut out = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if method == "POST" {
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let _ = s.write_all(out.as_bytes());
    let _ = s.write_all(body);
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let p = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..p]).to_string();
    let code = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = String::from_utf8_lossy(raw.get(p + 4..).unwrap_or(&[])).to_string();
    Resp { code, head, body }
}

fn get(at: &str, path: &str, host: &str, cookie: Option<&str>) -> Resp {
    let mut h = vec![("Host", host)];
    if let Some(c) = cookie {
        h.push(("Cookie", c));
    }
    req(at, "GET", path, &h, b"")
}

fn pair_json(at: &str, code: &str) -> Resp {
    let body = format!("{{\"code\":\"{code}\"}}");
    req(
        at,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Content-Type", "application/json")],
        body.as_bytes(),
    )
}

fn admin_cookie(r: &Resp) -> Option<String> {
    r.head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        let v = v.trim();
        (k.eq_ignore_ascii_case("set-cookie") && v.starts_with("galion_admin="))
            .then(|| v.split(';').next().unwrap_or("").to_string())
    })
}

fn wait_up(at: &str) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if TcpStream::connect(at).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn sha_hex(s: &str) -> String {
    console_static::sha256::hex(s.as_bytes())
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn chmod(p: &Path, m: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

/// Remap the unit argv (VPS paths/ports) into the temp root.
fn remap(argv: &[String], exe: &Path, tmp: &Path, bind: &str) -> Vec<String> {
    let t = tmp.to_str().unwrap();
    argv.iter()
        .enumerate()
        .map(|(i, a)| {
            if i == 0 {
                return exe.to_str().unwrap().to_string();
            }
            a.replace("127.0.0.1:18080", bind)
                .replace("127.0.0.1:8080", bind)
                .replace("127.0.0.1:8091", "127.0.0.1:21838")
                .replace("127.0.0.1:8646", "127.0.0.1:21839")
                .replace("/opt/galion/console-dist", &format!("{t}/opt/console-dist"))
                .replace("/root/galion-secrets", &format!("{t}/root/galion-secrets"))
                .replace(
                    "/workspace/galion-light/secrets/persist",
                    &format!("{t}/persist"),
                )
        })
        .collect()
}

fn spawn(argv: &[String], at: &str) -> Kill {
    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn galion-console-serve");
    let k = Kill(child);
    assert!(wait_up(at), "server did not bind {at}");
    k
}

const BOARD: &[&str] = &[
    "/api/v1/services",
    "/api/v1/domains",
    "/api/v1/repos",
    "/api/v1/catalog",
    "/api/v1/ledger",
    "/api/v1/docs",
    "/api/v1/protocol",
    "/api/v1/protocol/snapshot",
    "/api/v1/markers",
    "/api/v1/health",
];

fn board_ok(at: &str, tag: &str) {
    for p in BOARD {
        let r = get(at, p, "127.0.0.1", None);
        assert_eq!(r.code, 200, "{tag} {p}: {}", r.body);
    }
    eprintln!("{tag}: {} board routes 200 on loopback", BOARD.len());
}

#[test]
fn real_admin_flags_stage() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let fixture = std::env::var("CONSOLE_DIST_STATIC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static")
        });
    assert!(
        fixture.join("favicon.svg").is_file(),
        "fixture {}",
        fixture.display()
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_galion-console-serve"));
    let tmp = std::env::temp_dir().join(format!("real-admin-313-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // Same layout as the VPS: <dist>/static, store dir 0700, file 0600.
    let dist = tmp.join("opt/console-dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::os::unix::fs::symlink(&fixture, dist.join("static")).unwrap();
    let sdir = tmp.join("root/galion-secrets");
    let mdir = tmp.join("persist");
    for d in [&sdir, &mdir] {
        std::fs::create_dir_all(d).unwrap();
        chmod(d, 0o700);
    }
    let store = sdir.join("admin-device.json");
    let mirror = mdir.join("admin-device.json");
    // Node admin-device.ts format: an already-paired phone (OLD_TOKEN).
    let node = format!(
        "{{\"codeHash\":\"{}\",\"tokenHash\":\"{t}\",\"tokenHashes\":[\"{t}\"],\"deviceHash\":\"{}\",\"fails\":0}}\n",
        sha_hex(CODE),
        sha_hex("slot313-old-device"),
        t = sha_hex(OLD_TOKEN)
    );
    std::fs::write(&store, &node).unwrap();
    chmod(&store, 0o600);
    std::fs::write(&mirror, &node).unwrap();
    chmod(&mirror, 0o600);
    let old = format!("galion_admin={OLD_TOKEN}");

    let staged = units::exec_argv(&[LIVE, DROP]);
    let live = units::exec_argv(&[LIVE]);
    let t0 = Instant::now();

    // ---- A: drop-in as staged ----
    let a_at = "127.0.0.1:21830";
    let argv = remap(&staged, &exe, &tmp, a_at);
    assert!(argv.iter().any(|a| a == "--admin-store"));
    eprintln!("A argv (remapped from drop-in): {}", argv[1..].join(" "));
    {
        let _k = spawn(&argv, a_at);
        board_ok(a_at, "A");
        let r = get(a_at, "/api/v1/admin", REMOTE, None);
        assert_eq!(
            (r.code, r.body.as_str()),
            (200, r#"{"ok":true,"paired":true,"you":false}"#)
        );
        let r = get(a_at, "/api/v1/admin", REMOTE, Some(&old));
        assert_eq!(
            r.body, r#"{"ok":true,"paired":true,"you":true}"#,
            "Node-format cookie"
        );
        let c = get(a_at, "/console", REMOTE, None);
        assert_eq!(c.code, 200);
        assert!(c.body.contains("/_ui/pair") && !c.body.contains("not answering"));
        let c = get(a_at, "/services", REMOTE, Some(&old));
        assert_eq!(c.code, 200);
        assert!(!c.body.contains("/_ui/pair"), "paired phone sees the page");
        assert_eq!(get(a_at, "/api/v1/services", REMOTE, None).code, 401);
        assert_eq!(get(a_at, "/api/v1/services", REMOTE, Some(&old)).code, 200);
        let p = pair_json(a_at, &CODE.to_lowercase());
        assert_eq!(p.code, 200, "{}", p.body);
        let fresh = admin_cookie(&p).expect("admin cookie");
        let r = get(a_at, "/api/v1/admin", REMOTE, Some(&fresh));
        assert!(r.body.contains(r#""you":true"#));
        let raw = std::fs::read_to_string(&store).unwrap();
        assert_eq!(
            std::fs::read_to_string(&mirror).unwrap(),
            raw,
            "mirror == store"
        );
        assert_eq!((mode(&store), mode(&mirror)), (0o600, 0o600));
        let tok = fresh.trim_start_matches("galion_admin=");
        assert!(!raw.contains(CODE) && !raw.contains(&CODE.to_lowercase()));
        assert!(
            !raw.contains(tok) && !raw.contains(OLD_TOKEN),
            "hashes only"
        );
        assert!(raw.contains(&sha_hex(OLD_TOKEN)) && raw.contains(&sha_hex(tok)));
        // still works for the old phone after the new pairing
        assert!(get(a_at, "/api/v1/admin", REMOTE, Some(&old))
            .body
            .contains(r#""you":true"#));
        eprintln!("A: remote admin 200, pair page 200 (not 503), Node cookie ok, new pair 200, store=mirror 0600, hashes only");
    }

    // ---- B: same argv, but no ReadWritePaths (both dirs read-only) ----
    let b_at = "127.0.0.1:21831";
    let before = std::fs::read(&store).unwrap();
    chmod(&sdir, 0o500);
    chmod(&mdir, 0o500);
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, b_at), b_at);
        board_ok(b_at, "B");
        assert!(get(b_at, "/api/v1/admin", REMOTE, Some(&old))
            .body
            .contains(r#""you":true"#));
        let p = pair_json(b_at, CODE);
        assert_eq!(
            (p.code, p.body.as_str()),
            (503, r#"{"error":"store not writable"}"#)
        );
        assert!(admin_cookie(&p).is_none(), "no cookie when not stored");
        assert_eq!(std::fs::read(&store).unwrap(), before, "store unchanged");
        // F-313-1 fixed (slot 323): the CODE try above was charged in memory
        // as attempt 1; wrong codes answer the same 503 as the right one,
        // then the in-memory lock answers 429 for both (no oracle).
        let codes: Vec<u16> = (0..6)
            .map(|i| pair_json(b_at, &format!("WRONG{i}")).code)
            .collect();
        let right = pair_json(b_at, CODE).code;
        assert_eq!(codes, vec![503, 503, 503, 503, 429, 429], "F-313-1 fixed");
        assert_eq!(right, 429, "right code locked too");
        eprintln!("B: F-313-1 fixed: RO store 6 wrong -> {codes:?}, right -> {right}");
        assert_eq!(
            std::fs::read(&store).unwrap(),
            before,
            "store still unchanged"
        );
        eprintln!(
            "B: no RW -> paired cookie ok, new pair 503 'store not writable', store unchanged"
        );
    }
    chmod(&sdir, 0o700);
    chmod(&mdir, 0o700);
    let leftovers: Vec<_> = std::fs::read_dir(&sdir)
        .unwrap()
        .chain(std::fs::read_dir(&mdir).unwrap())
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");

    // ---- C: binary swap only (live argv, no admin flags) ----
    let c_at = "127.0.0.1:21832";
    {
        let _k = spawn(&remap(&live, &exe, &tmp, c_at), c_at);
        board_ok(c_at, "C");
        let r = get(c_at, "/api/v1/admin", REMOTE, None);
        assert_eq!(r.code, 503, "{}", r.body);
        assert!(r.body.contains("admin store not configured"), "{}", r.body);
        let c = get(c_at, "/console", REMOTE, None);
        eprintln!("C: live argv on new build -> board 200, remote /api/v1/admin 503 '{}', remote /console {}", r.body.trim(), c.code);
    }

    // ---- D: mirror dir unwritable only ----
    let d_at = "127.0.0.1:21833";
    let mirror_before = std::fs::read(&mirror).unwrap();
    chmod(&mdir, 0o500);
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, d_at), d_at);
        let p = pair_json(d_at, CODE);
        assert_eq!(p.code, 200, "{}", p.body);
        assert_eq!(
            std::fs::read(&mirror).unwrap(),
            mirror_before,
            "mirror stale"
        );
        assert_ne!(
            std::fs::read(&store).unwrap(),
            mirror_before,
            "store moved on"
        );
        eprintln!("D: mirror RO -> pair 200 from store, mirror stale (best effort)");
    }
    chmod(&mdir, 0o700);

    eprintln!(
        "real_admin_flags_stage: A-D pass in {} ms",
        t0.elapsed().as_millis()
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
