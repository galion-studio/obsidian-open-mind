// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Memory provider against a fake memory gateway on loopback: recall is
//! injected into the system prompt, capture and session/end arrive with the
//! bearer key, and a dead gateway never breaks a chat turn.

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::memory::{MemoryCfg, MemoryProvider};
use hermes_rs::provider::MockProvider;
use hermes_rs::session::MemStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::Registry;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const KEY: &str = "test-key-0123456789abcdef";
const MEM_KEY: &str = "fake-memory-key-42";

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    auth: Option<String>,
    body: Value,
}

/// Fake gateway: records every request; /recall answers `context`.
fn fake_gateway(context: &'static str) -> (u16, Arc<Mutex<Vec<Seen>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let s3 = s2.clone();
            std::thread::spawn(move || serve_one(c, s3, context));
        }
    });
    (port, seen)
}

fn serve_one(mut c: TcpStream, seen: Arc<Mutex<Vec<Seen>>>, context: &str) {
    c.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let (head, mut body) = loop {
        let n = c.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break (
                String::from_utf8_lossy(&buf[..i]).to_string(),
                buf[i + 4..].to_vec(),
            );
        }
    };
    let len: usize = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse().unwrap_or(0))
        })
        .unwrap_or(0);
    while body.len() < len {
        let n = c.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
    let auth = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case("authorization")
            .then(|| v.trim().to_string())
    });
    seen.lock().unwrap().push(Seen {
        path: path.clone(),
        auth,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    let resp = match path.as_str() {
        "/recall" => json!({"context": context, "strategy": "hybrid", "memory_count": 1}),
        "/capture" => json!({"l0_recorded": 2, "scheduler_notified": false}),
        "/session/end" => json!({"flushed": true}),
        _ => json!({"status": "ok"}),
    }
    .to_string();
    let _ = c.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
            resp.len()
        )
        .as_bytes(),
    );
}

fn gateway_with_memory(mem_port: u16) -> (SocketAddr, Arc<MockProvider>, Arc<MemoryProvider>) {
    let home = std::env::temp_dir().join(hermes_rs::new_id("hermes-rs-mem-"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("SOUL.md"), "You are Hermes.").unwrap();
    let mut cfg = Config::defaults(&home);
    cfg.apply_yaml(&hermes_rs::yaml::parse("memory:\n  provider: memory_tencentdb\n").unwrap());
    let env = move |k: &str| match k {
        "MEMORY_TENCENTDB_GATEWAY_PORT" => Some(mem_port.to_string()),
        "TDAI_GATEWAY_API_KEY" => Some(MEM_KEY.to_string()),
        _ => None,
    };
    cfg.apply_env(&env).unwrap();
    let mcfg: MemoryCfg = cfg.memory.clone().expect("memory enabled by config.yaml");
    let mem = MemoryProvider::new(mcfg, &env);
    assert_eq!(mem.key_env, Some("TDAI_GATEWAY_API_KEY"));
    let cfg = Arc::new(cfg);
    let mock = Arc::new(MockProvider::echo());
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let agent = hermes_rs::agent::boss(cfg.clone(), mock.clone(), spend, Registry::new());
    let mut gw = Gateway::new(cfg, agent, Arc::new(MemStore::default()), Some(KEY.into()));
    gw.memory = Some(mem.clone());
    let addr = hermes_rs::api::spawn(Arc::new(gw), "127.0.0.1:0").unwrap();
    (addr, mock, mem)
}

fn chat(addr: SocketAddr, session: &str, text: &str) -> (u16, Value) {
    let b = json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}], "max_tokens": 16}).to_string();
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(
        s,
        "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {KEY}\r\nX-Hermes-Session-Id: {session}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn wait_for(seen: &Arc<Mutex<Vec<Seen>>>, path: &str) -> Seen {
    let t0 = Instant::now();
    loop {
        if let Some(s) = seen.lock().unwrap().iter().find(|s| s.path == path) {
            return s.clone();
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "no {path} seen");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn recall_capture_and_session_end_reach_the_gateway() {
    let (port, seen) = fake_gateway("- User drinks green tea");
    let (addr, mock, mem) = gateway_with_memory(port);
    let (status, body) = chat(addr, "api-memtest1", "what do I drink?");
    assert_eq!(status, 200, "{body}");

    let recall = wait_for(&seen, "/recall");
    assert_eq!(recall.auth.as_deref(), Some(&*format!("Bearer {MEM_KEY}")));
    assert_eq!(
        recall.body,
        json!({"query": "what do I drink?", "session_key": "api-memtest1", "user_id": "default"})
    );
    // Recalled context is in the system prompt the model saw.
    let calls = mock.calls.lock().unwrap();
    let sys = calls[0].1.messages[0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    drop(calls);
    assert!(
        sys.contains("## memory-tencentdb Memory\n- User drinks green tea"),
        "{sys}"
    );

    let cap = wait_for(&seen, "/capture");
    assert_eq!(cap.auth.as_deref(), Some(&*format!("Bearer {MEM_KEY}")));
    assert_eq!(cap.body["user_content"], "what do I drink?");
    assert_eq!(cap.body["session_key"], "api-memtest1");
    assert!(!cap.body["assistant_content"].as_str().unwrap().is_empty());

    // Session close: idle reaper / shutdown sends /session/end.
    assert_eq!(mem.end_idle(Duration::from_secs(3600)), 0);
    assert_eq!(mem.shutdown(), 1);
    let end = wait_for(&seen, "/session/end");
    assert_eq!(
        end.body,
        json!({"session_key": "api-memtest1", "user_id": "default"})
    );
    let st = mem.stats();
    assert_eq!((st.recalls, st.recall_hits, st.session_ends), (1, 1, 1));
}

#[test]
fn empty_recall_injects_nothing() {
    let (port, _seen) = fake_gateway("");
    let (addr, mock, _mem) = gateway_with_memory(port);
    assert_eq!(chat(addr, "api-memtest2", "hello").0, 200);
    let calls = mock.calls.lock().unwrap();
    let sys = calls[0].1.messages[0]["content"].as_str().unwrap();
    assert!(!sys.contains("memory-tencentdb"), "{sys}");
}

#[test]
fn dead_gateway_does_not_break_chat() {
    // Bind then drop: nothing listens on the port any more.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (addr, _mock, mem) = gateway_with_memory(port);
    let t0 = Instant::now();
    let (status, body) = chat(addr, "api-memtest3", "still works?");
    assert_eq!(status, 200, "{body}");
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert_eq!(body["choices"][0]["message"]["role"], "assistant");
    std::thread::sleep(Duration::from_millis(300));
    assert!(mem.stats().failures >= 1);
}
