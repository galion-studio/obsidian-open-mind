// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the console-port-3 rooms (`/instances`, `/keys`,
//! `/markers`) against the **live VPS**, read-only.
//!
//! The test opens one `ssh -N` forward (nothing runs or is written on the
//! VPS): box 127.0.0.1:20071 -> VPS 127.0.0.1:8080 (console API) and
//! 20073 -> 8670 (galion-nerve), then starts the real
//! `galion-console-static --ui rust` binary on 127.0.0.1:20070 (slot 137
//! ports 20070-20079) and compares each room with the live API answer read
//! directly through the same forward. Only GET requests are made, to the
//! binary and to the VPS: Mint / Revoke / Ping are covered by
//! `tests/ui_port3.rs` against a fake upstream, never live. The nerve
//! `/state` is read before and after to show the console and its
//! neighbours stayed healthy, and the live marker ring is read before and
//! after to show it did not change.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_port3 -- --ignored --nocapture`.
//! `PORT3_VPS` names the ssh target (default `slavik-vps`, an ssh config
//! alias, so no address is in the code). Nothing secret is printed: the
//! laptop public key is compared, never echoed.

use console_ui::html::esc;
use console_ui::json::{self, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::console_routes as routes;
use galion_realtest::ports::Block;

/// Slot 375 (real-ports-batch-2): port block movable by `PORT3_REAL_PORT_BASE`; unset keeps
/// the old 20070 block.
static PORTS: Block = Block::new("PORT3_REAL_PORT_BASE", 20070, 10);
fn api_at() -> &'static str {
    PORTS.addr(1)
}
fn bind_at() -> &'static str {
    PORTS.addr(0)
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

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).as_str().unwrap_or("")
}

/// `name:port status` of the nerve components next to the console.
fn nerve_watch(tag: &str) -> Vec<String> {
    let (c, v) = live_json(nerve_at(), "/state");
    assert_eq!(c, 200, "nerve /state");
    let mut out = Vec::new();
    for comp in v.get("components").as_array() {
        let port = comp.get("port").as_f64().unwrap_or(0.0) as u16;
        if [8080u16, 8642, 8091, 8646].contains(&port) {
            out.push(format!(
                "{}:{} {}",
                s(comp, "name"),
                port,
                s(comp, "status")
            ));
        }
    }
    eprintln!(
        "real: nerve {tag}: cycle {} mode {} components {} -> {:?}",
        v.get("cycle").to_json(),
        v.get("mode").as_str().unwrap_or("?"),
        v.get("components").as_array().len(),
        out
    );
    out
}

