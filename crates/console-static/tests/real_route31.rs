// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for console-route-31 (wave 4 slot 170): the real release
//! `galion-console-static --ui rust` **with an SPA fallback** (`--root` with
//! a stand-in shell, the 31 live-source page routes, `/api/` proxied)
//! against the **live VPS** console API, read-only.
//!
//! One `ssh -N` forward (nothing runs or is written on the VPS): box
//! 127.0.0.1:20401 -> VPS 127.0.0.1:8080 (console API), 20403 -> 8670
//! (galion-nerve). The binary listens on 127.0.0.1:20400 (slot 170 ports
//! 20400-20409). Checks: `/hermes` and `/hermes?id=` are the Rust page
//! (not the SPA) with the live thread count, no script, CSP unchanged;
//! POST/PUT to `/hermes` are 405 in the binary and never leave the box;
//! every other live route goes where `Ui::spa_fallbacks(true)` says; the
//! old SPA's `/api/hermes/*` proxy still answers (GET only); the nerve
//! `/state` before and after.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_route31 -- --ignored --nocapture`.
//! `ROUTE31_VPS` names the ssh target (default `slavik-vps`, an ssh config
//! alias, so no address is in the code).

use console_ui::json::{self, Value};
use console_ui::port2::LIVE_SRC_ROUTES;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:20400";
const API: &str = "127.0.0.1:20401";
const NERVE: &str = "127.0.0.1:20403";
const CSP: &str = "default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn raw(at: &str, req: &str) -> Resp {
    let Ok(mut s) = TcpStream::connect(at) else {
        return Resp {
            code: 0,
            head: String::new(),
            body: String::new(),
        };
    };
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let _ = s.write_all(req.as_bytes());
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
    Resp {
        code,
        head: head.to_string(),
        body,
    }
}

fn get(at: &str, path: &str) -> Resp {
    raw(
        at,
        &format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        ),
    )
}

