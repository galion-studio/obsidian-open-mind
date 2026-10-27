// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test: board API parity for the Rust console (wave-4 slot 303).
//!
//! Starts the release `galion-console-serve` with `--dist` (local copy of the
//! live `console-dist/static`), `--ui rust` and a throwaway admin store, then
//! drives it over real TCP the way cloudflared and a phone would:
//! - the 11 board catalog routes + markers POST + boot POST on loopback;
//! - the remote console host (`console.galion.app`): 401 until paired, the
//!   Rust pair page instead of the 503 "Console API is not answering",
//!   wrong code 401, `/_ui/pair` form pairing (admin + device cookies),
//!   device resume, then pages and API with the cookie;
//! - lockout (5 wrong codes -> 429), oversize (413) and chunked (411)
//!   bodies, foreign host 401, store holds hashes only.
//!
//! Optional `CONSOLE_BOARD_LIVE=1`: prints live VPS :8080 codes through an
//! `ssh -L` forward on 21731 next to the Rust codes (no live assertions).
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_board_api_parity -- --nocapture`
//! Ports: 21730 (binary), 21731 (optional live forward). No live change.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:21730";
const LIVE_FWD: &str = "127.0.0.1:21731";
const REMOTE: &str = "console.galion.app";
const CODE: &str = "SLOT303-REAL-TEST";

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

impl Resp {
    fn cookies(&self) -> Vec<String> {
        self.head
            .lines()
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case("set-cookie")
                    .then(|| v.trim().to_string())
            })
            .collect()
    }
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
    if !body.is_empty() || method == "POST" {
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

fn dist_root() -> PathBuf {
    std::env::var("CONSOLE_DIST_STATIC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/workspace/swarm/artifacts/console-rust-serve-impl/console-dist-static")
        })
}

fn sha_hex(s: &str) -> String {
    console_static::sha256::hex(s.as_bytes())
}

fn pair_of(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap_or("").to_string()
}

#[test]
fn real_board_api_parity() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let root = dist_root();
    assert!(
        root.join("favicon.svg").is_file(),
        "fixture {}",
        root.display()
    );
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_galion-console-serve"));
    let tmp = std::env::temp_dir().join(format!("real-board-303-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let store = tmp.join("secrets/admin-device.json");
    let mirror = tmp.join("persist/admin-device.json");
    std::fs::create_dir_all(store.parent().unwrap()).unwrap();
    std::fs::write(&store, format!("{{\"codeHash\":\"{}\"}}\n", sha_hex(CODE))).unwrap();

    let child = Command::new(&exe)
        .args([
            "--dist",
            root.to_str().unwrap(),
            "--bind",
            BIND,
            "--ui",
            "rust",
        ])
        .args(["--admin-store", store.to_str().unwrap()])
        .args(["--admin-mirror", mirror.to_str().unwrap()])
        .arg("--quiet")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn galion-console-serve");
    let _k = Kill(child);
    assert!(wait_up(BIND), "server did not bind {BIND}");
    let t0 = Instant::now();

    // 1. Loopback catalog routes (cloudflared-free local probes, guardian).
    let lo = "127.0.0.1";
    let shapes: &[(&str, &[&str])] = &[
        ("/api/v1/services", &["count", "active", "chill", "items"]),
        ("/api/v1/domains", &["count", "items"]),
        (
            "/api/v1/repos",
            &["owner", "public", "privateVisible", "items"],
        ),
        ("/api/v1/catalog", &["version", "live", "held", "items"]),
        (
            "/api/v1/ledger",
            &[
                "version",
                "project",
                "principles",
                "domains",
                "services",
                "fridge",
                "repos",
                "apis",
                "counts",
            ],
        ),
        ("/api/v1/docs", &["openapi", "info", "servers", "paths"]),
        (
            "/api/v1/protocol",
            &[
                "name",
                "version",
                "transport",
                "auth",
                "zeroai",
                "models",
                "channels",
                "envelope",
                "routes",
            ],
        ),
        ("/api/v1/protocol/snapshot", &["v", "ts", "frames"]),
        ("/api/v1/markers", &["count", "cap", "items"]),
        (
            "/api/v1/health",
            &[
                "status",
                "service",
                "version",
                "implementation",
                "mode",
                "note",
            ],
        ),
        (
            "/api/v1/ping",
            &["ok", "service", "version", "implementation"],
        ),
    ];
    for (path, keys) in shapes {
        let r = get(BIND, path, lo, None);
        assert_eq!(r.code, 200, "{path}: {}", r.body);
        assert!(
            r.head
                .to_ascii_lowercase()
                .contains("content-type: application/json"),
            "{path}"
        );
        let v = console_ui::json::parse(r.body.as_bytes()).expect(path);
        let got: Vec<String> = console_ui::json::keys(&v).into_keys().collect();
        let mut want: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(got, want, "{path} key set");
        eprintln!("rust 200 {path} {} B", r.body.len());
    }
    let sv = get(BIND, "/api/v1/services", lo, None);
    assert!(sv.body.contains(r#""count":16"#), "{}", sv.body);
    assert!(sv.body.contains(r#""zone":"active""#));
    let pm = req(
        BIND,
        "POST",
        "/api/v1/markers",
        &[("Host", lo), ("Content-Type", "application/json")],
        br#"{"zone":"active","kind":"origin","source":"slot303","note":"real test"}"#,
    );
    assert_eq!(pm.code, 201, "{}", pm.body);
    assert!(pm.body.contains(r#""seq":42"#), "{}", pm.body);
    assert!(get(BIND, "/api/v1/markers", lo, None)
        .body
        .contains("slot303"));
    let boot = req(BIND, "POST", "/api/v1/boot", &[("Host", lo)], b"{}");
    assert_eq!(boot.code, 200);
    assert!(
        boot.body.contains(r#""started":["console""#),
        "{}",
        boot.body
    );
    // Rust UI pages that read these APIs now render real rows.
    let page = get(BIND, "/services", lo, None);
    assert_eq!(page.code, 200);
    assert!(page.body.contains("Edge gateway"), "services page rows");
    let page = get(BIND, "/domains", lo, None);
    assert!(page.body.contains("merlinelement.com"), "domains page rows");
    let page = get(BIND, "/markers", lo, None);
    assert!(page.body.contains("slot303"), "markers page ring");

    // 2. Foreign host + oversize/chunked bodies.
    assert_eq!(
        get(BIND, "/api/v1/services", "evil.example", None).code,
        401
    );
    assert_eq!(get(BIND, "/api/v1/admin", "evil.example", None).code, 401);
    let big = vec![b'x'; 20 * 1024];
    let r = req(BIND, "POST", "/api/v1/markers", &[("Host", lo)], &big);
    assert_eq!(r.code, 413);
    let r = req(
        BIND,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Transfer-Encoding", "chunked")],
        b"",
    );
    assert_eq!(r.code, 411);

    // 3. Remote console host before pairing: API 401, pages show the pair
    //    form (not the 503 the live :8080 shows today).
    for p in ["/api/v1/health", "/api/v1/services", "/api/v1/markers"] {
        assert_eq!(get(BIND, p, REMOTE, None).code, 401, "{p}");
    }
    let a = get(BIND, "/api/v1/admin", REMOTE, None);
    assert_eq!(a.code, 200);
    assert_eq!(a.body, r#"{"ok":true,"paired":false,"you":false}"#);
    let c = get(BIND, "/console", REMOTE, None);
    assert_eq!(c.code, 200, "pair page, not 503");
    assert!(c.body.contains("/_ui/pair"), "pair form");
    assert!(!c.body.contains("not answering"));

    // 4. Wrong code, then the zero-JS pair form with the right code.
    let w = req(
        BIND,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE), ("Content-Type", "application/json")],
        br#"{"code":"WRONG"}"#,
    );
    assert_eq!(
        (w.code, w.body.as_str()),
        (401, r#"{"error":"wrong code"}"#)
    );
    let form = format!("code={}", CODE.to_lowercase());
    let p = req(
        BIND,
        "POST",
        "/_ui/pair?back=/services",
        &[
            ("Host", REMOTE),
            ("Origin", "https://console.galion.app"),
            ("Content-Type", "application/x-www-form-urlencoded"),
        ],
        form.as_bytes(),
    );
    assert_eq!(p.code, 303, "{}", p.head);
    let cookies = p.cookies();
    let admin = cookies
        .iter()
        .find(|c| c.starts_with("galion_admin="))
        .expect("admin cookie");
    assert!(
        admin.contains("HttpOnly") && admin.contains("Secure"),
        "{admin}"
    );
    let device = cookies
        .iter()
        .find(|c| c.starts_with("galion_device="))
        .expect("device cookie");
    let admin_pair = pair_of(admin);
    let dev_pair = pair_of(device);
    let a = get(BIND, "/api/v1/admin", REMOTE, Some(&admin_pair));
    assert_eq!(a.body, r#"{"ok":true,"paired":true,"you":true}"#);
    assert_eq!(
        get(BIND, "/api/v1/services", REMOTE, Some(&admin_pair)).code,
        200
    );
    let s = get(BIND, "/services", REMOTE, Some(&admin_pair));
    assert_eq!(s.code, 200);
    assert!(s.body.contains("Edge gateway"));
    assert_eq!(get(BIND, "/console", REMOTE, Some(&admin_pair)).code, 200);

    // 5. Device resume (new phone session, only the device cookie).
    let r = get(BIND, "/status", REMOTE, Some(&dev_pair));
    assert_eq!(r.code, 303, "device resume redirect: {}", r.head);
    let resumed = r
        .cookies()
        .into_iter()
        .find(|c| c.starts_with("galion_admin="))
        .expect("resumed admin cookie");
    assert_ne!(pair_of(&resumed), admin_pair);
    let ok = get(BIND, "/status", REMOTE, Some(&pair_of(&resumed)));
    assert_eq!(ok.code, 200);

    // 6. Store: hashes only, mirror identical, 0600.
    let raw = std::fs::read_to_string(&store).unwrap();
    let tok = admin_pair.trim_start_matches("galion_admin=");
    let dev = dev_pair.trim_start_matches("galion_device=");
    assert!(!raw.contains(tok) && !raw.contains(dev) && !raw.contains(CODE));
    assert!(raw.contains(&sha_hex(tok)));
    assert_eq!(std::fs::read_to_string(&mirror).unwrap(), raw);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&store).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // 7. Lockout: 5 wrong codes -> 429 even for the right code; the paired
    //    device still resumes.
    for _ in 0..5 {
        let r = req(
            BIND,
            "POST",
            "/api/v1/admin",
            &[("Host", REMOTE)],
            br#"{"code":"no"}"#,
        );
        assert_eq!(r.code, 401);
    }
    let body = format!(r#"{{"code":"{CODE}"}}"#);
    let r = req(
        BIND,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE)],
        body.as_bytes(),
    );
    assert_eq!(r.code, 429, "{}", r.body);
    let body = format!(r#"{{"device":"{dev}"}}"#);
    let r = req(
        BIND,
        "POST",
        "/api/v1/admin",
        &[("Host", REMOTE)],
        body.as_bytes(),
    );
    assert_eq!(r.code, 200, "device bypasses lock");
    eprintln!("rust real flow ok in {:?}", t0.elapsed());

    // 8. Optional live comparison (prints only).
    if std::env::var("CONSOLE_BOARD_LIVE").ok().as_deref() == Some("1") {
        let mut ssh = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ExitOnForwardFailure=yes",
                "-N",
                "-L",
            ])
            .arg(format!("{LIVE_FWD}:127.0.0.1:8080"))
            .arg("slavik-vps")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("ssh forward");
        let up = {
            let start = Instant::now();
            let mut ok = false;
            while start.elapsed() < Duration::from_secs(15) {
                if get(LIVE_FWD, "/api/v1/health", lo, None).code == 200 {
                    ok = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            ok
        };
        if up {
            eprintln!("path | host | live :8080 | rust");
            for (path, host) in [
                ("/api/v1/health", lo),
                ("/api/v1/services", lo),
                ("/api/v1/domains", lo),
                ("/api/v1/repos", lo),
                ("/api/v1/catalog", lo),
                ("/api/v1/ledger", lo),
                ("/api/v1/docs", lo),
                ("/api/v1/protocol", lo),
                ("/api/v1/protocol/snapshot", lo),
                ("/api/v1/markers", lo),
                ("/api/v1/admin", REMOTE),
                ("/api/v1/services", REMOTE),
                ("/console", REMOTE),
                ("/services", lo),
            ] {
                let l = get(LIVE_FWD, path, host, None).code;
                let r = get(BIND, path, host, None).code;
                eprintln!("{path} | {host} | {l} | {r}");
            }
        } else {
            eprintln!("live forward not up; skipped live table");
        }
        let _ = ssh.kill();
        let _ = ssh.wait();
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
