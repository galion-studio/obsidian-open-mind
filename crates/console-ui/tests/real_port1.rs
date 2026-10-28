// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the console-port-1 rooms (`/analytics`, `/arena`,
//! `/bank`, `/cache`) against the live VPS services, read-only.
//!
//! ```text
//! GALION_REAL=1 PORT1_VPS=user@host \
//!   cargo test -p console-ui --release --test real_port1 -- --ignored --nocapture
//! ```
//!
//! 1. `cargo build --release -p console-static` (same target dir);
//! 2. opens ONE read-only `ssh -N` forward: 127.0.0.1:20059 -> VPS 8080
//!    (console API), 20058 -> 8646 (galion-desk), 20057 -> 8670 (nerve);
//!    nothing runs or is written on the VPS;
//! 3. starts `galion-console-static --ui rust --desk ...` on 127.0.0.1:20050
//!    (slot 135 range 20050-20059);
//! 4. GETs the four pages, reads the same live payloads directly, and asserts
//!    each page shows what the live service says;
//! 5. records the nerve `/state` size and keys (the new Rust infrastructure,
//!    observed while the test runs) and the live TS console's status for the
//!    same four paths.
//!
//! GET only. Never POSTs (`/api/arena` POST would breed, a paid run), never
//! opens `/api/v1/markers/stream`, never calls `/api/v1/ping`.

use console_ui::client::{self, Forward};
use console_ui::json::{self, Value};
use console_ui::pages;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::console_routes as routes;
use galion_realtest::ports::Block;

/// Slot 375 (real-ports-batch-2): port block movable by `PORT1_REAL_PORT_BASE`; unset keeps
/// the old 20050 block.
static PORTS: Block = Block::new("PORT1_REAL_PORT_BASE", 20050, 10);
fn desk_at() -> &'static str {
    PORTS.addr(8)
}
fn front_at() -> &'static str {
    PORTS.addr(0)
}
fn nerve_at() -> &'static str {
    PORTS.addr(7)
}
fn up_at() -> &'static str {
    PORTS.addr(9)
}
/// Slot 384: galion-one :8091, the Rust owner of `/api/v1/analytics`.
fn one_at() -> &'static str {
    PORTS.addr(6)
}

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"))
}