fn header(r: &Resp, name: &str) -> Option<String> {
    r.head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

fn live_json(at: &str, path: &str) -> (u16, Value) {
    let r = get(at, path);
    (
        r.code,
        json::parse(r.body.as_bytes()).unwrap_or(Value::Null),
    )
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
    let (c, v) = live_json(NERVE, "/state");
    assert_eq!(c, 200, "nerve /state");
    let mut out = Vec::new();
    for comp in v.get("components").as_array() {
        let port = comp.get("port").as_f64().unwrap_or(0.0) as u16;
        if [8080u16, 8642].contains(&port) {
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
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 20400-20403"]
fn real_route31_hermes_rust_first_against_live_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    // Stand-in SPA root: a shell that is easy to recognise.
    let root = std::env::temp_dir().join(format!("route31-real-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("assets")).unwrap();
    fs::write(
        root.join("_shell.html"),
        "<!doctype html><title>old-spa-shell</title><script src=\"/assets/a.js\"></script>",
    )
    .unwrap();
    fs::write(root.join("assets/a.js"), "0").unwrap();
    let routes: String = LIVE_SRC_ROUTES
        .iter()
        .map(|p| {
            if *p == "/md/user" {
                "page /md/$name\n".to_string()
            } else {
                format!("page {p}\n")
            }
        })
        .collect();
    let routes_file = root.with_extension("routes");
    fs::write(&routes_file, routes).unwrap();

    let vps = std::env::var("ROUTE31_VPS").unwrap_or_else(|_| "slavik-vps".into());
    let ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-N",
        ])
        .args(["-L", &format!("{API}:127.0.0.1:8080")])
        .args(["-L", &format!("{NERVE}:127.0.0.1:8670")])
        .arg(&vps)
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _ssh = Kill(ssh, "ssh forward");
    wait_for("ssh forward", 25, || get(API, "/api/v1/health").code == 200);
    eprintln!("real: forward {API}->8080 {NERVE}->8670 (ssh -N, GET only)");
    let before = nerve_watch("before");

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args(["--ui", "rust", "--upstream", API, "--bind", BIND, "--quiet"])
        .args(["--root", root.to_str().unwrap()])
        .args(["--routes", routes_file.to_str().unwrap()])
        .args(["--proxy-prefix", "/api/"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let srv = Kill(child, "galion-console-static");
    wait_for("binary", 10, || TcpStream::connect(BIND).is_ok());
    eprintln!("real: binary on {BIND} with --root (SPA fallback) + 31 page routes");

    // /hermes is the Rust room, against live data.
    let (hc, hist) = live_json(API, "/api/hermes/history");
    assert_eq!(hc, 200, "live /api/hermes/history");
    let items = hist.get("items").as_array();
    let t = Instant::now();
    let r = get(BIND, "/hermes");
    eprintln!(
        "real: /hermes {} {} B in {} ms",
        r.code,
        r.body.len(),
        t.elapsed().as_millis()
    );
    assert_eq!(r.code, 200);
    assert!(!r.body.contains("old-spa-shell"), "SPA took /hermes");
    assert!(r.body.contains("Hermes Research"));
    assert!(!r.body.to_ascii_lowercase().contains("<script"));
    assert!(!r.body.contains("<form"));
    assert!(!r.body.contains("127.0.0.1:8642"), "bind address hidden");
    assert!(!r.body.contains("did not load"), "{}", r.body);
    assert!(r.body.contains(&format!("Threads · {} ", items.len())));
    assert_eq!(header(&r, "content-security-policy").as_deref(), Some(CSP));
    assert_eq!(header(&r, "x-frame-options").as_deref(), Some("DENY"));
    let shown = items.len().min(console_ui::pages::hermes::MAX_THREADS);
    assert_eq!(r.body.matches("href=\"/hermes?id=").count(), shown);
    let first = items
        .iter()
        .filter_map(|t| t.get("id").as_str())
        .find(|i| console_ui::pages::hermes::id_ok(i))
        .expect("a thread id");
    let (tc, th) = live_json(API, &console_ui::pages::hermes::thread_path(first));
    assert_eq!(tc, 200);
    let n = th.get("items").as_array().len();
    let r = get(
        BIND,
        &format!("/hermes?id={}", console_ui::html::url_enc(first)),
    );
    assert_eq!(r.code, 200);
    assert!(!r.body.contains("old-spa-shell"));
    assert!(r.body.contains(&format!("<p class=\"small\">{n} items ·")));
    eprintln!(
        "real: /hermes threads {} (listed {shown}), first thread items {n}, CSP unchanged, no script",
        items.len()
    );

    // Held writes: refused in the binary, never forwarded.
    for m in ["POST", "PUT", "DELETE"] {
        let body = "input=x";
        let r = raw(
            BIND,
            &format!(
                "{m} /hermes HTTP/1.1\r\nHost: 127.0.0.1\r\nSec-Fetch-Site: same-origin\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ),
        );
        assert_eq!(r.code, 405, "{m} /hermes");
        assert_eq!(header(&r, "allow").as_deref(), Some("GET, HEAD"));
    }
    eprintln!(
        "real: POST/PUT/DELETE /hermes -> 405 Allow: GET, HEAD (in the binary, nothing sent)"
    );

    // Every live-source route: Rust or SPA, as spa_fallbacks says.
    let ui = console_ui::Ui::new(console_ui::UiConfig {
        upstream: API.parse().unwrap(),
        pulse: None,
        extra_hosts: vec![],
        desk: None,
        files: Default::default(), // main-staging: console-port-4 field
    });
    let fallbacks = ui.spa_fallbacks(true);
    let mut rust = Vec::new();
    for p in LIVE_SRC_ROUTES {
        let r = get(BIND, p);
        let spa = r.body.contains("old-spa-shell");
        if fallbacks.contains(p) {
            assert_eq!(r.code, 200, "{p}");
            assert!(spa, "{p}: expected SPA");
        } else {
            assert!(!spa, "{p}: expected Rust");
            assert!(!r.body.to_ascii_lowercase().contains("<script"), "{p}");
            rust.push(format!("{p} {}", r.code));
        }
    }
    eprintln!("real: Rust-first {} -> {:?}", rust.len(), rust);
    eprintln!("real: SPA fallback {} -> {:?}", fallbacks.len(), fallbacks);
    assert_eq!(rust.len(), console_ui::PORTED.len() + 1, "PORTED + /");
    assert_eq!(rust.len() + fallbacks.len(), 31);
    assert!(!fallbacks.contains(&"/hermes"));

    // The old SPA's Hermes API path through the proxy (GET only).
    let r = get(BIND, "/api/hermes/health");
    let direct = get(API, "/api/hermes/health").code;
    assert_eq!(r.code, direct, "proxy answer equals a direct GET");
    eprintln!(
        "real: proxy GET /api/hermes/health -> {} (direct {direct})",
        r.code
    );

    drop(srv);
    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len());
    for a in &after {
        assert!(!a.ends_with(" down"), "component down after the run: {a}");
    }
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_file(&routes_file);
}
