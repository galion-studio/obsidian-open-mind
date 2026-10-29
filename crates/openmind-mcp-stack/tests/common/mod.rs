// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Shared test helpers: a temp vault and a mock openmind-memory gateway.

#![allow(dead_code)]

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

static N: AtomicUsize = AtomicUsize::new(0);

/// A temp folder removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "omcp-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Vault layout: notes, a hidden folder, a non-note, an outside secret and
/// a symlink pointing at it.
pub fn make_vault() -> (TempDir, TempDir) {
    let v = TempDir::new("vault");
    let outside = TempDir::new("outside");
    let r = v.path();
    std::fs::create_dir_all(r.join("daily")).unwrap();
    std::fs::create_dir_all(r.join(".obsidian")).unwrap();
    std::fs::create_dir_all(r.join(".git")).unwrap();
    std::fs::write(
        r.join("23-vault.md"),
        "# Vault\nThis folder is the Galion memory vault.\n",
    )
    .unwrap();
    std::fs::write(
        r.join("daily/2026-10-29.md"),
        "Line one\nThe Hermes MCP test note\n",
    )
    .unwrap();
    std::fs::write(r.join("big.md"), "x".repeat(300 * 1024)).unwrap();
    std::fs::write(r.join("image.png"), [0u8, 1, 2]).unwrap();
    std::fs::write(r.join(".obsidian/app.json"), "{\"hidden\":true}").unwrap();
    std::fs::write(r.join(".git/config"), "[core] hidden vault git\n").unwrap();
    std::fs::write(outside.path().join("secret.md"), "OUTSIDE-SECRET-TEXT\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("secret.md"), r.join("escape.md")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), r.join("escape-dir")).unwrap();
    (v, outside)
}

/// Every file under `root` with its bytes (to prove nothing was written).
pub fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(d: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut es: Vec<_> = std::fs::read_dir(d).unwrap().flatten().collect();
        es.sort_by_key(std::fs::DirEntry::file_name);
        for e in es {
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            if m.is_dir() {
                walk(&e.path(), out);
            } else if m.is_file() {
                out.push((e.path(), std::fs::read(e.path()).unwrap()));
            } else {
                out.push((e.path(), b"<link>".to_vec()));
            }
        }
    }
    walk(root, &mut out);
    out
}

/// One request the mock memory saw.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub auth: Option<String>,
    pub body: Value,
}

/// Mock openmind-memory: answers the five routes the server uses, keeps
/// saved texts in memory and returns them from search/recall.
pub struct MockMemory {
    pub url: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
}

pub fn mock_memory(require_key: Option<&str>) -> MockMemory {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let saved: Arc<Mutex<Vec<String>>> = Arc::default();
    let key = require_key.map(String::from);
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for stream in l.incoming().flatten() {
            let mut stream = stream;
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let (head_end, clen) = loop {
                let n = stream.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break (None, 0);
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]).to_string();
                    let clen = head
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    break (Some(p), clen);
                }
            };
            let Some(p) = head_end else { continue };
            while buf.len() < p + 4 + clen {
                let n = stream.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            let head = String::from_utf8_lossy(&buf[..p]).to_string();
            let mut first = head.lines().next().unwrap_or("").split_whitespace();
            let method = first.next().unwrap_or("").to_string();
            let path = first.next().unwrap_or("").to_string();
            let auth = head
                .lines()
                .find_map(|l| l.strip_prefix("Authorization: Bearer ").map(String::from));
            let body: Value =
                serde_json::from_slice(&buf[p + 4..p + 4 + clen]).unwrap_or(Value::Null);
            s2.lock().unwrap().push(Seen {
                method: method.clone(),
                path: path.clone(),
                auth: auth.clone(),
                body: body.clone(),
            });
            let (status, reply) = if path != "/health"
                && key.is_some()
                && auth.as_deref() != key.as_deref()
            {
                (401, json!({"error": "Unauthorized: invalid token"}))
            } else {
                match (method.as_str(), path.as_str()) {
                    ("GET", "/health") => (200, json!({"status": "ok", "implementation": "mock"})),
                    ("POST", "/capture") => {
                        saved
                            .lock()
                            .unwrap()
                            .push(body["user_content"].as_str().unwrap_or("").into());
                        (200, json!({"l0_recorded": 1, "scheduler_notified": false}))
                    }
                    ("POST", "/search/memories")
                    | ("POST", "/search/conversations")
                    | ("POST", "/recall") => {
                        let q = body["query"].as_str().unwrap_or("").to_lowercase();
                        let hits: Vec<String> = saved
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|t| t.to_lowercase().contains(&q))
                            .cloned()
                            .collect();
                        if path == "/recall" {
                            (
                                200,
                                json!({"context": hits.join("\n"), "memory_count": hits.len(), "scope": body["scope"]}),
                            )
                        } else {
                            (
                                200,
                                json!({"results": hits.join("\n"), "total": hits.len()}),
                            )
                        }
                    }
                    _ => (404, json!({"error": "Not found"})),
                }
            };
            let b = reply.to_string();
            let resp = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                b.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    MockMemory { url, seen }
}
