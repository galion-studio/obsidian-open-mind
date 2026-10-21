// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! hermes-mock-model: a scripted, OpenAI-compatible `/v1/chat/completions`
//! server for real-life tests of hermes-rs without any paid model call.
//!
//!   hermes-mock-model --port 19171 --scripts DIR [--log FILE]
//!
//! The user message names a script with `SCRIPT:<name>`; DIR/<name>.json is
//!   {"cost_per_call": 0.0, "steps": [{"tool": "git", "args": {...}}, ..., {"final": "done"}]}
//! The step index is the number of assistant tool-call messages after the
//! last user message, so the server is stateless and one session = one run.
//! String values `@FILE:<path>` (relative to DIR) are replaced by that file,
//! and `@NAME@` by `--var NAME=value`, after the file is loaded.
//! Binds 127.0.0.1 only. Every request is logged as one JSON line (step,
//! offered tool names, the last tool result; cut to 400 chars on stdout, full in --log).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

static REQS: AtomicU64 = AtomicU64::new(0);

struct Srv {
    scripts: PathBuf,
    vars: Vec<(String, String)>,
    log: Option<Mutex<std::fs::File>>,
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port: u16 = arg(&args, "--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or(19171);
    let scripts = PathBuf::from(arg(&args, "--scripts").unwrap_or_else(|| ".".into()));
    let log = arg(&args, "--log").map(|p| {
        Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .expect("open --log"),
        )
    });
    let l = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("bind 127.0.0.1:{port}: {e}");
        std::process::exit(1)
    });
    eprintln!("hermes-mock-model listening on 127.0.0.1:{port}");
    let vars = args
        .windows(2)
        .filter(|w| w[0] == "--var")
        .filter_map(|w| w[1].split_once('='))
        .map(|(k, v)| (format!("@{k}@"), v.to_string()))
        .collect();
    let srv = Arc::new(Srv { scripts, vars, log });
    for s in l.incoming().flatten() {
        let srv = srv.clone();
        std::thread::spawn(move || {
            let _ = handle(&srv, s);
        });
    }
}

fn respond(s: &mut TcpStream, code: u16, body: &Value) -> std::io::Result<()> {
    let b = body.to_string();
    let reason = if code == 200 { "OK" } else { "Error" };
    write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    )
}

fn handle(srv: &Srv, mut s: TcpStream) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut len = 0usize;
    let mut expect = false;
    let mut auth = false;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        let (k, v) = h.split_once(':').unwrap_or((&h, ""));
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        if k == "content-length" {
            len = v.parse().unwrap_or(0);
        }
        if k == "expect" && v.eq_ignore_ascii_case("100-continue") {
            expect = true;
        }
        if k == "authorization" {
            auth = true;
        }
    }
    if expect {
        s.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    if len > 16 << 20 {
        return respond(&mut s, 413, &json!({"error": {"message": "too large"}}));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    if line.starts_with("GET /health") || line.starts_with("GET /v1/models") {
        return respond(&mut s, 200, &json!({"ok": true, "mock": true}));
    }
    if !line.starts_with("POST ") || !line.contains("/chat/completions") {
        return respond(&mut s, 404, &json!({"error": {"message": "not found"}}));
    }
    let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let (code, resp, logline) = answer(&srv.scripts, &srv.vars, &req);
    let n = REQS.fetch_add(1, Ordering::SeqCst) + 1;
    let mut logline = logline;
    logline["req_no"] = json!(n);
    logline["authorization_header_sent"] = json!(auth);
    let mut short = logline.clone();
    if let Some(s) = short["last_tool_result"].as_str() {
        short["last_tool_result"] = json!(s.chars().take(400).collect::<String>());
    }
    println!("{short}");
    if let Some(f) = &srv.log {
        if let Ok(mut f) = f.lock() {
            let _ = writeln!(f, "{logline}");
        }
    }
    respond(&mut s, code, &resp)
}

/// Pure: pick the scripted reply for a request. (status, body, log line)
pub fn answer(dir: &Path, vars: &[(String, String)], req: &Value) -> (u16, Value, Value) {
    let msgs = req["messages"].as_array().cloned().unwrap_or_default();
    let last_user = msgs.iter().rposition(|m| m["role"] == "user");
    let user_text = last_user
        .and_then(|i| msgs[i]["content"].as_str())
        .unwrap_or("");
    let name: String = user_text
        .split("SCRIPT:")
        .nth(1)
        .unwrap_or("")
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let after = last_user.map(|i| &msgs[i + 1..]).unwrap_or(&[]);
    let step = after
        .iter()
        .filter(|m| m["role"] == "assistant" && m.get("tool_calls").is_some())
        .count();
    let last_tool: String = after
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string();
    let tools: Vec<Value> = req["tools"]
        .as_array()
        .map(|a| a.iter().map(|t| t["function"]["name"].clone()).collect())
        .unwrap_or_default();
    let mut log = json!({"script": name, "step": step, "model": req["model"], "tools_offered": tools, "last_tool_result": last_tool});
    if name.is_empty() {
        return (
            400,
            json!({"error": {"message": "no SCRIPT:<name> in the user message"}}),
            log,
        );
    }
    let path = dir.join(format!("{name}.json"));
    let script: Value = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(v) => v,
        None => {
            return (
                400,
                json!({"error": {"message": format!("unknown script {name}")}}),
                log,
            )
        }
    };
    let script = expand(dir, vars, script);
    let cost = script["cost_per_call"].as_f64().unwrap_or(0.0);
    let steps = script["steps"].as_array().cloned().unwrap_or_default();
    let usage =
        json!({"prompt_tokens": 100, "completion_tokens": 20, "total_tokens": 120, "cost": cost});
    let st = steps
        .get(step)
        .cloned()
        .unwrap_or(json!({"final": "script finished"}));
    let message = if let Some(t) = st.get("tool").and_then(Value::as_str) {
        log["reply"] = json!(format!("tool_call {t}"));
        json!({"role": "assistant", "content": null, "tool_calls": [{
            "id": format!("call_{step}"), "type": "function",
            "function": {"name": t, "arguments": st["args"].to_string()}
        }]})
    } else {
        let text = st["final"].as_str().unwrap_or("done").to_string();
        log["reply"] = json!("final");
        json!({"role": "assistant", "content": text})
    };
    let finish = if message.get("tool_calls").is_some() {
        "tool_calls"
    } else {
        "stop"
    };
    (
        200,
        json!({"id": format!("mock-{step}"), "object": "chat.completion", "model": req["model"],
               "choices": [{"index": 0, "message": message, "finish_reason": finish}], "usage": usage}),
        log,
    )
}

/// Fill `@FILE:<path>` and `@NAME@` placeholders in every string value.
fn expand(dir: &Path, vars: &[(String, String)], v: Value) -> Value {
    match v {
        Value::String(s) => {
            let mut s = match s.strip_prefix("@FILE:") {
                Some(p) => {
                    std::fs::read_to_string(dir.join(p)).unwrap_or_else(|e| format!("<{p}: {e}>"))
                }
                None => s,
            };
            for (k, val) in vars {
                s = s.replace(k.as_str(), val);
            }
            Value::String(s)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(|x| expand(dir, vars, x)).collect()),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, x)| (k, expand(dir, vars, x)))
                .collect(),
        ),
        other => other,
    }
}
