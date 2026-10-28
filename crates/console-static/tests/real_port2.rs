// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the console-port-2 rooms (`/context`, `/gateway`,
//! `/guardian`, `/hermes`) against the **live VPS**, read-only.
//!
//! The test opens one `ssh -N` forward (nothing runs or is written on the
//! VPS): box 127.0.0.1:20061 -> VPS 127.0.0.1:8080 (console API),
//! 20062 -> 8646 (galion-desk), 20063 -> 8670 (galion-nerve), then starts
//! the real `galion-console-static --ui rust` binary on 127.0.0.1:20060
//! (slot 136 ports 20060-20069) and compares each room with the live API
//! answer read directly through the same forward. Only GET requests reach
//! the VPS: Scan / Heal / Boot / Talk are covered by `tests/ui_port2.rs`
//! against a fake upstream, never live. The nerve `/state` is read before
//! and after to show the console, desk and Hermes components stayed
//! healthy.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_port2 -- --ignored --nocapture`.
//! `PORT2_VPS` names the ssh target (default `slavik-vps`, an ssh config
//! alias, so no address is in the code).

use console_ui::json::{self, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::console_routes as routes;
use galion_realtest::ports::Block;

/// Slot 375 (real-ports-batch-2): port block movable by `PORT2_REAL_PORT_BASE`; unset keeps
/// the old 20060 block.
static PORTS: Block = Block::new("PORT2_REAL_PORT_BASE", 20060, 10);
fn api_at() -> &'static str {
    PORTS.addr(1)
}
fn bind_at() -> &'static str {
    PORTS.addr(0)
}
fn desk_at() -> &'static str {
    PORTS.addr(2)
}
fn nerve_at() -> &'static str {
    PORTS.addr(3)
}

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

fn live_json(at: &str, path: &str) -> (u16, Value) {
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

/// `name status` of the nerve components the four rooms read.
fn nerve_watch(tag: &str) -> Vec<String> {
    let (c, v) = live_json(nerve_at(), "/state");
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
        "real: nerve {tag}: cycle {} mode {} components {} -> {:?}",
        json::Value::to_json(v.get("cycle")),
        v.get("mode").as_str().unwrap_or("?"),
        v.get("components").as_array().len(),
        out
    );
    out
}

