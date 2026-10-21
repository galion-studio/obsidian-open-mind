// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! A loopback, OpenAI-compatible `POST /v1/chat/completions` server whose
//! "model" is a [`Brain`]. hermes-rs talks to it with its real curl
//! provider, so the live-model wire path (request body, tool schemas,
//! tool_calls parsing, auth header plumbing) is exercised with zero spend.
//!
//! Binds 127.0.0.1 only. It records, per request, the tool names offered and
//! whether an `Authorization` header was present, never the header value.

use crate::planner::{Brain, Step};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_BODY: usize = 8 << 20;

#[derive(Debug, Clone, Default)]
pub struct Seen {
    pub requests: u64,
    pub with_auth: u64,
    pub tool_names: Vec<String>,
    pub models: Vec<String>,
}

pub struct MockLlm {
    pub port: u16,
    pub seen: Arc<Mutex<Seen>>,
    /// The full message list of every finished turn (the request on which
    /// the brain answered `Done`), for runs whose agent loop is remote.
    pub transcripts: Arc<Mutex<Vec<Vec<Value>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MockLlm {
    /// Start on `127.0.0.1:port` (0 = any free port).
    pub fn start(port: u16, brain: Brain) -> std::io::Result<MockLlm> {
        let l = TcpListener::bind(("127.0.0.1", port))?;
        let port = l.local_addr()?.port();
        let seen = Arc::new(Mutex::new(Seen::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let transcripts = Arc::new(Mutex::new(Vec::new()));
        let (s2, st2, tr2) = (seen.clone(), stop.clone(), transcripts.clone());
        let thread = std::thread::spawn(move || {
            for conn in l.incoming() {
                if st2.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(c) = conn else { continue };
                let _ = handle(c, &brain, &s2, &tr2);
            }
        });
        Ok(MockLlm {
            port,
            seen,
            transcripts,
            stop,
            thread: Some(thread),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    pub fn seen(&self) -> Seen {
        self.seen.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl Drop for MockLlm {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // wake the accept loop
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        // join so the listener is closed (port reusable) when drop returns
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn handle(
    mut s: TcpStream,
    brain: &Brain,
    seen: &Mutex<Seen>,
    transcripts: &Mutex<Vec<Vec<Value>>>,
) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();
    let (mut len, mut auth) = (0usize, false);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k == "content-length" {
                len = v.trim().parse().unwrap_or(0);
            }
            if k == "authorization" && !v.trim().is_empty() {
                auth = true;
            }
        }
    }
    let (status, body) = if len > MAX_BODY {
        (413, json!({"error": {"message": "body too large"}}))
    } else if method != "POST" || !path.ends_with("/chat/completions") {
        (404, json!({"error": {"message": "not found"}}))
    } else {
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        match serde_json::from_slice::<Value>(&buf) {
            Ok(req) => (200, complete(&req, brain, seen, transcripts, auth)),
            Err(e) => (400, json!({"error": {"message": e.to_string()}})),
        }
    };
    let b = body.to_string();
    write!(
        s,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    )?;
    s.flush()
}

fn complete(
    req: &Value,
    brain: &Brain,
    seen: &Mutex<Seen>,
    transcripts: &Mutex<Vec<Vec<Value>>>,
    auth: bool,
) -> Value {
    {
        let mut sn = seen.lock().unwrap_or_else(|p| p.into_inner());
        sn.requests += 1;
        sn.with_auth += u64::from(auth);
        if sn.tool_names.is_empty() {
            sn.tool_names = req["tools"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["function"]["name"].as_str().map(str::to_string))
                .collect();
        }
        if let Some(m) = req["model"].as_str() {
            if !sn.models.iter().any(|x| x == m) {
                sn.models.push(m.into());
            }
        }
    }
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    let (message, finish) = match brain.respond(&msgs) {
        Step::Call { tool, args } => (
            json!({"role": "assistant", "content": null, "tool_calls": [{
                "id": brain.next_call_id(), "type": "function",
                "function": {"name": tool, "arguments": args.to_string()}}]}),
            "tool_calls",
        ),
        Step::Done(text) => {
            let mut t = transcripts.lock().unwrap_or_else(|p| p.into_inner());
            if t.len() >= 64 {
                t.remove(0);
            }
            t.push(msgs.clone());
            (json!({"role": "assistant", "content": text}), "stop")
        }
    };
    json!({
        "id": "mockllm", "object": "chat.completion", "model": req["model"],
        "choices": [{"index": 0, "message": message, "finish_reason": finish}],
        "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0, "cost": 0.0}
    })
}
