// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (GALION_REAL=1): the release binary against a local mock of the
//! Replicate HTTP API on 127.0.0.1:21400 (slot 270 ports 21400-21409). The real
//! Replicate API is paid and is never called. curl is the real system curl.
//! While each request is served, the mock scans every /proc/*/cmdline for the
//! token: it must never appear on any argv.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "r8_TESTONLY_slot270_not_a_real_token";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake-picture-bytes";

#[derive(Default, Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: Option<String>,
    prefer: Option<String>,
    body: String,
    token_on_argv: bool,
}

type Log = Arc<Mutex<Vec<Seen>>>;

fn token_on_any_argv() -> bool {
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return false;
    };
    rd.flatten().any(|e| {
        std::fs::read(e.path().join("cmdline"))
            .map(|b| String::from_utf8_lossy(&b).contains(TOKEN))
            .unwrap_or(false)
    })
}

fn serve(port: u16, script: &'static str) -> (Log, std::thread::JoinHandle<()>) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind mock");
    let log: Log = Arc::default();
    let lg = log.clone();
    let h = std::thread::spawn(move || {
        let mut polls = 0;
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let seen = read_req(&s);
            if seen.path == "/quit" {
                break;
            }
            let (code, body): (u16, Vec<u8>) = route(script, &seen, &mut polls, port);
            lg.lock().unwrap().push(seen);
            reply(s, code, &body);
        }
    });
    (log, h)
}

fn route(script: &str, r: &Seen, polls: &mut u32, port: u16) -> (u16, Vec<u8>) {
    let base = format!("http://127.0.0.1:{port}");
    if r.path == "/files/pic.png" {
        return (200, PNG.to_vec());
    }
    let dev = r.path.ends_with("flux-dev/predictions");
    let schnell = r.path.ends_with("flux-schnell/predictions");
    match script {
        "fallback-poll" if dev => (422, b"{\"detail\":\"bad\ninput\"}".to_vec()),
        "fallback-poll" if schnell => (
            201,
            format!(
                "{{\"status\":\"processing\",\"urls\":{{\"get\":\"{base}/v1/predictions/p1\"}}}}"
            )
            .into_bytes(),
        ),
        "fallback-poll" if r.path == "/v1/predictions/p1" => {
            *polls += 1;
            if *polls < 2 {
                (200, b"{\"status\":\"processing\"}".to_vec())
            } else {
                (
                    200,
                    format!("{{\"status\":\"succeeded\",\"output\":[\"{base}/files/pic.png\"]}}")
                        .into_bytes(),
                )
            }
        }
        "direct" if dev => (
            201,
            format!("{{\"status\":\"succeeded\",\"output\":\"{base}/files/pic.png\"}}")
                .into_bytes(),
        ),
        "all-fail" => (
            500,
            format!(
                "{{\"err\":\"down {}\"}}",
                if dev { "dev" } else { "schnell" }
            )
            .into_bytes(),
        ),
        "failed" if dev => (
            201,
            format!(
                "{{\"status\":\"starting\",\"urls\":{{\"get\":\"{base}/v1/predictions/p2\"}}}}"
            )
            .into_bytes(),
        ),
        "failed" if r.path == "/v1/predictions/p2" => (
            200,
            b"{\"status\":\"failed\",\"error\":\"NSFW content detected\"}".to_vec(),
        ),
        _ => (404, b"{\"detail\":\"not found\"}".to_vec()),
    }
}

