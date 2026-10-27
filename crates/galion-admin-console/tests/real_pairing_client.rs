// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1), wave-4 slot 319: the Rust pairing client
//! (`galion-admin-console client-*`) against
//!
//! 1. the real slot-303 Rust console (`galion-console-serve --dist ..
//!    --ui rust --admin-store ..`, release binary from
//!    swarm/console-board-api-parity 9c0f9d1a) on 127.0.0.1:21890, driven
//!    as the remote host `console.galion.app` (the real admin gate), and
//! 2. this crate's own `serve` (slot 307) on 127.0.0.1:21891 (compat).
//!
//! The console binary is not in this branch: point `CONSOLE_SERVE_BIN` at
//! it (see docs/real-tests/android-admin-pairing.md for the build line).
//! `CONSOLE_DIST_STATIC` overrides the dist fixture.
//!
//! Run: `GALION_REAL=1 CONSOLE_SERVE_BIN=.. cargo test --release -p galion-admin-console --test real_pairing_client -- --nocapture`
//! Ports 21890-21899 (slot 319). Box only; no live change.

use std::io::Write;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_galion-admin-console");
const CONSOLE: &str = "127.0.0.1:21890";
const OWN: &str = "127.0.0.1:21891";
const REMOTE: &str = "console.galion.app";
const CODE: &str = "SLOT319-PAIR-CLIENT";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_up(at: &str) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if TcpStream::connect(at).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn sha_hex(s: &str) -> String {
    galion_common::sha256::hex(s.as_bytes())
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

/// Run the client CLI; `stdin` feeds `--code-stdin`.
fn cli(args: &[&str], stdin: Option<&str>) -> Run {
    let mut c = Command::new(BIN);
    c.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut ch = c.spawn().expect("spawn client");
    if let Some(s) = stdin {
        ch.stdin.as_mut().unwrap().write_all(s.as_bytes()).unwrap();
    }
    drop(ch.stdin.take());
    let o: Output = ch.wait_with_output().unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into(),
        err: String::from_utf8_lossy(&o.stderr).into(),
    }
}

fn field<'a>(raw: &'a str, k: &str) -> &'a str {
    raw.lines()
        .find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
        .unwrap_or("")
}

/// Secrets from the creds file (read here only to prove they never leak).
fn secrets(dir: &Path) -> (String, String) {
    let raw = std::fs::read_to_string(dir.join("client.creds")).unwrap();
    (field(&raw, "token").into(), field(&raw, "device").into())
}

fn no_leak(r: &Run, s: &(String, String)) {
    for v in [&s.0, &s.1] {
        if !v.is_empty() {
            assert!(
                !r.out.contains(v.as_str()) && !r.err.contains(v.as_str()),
                "leak"
            );
        }
    }
}

/// `cmd --client-state ST --console CONSOLE --console-host REMOTE extra..`
fn a<'a>(cmd: &'a str, st: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec![
        cmd,
        "--client-state",
        st,
        "--console",
        CONSOLE,
        "--console-host",
        REMOTE,
    ];
    v.extend_from_slice(extra);
    v
}

fn dist_root() -> PathBuf {
    std::env::var("CONSOLE_DIST_STATIC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static")
        })
}

