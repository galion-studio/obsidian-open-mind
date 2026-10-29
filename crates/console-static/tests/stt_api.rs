// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--stt` (owner order t362u, 2026-10-29 CET +01:00): the console's live
//! dictation routes `/api/stt/health` and `/api/stt/stream` end to end over
//! loopback against a mock galion-stt (ephemeral ports, no network). Covers
//! the pairing gate (401 before galion-stt is touched), the Origin check,
//! the session cap, the byte-transparent WebSocket tunnel (no cookie or
//! origin forwarded upstream), STT offline and the flag checks.

use console_static::admin::sha_hex;
use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const TOKEN: &str = "paired-device-token-for-stt-tests";
const WS: &str = "Upgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n";
const HOST: &str = "console.galion.app";
const ORIGIN: &str = "Origin: https://console.galion.app\r\n";

type Heads = Arc<Mutex<Vec<String>>>;

fn tmp() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "stt-api-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Mock galion-stt: `GET /health` -> `{"ok":true,...}`; `GET /stream` with an
/// upgrade -> 101 plus a greeting in the same write, then echoes bytes back.
fn mock_stt(heads: Heads) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let heads = Arc::clone(&heads);
            thread::spawn(move || serve(s, heads));
        }
    });
    addr
}

fn serve(mut s: TcpStream, heads: Heads) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    heads.lock().unwrap().push(head.clone());
    if head.starts_with("GET /health ") {
        let b = r#"{"ok":true,"model":"moonshine-tiny-streaming"}"#;
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            b.len()
        );
        return;
    }
    if head.starts_with("GET /stream ") && head.to_ascii_lowercase().contains("upgrade: websocket")
    {
        let _ = s.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: mock-accept-value\r\n\r\nHELLO|");
        let _ = s.write_all(&buf[end..]);
        loop {
            match s.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => {
                    if s.write_all(&chunk[..n]).is_err() {
                        return;
                    }
                }
            }
        }
    }
    let _ =
        s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
}

struct Box {
    addr: SocketAddr,
    heads: Heads,
}

