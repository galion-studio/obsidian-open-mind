// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life API-path matrix for the console rooms (wave 4 slot 364,
//! F-349-1): which API paths the `real_port2/3/4` rooms read, what the live
//! VPS `:8080` (Rust `galion-console-serve --dist --local-api`, since the
//! cutover) answers for each, and what **this branch's** binary answers in
//! the exact live process shape. READ-ONLY on the VPS.
//!
//! - One `ssh -N` forward (GET only, nothing written on the VPS):
//!   base+1 -> VPS 8080 (live console), base+2 -> 8646 (galion-desk),
//!   base+3 -> 8670 (galion-nerve), base+4 -> 8091 (galion-one pulse).
//! - The real release `galion-console-static` runs on base+0 with the live
//!   flags (`--dist DIR --ui rust --local-api --pulse --desk`, self-upstream)
//!   and a throwaway dist dir on the box.
//! - Checks: every path in [`MATRIX`] is answered by the candidate with the
//!   pinned status (board routes 200, retired Node routes 404); the live
//!   status is recorded next to it (a route the live binary does not serve
//!   yet is a deploy delta, not a test failure); the rooms render live data
//!   where the candidate has it (`/markers` = its own ring, `/gateway` = its
//!   own statuses, `/context` = live desk) and an honest "did not load"
//!   where the route is retired (`/hermes`, `/instances`, `/mesh`,
//!   `/guardian`). Nerve `/state` before/after: nothing went down.
//!
//! `GALION_REAL=1 CONSOLE_REAL_PORT_BASE=22340 cargo test --release -p console-static --test real_port_shape -- --ignored --nocapture`
//! (`PORT_SHAPE_VPS` = ssh target, default the `slavik-vps` ssh alias.)

use console_ui::json::{self, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod common;

const DEFAULT_BASE: u16 = 22340;
fn bind() -> &'static str {
    common::addr(DEFAULT_BASE, 0)
}
fn api() -> &'static str {
    common::addr(DEFAULT_BASE, 1)
}
fn desk() -> &'static str {
    common::addr(DEFAULT_BASE, 2)
}
fn nerve() -> &'static str {
    common::addr(DEFAULT_BASE, 3)
}
fn pulse() -> &'static str {
    common::addr(DEFAULT_BASE, 4)
}

/// Where a path is served from in the Rust console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Src {
    /// `--local-api` board/health routes (console-static `api.rs`).
    Board,
    /// Old Node-console route, not ported: 404 everywhere since the cutover.
    Retired,
}

/// Every API path the ported rooms read, with the candidate's pinned status.
const MATRIX: &[(&str, Src, u16, &str)] = &[
    (
        "/api/v1/health",
        Src::Board,
        200,
        "/gateway /node header-dot",
    ),
    ("/api/v1/ping", Src::Board, 200, "/gateway"),
    ("/api/v1/domains", Src::Board, 200, "/gateway /domains"),
    (
        "/api/v1/services",
        Src::Board,
        200,
        "/gateway /domains /services",
    ),
    ("/api/v1/repos", Src::Board, 200, "/gateway"),
    ("/api/v1/markers", Src::Board, 200, "/gateway /markers"),
    ("/api/v1/ledger", Src::Board, 200, "/gateway"),
    ("/api/v1/catalog", Src::Board, 200, "/gateway /docs"),
    ("/api/v1/docs", Src::Board, 200, "(board only, no room)"),
    ("/api/v1/protocol", Src::Board, 200, "(board only, no room)"),
    ("/api/v1/protocol/snapshot", Src::Board, 200, "/gateway"),
    ("/api/v3/guardian/status", Src::Retired, 404, "/guardian"),
    ("/api/hermes/history", Src::Retired, 404, "/hermes"),
    ("/api/hermes/health", Src::Retired, 404, "/hermes"),
    ("/api/hermes/models", Src::Retired, 404, "/hermes"),
    ("/api/hermes/modes", Src::Retired, 404, "/hermes"),
    ("/api/hermes/guard", Src::Retired, 404, "/hermes"),
    ("/api/v1/instances", Src::Retired, 404, "/instances"),
    ("/api/v1/laptop", Src::Retired, 404, "/instances"),
    ("/api/v1/keys", Src::Retired, 404, "/keys"),
    ("/api/mesh", Src::Retired, 404, "/mesh"),
];

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

