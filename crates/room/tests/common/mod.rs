// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Shared helpers for the room integration tests: a fixture data root, a
//! running `room --listen` child that is always killed, and a raw HTTP GET.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A temporary data root laid out like `/workspace`. Removed on drop.
pub struct Root(pub PathBuf);

impl Root {
    pub fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("room-it-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("root");
        let root = Root(dir);
        root.put(
            "galion-light/room/index.html",
            "<!doctype html><title>room</title>",
        );
        root.put("galion-light/docs/guide.md", "# Operator guide\nbody\n");
        root.put("galion-light/docs/secret-keys.md", "# must never show\n");
        root.put(
            "galion-src/galion-app/package.json",
            "{\"name\": \"galion-app\"}",
        );
        root.put(
            "galion-src/galion-app/src/api/route.ts",
            "export const a = 1;\n",
        );
        root.put("galion-src/galion-app/src/OrbScene.tsx", "const orb = 1;\n");
        root.put("galion-src/galion-docs/papers/gaia.md", "# Gaia paper\n");
        root.put("outside/private.md", "# private outside the read roots\n");
        std::os::unix::fs::symlink(
            root.0.join("outside/private.md"),
            root.0.join("galion-src/galion-docs/escape.md"),
        )
        .expect("symlink");
        root
    }

    pub fn put(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(&p, body).expect("write");
        p
    }

    pub fn path(&self, rel: &str) -> String {
        self.0.join(rel).to_string_lossy().into_owned()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A running `room --listen`. Killed and reaped on drop.
pub struct Room {
    pub child: Child,
    pub addr: String,
    pub objects_line: String,
}

impl Room {
    /// Starts `bin --listen` with `ROOM_BIND=bind`, `ROOM_ROOT=root`, and waits
    /// (up to 20 s) for the `listening on` line on stderr.
    pub fn start(bin: &str, bind: &str, root: &Path) -> Room {
        Self::start_with(Command::new(bin).arg("--listen"), bind, root)
    }

    pub fn start_with(cmd: &mut Command, bind: &str, root: &Path) -> Room {
        let mut child = cmd
            .env("ROOM_BIND", bind)
            .env("ROOM_ROOT", root)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn room");
        let stderr = child.stderr.take().expect("stderr");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut objects_line = String::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(20)) {
                Ok(line) if line.starts_with("room: listening on ") => {
                    let addr = line.trim_start_matches("room: listening on ").to_string();
                    return Room {
                        child,
                        addr,
                        objects_line,
                    };
                }
                Ok(line) => objects_line = line,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("room did not report listening; last stderr: {objects_line}");
                }
            }
        }
    }
}

impl Drop for Room {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A parsed HTTP response.
pub struct Reply {
    pub status: u16,
    pub head: String,
    pub body: String,
}

/// Sends a raw request and reads the full reply (the server closes).
pub fn raw(addr: &str, request: &str) -> Reply {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    Reply {
        status,
        head: head.to_string(),
        body: body.to_string(),
    }
}

pub fn get(addr: &str, path: &str) -> Reply {
    raw(addr, &format!("GET {path} HTTP/1.1\r\nHost: room\r\n\r\n"))
}

/// Percent-encodes everything but unreserved characters and `/`.
pub fn enc(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'/' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The full session every listener test runs: health, page, reader, and the
/// isolation rules of the reader. Panics with a message on the first failure.
pub fn check_session(addr: &str, root: &Root) {
    let health = get(addr, "/health");
    assert_eq!((health.status, health.body.as_str()), (200, "ok\n"));
    assert!(health.head.contains("Connection: close"));

    let index = get(addr, "/");
    assert_eq!(index.status, 200);
    assert!(index.body.contains("<title>room</title>"));

    let objects = get(addr, "/api/objects");
    assert_eq!(objects.status, 200);
    assert!(objects.head.contains("Content-Type: application/json"));
    let page = &objects.body;
    assert!(page.starts_with("{\"objects\":[") && page.ends_with("]}"));
    assert!(page.contains("\"name\":\"room\""));
    assert!(page.contains("\"note\":\"Operator guide\""));
    assert!(page.contains("\"kind\":\"library\",\"name\":\"galion-app\""));
    assert!(page.contains("\"kind\":\"api\""));
    assert!(page.contains("\"kind\":\"light\""));
    assert!(page.contains("\"kind\":\"paper\""));
    assert!(!page.contains("secret-keys"), "secret-looking file listed");
    assert!(!page.contains("private outside"), "symlink followed");

    let guide = root.path("galion-light/docs/guide.md");
    let read = get(addr, &format!("/api/read?path={}", enc(&guide)));
    assert_eq!(read.status, 200);
    assert!(read.body.starts_with("# Operator guide"));

    for (what, path) in [
        ("secret", root.path("galion-light/docs/secret-keys.md")),
        ("outside root", root.path("outside/private.md")),
        (
            "symlink escape",
            root.path("galion-src/galion-docs/escape.md"),
        ),
        (
            "dotdot",
            format!("{}/../outside/private.md", root.path("galion-src")),
        ),
        (
            "non-doc",
            root.path("galion-src/galion-app/src/api/route.ts"),
        ),
        ("system file", "/etc/passwd".to_string()),
    ] {
        let r = get(addr, &format!("/api/read?path={}", enc(&path)));
        assert_eq!((r.status, r.body.as_str()), (404, "held\n"), "{what}");
    }
    // Malformed escapes must not crash the server (it aborts on panic).
    assert_eq!(get(addr, "/api/read?path=%a\u{e9}%").status, 404);
    assert_eq!(raw(addr, "POST /api/objects HTTP/1.1\r\n\r\n").status, 405);
    assert_eq!(raw(addr, "\r\n").status, 400);
    assert_eq!(get(addr, "/../../etc/passwd").status, 404);
    // Still alive after the hostile requests.
    assert_eq!(get(addr, "/health").status, 200);
}
