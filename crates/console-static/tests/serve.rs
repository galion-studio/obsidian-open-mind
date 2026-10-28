// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! In-process end-to-end tests: a temp `dist/client` tree with planted
//! dotfiles, a symlink and stray source files, a fake upstream, and raw
//! HTTP/1.1 over loopback (ephemeral ports; the real-binary test is
//! tests/real_console_static.rs on the slot range).

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn tempdir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("console-static-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn site_dir() -> PathBuf {
    let d = tempdir("site");
    fs::create_dir_all(d.join("assets")).unwrap();
    fs::create_dir_all(d.join(".git")).unwrap();
    fs::create_dir_all(d.join("public")).unwrap();
    fs::write(d.join("_shell.html"), "<!doctype html><title>shell</title>").unwrap();
    fs::write(d.join("favicon.svg"), "<svg/>").unwrap();
    fs::write(d.join("manifest.webmanifest"), "{}").unwrap();
    fs::write(d.join("assets/index-8epzlwHQ.js"), "console.log(1)").unwrap();
    fs::write(d.join("assets/index-8epzlwHQ.js.map"), "{}").unwrap();
    fs::write(d.join(".env"), "PLANTED=not-a-secret").unwrap();
    fs::write(d.join(".git/config"), "[core]").unwrap();
    fs::write(d.join("public/.env"), "PLANTED=1").unwrap();
    fs::write(d.join("phpinfo.php"), "<?php").unwrap();
    fs::write(d.join("vite.config.ts"), "export {}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/hostname", d.join("link.js")).unwrap();
    d
}

/// Fake upstream: answers every request with the request head echoed in the
/// body. Paths starting /api/sse send two events 300 ms apart.
fn fake_upstream(hits: Arc<AtomicUsize>) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let hits = Arc::clone(&hits);
            thread::spawn(move || {
                let mut s = s;
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                hits.fetch_add(1, Ordering::SeqCst);
                let text = String::from_utf8_lossy(&buf).to_string();
                let end = text.find("\r\n\r\n").unwrap() + 4;
                let cl: usize = text
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .map(|v| v.trim().parse().unwrap())
                    .unwrap_or(0);
                let mut body = buf[end..].to_vec();
                while body.len() < cl {
                    let n = s.read(&mut chunk).unwrap();
                    body.extend_from_slice(&chunk[..n]);
                }
                if text.starts_with("GET /api/sse") {
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\ndata: one\n\n");
                    thread::sleep(Duration::from_millis(300));
                    let _ = s.write_all(b"data: two\n\n");
                    return;
                }
                let echo = format!("{}BODY:{}", &text[..end], String::from_utf8_lossy(&body));
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{echo}",
                    echo.len()
                );
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    addr
}

fn start(upstream: Option<SocketAddr>) -> SocketAddr {
    let root = site_dir();
    let mut args = vec![
        "--root".to_string(),
        root.display().to_string(),
        "--quiet".into(),
    ];
    if let Some(u) = upstream {
        args.push("--upstream".into());
        args.push(u.to_string());
    }
    let cfg = Config::from_args(args).unwrap();
    let site = Site::load(cfg.root.as_deref().unwrap(), &cfg.shell).unwrap();
    let routes =
        Routes::parse("page /\npage /agents\npage /md/$name\nredirect /home /agents\n").unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(site, routes, cfg));
    thread::spawn(move || server::serve(l, state));
    addr
}

struct Resp {
    code: u16,
    head: String,
    body: Vec<u8>,
}

