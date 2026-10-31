// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Loopback stand-ins for every network service the gateway talks to:
//!
//! * fake model — OpenAI/OpenRouter-compatible `/chat/completions` (SSE and
//!   plain JSON) and `/models`. Deterministic: the last user message carries
//!   `[dryrun:<scenario>:<nonce>]`, the scenario is a fixed list of tool
//!   calls, and the step is the number of tool results since that message.
//!   Usage cost is always 0. Unmarked calls (titles, summaries) get a fixed
//!   plain reply.
//! * memory gateway (TencentDB-memory API shape) returning a canary memory,
//! * nerve / cortex / queue (openmind stack status endpoints),
//! * sink — any other URL from the config is pointed here; every hit is an
//!   attempted external call and fails the run.
//! * stdio MCP stub with zero tools, for MCP binaries we do not copy.

use crate::util::{respond, respond_json, serve, Log, Req};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const PLAIN_REPLY: &str = "dryrun fake model: ok";

#[derive(Clone)]
pub struct Ctx {
    pub nonce: String,
    /// dummy secret value -> label (to record which key a request used).
    pub dummies: BTreeMap<String, String>,
    /// absolute path (inside the copy) of the dummy .env, for the deny scenario.
    pub env_file: String,
    pub log: Log,
    pub calls: Arc<AtomicU64>,
    pub sink_hits: Arc<Mutex<Vec<String>>>,
}