fn start(stt: Option<SocketAddr>, extra: &[&str]) -> Box {
    let heads: Heads = Arc::default();
    let up = stt.unwrap_or_else(|| mock_stt(Arc::clone(&heads)));
    let dir = tmp();
    let store = dir.join("admin-device.json");
    std::fs::write(
        &store,
        format!(r#"{{"tokenHashes":["{}"]}}"#, sha_hex(TOKEN)),
    )
    .unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let mut args: Vec<String> = [
        "--ui",
        "rust",
        "--local-api",
        "--quiet",
        "--bind",
        &addr.to_string(),
        "--admin-store",
        &store.display().to_string(),
        "--stt",
        &up.to_string(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    let cfg = Config::from_args(args).unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    Box { addr, heads }
}

fn code_of(head: &str) -> u16 {
    head.split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// One plain request; returns (status, head, body).
fn req(a: SocketAddr, raw: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let t = String::from_utf8_lossy(&out).to_string();
    let (h, b) = t.split_once("\r\n\r\n").unwrap_or((&t, ""));
    (code_of(h), h.to_string(), b.to_string())
}

fn health(a: SocketAddr, host: &str, extra: &str) -> (u16, String, String) {
    req(
        a,
        &format!(
            "GET /api/stt/health HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"
        ),
    )
}

/// Open the WebSocket through the console. Returns the socket and the
/// reply head (and any bytes after it).
fn open(a: SocketAddr, host: &str, extra: &str) -> (TcpStream, String, Vec<u8>) {
    let mut s = TcpStream::connect(a).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        s,
        "GET /api/stt/stream HTTP/1.1\r\nHost: {host}\r\n{extra}{WS}\r\n"
    )
    .unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break buf.len(),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let rest = buf[end..].to_vec();
    (s, head, rest)
}

fn paired() -> String {
    format!("{ORIGIN}Cookie: galion_admin={TOKEN}\r\n")
}

fn read_until(s: &mut TcpStream, have: &mut Vec<u8>, want: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut chunk = [0u8; 4096];
    while !have.windows(want.len()).any(|w| w == want) {
        assert!(Instant::now() < deadline, "timed out waiting for {want:?}");
        match s.read(&mut chunk) {
            Ok(0) => panic!("closed before {want:?}"),
            Ok(n) => have.extend_from_slice(&chunk[..n]),
            Err(_) => {}
        }
    }
}

#[test]
fn unpaired_requests_get_401_before_galion_stt() {
    let b = start(None, &[]);
    let (c, _, _) = health(b.addr, HOST, "");
    assert_eq!(c, 401);
    let (c, _, _) = health(b.addr, HOST, "Cookie: galion_admin=wrong\r\n");
    assert_eq!(c, 401);
    let (_, head, _) = open(b.addr, HOST, ORIGIN);
    assert_eq!(code_of(&head), 401, "{head}");
    let (_, head, _) = open(
        b.addr,
        HOST,
        &format!("{ORIGIN}Cookie: galion_admin=wrong\r\n"),
    );
    assert_eq!(code_of(&head), 401);
    assert!(b.heads.lock().unwrap().is_empty(), "galion-stt was reached");
}

#[test]
fn health_answers_paired_and_on_loopback() {
    let b = start(None, &[]);
    for (host, extra) in [("127.0.0.1", String::new()), (HOST, paired())] {
        let (c, h, body) = health(b.addr, host, &extra);
        assert_eq!(c, 200, "{host}: {body}");
        assert!(h.to_ascii_lowercase().contains("application/json"));
        assert!(body.contains(r#""ok":true"#), "{body}");
        assert!(body.contains("moonshine-tiny-streaming"), "{body}");
        assert!(body.contains(r#""max_sessions":2"#), "{body}");
    }
    let (c, _, _) = req(
        b.addr,
        "POST /api/stt/health HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(c, 405);
    let (c, _, body) = req(
        b.addr,
        "GET /api/stt/nope HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    assert_eq!(c, 404, "{body}");
}

#[test]
fn tunnel_is_transparent_and_strips_browser_identity() {
    let b = start(None, &[]);
    let (mut s, head, mut rest) = open(b.addr, HOST, &paired());
    assert_eq!(code_of(&head), 101, "{head}");
    assert!(
        head.contains("Sec-WebSocket-Accept: mock-accept-value"),
        "{head}"
    );
    assert!(head.to_ascii_lowercase().contains("upgrade: websocket"));
    read_until(&mut s, &mut rest, b"HELLO|");
    // Binary "audio" then the stop text both reach galion-stt and echo back.
    let pcm: Vec<u8> = (0..1600u32).map(|i| (i % 251) as u8).collect();
    s.write_all(&pcm).unwrap();
    s.write_all(br#"{"type":"stop"}"#).unwrap();
    read_until(&mut s, &mut rest, br#"{"type":"stop"}"#);
    assert!(rest.windows(pcm.len()).any(|w| w == pcm.as_slice()));
    drop(s);
    let heads = b.heads.lock().unwrap().clone();
    let up = heads
        .iter()
        .find(|h| h.starts_with("GET /stream "))
        .expect("upstream saw the stream");
    let low = up.to_ascii_lowercase();
    assert!(!low.contains("cookie"), "cookie forwarded: {up}");
    assert!(!low.contains("origin"), "origin forwarded: {up}");
    assert!(!up.contains(TOKEN));
    assert!(low.contains("sec-websocket-key: dghlihnhbxbszsbub25jzq=="));
}

#[test]
fn origin_and_handshake_are_checked() {
    let b = start(None, &[]);
    let cookie = format!("Cookie: galion_admin={TOKEN}\r\n");
    for (extra, want) in [
        (format!("Origin: https://evil.example\r\n{cookie}"), 403),
        (
            format!("Origin: https://console.galion.app.evil.example\r\n{cookie}"),
            403,
        ),
        (cookie.clone(), 403), // a browser always sends Origin
    ] {
        let (_, head, _) = open(b.addr, HOST, &extra);
        assert_eq!(code_of(&head), want, "{extra}: {head}");
    }
    // A local tool on loopback may omit Origin.
    let (_, head, _) = open(b.addr, "127.0.0.1", "");
    assert_eq!(code_of(&head), 101, "{head}");
    // Not an upgrade at all.
    let (c, _, body) = req(
        b.addr,
        &format!(
            "GET /api/stt/stream HTTP/1.1\r\nHost: {HOST}\r\n{}Connection: close\r\n\r\n",
            paired()
        ),
    );
    assert_eq!(c, 426, "{body}");
    assert!(body.contains("WebSocket"));
    let refused = b
        .heads
        .lock()
        .unwrap()
        .iter()
        .filter(|h| h.starts_with("GET /stream "))
        .count();
    assert_eq!(refused, 1, "only the loopback tool reached galion-stt");
}

#[test]
fn session_cap_holds_and_frees() {
    let b = start(None, &["--stt-max-sessions", "1"]);
    let (first, head, _) = open(b.addr, HOST, &paired());
    assert_eq!(code_of(&head), 101);
    let (_, head, rest) = open(b.addr, HOST, &paired());
    assert_eq!(code_of(&head), 429, "{head}");
    assert!(String::from_utf8_lossy(&rest).contains("too many dictation sessions"));
    let (_, _, body) = health(b.addr, "127.0.0.1", "");
    assert!(body.contains(r#""sessions":1"#), "{body}");
    drop(first);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (s, head, _) = open(b.addr, HOST, &paired());
        if code_of(&head) == 101 {
            drop(s);
            break;
        }
        assert!(Instant::now() < deadline, "slot never freed: {head}");
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn stt_offline_is_clear() {
    let dead = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let b = start(Some(dead), &[]);
    let (c, _, body) = health(b.addr, "127.0.0.1", "");
    assert_eq!(c, 200);
    assert!(
        body.contains(r#""ok":false"#) && body.contains("STT offline"),
        "{body}"
    );
    let (_, head, rest) = open(b.addr, HOST, &paired());
    assert_eq!(code_of(&head), 502, "{head}");
    assert!(String::from_utf8_lossy(&rest).contains("STT offline"));
    // The failed attempt released its slot.
    let (_, _, body) = health(b.addr, "127.0.0.1", "");
    assert!(body.contains(r#""sessions":0"#), "{body}");
}

#[test]
fn stt_flags_are_checked() {
    let parse = |extra: &[&str]| {
        Config::from_args(
            ["--ui", "rust", "--bind", "127.0.0.1:0"]
                .iter()
                .chain(extra.iter())
                .map(|s| s.to_string())
                .collect::<Vec<String>>(),
        )
    };
    assert!(
        parse(&["--stt", "127.0.0.1:8650"]).is_err(),
        "needs the API gate"
    );
    let c = parse(&["--local-api", "--stt", "127.0.0.1:8650"]).unwrap();
    assert_eq!(c.stt, Some("127.0.0.1:8650".parse().unwrap()));
    assert_eq!(c.stt_max_sessions, 2);
    assert!(
        parse(&["--local-api", "--stt", "10.1.2.3:8650"]).is_err(),
        "loopback only"
    );
    assert!(
        parse(&["--local-api", "--stt-max-sessions", "3"]).is_err(),
        "needs --stt"
    );
    for bad in ["0", "17", "x"] {
        assert!(
            parse(&[
                "--local-api",
                "--stt",
                "127.0.0.1:8650",
                "--stt-max-sessions",
                bad
            ])
            .is_err(),
            "{bad}"
        );
    }
    let c = parse(&[
        "--local-api",
        "--stt",
        "127.0.0.1:8650",
        "--stt-max-sessions",
        "4",
    ])
    .unwrap();
    assert_eq!(c.stt_max_sessions, 4);
}