impl Resp {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

fn raw(addr: SocketAddr, req: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let end = out
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("response head")
        + 4;
    let head = String::from_utf8_lossy(&out[..end]).to_string();
    let code = head[9..12].parse().unwrap();
    Resp {
        code,
        head,
        body: out[end..].to_vec(),
    }
}

fn get(addr: SocketAddr, path: &str) -> Resp {
    raw(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
    )
}

#[test]
fn shell_routes_files_and_cache() {
    let a = start(None);
    for p in ["/", "/agents", "/md/readme", "/agents?tab=1"] {
        let r = get(a, p);
        assert_eq!(r.code, 200, "{p}");
        assert_eq!(
            r.header("content-type").as_deref(),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(r.header("cache-control").as_deref(), Some("no-cache"));
        assert!(r.text().contains("<title>shell</title>"));
    }
    let r = get(a, "/assets/index-8epzlwHQ.js");
    assert_eq!(r.code, 200);
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        r.header("cache-control").as_deref(),
        Some("public, max-age=31536000, immutable")
    );
    assert_eq!(
        r.header("x-content-type-options").as_deref(),
        Some("nosniff")
    );
    let r = get(a, "/favicon.svg");
    assert_eq!(r.header("content-type").as_deref(), Some("image/svg+xml"));
    assert_eq!(
        r.header("cache-control").as_deref(),
        Some("public, max-age=3600")
    );
    let etag = r.header("etag").unwrap();
    let r = raw(a, &format!("GET /favicon.svg HTTP/1.1\r\nHost: x\r\nIf-None-Match: {etag}\r\nConnection: close\r\n\r\n"));
    assert_eq!(r.code, 304);
    assert!(r.body.is_empty());
    let r = get(a, "/manifest.webmanifest");
    assert_eq!(
        r.header("content-type").as_deref(),
        Some("application/manifest+json")
    );
    let r = raw(
        a,
        "HEAD /favicon.svg HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.code, 200);
    assert_eq!(r.header("content-length").as_deref(), Some("6"));
    assert!(r.body.is_empty());
}

#[test]
fn probes_and_strays_are_404() {
    let a = start(None);
    for p in [
        "/.env",
        "/public/.env",
        "/phpinfo.php",
        "/public/phpinfo.php",
        "/.git/config",
        "/.git/HEAD",
        "/opt/galion/console-ide/.git/config",
        "/@fs/opt/galion/console-ide/.git/config",
        "/%2eenv",
        "/%2e%2e/%2e%2e/etc/passwd",
        "/assets/../.env",
        "/assets//index-8epzlwHQ.js",
        "/assets/index-8epzlwHQ.js.map",
        "/assets/",
        "/assets",
        "/_shell.html",
        "/vite.config.ts",
        "/link.js",
        "/agents/",
        "/nope",
        "/@vite/client",
        "/src/lib/host.ts",
        "/package.json",
        "/api/v1/health",
    ] {
        let r = get(a, p);
        assert_eq!(r.code, 404, "{p}");
        assert!(!r.text().contains("PLANTED"), "{p}");
        assert_eq!(r.header("cache-control").as_deref(), Some("no-store"));
    }
}

#[test]
fn methods_redirects_and_keepalive() {
    let a = start(None);
    let r = raw(
        a,
        "POST /agents HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.code, 405);
    assert_eq!(r.header("allow").as_deref(), Some("GET, HEAD"));
    let r = raw(
        a,
        "DELETE /nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.code, 404);
    let r = get(a, "/home?x=1");
    assert_eq!(r.code, 302);
    assert_eq!(r.header("location").as_deref(), Some("/agents?x=1"));
    let r = raw(a, "GET / HTTP/1.1\r\n\r\n");
    assert_eq!(r.code, 400);
    // Two pipelined requests on one connection.
    let r = raw(a, "GET /favicon.svg HTTP/1.1\r\nHost: x\r\n\r\nGET /.env HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert_eq!(r.code, 200);
    let tail = r.text();
    assert!(
        tail.starts_with("<svg/>HTTP/1.1 404 Not Found\r\n"),
        "{tail}"
    );
}

#[test]
fn proxy_allowlist_forwarding_and_streaming() {
    let hits = Arc::new(AtomicUsize::new(0));
    let up = fake_upstream(Arc::clone(&hits));
    let a = start(Some(up));
    let r = raw(a, "GET /api/v1/health?x=1 HTTP/1.1\r\nHost: console.galion.app\r\nX-Forwarded-Proto: https\r\nConnection: keep-alive\r\n\r\n");
    assert_eq!(r.code, 200);
    let t = r.text();
    assert!(t.starts_with("GET /api/v1/health?x=1 HTTP/1.1\r\n"), "{t}");
    assert!(t.contains("Host: console.galion.app\r\n"));
    assert!(t.contains("X-Forwarded-For: 127.0.0.1\r\n"));
    assert!(t.contains("X-Forwarded-Proto: https\r\n"));
    assert!(t.contains("Connection: close\r\n"));
    assert_eq!(r.header("connection").as_deref(), Some("close"));
    let r = raw(a, "POST /api/hermes/runs HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\n{\"input\":\"x\"}");
    assert!(r.text().ends_with("BODY:{\"input\":\"x\"}"), "{}", r.text());
    let r = raw(
        a,
        "POST /_serverFn/abc HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
    );
    assert_eq!(r.code, 411);
    let before = hits.load(Ordering::SeqCst);
    for p in [
        "/api/.env",
        "/api/../.env",
        "/api/%2e%2e/.git/config",
        "/api",
        "/apix/y",
    ] {
        assert_eq!(get(a, p).code, 404, "{p}");
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "refused paths must not reach upstream"
    );
    let r = raw(
        a,
        "TRACE /api/v1/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(r.code, 405);
    // SSE: the first event arrives before the upstream finishes.
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(b"GET /api/sse HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let t0 = Instant::now();
    let mut got = Vec::new();
    let mut chunk = [0u8; 1024];
    while !String::from_utf8_lossy(&got).contains("data: one") {
        let n = s.read(&mut chunk).unwrap();
        assert!(n > 0);
        got.extend_from_slice(&chunk[..n]);
    }
    assert!(
        t0.elapsed() < Duration::from_millis(250),
        "first SSE frame was buffered"
    );
    let mut rest = Vec::new();
    let _ = s.read_to_end(&mut rest);
    got.extend_from_slice(&rest);
    let all = String::from_utf8_lossy(&got);
    assert!(all.contains("Content-Type: text/event-stream"));
    assert!(all.contains("data: two"));
}

#[test]
fn upstream_down_is_502() {
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let a = start(Some(dead));
    assert_eq!(get(a, "/api/v1/health").code, 502);
    assert_eq!(get(a, "/").code, 200);
}

#[cfg(unix)]
#[test]
fn symlinked_root_is_resolved_but_inner_links_are_not() {
    let real = site_dir();
    let link = tempdir("rootlink").join("current");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let site = Site::load(&link, "_shell.html").unwrap();
    assert!(site.file("/favicon.svg").is_some());
    assert!(site.file("/link.js").is_none());
    assert!(site.file("/.env").is_none());
    assert!(site.file("/phpinfo.php").is_none());
    assert!(site.file("/_shell.html").is_none());
    assert!(site.shell().is_some());
    assert!(Site::load(&real, "missing.html").is_err());
}
