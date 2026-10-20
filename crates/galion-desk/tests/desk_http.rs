// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Starts the built galion-desk on a spare loopback port with every path pointed
// at target/test-out and checks /health, /desk and one POST /desk act. Never
// touches the live state directory or port 8646.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spare_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn req(port: u16, raw: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            s.write_all(raw.as_bytes()).unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            return out;
        }
        assert!(Instant::now() < deadline, "desk did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn health_desk_and_note() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test-out/desk");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(out.join("state")).unwrap();
    std::fs::create_dir_all(out.join("one")).unwrap();
    std::fs::write(out.join("one/voice"), "jarvis\n").unwrap();
    let port = spare_port();
    let _child = Kill(
        Command::new(env!("CARGO_BIN_EXE_galion-desk"))
            .env("DESK_BIND", format!("127.0.0.1:{port}"))
            .env("DESK_STATE_DIR", out.join("state"))
            .env("DESK_ONE_DIR", out.join("one"))
            .env("DESK_HERMES_DIR", out.join("hermes"))
            .env("DESK_OLLAMA", "127.0.0.1:1")
            .env("DESK_HERMES_BIN", "/bin/false")
            .spawn()
            .unwrap(),
    );
    let health = req(port, "GET /health HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(health.starts_with("HTTP/1.1 200"), "{health}");
    assert!(health.ends_with("{\"ok\":true}"), "{health}");

    let desk = req(port, "GET /desk HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(desk.contains("\"foundation\":\"rust\""), "{desk}");
    assert!(desk.contains("\"voice\":\"jarvis\""));
    assert!(desk.contains("\"line\":false"));
    assert!(desk.contains("\"mindpack\":{}"));
    assert!(desk.contains("{\"name\":\"Laptop\",\"up\":false}"));

    let body = r#"{"act":"note","text":"hello desk"}"#;
    let post = format!(
        "POST /desk HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let after = req(port, &post);
    assert!(after.contains("\"text\":\"hello desk\""), "{after}");
    let notes = std::fs::read_to_string(out.join("state/desk-notes.jsonl")).unwrap();
    assert!(notes.contains("hello desk"));

    let missing = req(port, "GET /nope HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(missing.starts_with("HTTP/1.1 404"));
}
