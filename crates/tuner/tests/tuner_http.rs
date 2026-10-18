// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// End-to-end: the tuner server on a spare port, and tuner-publish against a
// fake cloudflared (a shell script under target/test-out) that prints a quick
// tunnel URL and exits. No network, no real cloudflared.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn out_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn server_serves_page_and_health() {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_tuner"))
        .env("TUNER_ADDR", format!("127.0.0.1:{port}"))
        .env("TUNER_ROOT", concat!(env!("CARGO_MANIFEST_DIR"), "/www"))
        .spawn()
        .unwrap();
    let get = |path: &str| -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
                s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .unwrap();
                let mut o = String::new();
                let _ = s.read_to_string(&mut o);
                return o;
            }
            assert!(Instant::now() < deadline, "tuner did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    assert!(get("/health").ends_with("\r\n\r\n{\"ok\":true}\n"));
    assert!(get("/pitch.js")
        .starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/javascript; charset=utf-8\r\n"));
    assert!(get("/secret").starts_with("HTTP/1.1 404 Not Found"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn publisher_writes_url_then_removes_it() {
    let d = out_dir("tuner-publish");
    let fake = d.join("cloudflared");
    let seen = d.join("seen.json");
    let url_file = d.join("url.json");
    // The fake copies the URL file (written by the publisher) before exiting.
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"args: $*\" >&2\necho 'INF |  https://test-quick-name.trycloudflare.com  |' >&2\nsleep 1\ncp {} {} 2>/dev/null\nexit 0\n",
            url_file.display(),
            seen.display()
        ),
    )
    .unwrap();
    Command::new("chmod").arg("+x").arg(&fake).status().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tuner-publish"))
        .env("TUNER_CLOUDFLARED", &fake)
        .env("TUNER_ADDR", "127.0.0.1:18650")
        .env("TUNER_URL_FILE", &url_file)
        .env("TUNER_PUBLISH_ONCE", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(
        log.contains("args: tunnel --no-autoupdate --url http://127.0.0.1:18650"),
        "{log}"
    );
    assert!(
        log.contains("public URL https://test-quick-name.trycloudflare.com"),
        "{log}"
    );
    let seen = std::fs::read_to_string(&seen).unwrap();
    assert!(seen.starts_with("{\"url\":\"https://test-quick-name.trycloudflare.com\",\"local\":\"http://127.0.0.1:18650\",\"at\":\"2"), "{seen}");
    assert!(seen.contains("+0"), "Zagreb offset expected: {seen}");
    assert!(!url_file.exists(), "dead URL must be removed");

    let dry = Command::new(env!("CARGO_BIN_EXE_tuner-publish"))
        .arg("--dry-run")
        .env_remove("TUNER_ADDR")
        .env_remove("TUNER_PUBLISH_LOCAL")
        .env_remove("TUNER_CLOUDFLARED")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&dry.stdout)
        .starts_with("cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650\n"));
}