fn read_req(s: &TcpStream) -> Seen {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let mut it = line.split_whitespace();
    let mut seen = Seen {
        method: it.next().unwrap_or("").into(),
        path: it.next().unwrap_or("").into(),
        ..Default::default()
    };
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((&h, ""));
        let v = v.trim().to_string();
        match k.to_ascii_lowercase().as_str() {
            "authorization" => seen.auth = Some(v),
            "prefer" => seen.prefer = Some(v),
            "content-length" => len = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    let mut b = vec![0; len];
    r.read_exact(&mut b).unwrap();
    seen.body = String::from_utf8_lossy(&b).into();
    seen.token_on_argv = token_on_any_argv();
    seen
}

fn reply(mut s: TcpStream, code: u16, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn quit(port: u16) {
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
        let _ = s.write_all(b"GET /quit HTTP/1.1\r\n\r\n");
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-replicate-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(port: u16, dir: &Path, key: Option<&str>) -> (i32, String, String) {
    let keyf = dir.join("replicate.key");
    if let Some(k) = key {
        std::fs::write(&keyf, format!("{k}\n")).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_replicate-picture"))
        .args([
            "--prompt",
            "a \"quoted\" galleon\nat dawn",
            "--aspect",
            "16:9",
            "--poll-ms",
            "50",
        ])
        .arg("--out")
        .arg(dir.join("sub/dir/pic.png"))
        .arg("--key-file")
        .arg(&keyf)
        .arg("--api")
        .arg(format!("http://127.0.0.1:{port}/v1"))
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into(),
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

fn real() -> bool {
    std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
}

#[test]
fn real_replicate_picture_scenarios() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    // S1: flux-dev 422 -> flux-schnell processing -> poll x2 -> download.
    let port = 21400;
    let dir = tmp("s1");
    let (log, h) = serve(port, "fallback-poll");
    let (code, out, err) = run(port, &dir, Some(TOKEN));
    quit(port);
    h.join().unwrap();
    let dest = dir.join("sub/dir/pic.png");
    assert_eq!(code, 0, "stderr: {err}");
    assert_eq!(
        out,
        format!("black-forest-labs/flux-schnell\n{}\n", dest.display())
    );
    assert_eq!(std::fs::read(&dest).unwrap(), PNG);
    assert!(!out.contains(TOKEN) && !err.contains(TOKEN));
    let log = log.lock().unwrap().clone();
    let paths: Vec<_> = log
        .iter()
        .map(|s| format!("{} {}", s.method, s.path))
        .collect();
    assert_eq!(
        paths,
        [
            "POST /v1/models/black-forest-labs/flux-dev/predictions",
            "POST /v1/models/black-forest-labs/flux-schnell/predictions",
            "GET /v1/predictions/p1",
            "GET /v1/predictions/p1",
            "GET /files/pic.png",
        ]
    );
    for s in &log[..4] {
        assert_eq!(s.auth.as_deref(), Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(s.prefer.as_deref(), Some("wait=60"));
    }
    assert_eq!(log[4].auth, None, "download must not carry the token");
    assert!(
        log.iter().all(|s| !s.token_on_argv),
        "token seen on a process argv"
    );
    let body: serde_json::Value = serde_json::from_str(&log[0].body).unwrap();
    assert_eq!(body["input"]["prompt"], "a \"quoted\" galleon\nat dawn");
    assert_eq!(body["input"]["aspect_ratio"], "16:9");
    assert_eq!(body["input"]["output_format"], "png");
    assert_eq!(body["input"]["num_outputs"], 1);
    eprintln!("S1 ok: {paths:?}");

    // S2: direct output string from flux-dev, one call + download.
    let port = 21401;
    let dir = tmp("s2");
    let (log, h) = serve(port, "direct");
    let (code, out, err) = run(port, &dir, Some(TOKEN));
    quit(port);
    h.join().unwrap();
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("black-forest-labs/flux-dev\n"));
    assert_eq!(log.lock().unwrap().len(), 2);
    eprintln!("S2 ok");

    // S3: both models 500 -> exit 1 with the LAST error (schnell), Python format.
    let port = 21402;
    let dir = tmp("s3");
    let (_log, h) = serve(port, "all-fail");
    let (code, out, err) = run(port, &dir, Some(TOKEN));
    quit(port);
    h.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(out, "");
    assert_eq!(err.trim(), "500 {\"err\":\"down schnell\"}");
    assert!(!dir.join("sub/dir/pic.png").exists());
    eprintln!("S3 ok: {}", err.trim());

    // S4: prediction fails while polling -> exit 1 with the prediction error, no fallback.
    let port = 21403;
    let dir = tmp("s4");
    let (log, h) = serve(port, "failed");
    let (code, _out, err) = run(port, &dir, Some(TOKEN));
    quit(port);
    h.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(err.trim(), "NSFW content detected");
    assert_eq!(log.lock().unwrap().len(), 2);
    eprintln!("S4 ok: {}", err.trim());

    // S5: no key file -> exit 1, no request made.
    let port = 21404;
    let dir = tmp("s5");
    let (log, h) = serve(port, "direct");
    let (code, _out, err) = run(port, &dir, None);
    quit(port);
    h.join().unwrap();
    assert_eq!(code, 1);
    assert_eq!(err.trim(), "missing replicate key file");
    assert!(log.lock().unwrap().is_empty());
    eprintln!("S5 ok");

    // S6: API unreachable (nothing on 21405) -> transport error, exit 1.
    let dir = tmp("s6");
    let (code, _out, err) = run(21405, &dir, Some(TOKEN));
    assert_eq!(code, 1);
    assert!(err.starts_with("URLError"), "{err}");
    assert!(!err.contains(TOKEN));
    eprintln!("S6 ok: {}", err.trim());
    for n in ["s1", "s2", "s3", "s4", "s5", "s6"] {
        let _ = std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("real-replicate-{n}-{}", std::process::id())),
        );
    }
}