fn now() -> String {
    Command::new("date")
        .arg("-Iseconds")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn get(a: SocketAddr, path: &str) -> Option<(u16, Vec<u8>, Duration)> {
    assert!(
        !path.contains("markers/stream") && !path.contains("/ping"),
        "{path}"
    );
    let fwd = Forward {
        host: "127.0.0.1".into(),
        ..Forward::default()
    };
    let t = Instant::now();
    let r = client::call(a, "GET", path, &fwd, None).ok()?;
    Some((r.status, r.body, t.elapsed()))
}

fn page(front: SocketAddr, path: &str) -> String {
    let (c, b, t) = get(front, path).unwrap_or_else(|| panic!("front GET {path}"));
    let html = String::from_utf8_lossy(&b).into_owned();
    eprintln!(
        "real: GET {path} -> {c} {} B {:.3} s, <script>: {}, style=: {}",
        b.len(),
        t.as_secs_f64(),
        html.matches("<script").count(),
        html.matches("style=\"").count()
    );
    assert_eq!(c, 200, "{path}");
    assert!(!html.contains("<script"), "{path}");
    assert!(!html.contains("style=\""), "{path}");
    assert!(!html.contains("Not in Rust yet"), "{path}");
    html
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(secs), "{what} timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn has(html: &str, text: &str, what: &str) {
    let e = console_ui::html::esc(text);
    assert!(html.contains(&e), "{what}: missing {text:?}");
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, PORT1_VPS and ports 20050-20059"]
fn port1_rooms_match_the_live_services() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1");
        return;
    }
    eprintln!("real: start {}", now());
    let front: SocketAddr = front_at().parse().unwrap();
    let up: SocketAddr = up_at().parse().unwrap();
    let desk: SocketAddr = desk_at().parse().unwrap();
    let nerve: SocketAddr = nerve_at().parse().unwrap();

    // 1. Release build.
    let st = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .env("CARGO_INCREMENTAL", "0")
        .args([
            "build",
            "--release",
            "-j",
            "2",
            "-q",
            "-p",
            "console-static",
        ])
        .status()
        .expect("cargo build");
    assert!(st.success(), "build failed");
    let bin = target_dir().join("release/galion-console-static");
    eprintln!(
        "real: binary {} bytes",
        std::fs::metadata(&bin).expect("binary").len()
    );

    // 2. One read-only forward.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let key = std::env::var("PORT1_SSH_KEY").unwrap_or(format!("{home}/.ssh/id_ed25519_vps"));
    let ssh = Command::new("ssh")
        .args([
            "-i",
            &key,
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "BatchMode=yes",
        ])
        .args([
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=15",
        ])
        .arg("-N")
        .args(["-L", &format!("{up}:127.0.0.1:8080")])
        .args(["-L", &format!("{desk}:127.0.0.1:8646")])
        .args(["-L", &format!("{nerve}:127.0.0.1:8670")])
        .args(["-L", &format!("{}:127.0.0.1:8091", one_at())])
        .arg(std::env::var("PORT1_VPS").expect("set PORT1_VPS=user@host (the VPS)"))
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _tunnel = Kill(ssh, "ssh forward");
    wait_for("ssh forward", 20, || {
        matches!(get(up, "/api/v1/health"), Some((200, _, _)))
    });
    eprintln!("real: forward {up}->8080, {desk}->8646, {nerve}->8670 (ssh -N, read-only)");

    // 3. The front.
    let log = std::env::temp_dir().join("console-port-1-real.log");
    let child = Command::new(&bin)
        .args([
            "--bind",
            front_at(),
            "--ui",
            "rust",
            "--upstream",
            up_at(),
            "--desk",
            desk_at(),
        ])
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("front");
    let pid = child.id();
    let _front = Kill(child, "front");
    wait_for("front", 10, || TcpStream::connect(front).is_ok());

    // 4a. /analytics: the live API needs the phone cookie; we have none, so
    // the live answer is 401 and the page must say exactly that.
    let (ac, _, at) = get(up, "/api/v1/analytics").expect("live analytics");
    eprintln!(
        "real: live GET /api/v1/analytics -> {ac} {:.3} s",
        at.as_secs_f64()
    );
    let html = page(front, "/analytics");
    has(&html, "This pod", "/analytics");
    if routes::decide("/api/v1/analytics", ac) == routes::Live::Gone {
        // D-375-1 option A: the live Rust console has no analytics route (its
        // owner is galion-one :8091, same unpaired 401); the room must say the
        // read failed instead of inventing numbers.
        assert!(
            routes::page_says_unavailable(&html),
            "/analytics: no honest miss"
        );
        if routes::mode() == routes::Mode::Rust {
            let one: SocketAddr = one_at().parse().unwrap();
            let (oc, ob, _) = get(one, "/api/v1/analytics").expect("galion-one analytics");
            eprintln!(
                "real: Rust owner galion-one :8091 /api/v1/analytics -> {oc} {:?}",
                String::from_utf8_lossy(&ob)
            );
            assert!(oc == 401 || oc == 200, "galion-one analytics {oc}");
        }
    } else if ac == 401 {
        has(&html, "The analytics API did not answer.", "/analytics");
        has(&html, "paired phones only", "/analytics");
    } else {
        assert_eq!(ac, 200, "live analytics: unexpected {ac}");
    }
    for b in pages::analytics::BOARDS {
        let h = page(
            front,
            &format!("/analytics?board={}", b.to_ascii_lowercase()),
        );
        assert!(h.contains(&format!("aria-current=\"page\">{b}</a>")), "{b}");
    }

    // 4b. /arena vs live GET /api/arena (arena.py status, read-only).
    let (c, b, t) = get(up, "/api/arena").expect("live arena");
    eprintln!(
        "real: live GET /api/arena -> {c} {} B {:.3} s",
        b.len(),
        t.as_secs_f64()
    );
    let arena_gone = routes::decide("/api/arena", c) == routes::Live::Gone;
    if !arena_gone {
        assert_eq!(c, 200);
    }
    let html = page(front, "/arena");
    // Sentinel that never occurs in a page while the live arena is gone.
    let mut spend = String::from("\u{1}no-live-arena");
    if arena_gone {
        assert!(
            routes::page_says_unavailable(&html),
            "/arena: no honest miss"
        );
        assert!(
            !html.contains("<span class=\"who\">"),
            "/arena: rows without data"
        );
        assert!(
            !html.contains("<form method=\"post\""),
            "/arena has no actions"
        );
        eprintln!("real: /arena live {c} -> room states the miss, 0 rows, no actions");
    } else {
        let arena = json::parse(&b).expect("arena json");
        spend = pages::arena::spend_line(Some(&arena));
        has(&html, &spend, "/arena spend");
        let rows = arena.get("rows").as_array();
        for r in rows {
            if let Some(id) = r.get("id").as_str() {
                has(&html, id, "/arena row");
            }
        }
        let shown = html.matches("<span class=\"who\">").count();
        assert_eq!(shown, rows.len(), "/arena rows");
        assert!(
            !html.contains("<form method=\"post\""),
            "/arena has no actions"
        );
        eprintln!(
            "real: /arena spend {spend:?}, rows {} (live) = {shown} (page), champion {}",
            rows.len(),
            if arena.get("champion").is_null() {
                "none"
            } else {
                "set"
            }
        );
    }

    // 4c. /bank vs live GET /api/v1/bank.
    let (bc, bb, _) = get(up, "/api/v1/bank").expect("live bank");
    let bb = String::from_utf8_lossy(&bb).into_owned();
    eprintln!("real: live GET /api/v1/bank (Accept: application/json) -> {bc} {bb:?}");
    if bc != 500 {
        routes::decide("/api/v1/bank", bc);
    }
    let bc = if bc == 500 && bb.contains("Only HTML requests are supported here") {
        404
    } else {
        bc
    };
    let html = page(front, "/bank");
    has(&html, "The cash the stack can measure", "/bank");
    if bc == 404 {
        has(&html, "no /api/v1/bank route yet", "/bank");
    }

    // 4d. /cache vs live galion-desk GET /desk.
    let (dc, db, dt) = get(desk, "/desk").expect("live desk");
    eprintln!(
        "real: live GET :8646/desk -> {dc} {} B {:.3} s",
        db.len(),
        dt.as_secs_f64()
    );
    assert_eq!(dc, 200);
    let d = json::parse(&db).expect("desk json");
    let html = page(front, "/cache");
    let m = d.get("mindpack");
    let c = m.get("cache");
    let n = |v: &Value| v.as_f64().unwrap_or(0.0);
    let headline = format!(
        "{} read · {} write · {} files · {}",
        pages::cache::compact(n(c.get("read"))),
        pages::cache::compact(n(c.get("write"))),
        pages::jsfmt::text(c.get("files"), "0"),
        pages::cache::bytes(n(c.get("file_bytes")))
    );
    // galion-desk may have moved between the two reads; allow one retry.
    let html = if html.contains(&headline) {
        html
    } else {
        page(front, "/cache")
    };
    eprintln!("real: /cache headline {headline:?}");
    for s in m.get("stores").as_array() {
        has(
            &html,
            &pages::jsfmt::text(s.get("name"), ""),
            "/cache store",
        );
    }
    for s in m.get("models").as_array() {
        has(
            &html,
            &pages::jsfmt::text(s.get("name"), ""),
            "/cache model",
        );
    }
    let meters = html.matches("<meter ").count();
    assert_eq!(meters, m.get("stores").as_array().len(), "/cache bars");
    eprintln!(
        "real: /cache cached {}, stores {}, models {}, recent {}, meters {meters}",
        m.get("cached").as_array().len(),
        m.get("stores").as_array().len(),
        m.get("models").as_array().len(),
        m.get("recent").as_array().len()
    );

    // 4e. Methods and the gate, against the live-backed front. Non-GET
    // methods must be refused by the front itself (405) before any upstream
    // call; a console host without the phone cookie gets the pairing page and
    // none of the live data; a public host gets "Soon online.".
    let (adc, _, _) = get(up, "/api/v1/admin").expect("live admin");
    let admin_gone = routes::decide("/api/v1/admin", adc) == routes::Live::Gone;
    for p in ["/analytics", "/arena", "/bank", "/cache"] {
        for m in ["POST", "PUT", "DELETE"] {
            let r = raw(front, &format!("{m} {p} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
            assert!(
                r.starts_with("HTTP/1.1 405"),
                "{m} {p}: {}",
                r.lines().next().unwrap_or("")
            );
            assert!(r.contains("Allow: GET, HEAD"), "{m} {p}");
        }
        let r = raw(
            front,
            &format!("HEAD {p} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
        );
        assert!(r.starts_with("HTTP/1.1 200"), "HEAD {p}");
        assert!(
            r.split_once("\r\n\r\n")
                .map(|(_, b)| b.is_empty())
                .unwrap_or(false),
            "HEAD {p} body"
        );
        let r = raw(
            front,
            &format!("GET {p} HTTP/1.1\r\nHost: console.galion.app\r\nConnection: close\r\n\r\n"),
        );
        let code: u16 = r.get(9..12).and_then(|c| c.parse().ok()).unwrap_or(0);
        assert!(
            routes::gate_ok(code, &r, admin_gone),
            "{p}: unpaired console host gets pairing (or fails closed while /api/v1/admin is gone)"
        );
        assert!(
            !r.contains(&console_ui::html::esc(&spend))
                && !r.contains(&console_ui::html::esc(&headline)),
            "{p}: no live data unpaired"
        );
        let r = raw(
            front,
            &format!("GET {p} HTTP/1.1\r\nHost: sentialink.com\r\nConnection: close\r\n\r\n"),
        );
        assert!(r.contains("Soon online."), "{p}: public host");
    }
    eprintln!("real: POST/PUT/DELETE x4 pages -> 405 (Allow: GET, HEAD); HEAD x4 -> 200 empty; unpaired console host -> pairing page (admin gone on live: {admin_gone} -> 503 fail-closed), no live data; public host -> Soon online.");

    // 5. Nerve /state (observation only) and the live TS console paths.
    match get(nerve, "/state") {
        Some((c, b, t)) => {
            let keys = json::parse(&b)
                .ok()
                .map(|v| json::keys(&v).len())
                .unwrap_or(0);
            eprintln!(
                "real: nerve GET :8670/state -> {c} {} B {:.3} s, {keys} top-level keys",
                b.len(),
                t.as_secs_f64()
            );
        }
        None => eprintln!("real: nerve :8670/state did not answer"),
    }
    for p in ["/analytics", "/arena", "/bank", "/cache"] {
        let r = get(up, p).map(|(c, b, _)| (c, b.len()));
        eprintln!("real: live TS console GET {p} -> {r:?}");
    }
    eprintln!("real: front RSS {} kB", rss_kb(pid));
    let access = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        access
            .lines()
            .filter(|l| !l.starts_with("GET ")
                && !l.starts_with("HEAD ")
                && !l.starts_with("galion-console-static:"))
            .all(|l| l.ends_with(" 405")),
        "a non-GET got past the front"
    );
    eprintln!("real: done {}", now());
}

fn rss_kb(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("VmRSS"))
                .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string())
        })
        .unwrap_or_else(|| "?".into())
}

/// One raw HTTP/1.1 exchange (for methods and Host headers the client does
/// not send).
fn raw(a: SocketAddr, req: &str) -> String {
    use std::io::{Read, Write};
    let mut s = TcpStream::connect(a).expect("front");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}
