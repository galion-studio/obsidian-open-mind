// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Hermetic parity: every request in `crates/tuner/tests/fixtures/requests.txt` run
//! through `tuner::respond` with the bundled www must give the exact bytes the
//! live tuner on the VPS gave (`tests/fixtures/live/<name>.http`). No network.

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `printf` subset the request file uses: `\r`, `\n`, `\\`.
fn unescape(fmt: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = fmt.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut tmp = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
            continue;
        }
        match chars.next() {
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('\\') => out.push(b'\\'),
            other => panic!("unsupported escape {other:?} in {fmt}"),
        }
    }
    out
}

fn matrix() -> Vec<(String, Vec<u8>)> {
    let path = manifest().join("tests/fixtures/requests.txt");
    let text = fs::read_to_string(&path).expect("request matrix");
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (name, fmt) = l.split_once(char::is_whitespace).expect("name fmt");
            (name.to_string(), unescape(fmt.trim()))
        })
        .collect()
}

#[test]
fn every_route_matches_the_live_bytes() {
    let www = manifest().join("www");
    let www = www.to_str().unwrap();
    let cases = matrix();
    assert!(cases.len() >= 20, "matrix too small: {}", cases.len());
    for (name, req) in cases {
        let want = fs::read(manifest().join(format!("tests/fixtures/live/{name}.http")))
            .unwrap_or_else(|e| panic!("fixture {name}: {e}"));
        let got = tuner::respond(&req, www).expect("live answered, so must we");
        assert!(
            got == want,
            "{name}: {} bytes vs live {} bytes\n--- got\n{}\n--- live\n{}",
            got.len(),
            want.len(),
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&want)
        );
    }
}

/// HEAD and non-GET methods were not sent to the live server (GET-only rule).
/// Their expected bytes are derived from the live GET fixtures and the live
/// source (`/opt/galion/tuner/src/main.rs`): HEAD is the GET header block with
/// no body; every other method is a 405 whose header has no HEAD stripping.
#[test]
fn head_is_the_live_get_header_and_others_are_405() {
    let www = manifest().join("www");
    let www = www.to_str().unwrap();
    for (name, path) in [
        ("root", "/"),
        ("app-js", "/app.js"),
        ("health", "/health"),
        ("nope", "/nope"),
    ] {
        let live = fs::read(manifest().join(format!("tests/fixtures/live/{name}.http"))).unwrap();
        let split = live.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let req = format!("HEAD {path} HTTP/1.1\r\n\r\n");
        let got = tuner::respond(req.as_bytes(), www).unwrap();
        assert_eq!(got, live[..split].to_vec(), "HEAD {path}");
    }
    let nope = fs::read(manifest().join("tests/fixtures/live/nope.http")).unwrap();
    let nope = String::from_utf8(nope).unwrap();
    for method in ["POST", "PUT", "DELETE", "OPTIONS", "get", "BREW"] {
        let req = format!("{method} /health HTTP/1.1\r\n\r\n");
        let got = String::from_utf8(tuner::respond(req.as_bytes(), www).unwrap()).unwrap();
        let want = nope
            .replace("404 Not Found", "405 Method Not Allowed")
            .replace("Content-Length: 10", "Content-Length: 19")
            .replace("not found\n", "method not allowed\n");
        assert_eq!(got, want, "{method}");
    }
}

#[test]
fn empty_read_gets_no_answer_like_live() {
    assert!(tuner::respond(b"", "/nonexistent").is_none());
}
