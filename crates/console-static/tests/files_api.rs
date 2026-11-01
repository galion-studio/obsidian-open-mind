// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--files` (console-files, 2026-11-01 CET +01:00) end to end over loopback:
//! the pairing gate comes first, the page and editor bundle, listing and
//! byte ranges, raw responses that cannot run as pages, traversal refused,
//! read-only mode, the write guard (header + same origin), and every write
//! (edit with backup, mkdir, rename, upload, extract, delete-to-trash).

use console_static::admin::sha_hex;
use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

const TOKEN: &str = "paired-files-token-for-tests";
const HOST: &str = "console.galion.app";
const ORIGIN: &str = "https://console.galion.app";

fn tmp(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "console-files-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

struct Box {
    addr: SocketAddr,
    share: PathBuf,
}

fn start(extra: &[&str]) -> Box {
    let root = tmp("box");
    let store = root.join("admin-device.json");
    std::fs::write(
        &store,
        format!(r#"{{"tokenHashes":["{}"]}}"#, sha_hex(TOKEN)),
    )
    .unwrap();
    let share = root.join("share");
    std::fs::create_dir_all(share.join("clips")).unwrap();
    std::fs::write(share.join("notes.md"), "# Notes\n\nhello\n").unwrap();
    std::fs::write(share.join("page.html"), "<script>alert(1)</script>").unwrap();
    std::fs::write(share.join(".env"), "A=1\n").unwrap();
    let data: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
    std::fs::write(share.join("clips/a.mp4"), &data).unwrap();
    std::fs::write(root.join("outside.txt"), "outside").unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--local-api".into(),
        "--quiet".into(),
        "--bind".into(),
        addr.to_string(),
        "--admin-store".into(),
        store.display().to_string(),
        "--files".into(),
        share.display().to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let cfg = Config::from_args(args).unwrap();
    let state = std::sync::Arc::new(State::new(
        Site::default(),
        Routes::parse("page /").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    Box { addr, share }
}

struct Resp {
    code: u16,
    head: String,
    body: Vec<u8>,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    fn has(&self, h: &str) -> bool {
        self.head
            .to_ascii_lowercase()
            .contains(&h.to_ascii_lowercase())
    }
}

fn send(a: SocketAddr, head: &str, body: &[u8]) -> Resp {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let _ = s.write_all(body);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let split = out
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(out.len());
    let head = String::from_utf8_lossy(&out[..split]).into_owned();
    let body = out.get(split + 4..).unwrap_or(&[]).to_vec();
    Resp {
        code: head
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        head,
        body,
    }
}

fn cookie() -> String {
    format!("Cookie: galion_admin={TOKEN}\r\n")
}

fn get(a: SocketAddr, path: &str, extra: &str) -> Resp {
    send(
        a,
        &format!(
            "GET {path} HTTP/1.1\r\nHost: {HOST}\r\n{}{extra}Connection: close\r\n\r\n",
            cookie()
        ),
        b"",
    )
}

fn post_with(a: SocketAddr, path: &str, body: &Value, hdrs: &str) -> Resp {
    let b = body.to_string();
    send(
        a,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {HOST}\r\n{}{hdrs}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            cookie(),
            b.len()
        ),
        b.as_bytes(),
    )
}

fn post(a: SocketAddr, path: &str, body: &Value) -> Resp {
    post_with(
        a,
        path,
        body,
        &format!("Origin: {ORIGIN}\r\nX-Galion-Files: 1\r\n"),
    )
}

fn put(a: SocketAddr, path: &str, body: &[u8]) -> Resp {
    send(
        a,
        &format!(
            "PUT {path} HTTP/1.1\r\nHost: {HOST}\r\n{}Origin: {ORIGIN}\r\nX-Galion-Files: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            cookie(),
            body.len()
        ),
        body,
    )
}

#[test]
fn gate_comes_first_then_page_and_editor() {
    let b = start(&[]);
    for p in [
        "/api/files/list?path=",
        "/api/files/raw?path=notes.md",
        "/api/files/kinds",
    ] {
        let r = send(
            b.addr,
            &format!("GET {p} HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n"),
            b"",
        );
        assert_eq!(r.code, 401, "{p} unpaired");
        assert!(!r.text().contains("notes.md"));
        let r = send(
            b.addr,
            &format!(
                "GET {p} HTTP/1.1\r\nHost: evil.example\r\n{}Connection: close\r\n\r\n",
                cookie()
            ),
            b"",
        );
        assert_eq!(r.code, 401, "{p} foreign host");
    }
    let r = get(b.addr, "/files", "");
    assert_eq!(r.code, 200);
    assert!(r.has("content-security-policy: default-src 'none'"));
    assert!(r.text().contains("id=\"fx-list\""));
    let r = get(b.addr, "/files/editor.js?v=1", "");
    assert_eq!(r.code, 200);
    assert!(r.has("application/javascript"));
    assert!(r.text().starts_with("var GalionEditor="));
    let r = send(
        b.addr,
        "GET /files HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n",
        b"",
    );
    assert_ne!(r.code, 200, "page only on the console host");
    // Unpaired (no cookie, or a wrong one): the page and the editor are 401.
    for (p, c) in [
        ("/files", ""),
        ("/files/editor.js?v=1", ""),
        ("/files", "Cookie: galion_admin=not-the-token\r\n"),
    ] {
        let r = send(
            b.addr,
            &format!("GET {p} HTTP/1.1\r\nHost: {HOST}\r\n{c}Connection: close\r\n\r\n"),
            b"",
        );
        assert_eq!(r.code, 401, "{p} unpaired");
        assert!(!r.text().contains("fx-list") && !r.text().contains("GalionEditor"));
    }
}

#[test]
fn listing_ranges_and_safe_raw() {
    let b = start(&[]);
    let l = get(b.addr, "/api/files/list?path=", "").json();
    let names: Vec<&str> = l["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["clips", "notes.md", "page.html"]);
    assert_eq!(l["writable"], false);
    let l = get(b.addr, "/api/files/list?path=&hidden=1", "").json();
    assert_eq!(l["entries"].as_array().unwrap().len(), 4);
    let v = get(b.addr, "/api/files/info?path=clips%2Fa.mp4", "").json();
    assert_eq!(v["kind"], "video");
    assert_eq!(v["preview"], "video");
    // Video seeking: byte ranges.
    let r = get(
        b.addr,
        "/api/files/raw?path=clips/a.mp4",
        "Range: bytes=256-511\r\n",
    );
    assert_eq!(r.code, 206);
    assert!(r.has("content-range: bytes 256-511/10000"));
    assert_eq!(r.body.len(), 256);
    assert_eq!(r.body[0], 0);
    assert_eq!(
        get(
            b.addr,
            "/api/files/raw?path=clips/a.mp4",
            "Range: bytes=20000-\r\n"
        )
        .code,
        416
    );
    let r = get(b.addr, "/api/files/raw?path=clips/a.mp4", "");
    assert_eq!((r.code, r.body.len()), (200, 10_000));
    assert!(r.has("content-type: video/mp4") && r.has("accept-ranges: bytes"));
    // HTML never renders on the console origin.
    let r = get(b.addr, "/api/files/raw?path=page.html", "");
    assert!(r.has("content-type: text/plain"), "{}", r.head);
    assert!(r.has("content-security-policy: sandbox"));
    let r = get(b.addr, "/api/files/raw?path=notes.md&dl=1", "");
    assert!(r.has("content-disposition: attachment; filename=\"notes.md\""));
    assert!(r.has("application/octet-stream"));
    // Text + kinds.
    assert_eq!(
        get(b.addr, "/api/files/text?path=notes.md", "").json()["text"],
        "# Notes\n\nhello\n"
    );
    assert!(
        get(b.addr, "/api/files/kinds", "").json()["extensions"]
            .as_array()
            .unwrap()
            .len()
            >= 140
    );
    // Traversal in every spelling.
    for p in [
        "..",
        "../outside.txt",
        "%2e%2e%2foutside.txt",
        "clips%2F..%2F..%2Foutside.txt",
        "%2Fetc%2Fpasswd",
    ] {
        let r = get(b.addr, &format!("/api/files/raw?path={p}"), "");
        assert!(r.code == 400 || r.code == 404, "{p}: {}", r.code);
        assert!(!r.text().contains("outside"), "{p}");
    }
    assert_eq!(get(b.addr, "/api/files/nope", "").code, 404);
    assert_eq!(
        get(b.addr, "/api/files/thumb?path=clips/a.mp4", "").code,
        404,
        "no ffmpeg configured"
    );
}

#[test]
fn read_only_refuses_writes() {
    let b = start(&[]);
    let r = post(
        b.addr,
        "/api/files/write",
        &json!({"path": "notes.md", "content": "x"}),
    );
    assert_eq!(r.code, 403, "{}", r.text());
    assert_eq!(
        put(b.addr, "/api/files/upload?dir=&name=x.txt", b"hi").code,
        403
    );
    assert_eq!(
        post(b.addr, "/api/files/trash", &json!({"path": "notes.md"})).code,
        403
    );
    assert!(b.share.join("notes.md").exists() && !b.share.join("x.txt").exists());
    assert_eq!(get(b.addr, "/api/files/write", "").code, 405);
}

#[test]
fn writes_need_header_and_same_origin() {
    let b = start(&["--files-write", "--files-max-upload", "1000"]);
    let body = json!({"path": "", "name": "new"});
    assert_eq!(
        post_with(
            b.addr,
            "/api/files/mkdir",
            &body,
            &format!("Origin: {ORIGIN}\r\n")
        )
        .code,
        403
    );
    assert_eq!(
        post_with(
            b.addr,
            "/api/files/mkdir",
            &body,
            "Origin: https://evil.example\r\nX-Galion-Files: 1\r\n"
        )
        .code,
        403
    );
    assert_eq!(
        post_with(
            b.addr,
            "/api/files/mkdir",
            &body,
            "Origin: null\r\nX-Galion-Files: 1\r\n"
        )
        .code,
        403
    );
    assert!(!b.share.join("new").exists());
    assert_eq!(post(b.addr, "/api/files/mkdir", &body).code, 200);
    assert!(b.share.join("new").is_dir());
}

#[test]
fn full_write_round_trip() {
    let b = start(&["--files-write", "--files-max-upload", "1000"]);
    // Edit with backup + conflict check.
    let t = get(b.addr, "/api/files/text?path=notes.md", "").json();
    let r = post(
        b.addr,
        "/api/files/write",
        &json!({"path": "notes.md", "content": "# New\n", "version": t["version"]}),
    );
    assert_eq!(r.code, 200, "{}", r.text());
    let bak = r.json()["backup"].as_str().unwrap().to_string();
    assert_eq!(
        std::fs::read_to_string(b.share.join(&bak)).unwrap(),
        "# Notes\n\nhello\n"
    );
    let r = post(
        b.addr,
        "/api/files/write",
        &json!({"path": "notes.md", "content": "x", "version": t["version"]}),
    );
    assert_eq!(r.code, 409);
    // Upload: unique name, cap.
    let r = put(b.addr, "/api/files/upload?dir=clips&name=a.mp4", b"small");
    assert_eq!(r.json()["path"], "clips/a (1).mp4");
    assert_eq!(
        put(b.addr, "/api/files/upload?dir=&name=big.bin", &[0u8; 2000]).code,
        413
    );
    assert_eq!(
        put(b.addr, "/api/files/upload?dir=..&name=x", b"x").code,
        400
    );
    // Rename + move + copy.
    assert_eq!(
        post(
            b.addr,
            "/api/files/rename",
            &json!({"path": "page.html", "name": "page.txt"})
        )
        .code,
        200
    );
    assert_eq!(
        post(
            b.addr,
            "/api/files/rename",
            &json!({"path": "page.txt", "name": "../x"})
        )
        .code,
        400
    );
    assert_eq!(
        post(
            b.addr,
            "/api/files/copy",
            &json!({"path": "page.txt", "dest": "clips"})
        )
        .json()["path"],
        "clips/page.txt"
    );
    assert_eq!(
        post(
            b.addr,
            "/api/files/move",
            &json!({"path": "page.txt", "dest": "clips"})
        )
        .code,
        409
    );
    // Extract a zip (hostile member skipped).
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    z.start_file("in/a.txt", o).unwrap();
    z.write_all(b"zipped").unwrap();
    z.start_file("../../evil.txt", o).unwrap();
    z.write_all(b"evil").unwrap();
    let zb = z.finish().unwrap().into_inner();
    assert_eq!(
        put(b.addr, "/api/files/upload?dir=&name=pack.zip", &zb).code,
        200
    );
    let a = get(b.addr, "/api/files/archive?path=pack.zip", "").json();
    assert_eq!(a["count"], 2);
    let x = post(b.addr, "/api/files/extract", &json!({"path": "pack.zip"})).json();
    assert_eq!(x["path"], "pack");
    assert_eq!(x["files"], 1);
    assert_eq!(
        std::fs::read_to_string(b.share.join("pack/in/a.txt")).unwrap(),
        "zipped"
    );
    assert!(!b.share.parent().unwrap().join("evil.txt").exists());
    // Delete = trash.
    let r = post(b.addr, "/api/files/trash", &json!({"path": "clips"})).json();
    let p = r["path"].as_str().unwrap();
    assert!(p.starts_with(".trash/"));
    assert!(b.share.join(p).join("a.mp4").is_file(), "nothing erased");
    assert!(!b.share.join("clips").exists());
}

/// With a host ffmpeg, the poster route returns a JPEG of a generated clip.
#[test]
fn poster_with_host_ffmpeg() {
    let ff = "/usr/bin/ffmpeg";
    if !std::path::Path::new(ff).is_file() {
        eprintln!("skip: no {ff}");
        return;
    }
    let cache = tmp("thumbs");
    let b = start(&[
        "--files-ffmpeg",
        ff,
        "--files-cache",
        &cache.display().to_string(),
    ]);
    let out = b.share.join("clips/real.mp4");
    let st = std::process::Command::new(ff)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=10:duration=2",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success());
    let r = get(b.addr, "/api/files/thumb?path=clips/real.mp4&w=200", "");
    assert_eq!(r.code, 200, "{}", r.text());
    assert_eq!(&r.body[..2], &[0xff, 0xd8]);
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1, "cached");
    let i = get(b.addr, "/api/files/info?path=clips/real.mp4", "").json();
    assert_eq!(i["posters"], true);
    assert!(i["media"]["duration_s"].as_f64().unwrap() > 1.5, "{i}");
}