#[test]
fn real_pairing_client() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let serve = PathBuf::from(
        std::env::var("CONSOLE_SERVE_BIN")
            .expect("GALION_REAL=1 needs CONSOLE_SERVE_BIN (slot-303 galion-console-serve)"),
    );
    assert!(serve.is_file(), "{}", serve.display());
    let root = dist_root();
    assert!(
        root.join("favicon.svg").is_file(),
        "fixture {}",
        root.display()
    );
    let tmp = std::env::temp_dir().join(format!("real-319-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let secrets_dir = tmp.join("secrets");
    std::fs::create_dir_all(&secrets_dir).unwrap();
    let store = secrets_dir.join("admin-device.json");
    std::fs::write(&store, format!("{{\"codeHash\":\"{}\"}}\n", sha_hex(CODE))).unwrap();
    let cs = tmp.join("client");
    let cs2 = tmp.join("client2");
    let (c1, c2) = (cs.to_str().unwrap(), cs2.to_str().unwrap());
    let t0 = Instant::now();

    let child = Command::new(&serve)
        .args([
            "--dist",
            root.to_str().unwrap(),
            "--bind",
            CONSOLE,
            "--ui",
            "rust",
        ])
        .args(["--admin-store", store.to_str().unwrap(), "--quiet"])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn galion-console-serve");
    let _k = Kill(child);
    assert!(wait_up(CONSOLE), "console did not bind");
    let (c1s, c2s) = (c1, c2);

    // A. Before pairing: not paired, exit 3.
    let r = cli(&a("client-status", c1s, &[]), None);
    println!("A status: exit {} {}", r.code, r.out.trim());
    assert_eq!(r.code, 3);
    assert_eq!(r.out.trim(), "paired=false you=false local=none");

    // B. Refusals: code on argv impossible; non-loopback console; wrong code.
    let r = cli(&a("client-pair", c1s, &[]), None);
    assert_eq!(r.code, 2, "{}", r.err);
    let r = cli(
        &[
            "client-status",
            "--client-state",
            c1s,
            "--console",
            "192.0.2.1:80",
        ],
        None,
    );
    assert_eq!(r.code, 2, "non-loopback refused: {}", r.err);
    let r = cli(&a("client-pair", c1s, &["--code-stdin"]), Some("WRONG\n"));
    println!("B wrong code: exit {} {}", r.code, r.err.trim());
    assert_eq!(r.code, 3);
    assert!(!cs.join("client.creds").exists());

    // C. Right code (lower case, as typed on a phone) -> paired, device issued.
    let r = cli(
        &a("client-pair", c1s, &["--code-stdin", "--label", "box test"]),
        Some(&format!("{}\n", CODE.to_lowercase())),
    );
    println!("C pair: exit {} {}", r.code, r.out.trim());
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("device=issued"));
    assert!(
        r.out.contains("HttpOnly:true Secure:true SameSite:Lax"),
        "{}",
        r.out
    );
    let s1 = secrets(&cs);
    no_leak(&r, &s1);
    assert_eq!(s1.0.len(), 43);
    assert_eq!(s1.1.len(), 43);
    let mode = std::fs::metadata(cs.join("client.creds"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let raw = std::fs::read_to_string(&store).unwrap();
    assert!(
        !raw.contains(&s1.0) && !raw.contains(&s1.1),
        "store hashes only"
    );
    assert!(raw.contains(&sha_hex(&s1.0)) && raw.contains(&sha_hex(&s1.1)));

    // D. Status with the cookie and authenticated API/page reads.
    let r = cli(&a("client-status", c1s, &[]), None);
    println!("D status: exit {} {}", r.code, r.out.trim());
    assert_eq!(r.code, 0);
    assert!(r.out.starts_with("paired=true you=true local=node-"));
    let r = cli(&a("client-get", c1s, &["/api/v1/services"]), None);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("\"items\"") && r.err.contains("status 200"));
    let r = cli(&a("client-get", c1s, &["/services"]), None);
    assert!(
        r.out.contains("Edge gateway"),
        "Rust services page with the cookie"
    );
    no_leak(&r, &s1);
    // Same request without the cookie stays closed.
    let raw_get = galion_common::http::send(
        CONSOLE.parse().unwrap(),
        &galion_common::http::ClientRequest::get("/api/v1/services", REMOTE),
        Duration::from_secs(5),
        1 << 20,
    )
    .unwrap();
    assert_eq!(raw_get.status, 401);

    // E. Device resume: fresh token, device kept; after 4 more resumes the
    //    first token is rotated out server-side (MAX_TOKENS = 4).
    std::fs::create_dir_all(&cs2).unwrap();
    std::fs::set_permissions(&cs2, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::copy(cs.join("client.creds"), cs2.join("client.creds")).unwrap();
    let r = cli(&a("client-resume", c1s, &[]), None);
    println!("E resume: exit {} {}", r.code, r.out.trim());
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("device=kept"));
    let s2 = secrets(&cs);
    assert_ne!(s2.0, s1.0);
    assert_eq!(s2.1, s1.1);
    assert_eq!(
        cli(&a("client-status", c2s, &[]), None).code,
        0,
        "old token still valid"
    );
    for _ in 0..4 {
        assert_eq!(cli(&a("client-resume", c1s, &[]), None).code, 0);
    }
    let r = cli(&a("client-status", c2s, &[]), None);
    println!("E rotated-out old token: exit {} {}", r.code, r.out.trim());
    assert_eq!(r.code, 3);
    assert!(r.out.starts_with("paired=true you=false"));

    // F. Lockout: 5 wrong codes, then the right code is 429 (exit 5); the
    //    paired device still resumes under the lock.
    for i in 0..5 {
        let r = cli(&a("client-pair", c2s, &["--code-stdin"]), Some("NOPE\n"));
        assert_eq!(r.code, 3, "wrong {i}");
    }
    let r = cli(&a("client-pair", c2s, &["--code-stdin"]), Some(CODE));
    println!("F locked: exit {} {}", r.code, r.err.trim());
    assert_eq!(r.code, 5);
    let r = cli(&a("client-resume", c1s, &[]), None);
    assert_eq!(r.code, 0, "device resume under lock: {}", r.err);

    // G. F-313-1 as seen by the client: read-only store dir -> wrong codes
    //    never lock (401 each) and a right code is a 503 (exit 1).
    let before = std::fs::read(&store).unwrap();
    std::fs::write(&store, format!("{{\"codeHash\":\"{}\"}}\n", sha_hex(CODE))).unwrap();
    std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let ro_root = std::fs::write(secrets_dir.join(".probe"), b"x").is_ok();
    let mut g = Vec::new();
    if !ro_root {
        for _ in 0..6 {
            g.push(cli(&a("client-pair", c2s, &["--code-stdin"]), Some("NOPE\n")).code);
        }
        let r = cli(&a("client-pair", c2s, &["--code-stdin"]), Some(CODE));
        println!(
            "G ro-store: wrong x6 exits {g:?}; right code exit {} {}",
            r.code,
            r.err.trim()
        );
        assert_eq!(g, vec![3; 6], "F-313-1: no lock on a read-only store");
        assert_eq!(r.code, 1);
        assert!(r.err.contains("503"));
    }
    std::fs::set_permissions(&secrets_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&store, &before).unwrap();

    // H. Corrupt store: status fails closed (503, exit 1); file untouched.
    std::fs::write(&store, b"{not json").unwrap();
    let r = cli(&a("client-status", c1s, &[]), None);
    println!("H corrupt: exit {} {}", r.code, r.err.trim());
    assert_eq!(r.code, 1);
    assert!(r.err.contains("store unavailable"));
    assert_eq!(std::fs::read(&store).unwrap(), b"{not json");
    std::fs::write(&store, &before).unwrap();

    // I. Loose creds permissions refused; forget.
    std::fs::set_permissions(
        cs2.join("client.creds"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let r = cli(&a("client-status", c2s, &[]), None);
    assert_eq!(r.code, 1, "0644 creds refused: {}", r.err);
    assert!(r.err.contains("permissions"));
    let r = cli(&a("client-forget", c1s, &[]), None);
    assert_eq!((r.code, r.out.trim()), (0, "forgot local credentials"));
    assert_eq!(cli(&a("client-status", c1s, &[]), None).code, 3);

    // J. Compat: the 307 own server (code only, no device secret).
    let own_state = tmp.join("own");
    let origin = format!("http://{OWN}");
    let own = Command::new(BIN)
        .args([
            "serve",
            "--bind",
            OWN,
            "--state",
            own_state.to_str().unwrap(),
        ])
        .args(["--origin", &origin, "--insecure-cookie"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _k2 = Kill(own);
    assert!(wait_up(OWN));
    let code = cli(
        &["issue-code", "--state", own_state.to_str().unwrap()],
        None,
    );
    assert_eq!(code.code, 0);
    let cs3 = tmp.join("client3");
    let c3 = cs3.to_str().unwrap();
    let ja = |cmd: &str, extra: &[&str]| -> Run {
        let mut v = vec![
            cmd,
            "--client-state",
            c3,
            "--console",
            OWN,
            "--console-host",
            "127.0.0.1",
        ];
        v.extend_from_slice(&["--origin", &origin]);
        v.extend_from_slice(extra);
        cli(&v, None)
    };
    let mut v = vec!["client-pair", "--client-state", c3, "--console", OWN];
    v.extend_from_slice(&[
        "--console-host",
        "127.0.0.1",
        "--origin",
        &origin,
        "--code-stdin",
    ]);
    let r = cli(&v, Some(code.out.trim()));
    println!("J own pair: exit {} {}", r.code, r.out.trim());
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains("device=none") && r.out.contains("SameSite:Strict"));
    let s3 = secrets(&cs3);
    no_leak(&r, &s3);
    let r = ja("client-status", &[]);
    assert_eq!(r.code, 0);
    let devs = cli(&["devices", "--state", own_state.to_str().unwrap()], None);
    let node = r.out.trim().rsplit("local=").next().unwrap().to_string();
    assert!(
        devs.out.contains(&node),
        "same node id client/server: {node}"
    );
    let r = ja("client-get", &["/api/v1/admin/status"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(r.out.contains(&node));
    let r = ja("client-resume", &[]);
    assert_eq!(r.code, 2, "no device secret -> usage-level refusal");
    let r = cli(&v, Some(code.out.trim()));
    assert_eq!(r.code, 3, "single-use code refused the second time");
    let au = cli(
        &["audit-verify", "--state", own_state.to_str().unwrap()],
        None,
    );
    assert_eq!(au.code, 0);
    assert!(!std::fs::read_to_string(own_state.join("audit.jsonl"))
        .unwrap_or_default()
        .contains(&s3.0));

    println!(
        "real_pairing_client: all steps ok in {} ms (ro-store step {})",
        t0.elapsed().as_millis(),
        if ro_root {
            "skipped: running as root"
        } else {
            "ran"
        }
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