fn get(at: &str, path: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(at) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        String::from_utf8_lossy(&console_ui::client::dechunk(body.as_bytes()).unwrap_or_default())
            .to_string()
    } else {
        body.to_string()
    };
    (code, body)
}

fn get_json(at: &str, path: &str) -> (u16, Value) {
    let (c, b) = get(at, path);
    (c, json::parse(b.as_bytes()).unwrap_or(Value::Null))
}

fn wait_for(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !ok() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "{what} did not come up"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn nerve_watch(tag: &str) -> Vec<String> {
    let (c, v) = get_json(nerve(), "/state");
    assert_eq!(c, 200, "nerve /state");
    let mut out = Vec::new();
    for comp in v.get("components").as_array() {
        let port = comp.get("port").as_f64().unwrap_or(0.0) as u16;
        if [8080u16, 8642, 8646, 8091].contains(&port) {
            out.push(format!(
                "{}:{} {}",
                comp.get("name").as_str().unwrap_or("?"),
                port,
                comp.get("status").as_str().unwrap_or("?")
            ));
        }
    }
    eprintln!(
        "real: nerve {tag}: cycle {} mode {} -> {:?}",
        v.get("cycle").to_json(),
        v.get("mode").as_str().unwrap_or("?"),
        out
    );
    out
}

/// Throwaway dist dir in the live layout (`DIR/static/...`).
struct Dist(PathBuf);
impl Drop for Dist {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn dist_dir() -> Dist {
    let d = std::env::temp_dir().join(format!("real-364-dist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("static/assets")).unwrap();
    std::fs::write(d.join("static/assets/app.css"), "body{}").unwrap();
    Dist(d)
}

#[test]
fn matrix_is_consistent() {
    // Unit check (no network): unique paths, pinned codes match the source.
    let mut seen = std::collections::HashSet::new();
    for (p, src, code, rooms) in MATRIX {
        assert!(seen.insert(*p), "duplicate {p}");
        assert!(p.starts_with("/api/"));
        assert!(!rooms.is_empty());
        match src {
            Src::Board => assert_eq!(*code, 200, "{p}"),
            Src::Retired => assert_eq!(*code, 404, "{p}"),
        }
    }
    // Every GET the /gateway room offers is in the matrix.
    for e in console_ui::pages::gateway::ENDPOINTS {
        if e.method == "GET" {
            assert!(seen.contains(e.path), "gateway {} not in MATRIX", e.path);
        }
    }
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and CONSOLE_REAL_PORT_BASE+0..4"]
fn real_port_shape_matrix_live_vs_candidate() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let vps = std::env::var("PORT_SHAPE_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-N",
        ])
        .args(["-L", &format!("{}:127.0.0.1:8080", api())])
        .args(["-L", &format!("{}:127.0.0.1:8646", desk())])
        .args(["-L", &format!("{}:127.0.0.1:8670", nerve())])
        .args(["-L", &format!("{}:127.0.0.1:8091", pulse())])
        .arg(&vps)
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _ssh = Kill(ssh, "ssh forward");
    wait_for("ssh forward", 25, || get(api(), "/api/v1/health").0 == 200);
    let (_, lh) = get_json(api(), "/api/v1/health");
    eprintln!(
        "real: forward {}->8080 {}->8646 {}->8670 {}->8091 (ssh -N, GET only); live console {} {}",
        api(),
        desk(),
        nerve(),
        pulse(),
        lh.get("service").as_str().unwrap_or("?"),
        lh.get("version").as_str().unwrap_or("?")
    );
    let before = nerve_watch("before");

    // The candidate in the live process shape (see `systemctl status console`).
    let dist = dist_dir();
    let child = Command::new(env!("CARGO_BIN_EXE_galion-console-static"))
        .args(["--bind", bind(), "--dist"])
        .arg(&dist.0)
        .args([
            "--ui",
            "rust",
            "--local-api",
            "--pulse",
            pulse(),
            "--desk",
            desk(),
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let _srv = Kill(child, "galion-console-static (candidate)");
    wait_for("binary", 10, || TcpStream::connect(bind()).is_ok());

    // 1. The matrix.
    let mut delta = Vec::new();
    eprintln!(
        "real: {:<28} {:>4} {:>9}  rooms",
        "path", "live", "candidate"
    );
    for (p, src, want, rooms) in MATRIX {
        let (lc, _) = get(api(), p);
        let (cc, _) = get(bind(), p);
        eprintln!("real: {p:<28} {lc:>4} {cc:>9}  {rooms} ({src:?})");
        assert_eq!(cc, *want, "candidate {p}");
        match (src, lc) {
            (Src::Board, 200) | (Src::Retired, 404) => {}
            (Src::Board, 404) => delta.push(*p),
            (_, c) => panic!("live {p} answered {c}: matrix out of date"),
        }
    }
    eprintln!(
        "real: deploy delta (candidate 200, live 404): {} path(s) {:?}",
        delta.len(),
        delta
    );

    // 2. Rooms on the candidate: data where it has the route.
    let (mc, ring) = get_json(bind(), "/api/v1/markers");
    assert_eq!(mc, 200);
    let items = ring.get("items").as_array().len();
    let cap = ring.get("cap").as_f64().unwrap_or(0.0) as usize;
    let (c, page) = get(bind(), "/markers");
    assert_eq!(c, 200);
    assert!(
        page.contains(&format!("Ring of {cap} · {items} stored")),
        "/markers = own ring"
    );
    assert!(!page.contains("did not load"), "/markers");
    eprintln!("real: candidate /markers: Ring of {cap} · {items} stored (= its /api/v1/markers)");

    for (i, e) in console_ui::pages::gateway::ENDPOINTS.iter().enumerate() {
        if e.method != "GET" {
            continue;
        }
        let (direct, _) = get(bind(), e.path);
        let (c, page) = get(bind(), &format!("/gateway?ep={i}&send=1"));
        assert_eq!(c, 200);
        let shown = format!(
            "<span>{}</span><span class=\"muted\">{direct}</span>",
            console_ui::html::esc(e.title)
        );
        assert!(page.contains(&shown), "{}: {direct}", e.path);
        assert_eq!(direct, 200, "{}", e.path);
    }
    eprintln!("real: candidate /gateway: every GET endpoint shows 200 from its own local API");

    let (dc, dv) = get_json(desk(), "/desk");
    assert_eq!(dc, 200, "live /desk");
    let (c, page) = get(bind(), "/context");
    assert_eq!(c, 200);
    let ctx = dv.get("ctx").as_f64().unwrap_or(0.0) as i64;
    let window = dv
        .get("mindpack")
        .get("context")
        .get("model_window")
        .as_f64()
        .unwrap_or(0.0) as i64;
    assert!(
        page.contains(&format!("{ctx} live · {window} window")),
        "/context"
    );
    eprintln!("real: candidate /context: {ctx} live · {window} window (= live desk)");

    // 3. Rooms whose route is retired say so, with no data and no script.
    for (room, want) in [
        (
            "/hermes",
            "The thread list (/api/hermes/history) did not load.".to_string(),
        ),
        (
            "/instances",
            console_ui::pages::p3::miss("The machine list", Some(404), ""),
        ),
        (
            "/instances",
            console_ui::pages::p3::miss("The laptop status", Some(404), ""),
        ),
        ("/keys", "The key list did not load: HTTP 404.".to_string()),
        ("/mesh", "Mesh status did not load: HTTP 404".to_string()),
        (
            "/guardian",
            "Guardian status did not load: HTTP 404".to_string(),
        ),
    ] {
        let (c, page) = get(bind(), room);
        assert_eq!(c, 200, "{room}");
        assert!(page.contains(&want), "{room}: missing {want}");
        assert!(!page.to_ascii_lowercase().contains("<script"), "{room}");
        let line = want.trim_start_matches("<p class=\"note p3-bad\">");
        eprintln!(
            "real: candidate {room} -> {}",
            line.trim_end_matches("</p>")
        );
    }

    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len());
    for a in &after {
        assert!(!a.ends_with(" down"), "component down after the run: {a}");
    }
}
