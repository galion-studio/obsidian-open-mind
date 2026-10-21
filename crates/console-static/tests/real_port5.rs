// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the console-port-5 rooms (`/node`, `/security`,
//! `/session`) against the **live VPS**, read-only.
//!
//! One `ssh -N` forward (nothing runs or is written on the VPS): box
//! 127.0.0.1:20091 -> VPS 127.0.0.1:8080 (console API), 20093 -> 8670
//! (galion-nerve). The real `galion-console-static --ui rust` binary runs on
//! 127.0.0.1:20090 (slot 139 ports 20090-20099) and each room is compared
//! with the live API answer read directly through the same forward.
//!
//! The box has no paired phone (on purpose), so the live admin-only APIs
//! answer 401; the rooms must state that refusal exactly. To check the
//! paired view with real data, the test also reads the live build-session
//! folder over ssh (`stat` names and sizes and `cat` of one file, both
//! read-only, kept in memory, not printed or committed), serves them on
//! 127.0.0.1:20092 in the shape of `GET /api/v1/session` and starts a
//! second binary on 20094 against it.
//!
//! Only GET requests reach the VPS console. Scan / Heal are covered by
//! `tests/ui_port5.rs` against a fake upstream, never live. The nerve
//! `/state` is read before and after.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_port5 -- --ignored --nocapture`.
//! `PORT5_VPS` names the ssh target (default `slavik-vps`, an ssh config
//! alias, so no address is in the code).

use console_ui::html::esc;
use console_ui::json::{self, Value};
use console_ui::pages::session::name_ok;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIND: &str = "127.0.0.1:20090";
const API: &str = "127.0.0.1:20091";
const FAKE: &str = "127.0.0.1:20092";
const NERVE: &str = "127.0.0.1:20093";
const BIND2: &str = "127.0.0.1:20094";
const SESSION_DIR: &str = "/workspace/galion-light/docs/build-session";

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