/// One scripted model step.
enum Step {
    Tool(&'static str, Value),
    Final(&'static str),
}

fn script(scn: &str, nonce: &str, env_file: &str) -> Option<Vec<Step>> {
    use Step::*;
    let f = format!("dryrun/{nonce}.txt");
    Some(match scn {
        "ping" => vec![Final("DRYRUN-PONG")],
        "clock" => vec![Tool("clock", json!({})), Final("DRYRUN-CLOCK")],
        "files" => vec![
            Tool(
                "write_file",
                json!({"path": f, "content": format!("DRYRUN-FILE-{nonce}\n")}),
            ),
            Tool("read_file", json!({"path": f})),
            Final("DRYRUN-FILES"),
        ],
        "terminal" => vec![
            Tool(
                "terminal",
                json!({"command": format!("echo DRYRUN-TERM-{nonce}"), "timeout_s": 20}),
            ),
            Final("DRYRUN-TERM"),
        ],
        "vault" => vec![
            Tool(
                "mcp__openmind__vault_search",
                json!({"query": "DRYRUN-VAULT-CANARY"}),
            ),
            Final("DRYRUN-VAULT"),
        ],
        "memory" => vec![
            Tool(
                "mcp__openmind__memory_search",
                json!({"query": "dryrun canary"}),
            ),
            Final("DRYRUN-MEMORY"),
        ],
        "deny" => vec![
            Tool("read_file", json!({"path": env_file})),
            Tool(
                "terminal",
                json!({"command": format!("cat {env_file}"), "timeout_s": 20}),
            ),
            Final("DRYRUN-DENY"),
        ],
        _ => return None,
    })
}

fn content_text(m: &Value) -> String {
    match m.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn marker(s: &str) -> Option<(String, String)> {
    let i = s.find("[dryrun:")?;
    let rest = &s[i + 8..];
    let j = rest.find(']')?;
    let (scn, nonce) = rest[..j].split_once(':')?;
    Some((scn.to_string(), nonce.to_string()))
}

fn cut(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut e = n;
    while !s.is_char_boundary(e) {
        e -= 1;
    }
    format!("{}…", &s[..e])
}

fn model_handler(ctx: &Ctx, req: &Req) -> (u16, Value, bool) {
    let n = ctx.calls.fetch_add(1, Ordering::SeqCst) + 1;
    let auth = req
        .header("authorization")
        .map(|a| a.trim_start_matches("Bearer ").trim().to_string())
        .unwrap_or_default();
    let auth_label = ctx.dummies.get(&auth).cloned().unwrap_or_else(|| {
        if auth.is_empty() {
            "none".into()
        } else {
            "UNKNOWN-KEY".into()
        }
    });
    if req.method == "GET" {
        ctx.log
            .json(&json!({"n": n, "path": req.path, "method": "GET", "auth": auth_label}));
        return (
            200,
            json!({"object": "list", "data": [{"id": "dryrun/fake", "object": "model"}]}),
            false,
        );
    }
    let body = req.json();
    let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let msgs = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tools: Vec<String> = body
        .get("tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    t.pointer("/function/name")
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    let last_user = msgs
        .iter()
        .rposition(|m| m.get("role").and_then(Value::as_str) == Some("user"));
    let (scn, nonce) = last_user
        .and_then(|i| marker(&content_text(&msgs[i])))
        .unwrap_or_default();
    let tool_results: Vec<String> = last_user
        .map(|i| {
            msgs[i + 1..]
                .iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("tool"))
                .map(content_text)
                .collect()
        })
        .unwrap_or_default();
    let step = tool_results.len();
    let steps = script(&scn, &nonce, &ctx.env_file);
    let (text, call): (String, Option<(&str, Value)>) = match steps {
        Some(st) => match st.into_iter().nth(step) {
            Some(Step::Tool(name, args)) => (String::new(), Some((name, args))),
            Some(Step::Final(tag)) => {
                let joined = tool_results.join(" || ");
                (format!("{tag} {nonce} :: {}", cut(&joined, 6000)), None)
            }
            None => (format!("DRYRUN-OVERRUN {nonce}"), None),
        },
        None => (PLAIN_REPLY.to_string(), None),
    };
    ctx.log.json(&json!({
        "n": n, "path": req.path, "model": model, "stream": stream, "auth": auth_label,
        "scenario": scn, "nonce": nonce, "step": step, "tools_offered": tools,
        "tool_results": tool_results.iter().map(|t| cut(t, 400)).collect::<Vec<_>>(),
        "reply_tool": call.as_ref().map(|c| c.0), "reply_text": cut(&text, 300),
    }));
    let id = format!("chatcmpl-dryrun-{n}");
    let usage =
        json!({"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15, "cost": 0.0});
    let (message, finish) = match &call {
        Some((name, args)) => (
            json!({"role": "assistant", "content": null, "tool_calls": [{
                "id": format!("call_dryrun_{n}"), "type": "function",
                "function": {"name": name, "arguments": args.to_string()}}]}),
            "tool_calls",
        ),
        None => (json!({"role": "assistant", "content": text}), "stop"),
    };
    if !stream {
        return (
            200,
            json!({"id": id, "object": "chat.completion", "model": model, "created": crate::util::now_secs(),
                   "choices": [{"index": 0, "message": message, "finish_reason": finish}], "usage": usage}),
            false,
        );
    }
    // SSE: role chunk, content/tool chunk, finish chunk with usage, [DONE]
    let delta = match &call {
        Some((name, args)) => json!({"tool_calls": [{"index": 0, "id": format!("call_dryrun_{n}"),
            "type": "function", "function": {"name": name, "arguments": args.to_string()}}]}),
        None => json!({"content": text}),
    };
    let chunk = |d: Value, fin: Value, u: Option<Value>| {
        let mut c = json!({"id": id, "object": "chat.completion.chunk", "model": model,
            "created": crate::util::now_secs(), "choices": [{"index": 0, "delta": d, "finish_reason": fin}]});
        if let Some(u) = u {
            c["usage"] = u;
        }
        format!("data: {c}\n\n")
    };
    let mut sse = String::new();
    sse.push_str(&chunk(json!({"role": "assistant"}), Value::Null, None));
    sse.push_str(&chunk(delta, Value::Null, None));
    sse.push_str(&chunk(json!({}), json!(finish), Some(usage)));
    sse.push_str("data: [DONE]\n\n");
    (200, Value::String(sse), true)
}

pub fn start_model(port: u16, ctx: Ctx) -> Result<(), String> {
    serve(port, move |req, s| {
        let (code, v, sse) = model_handler(&ctx, &req);
        if sse {
            respond(
                s,
                code,
                "text/event-stream",
                v.as_str().unwrap_or("").as_bytes(),
            );
        } else {
            respond_json(s, code, &v);
        }
    })
}

pub fn memory_canary(nonce: &str) -> String {
    format!("DRYRUN-MEMORY-CANARY-{nonce}: the dry-run copy remembers a blue lighthouse.")
}

pub fn start_memory(port: u16, ctx: Ctx) -> Result<(), String> {
    serve(port, move |req, s| {
        let auth = req
            .header("authorization")
            .or(req.header("x-api-key"))
            .unwrap_or("")
            .to_string();
        let known = ctx.dummies.keys().any(|d| auth.contains(d.as_str()));
        ctx.log.json(&json!({"svc": "memory", "method": req.method, "path": req.path,
            "auth": if known { "dummy" } else if auth.is_empty() { "none" } else { "UNKNOWN-KEY" }}));
        let canary = memory_canary(&ctx.nonce);
        let p = req.path.split('?').next().unwrap_or("");
        let v = if p.ends_with("/health") || p == "/" {
            json!({"status": "ok", "ok": true, "dryrun": true})
        } else if p.contains("/search") {
            json!({"total": 1, "results": [{"id": "dryrun-1", "content": canary, "memory": canary, "score": 0.99}],
                   "memories": [{"id": "dryrun-1", "content": canary}]})
        } else if p.contains("/recall") {
            json!({"context": canary, "memories": [{"id": "dryrun-1", "content": canary}]})
        } else if p.contains("/capture") || req.method == "POST" {
            json!({"ok": true, "id": "dryrun-captured"})
        } else {
            json!({"ok": true, "dryrun": true})
        };
        respond_json(s, 200, &v);
    })
}

pub fn start_status(port: u16, name: &'static str, ctx: Ctx) -> Result<(), String> {
    serve(port, move |req, s| {
        ctx.log
            .json(&json!({"svc": name, "method": req.method, "path": req.path}));
        respond_json(
            s,
            200,
            &json!({"ok": true, "status": "ok", "dryrun": true, "service": name,
            "items": [], "alerts": [], "heals": [], "jobs": [], "depth": 0}),
        );
    })
}

pub fn start_sink(port: u16, ctx: Ctx) -> Result<(), String> {
    serve(port, move |req, s| {
        let what = format!("{} {}", req.method, cut(&req.path, 200));
        ctx.log.json(&json!({"svc": "sink", "hit": what}));
        ctx.sink_hits.lock().unwrap().push(what);
        respond_json(
            s,
            404,
            &json!({"error": "dry-run sink: external endpoint is not available in the dry run"}),
        );
    })
}

/// DNS sink on 127.0.0.1:53 (the namespace's resolv.conf points here).
/// Every query is an attempt to reach a named host: it is logged with its
/// name and answered NXDOMAIN at once, so nothing waits on it.
pub fn start_dns(ctx: Ctx, hits: Arc<Mutex<Vec<String>>>) -> Result<(), String> {
    let sock = std::net::UdpSocket::bind(("127.0.0.1", crate::netwatch::DNS_PORT))
        .map_err(|e| format!("bind 127.0.0.1:53: {e}"))?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 1500];
        while let Ok((n, from)) = sock.recv_from(&mut buf) {
            if n < 12 {
                continue;
            }
            let mut name = vec![];
            let mut i = 12;
            while i < n && buf[i] != 0 && buf[i] < 64 {
                let l = buf[i] as usize;
                if i + 1 + l > n {
                    break;
                }
                name.push(String::from_utf8_lossy(&buf[i + 1..i + 1 + l]).into_owned());
                i += 1 + l;
            }
            let qtype = if i + 2 < n {
                u16::from_be_bytes([buf[i + 1], buf[i + 2]])
            } else {
                0
            };
            let qname = name.join(".");
            ctx.log
                .json(&json!({"svc": "dns", "qname": qname, "qtype": qtype}));
            hits.lock().unwrap().push(qname);
            let mut r = buf[..n].to_vec();
            r[2] = 0x80 | (buf[2] & 0x01); // QR + copy RD
            r[3] = 0x80 | 0x03; // RA + NXDOMAIN
            r[6..12].fill(0); // no answer/authority/additional
            let _ = sock.send_to(&r, from);
        }
    });
    Ok(())
}

/// Minimal stdio MCP server with zero tools (stands in for MCP binaries
/// that are not part of the candidate, e.g. github-mcp-rs).
pub fn stub_mcp() -> i32 {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = v.get("id").cloned() else {
            continue;
        };
        let m = v.get("method").and_then(Value::as_str).unwrap_or("");
        let r = match m {
            "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": v.pointer("/params/protocolVersion").cloned().unwrap_or(json!("2024-11-05")),
                "capabilities": {"tools": {}}, "serverInfo": {"name": "hermes-dryrun-stub", "version": "0"}}}),
            "tools/list" => json!({"jsonrpc": "2.0", "id": id, "result": {"tools": []}}),
            "ping" => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
            _ => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "dry-run stub"}})
            }
        };
        if writeln!(out, "{r}").is_err() || out.flush().is_err() {
            break;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_parse() {
        assert_eq!(
            marker("hi [dryrun:files:ab12] go"),
            Some(("files".into(), "ab12".into()))
        );
        assert_eq!(marker("nothing"), None);
    }
}
