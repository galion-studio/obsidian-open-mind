// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Shared helpers for the galion-desk integration tests: scratch dirs, a raw
// HTTP client, and a fake loopback service that records what it was sent.

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("test-out/galion-desk-it")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("state")).unwrap();
    std::fs::create_dir_all(dir.join("one")).unwrap();
    std::fs::create_dir_all(dir.join("hermes")).unwrap();
    dir
}

/// Send raw bytes, return the whole response. Retries the connect for 5 s.
pub fn raw(addr: &str, req: &[u8]) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(mut s) = TcpStream::connect(addr) {
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            s.write_all(req).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            return out;
        }
        assert!(Instant::now() < deadline, "no listener on {addr}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn get(addr: &str, path: &str) -> String {
    raw(
        addr,
        format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes(),
    )
}

pub fn post(addr: &str, body: &str) -> String {
    raw(
        addr,
        format!(
            "POST /desk HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
}

pub fn body(resp: &str) -> &str {
    resp.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("")
}

pub fn json(resp: &str) -> serde_json::Value {
    serde_json::from_str(body(resp)).unwrap_or_else(|e| panic!("bad json ({e}): {resp}"))
}

/// A loopback service that answers every request with `reply` and records
/// each request (head + body) it saw.
pub struct Fake {
    pub addr: String,
    pub seen: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    pub fn start(reply: &'static str) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let log = Arc::clone(&log);
                std::thread::spawn(move || {
                    let mut s = stream;
                    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    while let Ok(n) = s.read(&mut tmp) {
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                            let want = head
                                .lines()
                                .find_map(|l| {
                                    let (k, v) = l.split_once(':')?;
                                    k.eq_ignore_ascii_case("content-length")
                                        .then(|| v.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if rest.len() >= want {
                                break;
                            }
                        }
                    }
                    log.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf).to_string());
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    let _ = s.write_all(resp.as_bytes());
                });
            }
        });
        Fake { addr, seen }
    }

    pub fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    pub fn wait_for(&self, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.requests().iter().any(|r| r.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

/// A TCP port with nothing listening (bound then dropped).
pub fn closed_addr() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().to_string()
}
