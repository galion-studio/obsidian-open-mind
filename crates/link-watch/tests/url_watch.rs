// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `LINK_WATCH_URLS` end to end with the built binary against a loopback
//! HTTP server: the trailing `urls` key, statuses, the per-URL rate limit
//! across separate `LINK_WATCH_ONCE` runs, and refusal of bad lists.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};

const GOLDEN_PLUS_URLS: [&str; 11] = [
    "at",
    "at_local",
    "laptop_port",
    "model",
    "model_ms",
    "hermes",
    "hermes_ms",
    "france",
    "laptop",
    "events",
    "urls",
];

fn out_dir(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/link-watch-urls")
        .join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Loopback server: `/ok` 200, `/moved` 301, anything else 404. Records paths.
fn serve() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut s = stream;
            let mut buf = [0u8; 2048];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = req.lines().next().unwrap_or("").to_string();
            log.lock().unwrap().push(line.clone());
            let path = line.split_whitespace().nth(1).unwrap_or("/");
            let status = match path {
                "/ok" => "200 OK",
                "/moved" => "301 Moved Permanently\r\nLocation: /ok",
                _ => "404 Not Found",
            };
            let _ = s.write_all(
                format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi")
                    .as_bytes(),
            );
        }
    });
    (port, seen)
}

fn run(dir: &std::path::Path, urls: &str, every: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_link-watch"));
    cmd.env("LINK_WATCH_ONCE", "1")
        .env("LINK_WATCH_OUT", dir.join("link.json"))
        .env("LINK_WATCH_LAPTOP", dir.join("laptop.json"))
        .env("LINK_WATCH_LOG", dir.join("log.json"))
        .env("LINK_WATCH_OLLAMA_URL", "http://127.0.0.1:1")
        .env("LINK_WATCH_HERMES_URL", "http://127.0.0.1:1")
        .env("OPENMIND_CONFIG", dir.join("missing.toml"))
        .env_remove("LINK_WATCH_FRANCE_URL")
        .env("LINK_WATCH_URLS", urls);
    match every {
        Some(e) => cmd.env("LINK_WATCH_URLS_EVERY", e),
        None => cmd.env_remove("LINK_WATCH_URLS_EVERY"),
    };
    cmd.output().unwrap()
}

fn read(dir: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join("link.json")).unwrap()).unwrap()
}

#[test]
fn urls_key_reports_each_status_and_is_last() {
    let (port, seen) = serve();
    let dir = out_dir("statuses");
    let list = format!(
        "http://127.0.0.1:{port}/ok, http://127.0.0.1:{port}/moved http://127.0.0.1:{port}/gone"
    );
    let out = run(&dir, &list, None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v = read(&dir);
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, GOLDEN_PLUS_URLS);
    let urls = v["urls"].as_array().unwrap();
    assert_eq!(urls.len(), 3);
    assert_eq!(
        (urls[0]["status"].as_u64(), urls[0]["ok"].as_bool()),
        (Some(200), Some(true))
    );
    assert_eq!(
        (urls[1]["status"].as_u64(), urls[1]["ok"].as_bool()),
        (Some(301), Some(true))
    );
    assert_eq!(
        (urls[2]["status"].as_u64(), urls[2]["ok"].as_bool()),
        (Some(404), Some(false))
    );
    let stamp = urls[0]["checked_at_local"].as_str().unwrap();
    assert!(
        stamp.ends_with("+02:00") || stamp.ends_with("+01:00"),
        "{stamp}"
    );
    // Redirects are reported, not followed.
    let lines = seen.lock().unwrap().clone();
    assert_eq!(
        lines.iter().filter(|l| l.contains("/ok")).count(),
        1,
        "{lines:?}"
    );
    assert!(
        lines.iter().all(|l| l.starts_with("GET ")),
        "GET only: {lines:?}"
    );
}

#[test]
fn rate_limit_holds_across_once_runs() {
    let (port, seen) = serve();
    let dir = out_dir("rate");
    let list = format!("http://127.0.0.1:{port}/ok");
    for _ in 0..3 {
        assert!(run(&dir, &list, Some("1")).status.success());
    }
    // Interval "1" is raised to the 60 s floor: one GET for three runs.
    assert_eq!(seen.lock().unwrap().len(), 1, "{:?}", seen.lock().unwrap());
    assert_eq!(read(&dir)["urls"][0]["status"], 200);

    // A URL added later is due at once; the old one still is not.
    let list2 = format!("{list},http://127.0.0.1:{port}/other");
    assert!(run(&dir, &list2, None).status.success());
    let lines = seen.lock().unwrap().clone();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[1].contains("/other"));
    assert_eq!(read(&dir)["urls"].as_array().unwrap().len(), 2);
}

#[test]
fn unset_urls_keeps_the_historical_key_set() {
    let dir = out_dir("unset");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_link-watch"));
    cmd.env("LINK_WATCH_ONCE", "1")
        .env("LINK_WATCH_OUT", dir.join("link.json"))
        .env("LINK_WATCH_LAPTOP", dir.join("laptop.json"))
        .env("LINK_WATCH_LOG", dir.join("log.json"))
        .env("LINK_WATCH_OLLAMA_URL", "http://127.0.0.1:1")
        .env("LINK_WATCH_HERMES_URL", "http://127.0.0.1:1")
        .env("OPENMIND_CONFIG", dir.join("missing.toml"))
        .env_remove("LINK_WATCH_URLS");
    assert!(cmd.status().unwrap().success());
    assert!(read(&dir).get("urls").is_none());
}

#[test]
fn bad_url_lists_are_refused_before_any_write() {
    let dir = out_dir("bad");
    for bad in [
        "ftp://example.com/",
        "https://user:pw@example.com/",
        "nonsense",
    ] {
        let out = run(&dir, bad, None);
        assert_eq!(out.status.code(), Some(1), "{bad}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("LINK_WATCH_URLS"), "{err}");
        assert!(!err.contains("pw@"), "credentials echoed: {err}");
        assert!(!dir.join("link.json").exists());
    }
}