fn timed(path: &str) -> (u16, String) {
    let t = Instant::now();
    let (c, page) = get(bind_at(), path);
    eprintln!(
        "real: {path} {c} {} B in {} ms",
        page.len(),
        t.elapsed().as_millis()
    );
    (c, page)
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 20070-20073"]
fn real_port3_rooms_against_live_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let vps = std::env::var("PORT3_VPS").unwrap_or_else(|_| "slavik-vps".into());
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
        "real: forward {api_at}->8080 {nerve_at}->8670 (ssh -N, GET only)",
        api_at = api_at(),
        nerve_at = nerve_at()
    );
    let before = nerve_watch("before");
    let (mc0, ring0) = live_json(api_at(), "/api/v1/markers");
    assert_eq!(mc0, 200, "live markers");

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            api_at(),
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

    // /instances vs live GET /api/v1/instances + /api/v1/laptop.
    let (ic, inst) = live_json(api_at(), "/api/v1/instances");
    let (lc, laptop) = live_json(api_at(), "/api/v1/laptop");
    assert_eq!(ic, 200, "live instances");
    assert_eq!(lc, 200, "live laptop");
    let (c, page) = timed("/instances");
    assert_eq!(c, 200);
    assert!(!page.contains("did not load"), "a call failed");
    let nodes = inst.get("nodes").as_array();
    let parts = inst.get("parts").as_array();
    let jobs = laptop.get("jobs").as_array();
    assert_eq!(
        page.matches("<li class=\"p3-card\">").count(),
        nodes.len() + jobs.len() + parts.len(),
        "cards = nodes + laptop jobs + parts"
    );
    let mut states = Vec::new();
    for n in nodes {
        let mark = n.get("mark").as_str().unwrap_or(s(n, "state"));
        let tone = match s(n, "state") {
            "online" => "online",
            "held" => "held",
            _ => "offline",
        };
        let want = format!(
            "<span class=\"p3-name ell\">{}</span><span class=\"badge {tone}\">{}</span>",
            esc(s(n, "name")),
            esc(mark)
        );
        assert!(page.contains(&want), "node {}", s(n, "name"));
        states.push(format!("{}={mark}", s(n, "id")));
    }
    let up = parts.iter().filter(|p| s(p, "state") == "online").count();
    assert!(page.contains(&format!("{up} of {} online", parts.len())));
    for p in parts {
        let want = format!(
            "<span class=\"p3-name ell\">{}</span><span class=\"badge {}\">{}</span>",
            esc(s(p, "name")),
            if s(p, "state") == "online" {
                "online"
            } else if s(p, "state") == "held" {
                "held"
            } else {
                "offline"
            },
            esc(s(p, "state"))
        );
        assert!(page.contains(&want), "part {}", s(p, "name"));
        states.push(format!("{}={}", s(p, "id"), s(p, "state")));
    }
    let (word, tone) = if laptop.get("connected").as_bool() == Some(true) {
        ("linked", "online")
    } else {
        ("no path", "offline")
    };
    assert!(
        page.contains(&format!(
            "<h2>Laptop</h2><span class=\"badge {tone}\">{word}</span>"
        )),
        "laptop {word}"
    );
    let key = s(inst.get("link"), "pubkey");
    if !key.is_empty() {
        assert!(
            page.contains(&esc(key)),
            "laptop key shown (not printed here)"
        );
    }
    eprintln!(
        "real: /instances ok: nodes {} parts {} ({up} online) laptop {word} jobs {} lines {}; {}",
        nodes.len(),
        parts.len(),
        jobs.len(),
        laptop.get("lines").as_array().len(),
        states.join(" ")
    );

    // /keys: live list without a paired cookie.
    let (kc, kv) = live_json(api_at(), "/api/v1/keys");
    let (c, page) = timed("/keys");
    assert_eq!(c, 200);
    if kc == 200 {
        let n = kv.get("keys").as_array().len();
        assert_eq!(page.matches("<li class=\"p3-key\">").count(), n);
        eprintln!("real: /keys live 200, {n} keys listed");
    } else {
        let err = s(&kv, "error");
        let want = if err.is_empty() {
            format!("The key list did not load: HTTP {kc}.")
        } else {
            format!("The key list did not load: HTTP {kc} ({}).", esc(err))
        };
        assert!(page.contains(&want), "missing {want}");
        assert!(!page.contains("No keys."));
        eprintln!("real: /keys live {kc} ({err}), page says: {want}");
    }
    assert!(
        page.contains("name=\"op\" value=\"mint\""),
        "mint form present"
    );
    assert!(
        !page.contains("gal_sk_") || kc == 200,
        "no key material on an unpaired page"
    );

    // /markers vs live ring.
    let (c, page) = timed("/markers");
    assert_eq!(c, 200);
    let (_, ring) = live_json(api_at(), "/api/v1/markers");
    let items = ring.get("items").as_array();
    let capn = ring.get("cap").as_f64().unwrap_or(42.0) as usize;
    assert!(page.contains(&format!("Ring of {capn} · {} stored", items.len())));
    assert_eq!(page.matches("<li class=\"p3-mk\">").count(), items.len());
    eprintln!(
        "real: /markers ok: cap {capn}, {} stored (live count {})",
        items.len(),
        ring.get("count").to_json()
    );

    // No script anywhere; repeat each room 5x for timings.
    for p in ["/instances", "/keys", "/markers"] {
        let mut ms = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let (c, b) = get(bind_at(), p);
            ms.push(t.elapsed().as_millis());
            assert_eq!(c, 200, "{p}");
            assert!(!b.to_ascii_lowercase().contains("<script"), "{p}");
        }
        eprintln!("real: {p} x5 ms {ms:?}");
    }

    // The live ring did not change (only GETs were sent) and the nerve
    // still sees the same components, none down.
    let (_, ring1) = live_json(api_at(), "/api/v1/markers");
    assert_eq!(
        ring0.get("count").to_json(),
        ring1.get("count").to_json(),
        "live marker ring changed"
    );
    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len());
    for a in &after {
        assert!(!a.ends_with(" down"), "component down after the run: {a}");
    }
}
