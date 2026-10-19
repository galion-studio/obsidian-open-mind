// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! The binary on a spare loopback port. Does not bind 8650 and does not touch
//! the live tuner.

#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Option<Child>,
}

impl Server {
    fn stop(&mut self) -> String {
        let Some(mut child) = self.child.take() else {
            return String::new();
        };
        let _ = child.kill();
        let mut log = String::new();
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut log);
        }
        let _ = child.wait();
        log
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tuner-b53-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn spare_port() -> u16 {
    for _ in 0..8 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        if port != 8650 {
            return port;
        }
    }
    panic!("could not reserve a port other than 8650");
}

fn get(port: u16, path: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = String::from("no connect");
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let req =
                format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
            if stream.write_all(req.as_bytes()).is_err() {
                last = String::from("write failed");
                thread_sleep();
                continue;
            }
            let mut body = String::new();
            match stream.read_to_string(&mut body) {
                Ok(_) if !body.is_empty() => return body,
                Ok(_) => last = String::from("empty"),
                Err(err) if body.is_empty() => last = err.to_string(),
                Err(_) => return body,
            }
        }
        thread_sleep();
    }
    panic!("no response on 127.0.0.1:{port}: {last}");
}

fn thread_sleep() {
    std::thread::sleep(Duration::from_millis(30));
}

#[test]
fn config_file_root_and_env_addr_serve_that_root() {
    let dir = scratch("serve");
    let root = dir.join("www");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("index.html"), b"B53-TEMP-ROOT-MARKER\n").unwrap();
    let root_s = root.to_str().unwrap();
    let cfg = dir.join("tuner.conf");
    fs::write(&cfg, format!("root = \"{root_s}\"\n")).unwrap();
    let port = spare_port();
    let mut server = Server {
        child: Some(
            Command::new(env!("CARGO_BIN_EXE_tuner"))
                .env("TUNER_ADDR", format!("127.0.0.1:{port}"))
                .env("TUNER_CONFIG", &cfg)
                .env_remove("TUNER_ROOT")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ),
    };

    let page = get(port, "/");
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
    assert!(
        page.contains("Content-Type: text/html; charset=utf-8\r\n"),
        "{page}"
    );
    assert!(page.ends_with("B53-TEMP-ROOT-MARKER\n"), "{page}");
    let health = get(port, "/health");
    assert!(health.ends_with("\r\n\r\n{\"ok\":true}\n"), "{health}");
    let missing = get(port, "/nope");
    assert!(
        missing.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "{missing}"
    );

    let log = server.stop();
    assert!(
        log.contains(&format!("tuner 127.0.0.1:{port} serving {root_s}")),
        "{log}"
    );
}
