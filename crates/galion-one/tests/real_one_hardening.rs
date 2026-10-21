// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Real-life test for swarm/one-hardening (slot 29, ports 19290-19299).
// Starts the real galion-one binary twice with fixture homes and checks:
//
//   A  127.0.0.1:19290 (mouth 19291), TZ=Europe/Zagreb, fake nvidia-smi,
//      console probe moved to a stand-in on 19292 (path /health), fake ear
//      on 19293, mind 19294 (nothing listens), device store with code
//      FIXTURE42.
//   B  127.0.0.1:19295 (mouth 19296), TZ=UTC, nvidia-smi absent from PATH,
//      ONE_CONSOLE=off, ONE_PDFTOTEXT=off, empty secrets dir, no store.
//
// A process named `cloudflared tunnel run` (a renamed `sleep`) is started so
// the /proc scan that replaced `ps` has something to find.
//
// Skipped unless GALION_REAL=1. GALION_ONE_BIN picks a release build.
//   GALION_REAL=1 cargo test -p galion-one --test real_one_hardening -- --nocapture

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const A: &str = "127.0.0.1:19290";
const A_MOUTH: &str = "127.0.0.1:19291";
const CONSOLE: &str = "127.0.0.1:19292";
const EAR: &str = "127.0.0.1:19293";
const MIND: &str = "127.0.0.1:19294";
const B: &str = "127.0.0.1:19295";
const B_MOUTH: &str = "127.0.0.1:19296";
// sha256("FIXTURE42"), the fixture pairing code (not a real secret).
const CODE_HASH: &str = "3315254ec1e783ba6819b1c99c44274501d057455eff3123d3da84d95b944e36";

struct Kill(Vec<Child>, PathBuf);
impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn request(
    addr: &str,
    method: &str,
    path: &str,
    extra: &str,
    body: &[u8],
) -> Option<(u16, String, String)> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok()?;
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    s.write_all(body).ok()?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let code = text.split_whitespace().nth(1)?.parse().ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    Some((code, head.to_string(), body.to_string()))
}

fn get(addr: &str, path: &str, extra: &str) -> (u16, String) {
    let (c, _, b) =
        request(addr, "GET", path, extra, b"").unwrap_or_else(|| panic!("GET {addr}{path}"));
    (c, b)
}

/// Tiny HTTP stand-in: answers every request with `body`, counts hits.
fn stand_in(addr: &str, body: &'static str) -> Arc<AtomicUsize> {
    let listener = TcpListener::bind(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            h.fetch_add(1, Ordering::Relaxed);
            let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
            let mut buf = vec![0u8; 1 << 16];
            let mut got = Vec::new();
            // Read headers plus any body that arrives quickly.
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&got[..end]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    hits
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(secs), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn field<'a>(json: &'a str, key: &str) -> &'a str {
    let pat = format!("\"{key}\":\"");
    let i = json
        .find(&pat)
        .unwrap_or_else(|| panic!("no {key} in {json}"))
        + pat.len();
    &json[i..i + json[i..].find('"').unwrap()]
}

/// `at` must be Europe/Zagreb RFC 3339 with an offset, within 30 s of now.
fn assert_zagreb(at: &str) {
    assert!(!at.ends_with('Z'), "{at}");
    assert!(at.ends_with("+02:00") || at.ends_with("+01:00"), "{at}");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let ok = (-30..=30).any(|d| zagreb_time::from_unix(now + d) == at);
    assert!(
        ok,
        "{at} is not Zagreb now ({})",
        zagreb_time::from_unix(now)
    );
}

