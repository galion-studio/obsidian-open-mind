// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The binary loads a config fixture: probe URLs and the output path come
//! from that file, and `LINK_WATCH_*` still wins.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const GOLDEN: &str = r#"["at","at_local","laptop_port","model","model_ms","hermes","hermes_ms","france","laptop","events"]"#;

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-out/link-watch");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn scrub(cmd: &mut Command) {
    for key in [
        "LINK_WATCH_OUT",
        "LINK_WATCH_LAPTOP",
        "LINK_WATCH_LOG",
        "LINK_WATCH_OLLAMA_URL",
        "LINK_WATCH_HERMES_URL",
        "LINK_WATCH_FRANCE_URL",
        "OPENMIND_CONFIG",
        "OPENMIND_HOSTS_OLLAMA",
        "OPENMIND_HOSTS_HERMES",
        "OPENMIND_HOSTS_REMOTE_PROBE",
        "OPENMIND_PORTS_OLLAMA",
        "OPENMIND_PORTS_HERMES",
        "OPENMIND_PORTS_WHISPER",
        "OPENMIND_PORTS_DOCS",
        "OPENMIND_PORTS_ONE",
        "OPENMIND_PORTS_DESK",
        "OPENMIND_PORTS_CONSOLE",
        "OPENMIND_PATHS_BASE",
        "OPENMIND_LIMITS_MAX_BODY_BYTES",
        "OPENMIND_LIMITS_REQUEST_TIMEOUT_SECS",
        "OPENMIND_LIMITS_MAX_CONNECTIONS",
    ] {
        cmd.env_remove(key);
    }
}

/// Loopback HTTP that answers 200 with a tiny Ollama `/api/ps` body.
fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut accepted = 0u32;
        while accepted < 8 && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut sock, _)) => {
                    accepted += 1;
                    let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
                    drain_headers(&mut sock);
                    let body = br#"{"models":[{"name":"fixture"}]}"#;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes());
                    let _ = sock.write_all(body);
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
    });
    port
}

fn drain_headers(sock: &mut TcpStream) {
    let mut buf = [0u8; 2048];
    let mut filled = 0usize;
    while filled < buf.len() {
        match sock.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => {
                filled += n;
                if buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn write_fixture(name: &str, port: u16, base: &std::path::Path) -> PathBuf {
    let path = out_dir().join(name);
    let body = format!(
        "\
[hosts]
ollama = \"127.0.0.1\"
hermes = \"127.0.0.1\"
remote_probe = \"127.0.0.1\"

[ports]
ollama = {port}
hermes = {port}
console = {port}

[paths]
base = '{base}'
",
        base = base.display()
    );
    std::fs::write(&path, body).unwrap();
    path
}

fn run(cmd: &mut Command) -> std::process::Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "link-watch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn binary_loads_config_fixture_for_probes_and_output_path() {
    let port = serve();
    let base = out_dir().join("bin-fixture-base");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let cfg = write_fixture("bin-fixture.toml", port, &base);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_link-watch"));
    scrub(&mut cmd);
    cmd.env("LINK_WATCH_ONCE", "1").env("OPENMIND_CONFIG", &cfg);
    run(&mut cmd);
    let out = base.join("link.json");
    let value: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    let golden: Vec<String> = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(keys(&value), golden);
    assert_eq!(value["laptop_port"], Value::Bool(true));
    assert_eq!(value["hermes"], Value::Bool(true));
    assert_eq!(value["france"], Value::Bool(true));
    assert_eq!(value["model"], Value::String("fixture".into()));
}

#[test]
fn binary_link_watch_env_wins_over_config_fixture() {
    let port = serve();
    let base = out_dir().join("bin-env-base");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let cfg = write_fixture("bin-env.toml", port, &base);
    let env_out = out_dir().join("bin-env-link.json");
    let _ = std::fs::remove_file(&env_out);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_link-watch"));
    scrub(&mut cmd);
    cmd.env("LINK_WATCH_ONCE", "1")
        .env("OPENMIND_CONFIG", &cfg)
        .env("LINK_WATCH_OUT", &env_out)
        .env("LINK_WATCH_OLLAMA_URL", "http://127.0.0.1:1")
        .env("LINK_WATCH_FRANCE_URL", "");
    run(&mut cmd);
    assert!(
        !base.join("link.json").exists(),
        "config output path must not be written when LINK_WATCH_OUT is set"
    );
    let value: Value = serde_json::from_slice(&std::fs::read(&env_out).unwrap()).unwrap();
    let golden: Vec<String> = serde_json::from_str(GOLDEN).unwrap();
    assert_eq!(keys(&value), golden);
    assert_eq!(value["laptop_port"], Value::Bool(false));
    assert_eq!(value["hermes"], Value::Bool(true));
    assert_eq!(value["france"], Value::String("unconfigured".into()));
}