fn get(at: &str, path: &str, extra: &str) -> (u16, String, String) {
    let Ok(mut s) = TcpStream::connect(at) else {
        return (0, String::new(), String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: application/json\r\n{extra}Connection: close\r\n\r\n"
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
    (code, head.to_string(), body)
}

fn live_json(at: &str, path: &str) -> (u16, Value) {
    let (c, _, b) = get(at, path, "");
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
    let (c, v) = live_json(NERVE, "/state");
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
        v.get("cycle").to_json(),
        v.get("mode").as_str().unwrap_or("?"),
        v.get("components").as_array().len(),
        out
    );
    out
}

/// `HTTP <c> (<error>).` exactly as the rooms print a refusal.
fn refusal(code: u16, v: &Value) -> String {
    match v.get("error").as_str().unwrap_or("") {
        "" => format!("HTTP {code}."),
        e => format!("HTTP {code} ({}).", esc(e)),
    }
}

fn ssh_read(vps: &str, cmd: &str) -> String {
    let o = Command::new("ssh")
        .args(["-o", "BatchMode=yes", vps, cmd])
        .stdin(Stdio::null())
        .output()
        .expect("ssh");
    assert!(o.status.success(), "ssh read failed");
    String::from_utf8_lossy(&o.stdout).to_string()
}

/// A paired-shape `/api/v1/session` built from the live folder.
fn serve_fake(list: String, name: String, doc: Arc<String>) {
    let l = TcpListener::bind(FAKE).expect("bind fake");
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = head.split(' ').nth(1).unwrap_or("").to_string();
            let (code, ctype, body): (u16, &str, String) = if path == "/api/v1/session" {
                (200, "application/json", list.clone())
            } else if path == format!("/api/v1/session?name={name}") {
                (200, "text/markdown; charset=utf-8", doc.to_string())
            } else if path == "/api/v1/health" {
                (200, "application/json", "{\"status\":\"healthy\"}".into())
            } else {
                (404, "application/json", "{\"error\":\"not found\"}".into())
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 20090-20094"]
fn real_port5_rooms_against_live_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let vps = std::env::var("PORT5_VPS").unwrap_or_else(|_| "slavik-vps".into());
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
    wait_for("ssh forward", 25, || {
        get(API, "/api/v1/health", "").0 == 200
    });
    eprintln!("real: forward {API}->8080 {NERVE}->8670 (ssh -N, GET only)");
    let before = nerve_watch("before");

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args(["--ui", "rust", "--upstream", API, "--bind", BIND, "--quiet"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let _srv = Kill(child, "galion-console-static (live upstream)");
    wait_for("binary", 10, || TcpStream::connect(BIND).is_ok());

    // /node vs live /api/v1/health target.
    let (hc, health) = live_json(API, "/api/v1/health");
    assert_eq!(hc, 200);
    let target = health.get("target");
    let ssh_line = console_ui::pages::node::ssh_command(target).expect("live ssh target");
    let ip = target.get("publicIp").as_str().expect("live publicIp");
    let t = Instant::now();
    let (c, _, page) = get(BIND, "/node", "");
    eprintln!(
        "real: /node {c} {} B in {} ms",
        page.len(),
        t.elapsed().as_millis()
    );
    assert_eq!(c, 200);
    assert!(page.contains(&esc(&ssh_line)), "ssh line");
    assert!(page.contains(&format!(
        "<span>Last IP</span><span class=\"muted mono\">{}</span>",
        esc(ip)
    )));
    assert!(!page.contains("did not load") && !page.contains("no usable SSH"));
    eprintln!(
        "real: /node ok: ssh line and Last IP equal live health target (pod {}, sshPort {})",
        target.get("podId").as_str().unwrap_or("?"),
        target.get("sshPort").to_json()
    );

    // /security: each of the three admin-only sources, as the live API answers it.
    let t = Instant::now();
    let (c, _, page) = get(BIND, "/security", "");
    eprintln!(
        "real: /security {c} {} B in {} ms",
        page.len(),
        t.elapsed().as_millis()
    );
    assert_eq!(c, 200);
    for (label, path) in [
        ("Security", "/api/v1/security"),
        ("Guardian status", "/api/v3/guardian/status"),
        ("Incidents", "/api/v3/guardian/incidents"),
    ] {
        let (lc, lv) = live_json(API, path);
        if lc == 200 {
            assert!(!page.contains(&format!("{label} did not load")), "{label}");
        } else {
            let want = format!("{label} did not load: {}", refusal(lc, &lv));
            assert!(page.contains(&want), "missing {want}");
        }
        eprintln!("real: /security {path} live {lc} -> page states it");
    }
    if live_json(API, "/api/v1/security").0 != 200 {
        assert!(page.contains("<li><span>Denylist</span><span class=\"muted\">—</span></li>"));
        assert!(!page.contains("None open."));
    }

    // /session list, one file, a refused name, and the zip through the proxy.
    let (lc, lv) = live_json(API, "/api/v1/session");
    let (c, _, page) = get(BIND, "/session", "");
    assert_eq!(c, 200);
    if lc == 200 {
        let n = lv.get("items").as_array().len();
        assert!(page.contains(&format!("{n} files.")));
    } else {
        assert!(page.contains(&format!("The file list did not load: {}", refusal(lc, &lv))));
    }
    eprintln!("real: /session live list {lc} -> page states it");
    let (dc, dv) = live_json(API, "/api/v1/session?name=00-summary.md");
    let (c, _, page) = get(BIND, "/session?name=00-summary.md", "");
    assert_eq!(c, 200);
    if dc != 200 {
        assert!(page.contains(&format!("The file did not load: {}", refusal(dc, &dv))));
    }
    eprintln!("real: /session?name=00-summary.md live {dc} -> page states it");
    let (c, _, page) = get(BIND, "/session?name=..%2F..%2Fetc%2Fpasswd", "");
    assert_eq!(c, 400);
    assert!(page.contains("Nothing was asked."));
    let (zl, _, _) = get(API, "/api/v1/session-pack", "");
    let (zc, _, _) = get(BIND, "/api/v1/session-pack", "");
    assert_eq!(zc, zl, "zip link through the /api/ proxy");
    eprintln!("real: /api/v1/session-pack via the Rust front {zc} = live {zl}");

    for p in ["/node", "/security", "/session"] {
        let (_, _, b) = get(BIND, p, "");
        assert!(!b.to_ascii_lowercase().contains("<script"), "{p}");
    }

    // Paired view with real data: the live build-session folder (read-only).
    let listing = ssh_read(
        &vps,
        &format!("cd {SESSION_DIR} && LC_ALL=C stat -c '%n %s' -- *.md"),
    );
    let mut rows: Vec<(String, u64)> = listing
        .lines()
        .filter_map(|l| {
            let (n, s) = l.rsplit_once(' ')?;
            Some((n.to_string(), s.parse().ok()?))
        })
        .filter(|(n, _)| name_ok(n))
        .collect();
    rows.sort();
    assert!(!rows.is_empty(), "live build-session folder is empty");
    let list = json::obj(&[
        ("ok", Value::Bool(true)),
        (
            "items",
            Value::Arr(
                rows.iter()
                    .map(|(n, s)| {
                        json::obj(&[
                            ("name", Value::Str(n.clone())),
                            ("bytes", Value::Num(*s as f64)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
    .to_json();
    let first = rows[0].0.clone();
    let doc = Arc::new(ssh_read(&vps, &format!("cat -- {SESSION_DIR}/{first}")));
    assert_eq!(doc.len() as u64, rows[0].1, "size from stat = bytes read");
    serve_fake(list, first.clone(), Arc::clone(&doc));
    let child2 = Command::new(bin)
        .args([
            "--ui",
            "rust",
            "--upstream",
            FAKE,
            "--bind",
            BIND2,
            "--quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary 2");
    let _srv2 = Kill(child2, "galion-console-static (real session data)");
    wait_for("binary 2", 10, || TcpStream::connect(BIND2).is_ok());
    let (c, _, page) = get(BIND2, "/session", "");
    assert_eq!(c, 200);
    assert!(page.contains(&format!("{} files.", rows.len())));
    assert_eq!(page.matches("href=\"/session?name=").count(), rows.len());
    for (n, s) in &rows {
        assert!(
            page.contains(&format!("href=\"/session?name={}\"", esc(n))),
            "{n}"
        );
        assert!(page.contains(&format!(">{s}</span>")), "{n} size");
    }
    let t = Instant::now();
    let (c, _, page) = get(BIND2, &format!("/session?name={first}"), "");
    assert_eq!(c, 200);
    let shown = page
        .split("<pre class=\"code\">")
        .nth(1)
        .unwrap_or("")
        .split("</pre>")
        .next()
        .unwrap_or("");
    let want: String = doc
        .chars()
        .take(console_ui::pages::session::MAX_DOC_CHARS)
        .collect();
    assert_eq!(shown, esc(&want), "file text rendered escaped and whole");
    assert!(!page.to_ascii_lowercase().contains("<script"));
    eprintln!(
        "real: /session paired view with live folder data: {} files listed (names+sizes equal), first file {} B rendered whole in {} ms",
        rows.len(),
        doc.len(),
        t.elapsed().as_millis()
    );

    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len());
    for a in &after {
        assert!(!a.ends_with(" down"), "component down after the run: {a}");
    }
}
