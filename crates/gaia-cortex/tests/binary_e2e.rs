// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The real `gaia-cortex` binary against a mock event-mode nerve on
//! loopback: subscribe to `/events/stream`, decide, `POST /actions`
//! (dry-run), alert to `POST /event`; push ingest; API shape. No network
//! beyond 127.0.0.1, no model.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Log = Arc<Mutex<Vec<(String, String)>>>;

fn mock_nerve() -> (String, Log) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let log: Log = Arc::default();
    let log2 = log.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let log = log2.clone();
            std::thread::spawn(move || serve(s, log));
        }
    });
    (format!("http://{addr}"), log)
}

fn serve(s: TcpStream, log: Log) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut first = String::new();
    r.read_line(&mut first).unwrap_or(0);
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    let _ = r.read_exact(&mut body);
    let body = String::from_utf8_lossy(&body).to_string();
    let target = first.split_whitespace().nth(1).unwrap_or("").to_string();
    log.lock().unwrap().push((target.clone(), body.clone()));
    let mut s = s;
    if target.starts_with("/events/stream") {
        let _ = s.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
        );
        // Already-seen history is only replayed on a resume.
        if target.contains("since=") {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        let lines = [
            r#"{"v":1,"seq":1,"ts":"2026-10-21T18:40:00+02:00","t":1792600800,"source":"journal","kind":"unit.exit","severity":"high","unit":"tuner.service","group":"other","key":"unit.exit:tuner.service:1","msg":"Main process exited","data":{"exit_code":"exited","exit_status":"1"},"count":1}"#,
            r#"{"v":1,"seq":2,"ts":"2026-10-21T18:40:00+02:00","t":1792600800,"source":"journal","kind":"unit.failed","severity":"high","unit":"tuner.service","group":"other","key":"unit.failed:tuner.service","msg":"Failed with result exit-code","data":{"unit_result":"exit-code"},"count":1}"#,
        ];
        for l in lines {
            let _ = s.write_all(format!("{l}\n").as_bytes());
        }
        let _ = s.flush();
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let reply = if target == "/actions" {
        let v: Value = serde_json::from_str(&body).unwrap_or_default();
        serde_json::json!({"request_id": v["request_id"], "action": "restart", "unit": v["unit"], "source": v["source"],
                           "result": "dry-run", "would_run": format!("systemctl restart --no-block {}", v["unit"].as_str().unwrap_or("")),
                           "why": ["NERVE_EVT_ACTIONS is not live", "request dry_run"]})
        .to_string()
    } else {
        r#"{"ok":true}"#.to_string()
    };
    let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).as_bytes());
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn get(port: u16, path: &str) -> Value {
    http(port, "GET", path, "")
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Value {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let b = out.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    serde_json::from_str(b).unwrap_or(Value::Null)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(f: &dyn Fn() -> bool, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn binary_subscribes_decides_and_asks_nerve() {
    let (nerve, log) = mock_nerve();
    let port = free_port();
    let dir = std::env::temp_dir().join(format!("gaia-cortex-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _child = Kill(
        Command::new(env!("CARGO_BIN_EXE_gaia-cortex"))
            .env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
            .env("CORTEX_NERVE_URL", &nerve)
            .env("CORTEX_DATA_DIR", &dir)
            .env("CORTEX_COALESCE_MS", "300")
            .env_remove("CORTEX_CONFIG")
            .env_remove("CORTEX_LLM_ENABLED")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(wait_for(
        &|| TcpStream::connect(("127.0.0.1", port)).is_ok(),
        10
    ));
    let h = get(port, "/health");
    assert_eq!(h["ok"], true);
    assert_eq!(h["mode"], "dry-run");
    assert_eq!(h["llm_enabled"], false);
    // Stream -> decision -> POST /actions (dry-run) -> alert POST /event.
    assert!(
        wait_for(&|| get(port, "/stats")["actions_simulated"] == 1, 10),
        "{}",
        get(port, "/stats")
    );
    // Slot 347 (F-337-1): `nerve` is real reachability; the stream is up.
    let h = get(port, "/health");
    assert_eq!(h["nerve"], true, "{h}");
    assert_eq!(h["nerve_configured"], true);
    assert_eq!(h["nerve_contact"]["state"], "ok", "{h}");
    assert_eq!(h["nerve_contact"]["stream"], "connected", "{h}");
    assert!(h["nerve_contact"]["age_s"].as_u64().unwrap() <= 10, "{h}");
    assert_eq!(h["nerve_contact"]["stale_s"], 660);
    let d = get(port, "/decisions");
    assert_eq!(d[0]["rule"], "R3-known-crash");
    assert_eq!(d[0]["action"]["by"], "nerve");
    assert_eq!(d[0]["action"]["status"], "simulated");
    assert!(d[0]["action"]["reason"]
        .as_str()
        .unwrap()
        .contains("systemctl restart --no-block tuner.service"));
    let posts = log.lock().unwrap().clone();
    let act: Vec<_> = posts.iter().filter(|(p, _)| p == "/actions").collect();
    assert_eq!(act.len(), 1);
    let req: Value = serde_json::from_str(&act[0].1).unwrap();
    assert_eq!(req["action"], "restart");
    assert_eq!(req["dry_run"], true);
    assert_eq!(req["source"], "gaia-cortex");
    assert!(wait_for(
        &|| log
            .lock()
            .unwrap()
            .iter()
            .any(|(p, b)| p == "/event" && b.contains("cortex.alert")),
        5
    ));
    // Push ingest: a protected unit is alert-only.
    let r = http(
        port,
        "POST",
        "/v1/events",
        r#"{"source":"journal","kind":"unit.failed","severity":"high","unit":"cloudflared.service","seq":99}"#,
    );
    assert_eq!(r["accepted"], 1);
    assert!(wait_for(
        &|| get(port, "/decisions")[0]["rule"] == "R1-protected",
        5
    ));
    let r = http(port, "POST", "/v1/events", "garbage");
    assert_eq!(r["invalid"], 1);
    let s = get(port, "/stats");
    assert_eq!(s["actions_requested"], 1, "{s}");
    assert_eq!(s["actions_executed"], 0);
    assert_eq!(s["llm_calls"], 0);
    assert_eq!(s["events_invalid"], 1);
    assert!(get(port, "/incidents")["open"].as_array().is_some());
    assert!(get(port, "/escalations")["open"].as_array().is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

fn start(port: u16, nerve: Option<&str>, dir: &std::path::Path) -> Kill {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"));
    c.env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
        .env("CORTEX_DATA_DIR", dir)
        .env_remove("CORTEX_CONFIG")
        .env_remove("CORTEX_LLM_ENABLED")
        .env_remove("CORTEX_NERVE_URL")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(n) = nerve {
        c.env("CORTEX_NERVE_URL", n);
    }
    let k = Kill(c.spawn().unwrap());
    assert!(wait_for(
        &|| TcpStream::connect(("127.0.0.1", port)).is_ok(),
        10
    ));
    k
}

/// Slot 347 (F-337-1 repro): a nerve URL with nothing listening used to
/// read `"nerve": true`. Now: false, `unreachable`, with the connect error.
#[test]
fn health_nerve_false_when_configured_port_is_dead() {
    let dead = free_port();
    let port = free_port();
    let dir = std::env::temp_dir().join(format!("gaia-cortex-e2e-dead-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _c = start(port, Some(&format!("http://127.0.0.1:{dead}")), &dir);
    assert!(
        wait_for(
            &|| get(port, "/health")["nerve_contact"]["fail"].as_u64() >= Some(1),
            10
        ),
        "{}",
        get(port, "/health")
    );
    let h = get(port, "/health");
    assert_eq!(h["nerve"], false, "{h}");
    assert_eq!(h["nerve_configured"], true);
    assert_eq!(h["nerve_contact"]["state"], "unreachable", "{h}");
    assert_eq!(h["nerve_contact"]["stream"], "down", "{h}");
    assert_eq!(h["nerve_contact"]["last_ok"], Value::Null);
    assert!(
        h["nerve_contact"]["last_error"]
            .as_str()
            .unwrap()
            .contains("connect"),
        "{h}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn health_nerve_none_without_url() {
    let port = free_port();
    let dir = std::env::temp_dir().join(format!("gaia-cortex-e2e-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _c = start(port, None, &dir);
    let h = get(port, "/health");
    assert_eq!(h["nerve"], false, "{h}");
    assert_eq!(h["nerve_configured"], false);
    assert_eq!(h["nerve_contact"]["state"], "none");
    assert_eq!(h["nerve_contact"]["stream"], "off");
    let _ = std::fs::remove_dir_all(&dir);
}
