// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end tests for the console-port-4 rooms (`/md/$name`, `/memory`,
//! `/mesh`): the real server with a fake console API upstream and a
//! temporary host-files tree, raw HTTP/1.1 over loopback (ephemeral ports).
//! Covers the TSX counting rules, escaping, the fixed `/md` name list (no
//! URL reaches the file system), the gate, GET-only upstream traffic, and
//! that no host path is ever shown.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

type Log = Arc<Mutex<Vec<String>>>;

/// How the fake `/api/mesh` answers.
#[derive(Clone, Copy)]
enum Mesh {
    Live,
    Down500,
    NotJson,
}

const MESH_LIVE: &str = r#"{"ok":true,"phase":"idle","n":55,"pool":8,"pass":0,"fail":0,"pending":55,"model_calls":0,"started":"2026-09-29T15:40:50+02:00","finished":"","tasks":[{"id":"m44"}],"score":["SCOPE 7/7 <live> & ok"],"cold":true}"#;

fn fake_api(log: Log, mesh: Mesh) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = Arc::clone(&log);
            thread::spawn(move || answer(s, log, mesh));
        }
    });
    addr
}

fn answer(mut s: TcpStream, log: Log, mesh: Mesh) {
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let first = head.lines().next().unwrap_or("").to_string();
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Cookie: "))
        .unwrap_or("-")
        .to_string();
    log.lock().unwrap().push(format!("{first} {cookie}"));
    let mut it = first.split(' ');
    let (method, path) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
    let paired = cookie.contains("galion_admin=good");
    let (code, out): (u16, String) = match (method, path, mesh) {
        ("GET", "/api/v1/admin", _) => (200, format!("{{\"ok\":true,\"you\":{paired}}}")),
        ("GET", "/api/v1/health", _) => (200, r#"{"status":"healthy"}"#.into()),
        ("GET", "/api/mesh", Mesh::Live) => (200, MESH_LIVE.into()),
        ("GET", "/api/mesh", Mesh::Down500) => (500, r#"{"ok":false,"error":"mesh down"}"#.into()),
        ("GET", "/api/mesh", Mesh::NotJson) => (200, "<html>".into()),
        _ => (404, "nope".into()),
    };
    let _ = write!(
        s,
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
}

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn touch(p: &Path, body: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::File::create(p).unwrap().write_all(body).unwrap();
}

/// A memory tree: l0 4 files (nested), l1 2, l2 1, l3 persona 777 bytes,
/// 3 scenes, 4 refs, 2 canvases; persona.md with markup; USER.md; model.
fn host_tree(tag: &str) -> Tmp {
    let t = Tmp(std::env::temp_dir().join(format!("ui-port4-{tag}-{}", std::process::id())));
    let _ = fs::remove_dir_all(&t.0);
    let m = t.0.join("mem");
    for f in ["a", "b", "s/c", "s/t/d"] {
        touch(&m.join("pyramid/l0").join(f), b"x");
    }
    touch(&m.join("pyramid/l1/1.json"), b"x");
    touch(&m.join("pyramid/l1/2.json"), b"x");
    touch(&m.join("pyramid/l2/1.json"), b"x");
    touch(&m.join("pyramid/l3/persona.md"), &[b'p'; 777]);
    for f in ["a.md", "b.md", "c.md", "x.txt", "n/d.md"] {
        touch(&m.join("scene_blocks").join(f), b"s");
    }
    for f in [
        "s1/refs/1.md",
        "s1/refs/2.md",
        "s2/refs/3.md",
        "s2/refs/x/refs/4.md",
        "s2/refs/no.json",
    ] {
        touch(&m.join("offload").join(f), b"r");
    }
    touch(&m.join("offload/s1/canvas.mmd"), b"c");
    touch(&m.join("offload/s2/k/canvas.mmd"), b"c");
    let mut persona = String::from("# Persona <script>alert(1)</script> & \"q\"\n");
    while persona.encode_utf16().count() < 900 {
        persona.push_str("Slavik ž line\n");
    }
    touch(&m.join("persona.md"), persona.as_bytes());
    touch(&t.0.join("USER.md"), b"# USER <i>facts</i>\n");
    touch(&t.0.join("model"), b"galion\n");
    t
}

fn start(mesh: Mesh, files: Option<&Path>) -> (SocketAddr, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let api = fake_api(Arc::clone(&log), mesh);
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--upstream".into(),
        api.to_string(),
        "--quiet".into(),
    ];
    if let Some(r) = files {
        for (k, v) in [
            ("--memory-root", r.join("mem")),
            ("--model-file", r.join("model")),
            ("--user-md", r.join("USER.md")),
        ] {
            args.push(k.into());
            args.push(v.to_string_lossy().into_owned());
        }
    }
    let cfg = Config::from_args(args).unwrap();
    let routes = Routes::parse("page /\n").unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(Site::default(), routes, cfg));
    thread::spawn(move || server::serve(l, state));
    (addr, log)
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn send(addr: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Resp {
        code: head
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn get(addr: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn row(v: &str, label: &str) -> String {
    format!("<span class=\"p4-n\">{v}</span><span class=\"ell\">{label}</span>")
}

fn upstream_methods(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

#[test]
fn memory_counts_from_host_tree() {
    let t = host_tree("mem");
    let (a, log) = start(Mesh::Live, Some(&t.0));
    let r = get(a, "/memory", "127.0.0.1", "");
    assert_eq!(r.code, 200, "{}", r.head);
    assert!(r
        .head
        .contains("Content-Security-Policy: default-src 'none'"));
    for (v, l) in [
        ("4", "L0 Conversation"),
        ("2", "L1 Atom"),
        ("1", "L2 Scenario"),
        ("777", "L3 Persona"),
        ("3", "scenes"),
        ("4", "raw tool refs"),
        ("2", "mermaid canvases"),
    ] {
        assert!(r.body.contains(&row(v, l)), "{l}: {}", r.body);
    }
    assert!(r.body.contains("<p class=\"p4-small\">galion</p>"));
    assert!(r
        .body
        .contains("# Persona &lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;q&quot;"));
    assert!(!r.body.contains("<script"), "no script, persona escaped");
    // Excerpt is 400 UTF-16 units: the text after it is not on the page.
    let persona = fs::read_to_string(t.0.join("mem/persona.md")).unwrap();
    let cut: String = persona.chars().take(400).collect();
    assert!(r.body.contains(
        &cut.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    ));
    assert_eq!(
        r.body.matches("Slavik ž line").count(),
        cut.matches("Slavik ž line").count()
    );
    // No host path on the page; only GETs upstream (the header dot).
    assert!(!r.body.contains(&*t.0.to_string_lossy()));
    let up = upstream_methods(&log);
    assert!(
        up.iter().all(|l| l.starts_with("GET /api/v1/health")),
        "{up:?}"
    );
    // HEAD answers without a body.
    let h = send(
        a,
        "HEAD /memory HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(h.code, 200);
    assert!(h.body.is_empty());
}

#[test]
fn memory_unconfigured_and_unreadable() {
    let (a, _) = start(Mesh::Live, None);
    let r = get(a, "/memory", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("No memory folder is configured"));
    assert_eq!(r.body.matches(">—</span>").count(), 7);
    let t = host_tree("gone");
    fs::remove_dir_all(t.0.join("mem")).unwrap();
    let (a, _) = start(Mesh::Live, Some(&t.0));
    let r = get(a, "/memory", "127.0.0.1", "");
    assert!(
        r.body.contains("could not be read: folder not found."),
        "{}",
        r.body
    );
    assert!(!r.body.contains(&*t.0.to_string_lossy()));
}

#[test]
fn mesh_live_and_failures() {
    let (a, log) = start(Mesh::Live, None);
    let r = get(a, "/mesh", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("One live line"));
    assert!(r
        .body
        .contains("<span class=\"p4-line\">SCOPE 7/7 &lt;live&gt; &amp; ok</span>"));
    assert!(r.body.contains("Cold. Nothing is spending."));
    assert!(!r.body.contains("<script"));
    let up = upstream_methods(&log);
    assert!(up.iter().any(|l| l.starts_with("GET /api/mesh ")), "{up:?}");
    assert!(up.iter().all(|l| l.starts_with("GET ")), "GET only: {up:?}");

    let (a, _) = start(Mesh::Down500, None);
    let r = get(a, "/mesh", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("No live line yet."));
    assert!(r
        .body
        .contains("Mesh status did not load: HTTP 500 (mesh down)."));
    assert!(r.body.contains("<p class=\"note\">Cold.</p>"));

    let (a, _) = start(Mesh::NotJson, None);
    let r = get(a, "/mesh", "127.0.0.1", "");
    assert!(
        r.body.contains("Mesh status did not load: HTTP 200."),
        "{}",
        r.body
    );

    // Upstream gone entirely.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap();
    drop(dead);
    let cfg = Config::from_args(
        ["--ui", "rust", "--upstream", &port.to_string(), "--quiet"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /\n").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    let r = get(a, "/mesh", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("the console API did not answer"));
}

#[test]
fn md_fixed_names_only() {
    let t = host_tree("md");
    let (a, log) = start(Mesh::Live, Some(&t.0));
    let r = get(a, "/md/persona", "127.0.0.1", "");
    assert_eq!(r.code, 200, "{}", r.body);
    let size = fs::metadata(t.0.join("mem/persona.md")).unwrap().len();
    assert!(
        r.body.contains(&format!("persona.md · {size} bytes · ")),
        "{}",
        r.body
    );
    assert!(r.body.contains(" (+02:00)</p>") || r.body.contains(" (+01:00)</p>"));
    assert!(r.body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!r.body.contains("<script"));
    assert!(r.body.contains("aria-current=\"page\">persona.md</a>"));
    let r = get(a, "/md/user", "127.0.0.1", "");
    assert_eq!(r.code, 200);
    assert!(r.body.contains("# USER &lt;i&gt;facts&lt;/i&gt;"));
    assert!(r.body.contains("USER.md · 20 bytes · "));
    // Anything else: 404 page, nothing read, no path shown.
    for p in [
        "/md/USER",
        "/md/user.md",
        "/md/x",
        "/md/user/x",
        "/md/..%2fUSER.md",
        "/md/%2e%2e/model",
        "/md/persona%00",
    ] {
        let r = get(a, p, "127.0.0.1", "");
        assert!(r.code == 404 || r.code == 400, "{p}: {}", r.head);
        assert!(
            !r.body.contains("facts") && !r.body.contains("Persona"),
            "{p}"
        );
        assert!(!r.body.contains(&*t.0.to_string_lossy()), "{p}");
    }
    assert!(
        upstream_methods(&log).is_empty(),
        "md never calls upstream on loopback"
    );
    // Not configured / missing file.
    let (a, _) = start(Mesh::Live, None);
    let r = get(a, "/md/user", "127.0.0.1", "");
    assert_eq!(r.code, 404);
    assert!(r
        .body
        .contains("USER.md is not shown: not configured on this console."));
    fs::remove_file(t.0.join("USER.md")).unwrap();
    let (a, _) = start(Mesh::Live, Some(&t.0));
    let r = get(a, "/md/user", "127.0.0.1", "");
    assert_eq!(r.code, 404);
    assert!(r.body.contains("USER.md is not shown: file not found."));
}

#[cfg(unix)]
#[test]
fn md_unreadable_file_is_500_without_path() {
    use std::os::unix::fs::PermissionsExt;
    let t = host_tree("perm");
    let u = t.0.join("USER.md");
    fs::set_permissions(&u, fs::Permissions::from_mode(0o000)).unwrap();
    // root can read anything; only meaningful as a normal user.
    if fs::read(&u).is_err() {
        let (a, _) = start(Mesh::Live, Some(&t.0));
        let r = get(a, "/md/user", "127.0.0.1", "");
        assert_eq!(r.code, 500);
        assert!(r
            .body
            .contains("permission denied (the console user cannot read it)"));
        assert!(!r.body.contains(&*t.0.to_string_lossy()));
    }
    fs::set_permissions(&u, fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn gate_and_methods() {
    let t = host_tree("gate");
    let (a, log) = start(Mesh::Live, Some(&t.0));
    // Public console host, unpaired: pairing page, no file content.
    for p in ["/memory", "/mesh", "/md/user", "/md/persona"] {
        let r = get(a, p, "console.galion.app", "");
        assert_eq!(r.code, 200, "{p}");
        assert!(r.body.contains("/_ui/pair"), "{p} pairing: {}", r.body);
        assert!(
            !r.body.contains("facts") && !r.body.contains("Persona") && !r.body.contains("SCOPE"),
            "{p}"
        );
    }
    assert!(!upstream_methods(&log)
        .iter()
        .any(|l| l.starts_with("GET /api/mesh")));
    // Paired: content.
    let r = get(
        a,
        "/md/user",
        "console.galion.app",
        "Cookie: galion_admin=good\r\n",
    );
    assert!(r.body.contains("# USER &lt;i&gt;facts&lt;/i&gt;"));
    let r = get(
        a,
        "/mesh",
        "console.galion.app",
        "Cookie: galion_admin=good\r\n",
    );
    assert!(r.body.contains("SCOPE 7/7"));
    // The admin cookie goes upstream; nothing else.
    assert!(upstream_methods(&log)
        .iter()
        .any(|l| l.starts_with("GET /api/mesh ") && l.ends_with("galion_admin=good")));
    // Not a console host: the quiet page.
    let r = get(a, "/md/user", "galion.app", "");
    assert!(r.body.contains("Soon"));
    assert!(!r.body.contains("facts"));
    // Loopback Host from a loopback peer is let through; POST is refused.
    for p in ["/memory", "/mesh", "/md/user"] {
        let r = send(
            a,
            &format!("POST {p} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        );
        assert_eq!(r.code, 405, "{p}");
    }
    assert!(upstream_methods(&log).iter().all(|l| l.starts_with("GET ")));
}

#[test]
fn ported_list_and_owning() {
    for p in ["/md/user", "/memory", "/mesh"] {
        assert!(console_ui::PORTED.contains(&p), "{p}");
    }
    let ui = console_ui::Ui::new(console_ui::UiConfig {
        upstream: "127.0.0.1:9".parse().unwrap(),
        pulse: None,
        extra_hosts: Vec::new(),
        files: Default::default(),
    });
    // With an SPA fallback the Rust rooms still answer these paths.
    for p in ["/memory", "/mesh", "/md/user", "/md/persona", "/md/other"] {
        assert!(ui.owns(p, true), "{p}");
    }
    assert!(!ui.owns("/md/", true));
}
