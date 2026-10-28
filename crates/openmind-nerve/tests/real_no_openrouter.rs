// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real binary, real sockets (GALION_REAL=1, release): the event-mode nerve
//! hands an incident burst to the cortex (`delegate`, zero spend) while
//! OpenRouter env vars are set, never connects anywhere for it, prints the
//! env NAMES only, and refuses OpenRouter / Hermes-gateway LLM endpoints at
//! start. Slot 241 ports 21110-21119 (21119 = decoy listener).

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn get(port: u16, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (
        code,
        out.split_once("\r\n\r\n")
            .map(|x| x.1.to_string())
            .unwrap_or_default(),
    )
}

fn json(port: u16, path: &str) -> Value {
    let (c, b) = get(port, path);
    assert_eq!(c, 200, "{path}: {b}");
    serde_json::from_str(&b).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("real-241-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

#[test]
fn delegate_handoff_with_openrouter_env_present() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let port = 21110;
    let decoy = TcpListener::bind("127.0.0.1:21119").unwrap();
    decoy.set_nonblocking(true).unwrap();
    let dir = tmp("delegate");
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        dir.join("components.json"),
        r#"[{"name":"real-241-dead","port":21118,"path":"/health","gaia":"DEMETER","system":"tuner"}]"#,
    )
    .unwrap();
    let mut child = Command::new(BIN)
        .args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{port}"),
        ])
        .args(["--log", &dir.join("events.jsonl").display().to_string()])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "3")
        .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
        // Present but ignored: a dummy key value and a base URL at the decoy.
        .env("OPENROUTER_API_KEY", "dummy-value-241-not-a-key")
        .env("OPENROUTER_BASE_URL", "http://127.0.0.1:21119/api/v1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while !(TcpStream::connect(("127.0.0.1", port)).is_ok() && get(port, "/health").0 == 200) {
        assert!(t0.elapsed() < Duration::from_secs(30), "health");
        std::thread::sleep(Duration::from_millis(100));
    }
    let st = json(port, "/stats");
    assert_eq!(st["config"]["escalate"]["mode"], "delegate", "{st}");
    assert_eq!(st["config"]["escalate"]["paid_enabled"], false, "{st}");
    let mut j = std::fs::OpenOptions::new().write(true).open(&fifo).unwrap();
    for _ in 0..5 {
        writeln!(j, r#"{{"MESSAGE_ID":"98e322203f7a4ed290d09fe03c09fe15","UNIT":"hermes-rs.service","MESSAGE":"hermes-rs.service: sim","_PID":"1","EXIT_STATUS":"1"}}"#).unwrap();
    }
    j.flush().unwrap();
    let t1 = Instant::now();
    let esc = loop {
        let e = json(port, "/escalations");
        if !e.as_array().unwrap().is_empty() {
            break e;
        }
        assert!(t1.elapsed() < Duration::from_secs(20), "no escalation");
        std::thread::sleep(Duration::from_millis(100));
    };
    let handoff_ms = t1.elapsed().as_millis();
    let st = json(port, "/stats");
    let outs: Vec<&str> = esc
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["outcome"].as_str().unwrap())
        .collect();
    assert!(outs.iter().all(|o| *o == "delegated"), "{esc}");
    assert_eq!(st["llm_calls"], 0, "{st}");
    assert_eq!(st["llm_usd"].as_f64().unwrap_or(0.0), 0.0, "{st}");
    let all = format!("{st}{esc}{}", json(port, "/inventory"));
    assert!(!all.contains("dummy-value-241"), "key value never exposed");
    let events = std::fs::read_to_string(dir.join("events.jsonl")).unwrap_or_default();
    assert!(!events.contains("dummy-value-241"));
    drop(j);
    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("ignoring LLM provider env"), "{err}");
    assert!(err.contains("OPENROUTER_API_KEY") && err.contains("OPENROUTER_BASE_URL"));
    assert!(!err.contains("dummy-value-241"), "value never printed");
    assert!(err.contains("delegated to cortex"), "{err}");
    assert!(decoy.accept().is_err(), "nothing connected to the decoy");
    eprintln!(
        "real-241 delegate: escalations={} outcomes={outs:?} llm_calls={} handoff_ms={handoff_ms} decoy_connections=0",
        esc.as_array().unwrap().len(),
        st["llm_calls"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn llm_endpoints_refused_at_start() {
    if !real() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let decoy = TcpListener::bind("127.0.0.1:21117").unwrap();
    decoy.set_nonblocking(true).unwrap();
    let dir = tmp("refuse");
    for url in [
        "https://openrouter.ai/api/v1/chat/completions",
        "https://openrouter%2Eai/api/v1/chat/completions",
        "http://127.0.0.1:8642/v1/chat/completions",
        "http://hermes.local:21117/v1/chat/completions",
    ] {
        let out = Command::new(BIN)
            .args(["serve", "--mode", "event", "--listen", "127.0.0.1:21111"])
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_EVT_ESCALATE", "openai")
            .env("NERVE_EVT_LLM_ENDPOINT", url)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{url}: {err}");
        assert!(err.contains("refused"), "{url}: {err}");
        eprintln!(
            "real-241 refuse {url}: exit 2: {}",
            err.lines().last().unwrap_or("")
        );
    }
    assert!(
        TcpStream::connect(("127.0.0.1", 21111)).is_err(),
        "never listened"
    );
    assert!(decoy.accept().is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
