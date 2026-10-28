// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (wave-4 slot 323, fixes F-313-1): the release
//! `galion-console-serve`, started with the argv of the staged drop-in
//! `deploy/console-rust-serve/console.service.d/60-admin-store.conf`
//! (only host paths/ports remapped into a temp root), attacked over real TCP:
//!
//! A. RW store, brute force: 5 wrong codes 401, then 429 for wrong AND right;
//!    `lockedUntil` persisted; store tampered back to a clean unlocked copy
//!    -> still 429 (memory lock); restart on the tampered-clean store -> right
//!    code pairs (documented: memory lock is per process); restart on a
//!    locked store -> 429 survives the restart.
//! B. RO store dir (F-313-1 shape: no ReadWritePaths): right and wrong codes
//!    answer byte-identical 503 (no oracle); after 5 attempts 429 for both;
//!    store bytes unchanged; the already-paired cookie still verifies.
//! C. Parallel brute: 8 threads x 6 wrong codes on a fresh store -> exactly 5
//!    answers 401, the other 43 are 429 (mutex-serialised counter).
//! D. Paired device secret resumes during a lock (TS behaviour) and resets it.
//! E. Corrupt store: 503 unreadable, file never overwritten.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_admin_lockout -- --nocapture`
//! Ports: 21930-21939 (box only). No VPS contact, no live change.

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
const CODE: &str = "SLOT323-LOCKOUT-CODE";
const OLD_TOKEN: &str = "slot323-already-paired-phone-token";
const DEVICE: &str = "slot323-paired-device-secret-0123456789";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug)]
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

fn pair(at: &str, field: &str, value: &str) -> Resp {
    let body = format!("{{\"{field}\":\"{value}\"}}");
    req(
        at,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Content-Type", "application/json")],
        body.as_bytes(),
    )
}

fn code(at: &str, c: &str) -> Resp {
    pair(at, "code", c)
}

fn has_cookie(r: &Resp) -> bool {
    r.head.lines().any(|l| {
        l.to_ascii_lowercase()
            .starts_with("set-cookie: galion_admin=")
    })
}

fn admin_get(at: &str, cookie: &str) -> Resp {
    req(
        at,
        "GET",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Cookie", cookie)],
        b"",
    )
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

fn chmod(p: &Path, m: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
}

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
                .replace("127.0.0.1:8091", "127.0.0.1:21938")
                .replace("127.0.0.1:8646", "127.0.0.1:21939")
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
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn galion-console-serve");
    let k = Kill(child);
    assert!(wait_up(at), "server did not bind {at}");
    k
}

fn store_json(fails: u32, locked_until: Option<u64>) -> String {
    let lock = locked_until
        .map(|u| format!(",\"lockedUntil\":{u}"))
        .unwrap_or_default();
    format!(
        "{{\"codeHash\":\"{}\",\"tokenHash\":\"{t}\",\"tokenHashes\":[\"{t}\"],\"deviceHash\":\"{}\",\"fails\":{fails}{lock}}}\n",
        sha_hex(CODE),
        sha_hex(DEVICE),
        t = sha_hex(OLD_TOKEN)
    )
}

