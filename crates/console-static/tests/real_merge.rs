// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the merged console (console-port-merge, wave 4 slot
//! 169): one `galion-console-static --ui rust --desk` binary serving all 31
//! routes against the **live VPS**, read-only.
//!
//! One `ssh -N` forward (nothing runs or is written on the VPS): box
//! 127.0.0.1:20391 -> VPS 127.0.0.1:8080 (console API), 20392 -> 8646
//! (galion-desk), 20393 -> 8670 (galion-nerve). The binary listens on
//! 127.0.0.1:20390 (slot 169 ports 20390-20399). Every route in
//! `console_ui::PORTED` and every room of `rooms::ROOMS` is fetched with GET:
//! 200, no `<script>`, never the "Not in Rust yet" page, and the overview
//! says "31 of 31 routes in Rust". The nerve `/state` is read before and
//! after. Only GET requests are sent; no POST, no `/api/v1/markers/stream`,
//! no `/api/v1/ping`.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_merge -- --ignored --nocapture`.
//! `MERGE_VPS` names the ssh target (default `slavik-vps`, an ssh config
//! alias, so no address is in the code).

use console_ui::json::{self, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:20390";
const API: &str = "127.0.0.1:20391";
const DESK: &str = "127.0.0.1:20392";
const NERVE: &str = "127.0.0.1:20393";

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
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
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
    let (c, b) = get(NERVE, "/state");
    assert_eq!(c, 200, "nerve /state");
    let v = json::parse(b.as_bytes()).unwrap_or(Value::Null);
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

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 20390-20393"]
fn real_merged_console_against_live_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let vps = std::env::var("MERGE_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-N",
        ])
        .args(["-L", &format!("{API}:127.0.0.1:8080")])
        .args(["-L", &format!("{DESK}:127.0.0.1:8646")])
        .args(["-L", &format!("{NERVE}:127.0.0.1:8670")])
        .arg(&vps)
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _ssh = Kill(ssh, "ssh forward");
    wait_for("ssh forward", 25, || get(API, "/api/v1/health").0 == 200);
    eprintln!("real: forward {API}->8080 {DESK}->8646 {NERVE}->8670 (ssh -N, GET only)");
    let before = nerve_watch("before");

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            API,
            "--desk",
            DESK,
            "--bind",
            BIND,
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let _srv = Kill(child, "galion-console-static");
    wait_for("binary", 10, || TcpStream::connect(BIND).is_ok());

    let mut paths: Vec<&str> = console_ui::PORTED.to_vec();
    for r in console_ui::rooms::ROOMS {
        if let Some(p) = r.to {
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }
    assert_eq!(console_ui::PORTED.len(), 30);
    let mut total = 0usize;
    for p in &paths {
        // `/md/user` stands for `/md/$name`: it needs a memory folder
        // (`--memory-root`), which `real_port4` mirrors from the VPS; without one the
        // merged front answers a clean 404, checked below.
        if p.starts_with("/md/") {
            let (c, page) = get(BIND, p);
            eprintln!("real: GET {p} -> {c} (no --memory-root; real_port4 covers it)");
            assert_eq!(c, 404, "{p}");
            assert_eq!(page.matches("<script").count(), 0, "{p}");
            continue;
        }
        let t = Instant::now();
        let (c, page) = get(BIND, p);
        let scripts = page.matches("<script").count();
        eprintln!(
            "real: GET {p} -> {c} {} B in {} ms, <script>: {scripts}",
            page.len(),
            t.elapsed().as_millis()
        );
        assert_eq!(c, 200, "{p}");
        assert_eq!(scripts, 0, "{p}");
        assert!(!page.contains("Not in Rust yet"), "{p}");
        assert!(page.contains("<html"), "{p}");
        total += 1;
    }
    let (c, ov) = get(BIND, "/console");
    assert_eq!(c, 200);
    assert!(ov.contains("31 of 31 routes in Rust"), "overview count");
    eprintln!("real: {total} routes 200 (+ /md/user 404 without --memory-root), 0 placeholders, overview says 31 of 31 routes in Rust");
    assert_eq!(get(BIND, "/nope").0, 404);

    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len(), "nerve components changed");
}