fn base_cmd(bin: &str, home: &Path, work: &Path) -> Command {
    let mut c = Command::new(bin);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("ONE_NO_WARM", "1")
        .env("GALION_HOME", home)
        .env("ONE_HERMES_DIR", work.join("hermes"))
        .env("ONE_HERMES_ENV", work.join("hermes.env"))
        .env("GALION_MIND", MIND)
        .env("GALION_EAR", EAR)
        .env("GALION_EYE", MIND)
        .env("GALION_MEMORY", MIND)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

#[test]
fn real_one_hardening_on_slot_29_ports() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }
    let bin = std::env::var("GALION_ONE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-one").to_string());
    let work = std::env::temp_dir().join(format!("one-hardening-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    for d in [
        "a/one",
        "b/one",
        "a/guardian",
        "hermes",
        "secrets-a",
        "secrets-b",
        "bin",
    ] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(work.join("hermes.env"), "").unwrap();
    let store_a = work.join("secrets-a/admin-device.json");
    std::fs::write(
        &store_a,
        format!(r#"{{"codeHash":"{CODE_HASH}","tokenHash":"","tokenHashes":[],"deviceHash":"","fails":0,"lockedUntil":0}}"#),
    )
    .unwrap();
    std::fs::set_permissions(&store_a, std::fs::Permissions::from_mode(0o600)).unwrap();
    let smi = work.join("bin/nvidia-smi");
    std::fs::write(&smi, "#!/bin/sh\necho 'Fake GPU 19290, 512, 8192, 42'\n").unwrap();
    std::fs::set_permissions(&smi, std::fs::Permissions::from_mode(0o700)).unwrap();

    let console_hits = stand_in(CONSOLE, "ok\n");
    let ear_hits = stand_in(EAR, r#"{"text":"hello from the fake ear"}"#);

    // A renamed sleep, so /proc shows `cloudflared tunnel run`.
    let decoy = Command::new("/bin/bash")
        .args(["-c", "exec -a 'cloudflared tunnel run' sleep 120"])
        .spawn()
        .expect("decoy");
    let a = base_cmd(&bin, &work.join("a"), &work)
        .env("TZ", "Europe/Zagreb")
        .env("ONE_BIND", A)
        .env("ONE_MOUTH_BIND", A_MOUTH)
        .env("ONE_DEVICE_STORE", &store_a)
        .env("ONE_NVIDIA_SMI", &smi)
        .env("ONE_CONSOLE", CONSOLE)
        .env("ONE_CONSOLE_PATH", "/health")
        .spawn()
        .expect("start A");
    let b = base_cmd(&bin, &work.join("b"), &work)
        .env("TZ", "UTC")
        .env("ONE_BIND", B)
        .env("ONE_MOUTH_BIND", B_MOUTH)
        .env("ONE_DEVICE_STORE", work.join("secrets-b/admin-device.json"))
        .env("ONE_CONSOLE", "off")
        .env("ONE_PDFTOTEXT", "off")
        .spawn()
        .expect("start B");
    let kids = vec![decoy, a, b];
    let _guard = Kill(kids, work.clone());

    for addr in [A, B] {
        wait_for("health", 10, || {
            request(addr, "GET", "/health", "", b"").is_some()
        });
    }

    // (3) A failed pairing with no store creates nothing in the secrets dir.
    for code in ["WRONG", "FIXTURE42", ""] {
        let (c, _, body) = request(
            B,
            "POST",
            "/pair",
            "",
            format!(r#"{{"code":"{code}"}}"#).as_bytes(),
        )
        .unwrap();
        assert_eq!((c, body.as_str()), (200, r#"{"error":"no"}"#));
    }
    assert_eq!(
        std::fs::read_dir(work.join("secrets-b")).unwrap().count(),
        0
    );
    eprintln!("B: 3 failed /pair, secrets dir still empty");

    // Pairing on A: wrong code, then the right one sets a cookie.
    let (_, _, body) = request(A, "POST", "/pair", "", br#"{"code":"WRONG"}"#).unwrap();
    assert_eq!(body, r#"{"error":"no"}"#);
    let (c, head, _) = request(A, "POST", "/pair", "", br#"{"code":"fixture42"}"#).unwrap();
    assert_eq!(c, 200);
    let token = head
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: galion_admin="))
        .and_then(|v| v.split(';').next())
        .expect("cookie")
        .to_string();
    let cookie = format!("Cookie: galion_admin={token}\r\n");
    assert_eq!(get(A, "/me", &cookie).1, r#"{"you":true}"#);
    eprintln!("A: wrong code -> no, right code -> cookie, /me you:true");

    // Let sample() run past tick 5 (GPU, units every 5th tick).
    wait_for("gpu on A", 15, || {
        get(A, "/api/v1/awake", "").1.contains(r#""gpu":1"#)
    });
    let awake_a = get(A, "/api/v1/awake", "").1;
    eprintln!("A /api/v1/awake: {awake_a}");
    wait_for("console pulse", 10, || {
        get(A, "/api/v1/awake", "")
            .1
            .contains(r#"{"id":"console","ok":true"#)
    });
    assert!(console_hits.load(Ordering::Relaxed) > 0);

    // (1) analytics `at`: Zagreb with offset, on A (TZ=Europe/Zagreb) too.
    let (c, full) = get(A, "/api/v1/analytics", &cookie);
    assert_eq!(c, 200);
    let at = field(&full, "at").to_string();
    assert_zagreb(&at);
    eprintln!("A analytics at: {at}");
    assert!(full.contains(r#""name":"Fake GPU 19290""#), "{full}");
    assert!(
        full.contains(r#"{"id":"cloudflare","ok":true,"note":"tunnel"}"#),
        "{full}"
    );
    let infra = get(A, "/api/v1/analytics/infrastructure", &cookie).1;
    eprintln!("A infrastructure: {infra}");
    let services = get(A, "/api/v1/analytics/services", &cookie).1;
    eprintln!("A analytics/services: {services}");
    assert!(
        services.contains(r#"{"id":"console","port":19292,"ok":true"#),
        "{services}"
    );
    let (c, v1) = get(A, "/api/v1/services", "");
    assert_eq!(c, 200);
    eprintln!("A /api/v1/services: {v1}");

    // Voice turn: fake ear hears, no mind on 19294, so the turn is kept with
    // a Zagreb `at` and the unix field.
    let _ = request(A, "POST", "/reply", &cookie, b"RIFF0000WAVEfake").unwrap();
    assert!(ear_hits.load(Ordering::Relaxed) > 0);
    let session = std::fs::read_to_string(work.join("a/one/session.jsonl")).unwrap();
    let line = session.lines().last().unwrap();
    eprintln!("A session.jsonl: {line}");
    assert!(line.contains(r#""unix":"#), "{line}");
    assert!(
        line.contains(r#""heard":"hello from the fake ear""#),
        "{line}"
    );
    assert_zagreb(field(line, "at"));
    let (_, s) = get(A, "/session", &cookie);
    assert!(s.contains("hello from the fake ear"), "{s}");

    // B: nvidia-smi absent gives no GPU (and no hang); no console pulse.
    wait_for("B sampled", 15, || {
        get(B, "/api/v1/awake", "")
            .1
            .contains(r#"{"id":"one","ok":true"#)
    });
    let (_, awake_b) = get(B, "/api/v1/awake", "");
    eprintln!("B /api/v1/awake: {awake_b}");
    assert!(awake_b.contains(r#""gpu":0"#), "{awake_b}");
    assert!(!awake_b.contains(r#""id":"console""#), "{awake_b}");
    assert_eq!(get(B, "/api/v1/analytics", "").0, 401);
    assert_eq!(get(B, "/health", "").1, "ok\n");
    assert_eq!(
        std::fs::read_dir(work.join("secrets-b")).unwrap().count(),
        0
    );
}