fn count(h: &str, needle: &str) -> usize {
    h.matches(needle).count()
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 20060-20063"]
fn real_port2_rooms_against_live_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let vps = std::env::var("PORT2_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-N",
        ])
        .args(["-L", &format!("{api_at}:127.0.0.1:8080", api_at = api_at())])
        .args([
            "-L",
            &format!("{desk_at}:127.0.0.1:8646", desk_at = desk_at()),
        ])
        .args([
            "-L",
            &format!("{nerve_at}:127.0.0.1:8670", nerve_at = nerve_at()),
        ])
        .arg(&vps)
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _ssh = Kill(ssh, "ssh forward");
    wait_for("ssh forward", 25, || {
        get(api_at(), "/api/v1/health").0 == 200
    });
    eprintln!(
        "real: forward {api_at}->8080 {desk_at}->8646 {nerve_at}->8670 (ssh -N, GET only)",
        api_at = api_at(),
        desk_at = desk_at(),
        nerve_at = nerve_at()
    );
    let before = nerve_watch("before");

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            api_at(),
            "--desk",
            desk_at(),
            "--bind",
            bind_at(),
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let _srv = Kill(child, "galion-console-static");
    wait_for("binary", 10, || TcpStream::connect(bind_at()).is_ok());

    // /context vs live GET /desk.
    let (dc, desk) = live_json(desk_at(), "/desk");
    assert_eq!(dc, 200, "live /desk");
    let t = Instant::now();
    let (c, page) = get(bind_at(), "/context");
    eprintln!(
        "real: /context {c} {} B in {} ms",
        page.len(),
        t.elapsed().as_millis()
    );
    assert_eq!(c, 200);
    assert!(!page.contains("not answering"));
    let ctx = desk.get("ctx").as_f64().unwrap_or(0.0);
    let window = desk
        .get("mindpack")
        .get("context")
        .get("model_window")
        .as_f64()
        .unwrap_or(0.0);
    let expect = format!("{} live · {} window", ctx as i64, window as i64);
    assert!(page.contains(&expect), "missing {expect}");
    let road = desk.get("mindpack").get("road").as_array().len();
    let tools = desk.get("mindpack").get("tools").as_array();
    let road_html = page
        .split("<ol class=\"road\">")
        .nth(1)
        .unwrap_or("")
        .split("</ol>")
        .next()
        .unwrap_or("");
    assert_eq!(count(road_html, "<li>"), road + 2, "Now + Cache + road");
    assert_eq!(count(&page, "<progress class=\"bar\""), tools.len());
    for t in tools {
        let name = t.get("name").as_str().unwrap_or("");
        assert!(
            page.contains(&format!(
                "<span class=\"ell\">{}</span>",
                console_ui::html::esc(name)
            )),
            "tool {name}"
        );
    }
    eprintln!(
        "real: /context ok: ctx {ctx} window {window} road {road} tools {}",
        tools.len()
    );

    // /gateway: every GET endpoint, status equal to a direct GET.
    for (i, e) in console_ui::pages::gateway::ENDPOINTS.iter().enumerate() {
        if e.method != "GET" {
            continue;
        }
        let (direct, _) = get(api_at(), e.path);
        let t = Instant::now();
        let (c, page) = get(bind_at(), &format!("/gateway?ep={i}&send=1"));
        assert_eq!(c, 200, "{}", e.path);
        let shown = format!(
            "<span>{}</span><span class=\"muted\">{direct}</span>",
            console_ui::html::esc(e.title)
        );
        assert!(
            page.contains(&shown),
            "{}: expected live status {direct}",
            e.path
        );
        eprintln!(
            "real: /gateway ep={i} GET {} -> live {direct}, page {} B in {} ms",
            e.path,
            page.len(),
            t.elapsed().as_millis()
        );
    }

    // /guardian unpaired: the page states the live refusal.
    let (gc, gv) = live_json(api_at(), "/api/v3/guardian/status");
    routes::decide("/api/v3/guardian/status", gc);
    let (c, page) = get(bind_at(), "/guardian");
    assert_eq!(c, 200);
    if gc == 200 {
        assert!(!page.contains("did not load"));
    } else {
        let err = gv.get("error").as_str().unwrap_or("");
        let want = if err.is_empty() {
            format!("HTTP {gc}.")
        } else {
            format!("HTTP {gc} ({})", console_ui::html::esc(err))
        };
        assert!(page.contains(&want), "missing {want}");
    }
    eprintln!("real: /guardian live status {gc}, page says so");

    // /hermes: thread list and the first thread.
    let (hc, hist) = live_json(api_at(), "/api/hermes/history");
    if routes::decide("/api/hermes/history", hc) == routes::Live::Gone {
        // D-375-1: no thread list on the live Rust console (owner hermes-rs
        // /api/sessions, bearer key only); the room must say so, list nothing.
        let (c, page) = get(bind_at(), "/hermes");
        assert_eq!(c, 200);
        assert!(
            routes::page_says_unavailable(&page),
            "/hermes: no honest miss"
        );
        assert_eq!(
            count(&page, "href=\"/hermes?id="),
            0,
            "/hermes: threads without data"
        );
        assert!(!page.contains("127.0.0.1:8642"), "bind address hidden");
        eprintln!("real: /hermes live history {hc} -> room states the miss, 0 threads");
    } else {
        assert_eq!(hc, 200);
        let items = hist.get("items").as_array();
        let t = Instant::now();
        let (c, page) = get(bind_at(), "/hermes");
        eprintln!(
            "real: /hermes {c} {} B in {} ms",
            page.len(),
            t.elapsed().as_millis()
        );
        assert_eq!(c, 200);
        assert!(page.contains(&format!("Threads · {} ", items.len())));
        assert!(!page.contains("did not load"), "{page}");
        assert!(!page.contains("127.0.0.1:8642"), "bind address hidden");
        let shown = items.len().min(console_ui::pages::hermes::MAX_THREADS);
        assert_eq!(count(&page, "href=\"/hermes?id="), shown);
        let first = items
            .iter()
            .filter_map(|t| t.get("id").as_str())
            .find(|i| console_ui::pages::hermes::id_ok(i))
            .expect("a thread id");
        let (tc, thread) = live_json(api_at(), &console_ui::pages::hermes::thread_path(first));
        assert_eq!(tc, 200);
        let n = thread.get("items").as_array().len();
        let (c, page) = get(
            bind_at(),
            &format!("/hermes?id={}", console_ui::html::url_enc(first)),
        );
        assert_eq!(c, 200);
        assert!(
            page.contains(&format!("<p class=\"small\">{n} items ·")),
            "thread items {n}"
        );
        let msgs = page
            .split("<ul class=\"msgs\">")
            .nth(1)
            .unwrap_or("")
            .split("</ul>")
            .next()
            .unwrap_or("");
        assert_eq!(count(msgs, "<li"), n);
        eprintln!(
            "real: /hermes threads {} (listed {shown}), thread items {n}",
            items.len()
        );
    }

    // No script anywhere, and the nerve still sees the same components healthy.
    for p in ["/context", "/gateway", "/guardian", "/hermes"] {
        let (_, b) = get(bind_at(), p);
        assert!(!b.to_ascii_lowercase().contains("<script"), "{p}");
    }
    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len());
    for a in &after {
        assert!(!a.ends_with(" down"), "component down after the run: {a}");
    }
}