#[test]
fn real_admin_lockout() {
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
    let tmp = std::env::temp_dir().join(format!("real-admin-323-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
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
    let reset = |fails: u32, lock: Option<u64>| {
        std::fs::write(&store, store_json(fails, lock)).unwrap();
        chmod(&store, 0o600);
    };
    let staged = units::exec_argv(&[LIVE, DROP]);
    let old = format!("galion_admin={OLD_TOKEN}");
    let t0 = Instant::now();

    // ---- A: RW store brute force + tamper + restart ----
    let a_at = "127.0.0.1:21930";
    reset(0, None);
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, a_at), a_at);
        let wrong: Vec<u16> = (0..5)
            .map(|i| code(a_at, &format!("WRONG{i}")).code)
            .collect();
        assert_eq!(wrong, vec![401; 5], "A: 5 wrong codes");
        let w6 = code(a_at, "WRONG5");
        let r = code(a_at, CODE);
        assert_eq!(
            (w6.code, r.code),
            (429, 429),
            "A: locked for wrong and right"
        );
        assert_eq!(w6.body, r.body, "A: 429 body identical");
        assert!(!has_cookie(&r));
        let raw = std::fs::read_to_string(&store).unwrap();
        assert!(
            raw.contains("\"lockedUntil\":") && raw.contains("\"fails\":5"),
            "{raw}"
        );
        // Tamper: swap in a clean unlocked copy while the process runs.
        reset(0, None);
        assert_eq!(
            code(a_at, CODE).code,
            429,
            "A: tampered-clean store stays locked"
        );
        reset(0, Some(1));
        assert_eq!(
            code(a_at, CODE).code,
            429,
            "A: expired lockedUntil in store"
        );
        eprintln!("A: 5x401 -> 429 (wrong+right, same body), lockedUntil persisted, tampered-clean store still 429");
    }
    // Restart on a locked store: the persisted lock survives.
    {
        let (fails, until) = (
            5,
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64)
                + 600_000,
        );
        reset(fails, Some(until));
        let _k = spawn(&remap(&staged, &exe, &tmp, a_at), a_at);
        assert_eq!(code(a_at, CODE).code, 429, "A: lock survives restart");
    }
    // Restart on a clean store: right code pairs, counters reset.
    {
        reset(0, None);
        let _k = spawn(&remap(&staged, &exe, &tmp, a_at), a_at);
        assert_eq!(code(a_at, "WRONGX").code, 401);
        let r = code(a_at, &CODE.to_lowercase());
        assert_eq!(r.code, 200, "{}", r.body);
        assert!(has_cookie(&r));
        let raw = std::fs::read_to_string(&store).unwrap();
        assert!(
            raw.contains("\"fails\":0") && !raw.contains("lockedUntil"),
            "{raw}"
        );
        assert!(!raw.contains(CODE) && !raw.contains(DEVICE), "hashes only");
        eprintln!(
            "A: restart on locked store 429; restart on clean store 401 then right 200, fails 0"
        );
    }

    // ---- B: RO store dir (no ReadWritePaths) ----
    let b_at = "127.0.0.1:21931";
    reset(0, None);
    let before = std::fs::read(&store).unwrap();
    chmod(&sdir, 0o500);
    chmod(&mdir, 0o500);
    let probe = sdir.join(".probe");
    let ro = std::fs::write(&probe, b"x").is_err();
    let _ = std::fs::remove_file(&probe);
    assert!(ro, "run as non-root: dir modes must be enforced");
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, b_at), b_at);
        assert!(
            admin_get(b_at, &old).body.contains(r#""you":true"#),
            "B: old cookie ok"
        );
        let right = code(b_at, CODE);
        let wrong = code(b_at, "WRONG0");
        assert_eq!(
            (right.code, right.body.as_str()),
            (503, r#"{"error":"store not writable"}"#)
        );
        assert_eq!(
            (wrong.code, &wrong.body),
            (right.code, &right.body),
            "B: no oracle"
        );
        assert!(!has_cookie(&right));
        let more: Vec<u16> = (1..4)
            .map(|i| code(b_at, &format!("WRONG{i}")).code)
            .collect();
        assert_eq!(more, vec![503; 3]);
        let lw = code(b_at, "WRONG4");
        let lr = code(b_at, CODE);
        assert_eq!((lw.code, lr.code), (429, 429), "B: RO store now locks");
        assert_eq!(lw.body, lr.body);
        let dev = pair(b_at, "device", DEVICE);
        assert_eq!(dev.code, 503, "B: device resume cannot store a token");
        assert_eq!(std::fs::read(&store).unwrap(), before, "B: store unchanged");
        // Dir becomes writable again (ReadWritePaths fixed): memory lock holds.
        chmod(&sdir, 0o700);
        chmod(&mdir, 0o700);
        assert_eq!(
            code(b_at, CODE).code,
            429,
            "B: lock held after dir becomes RW"
        );
        eprintln!("B: RO -> right/wrong identical 503 x5, then 429 for both; device 503; store unchanged; RW again still 429");
    }

    // ---- C: parallel brute force ----
    let c_at = "127.0.0.1:21932";
    reset(0, None);
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, c_at), c_at);
        let hs: Vec<_> = (0..8)
            .map(|t| {
                std::thread::spawn(move || {
                    (0..6)
                        .map(|i| code(c_at, &format!("T{t}W{i}")).code)
                        .collect::<Vec<u16>>()
                })
            })
            .collect();
        let all: Vec<u16> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
        let n401 = all.iter().filter(|c| **c == 401).count();
        let n429 = all.iter().filter(|c| **c == 429).count();
        assert_eq!((n401, n429, all.len()), (5, 43, 48), "C: {all:?}");
        assert_eq!(code(c_at, CODE).code, 429);
        eprintln!("C: 8x6 parallel wrong -> 401 x{n401}, 429 x{n429}; right code 429");
    }

    // ---- D: device resume during lock (TS behaviour) ----
    let d_at = "127.0.0.1:21933";
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, d_at), d_at);
        // store from C is locked
        assert_eq!(code(d_at, CODE).code, 429);
        let r = pair(d_at, "device", DEVICE);
        assert_eq!(r.code, 200, "{}", r.body);
        assert!(has_cookie(&r) && r.body.contains(r#""device":"""#));
        let raw = std::fs::read_to_string(&store).unwrap();
        assert!(
            raw.contains("\"fails\":0") && !raw.contains("lockedUntil"),
            "{raw}"
        );
        assert_eq!(
            code(d_at, CODE).code,
            200,
            "D: lock cleared by device resume"
        );
        eprintln!("D: locked 429 -> device resume 200 (no secret echoed) -> right code 200");
    }

    // ---- E: corrupt store ----
    let e_at = "127.0.0.1:21934";
    std::fs::write(&store, b"{corrupt").unwrap();
    {
        let _k = spawn(&remap(&staged, &exe, &tmp, e_at), e_at);
        let r = code(e_at, CODE);
        let w = code(e_at, "WRONG");
        assert_eq!(
            (r.code, r.body.as_str()),
            (503, r#"{"error":"admin store unreadable"}"#)
        );
        assert_eq!((w.code, &w.body), (r.code, &r.body));
        assert_eq!(std::fs::read(&store).unwrap(), b"{corrupt");
        eprintln!("E: corrupt store -> 503 unreadable for right+wrong, file untouched");
    }

    let leftovers: Vec<_> = std::fs::read_dir(&sdir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "no temp files left");
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!(
        "real_admin_lockout: A-E pass in {} ms",
        t0.elapsed().as_millis()
    );
}
