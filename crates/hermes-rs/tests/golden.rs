// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Golden tests: the exact request shapes the console (`hermes.ts`) and
//! Galion One (`crates/galion-one`) send, against a gateway on a loopback
//! port with the mock provider. No network beyond 127.0.0.1, no spend.

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::provider::{ChatResult, MockProvider, Reasoning};
use hermes_rs::session::MemStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{ClockTool, Registry};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const KEY: &str = "test-key-0123456789abcdef";

const CONFIG: &str = r#"
model:
  provider: openrouter
  default: anthropic/claude-opus-5.5
  context_length: 65536
  base_url: https://openrouter.ai/api/v1
model_aliases:
  worker:
    model: anthropic/claude-sonnet-5.5
    provider: openrouter
providers:
  river:
    api: https://api.river.ai/v1
    default_model: deepseek-ai/DeepSeek-V4.1-Flash
agent:
  reasoning_effort: "high"
  reasoning_overrides:
    "anthropic/claude-haiku-5.5": "none"
delegation:
  max_concurrent_children: 3
  max_spawn_depth: 1
  model: "anthropic/claude-sonnet-5.5"
  provider: "openrouter"
  reasoning_effort: "medium"
gateway:
  api_server:
    max_concurrent_runs: 3
platforms:
  api_server:
    extra:
      model_routes:
        galion:
          model: "anthropic/claude-haiku-5.5"
          provider: "openrouter"
"#;

pub struct Harness {
    pub addr: SocketAddr,
    pub mock: Arc<MockProvider>,
    pub home: PathBuf,
}

pub fn harness_with(mock: MockProvider, tweak: impl FnOnce(&mut Config)) -> Harness {
    let home = std::env::temp_dir().join(hermes_rs::new_id("hermes-rs-golden-"));
    std::fs::create_dir_all(home.join("channels")).unwrap();
    std::fs::write(home.join("SOUL.md"), "You are Hermes.").unwrap();
    std::fs::write(
        home.join("channels/galion-one.md"),
        "# Galion One\n\nThe voice in the room.",
    )
    .unwrap();
    let mut cfg = Config::defaults(&home);
    cfg.apply_yaml(&hermes_rs::yaml::parse(CONFIG).unwrap());
    tweak(&mut cfg);
    let cfg = Arc::new(cfg);
    let mock = Arc::new(mock);
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let mut tools = Registry::new();
    tools.add(Arc::new(ClockTool));
    let agent = hermes_rs::agent::boss(cfg.clone(), mock.clone(), spend, tools);
    let gw = Arc::new(Gateway::new(
        cfg,
        agent,
        Arc::new(MemStore::default()),
        Some(KEY.into()),
    ));
    let addr = hermes_rs::api::spawn(gw, "127.0.0.1:0").unwrap();
    Harness { addr, mock, home }
}

pub fn harness() -> Harness {
    harness_with(MockProvider::echo(), |_| {})
}

/// Raw HTTP exchange; returns (status, headers lowercased, body).
pub fn http(addr: SocketAddr, raw_head: &str, body: &str) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(raw_head.as_bytes()).unwrap();
    s.write_all(body.as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (status, head.to_ascii_lowercase(), body.to_string())
}

pub fn post(
    addr: SocketAddr,
    path: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> (u16, String, String) {
    let b = body.to_string();
    let mut head = format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n", b.len());
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    http(addr, &head, &b)
}

pub fn get(addr: SocketAddr, path: &str, auth: bool) -> (u16, String, String) {
    let a = if auth {
        format!("Authorization: Bearer {KEY}\r\n")
    } else {
        String::new()
    };
    http(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{a}\r\n"),
        "",
    )
}

pub fn bearer() -> String {
    format!("Bearer {KEY}")
}

/// Parse SSE `data:` frames (skips comments, `event:` lines, [DONE]).
pub fn sse_data(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect()
}

fn streamed_text(frames: &[Value]) -> String {
    frames
        .iter()
        .filter_map(|f| f["choices"][0]["delta"]["content"].as_str())
        .collect()
}

#[test]
fn health_is_open_and_shaped() {
    let h = harness();
    let (code, _, body) = get(h.addr, "/health", false);
    assert_eq!(code, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"], "ok");
    assert_eq!(v["platform"], "hermes-agent");
    // console hermesHealth() sends the bearer too
    assert_eq!(get(h.addr, "/health", true).0, 200);
}

#[test]
fn auth_is_required_with_upstream_envelope() {
    let h = harness();
    let (code, _, body) = get(h.addr, "/v1/models", false);
    assert_eq!(code, 401);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["code"], "gateway_auth_failed");
    assert_eq!(v["error"]["type"], "gateway_auth_error");
    let (code, _, _) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", "Bearer wrong-key-wrong-key")],
        &json!({"messages": [{"role": "user", "content": "x"}]}),
    );
    assert_eq!(code, 401);
}

#[test]
fn models_list_includes_galion_route() {
    let h = harness();
    let (code, _, body) = get(h.addr, "/v1/models", true);
    assert_eq!(code, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["object"], "list");
    assert_eq!(v["data"][0]["id"], "hermes-agent");
    let g = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == "galion")
        .unwrap();
    assert_eq!(g["root"], "anthropic/claude-haiku-5.5");
    assert_eq!(g["parent"], "hermes-agent");
}

/// galion-one `pull_hermes`: HTTP/1.0, Connection: close, stream, the lane
/// model (`galion`) swapped in for "hermes-agent", max_tokens 160.
#[test]
fn galion_one_voice_stream_http10() {
    let h = harness();
    let payload = json!({"model": "galion", "stream": true, "max_tokens": 160,
        "messages": [{"role": "system", "content": "voice setting"}, {"role": "user", "content": "how are you"}]})
    .to_string();
    let head = format!(
        "POST /v1/chat/completions HTTP/1.0\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nAuthorization: {}\r\nX-Hermes-Session-Id: one-voice\r\nX-Hermes-Session-Key: galion-one\r\nContent-Length: {}\r\n\r\n",
        h.addr,
        bearer(),
        payload.len()
    );
    let (code, headers, body) = http(h.addr, &head, &payload);
    assert_eq!(code, 200);
    assert!(headers.contains("content-type: text/event-stream"));
    assert!(headers.contains("x-hermes-session-id: one-voice"));
    assert!(body.trim_end().ends_with("data: [DONE]"));
    let frames = sse_data(&body);
    assert_eq!(frames[0]["object"], "chat.completion.chunk");
    assert_eq!(frames[0]["choices"][0]["delta"]["role"], "assistant");
    assert_eq!(streamed_text(&frames), "echo: how are you");
    let last = frames.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert!(last["usage"]["total_tokens"].as_u64().unwrap() > 0);
    let calls = h.mock.calls.lock().unwrap();
    let (t, r) = &calls[0];
    assert_eq!(t.model, "anthropic/claude-haiku-5.5");
    assert_eq!(r.reasoning, Some(Reasoning::Off));
    assert_eq!(r.max_tokens, Some(160));
    let sys = r.messages[0]["content"].as_str().unwrap();
    assert!(sys.starts_with("You are Hermes."));
    assert!(
        sys.contains("The voice in the room."),
        "galion-one channel prompt is loaded"
    );
    assert!(
        sys.ends_with("voice setting"),
        "client system message is layered last"
    );
}

/// galion-one `ask_hermes` / `admin_say`: non-stream; it reads the first
/// "content" string of the JSON body.
#[test]
fn galion_one_non_stream_json() {
    let h = harness();
    let (code, headers, body) = post(
        h.addr,
        "/v1/chat/completions",
        &[
            ("Authorization", &bearer()),
            ("X-Hermes-Session-Id", "one-admin"),
            ("X-Hermes-Session-Key", "galion-one"),
        ],
        &json!({"model": "galion", "stream": false, "max_tokens": 180,
                "messages": [{"role": "system", "content": "admin line"}, {"role": "user", "content": "status"}]}),
    );
    assert_eq!(code, 200);
    assert!(headers.contains("x-hermes-session-id: one-admin"));
    assert!(headers.contains("x-hermes-session-key: galion-one"));
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["choices"][0]["message"]["role"], "assistant");
    assert_eq!(v["choices"][0]["message"]["content"], "echo: status");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    let first_content = body.find("\"content\":\"").unwrap();
    assert!(body[first_content..].starts_with("\"content\":\"echo: status\""));
    assert!(v["usage"]["prompt_tokens"].is_u64());
}

/// console `hermesChat`: stream, River flash with reasoning disabled,
/// session key `console:user:<u>:project:<p>` and a session id.
#[test]
fn console_chat_stream_river() {
    let h = harness();
    let (code, _, body) = post(
        h.addr,
        "/v1/chat/completions",
        &[
            ("Authorization", &bearer()),
            (
                "X-Hermes-Session-Key",
                "console:user:u1:project:galion-light",
            ),
            (
                "X-Hermes-Session-Id",
                "6f1c2a3b-0000-4000-8000-000000000001",
            ),
        ],
        &json!({"model": "deepseek-ai/DeepSeek-V4.1-Flash", "provider": "river", "stream": true,
                "model_options": {"reasoning": {"enabled": false, "effort": "low"}},
                "messages": [{"role": "user", "content": "earlier"}, {"role": "assistant", "content": "ok"}, {"role": "user", "content": "now"}]}),
    );
    assert_eq!(code, 200);
    assert_eq!(streamed_text(&sse_data(&body)), "echo: now");
    let calls = h.mock.calls.lock().unwrap();
    let (t, r) = &calls[0];
    assert_eq!(t.provider, "river");
    assert_eq!(t.base_url, "https://api.river.ai/v1");
    assert_eq!(r.reasoning, Some(Reasoning::Off));
    // body history is used when the store has none: system + 2 history + new user
    assert_eq!(r.messages.len(), 4);
}

#[test]
fn session_history_and_model_pinning() {
    let h = harness();
    let hdr = [
        ("Authorization", bearer()),
        ("X-Hermes-Session-Id", "pin-1".to_string()),
    ];
    let hdr: Vec<(&str, &str)> = hdr.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let ask = |model: Option<&str>, text: &str| {
        let mut b = json!({"messages": [{"role": "user", "content": text}]});
        if let Some(m) = model {
            b["model"] = json!(m);
        }
        let (code, _, _) = post(h.addr, "/v1/chat/completions", &hdr, &b);
        assert_eq!(code, 200);
    };
    ask(Some("worker"), "one");
    ask(Some("hermes-agent"), "two");
    ask(Some("galion"), "three");
    let calls = h.mock.calls.lock().unwrap();
    assert_eq!(calls[0].0.model, "anthropic/claude-sonnet-5.5");
    assert_eq!(
        calls[1].0.model, "anthropic/claude-sonnet-5.5",
        "session stays pinned when the turn names no model"
    );
    assert_eq!(
        calls[2].0.model, "anthropic/claude-haiku-5.5",
        "an explicit model wins (Galion patch)"
    );
    // second turn saw the stored first turn
    let m = &calls[1].1.messages;
    assert_eq!(m[1]["content"], "one");
    assert_eq!(m[2]["content"], "echo: one");
    assert_eq!(m[3]["content"], "two");
    drop(calls);
    let (code, _, body) = get(h.addr, "/api/sessions/pin-1/messages", true);
    assert_eq!(code, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 6);
    let (_, _, body) = get(h.addr, "/api/sessions/pin-1", true);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["model"], "anthropic/claude-sonnet-5.5");
}

#[test]
fn session_key_resumes_latest_session() {
    let h = harness();
    let hdr = [
        ("Authorization", bearer()),
        ("X-Hermes-Session-Key", "galion-one".to_string()),
    ];
    let hdr: Vec<(&str, &str)> = hdr.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (_, h1, _) = post(
        h.addr,
        "/v1/chat/completions",
        &hdr,
        &json!({"messages": [{"role": "user", "content": "a"}]}),
    );
    let (_, h2, _) = post(
        h.addr,
        "/v1/chat/completions",
        &hdr,
        &json!({"messages": [{"role": "user", "content": "b"}]}),
    );
    let sid = |hs: &str| {
        hs.lines()
            .find(|l| l.starts_with("x-hermes-session-id:"))
            .map(|l| l.to_string())
    };
    assert_eq!(sid(&h1), sid(&h2));
    assert!(sid(&h1).is_some());
}

/// console `startHermesRun` -> 202 {run_id,status}; events SSE; poll.
#[test]
fn console_runs_lifecycle() {
    let h = harness();
    let (code, _, body) = post(
        h.addr,
        "/v1/runs",
        &[
            ("Authorization", &bearer()),
            ("X-Hermes-Session-Key", "console:user:u1:project:p"),
        ],
        &json!({"input": "research this", "instructions": "You are Hermes Research.",
                "model": "anthropic/claude-opus-5.5", "provider": "openrouter",
                "conversation_history": [{"role": "user", "content": "board"}, {"role": "assistant", "content": "read"}],
                "model_options": {"reasoning": {"enabled": true, "effort": "high"}}}),
    );
    assert_eq!(code, 202);
    let v: Value = serde_json::from_str(&body).unwrap();
    let run_id = v["run_id"].as_str().unwrap().to_string();
    assert_eq!(v["status"], "started");
    let (code, headers, body) = get(h.addr, &format!("/v1/runs/{run_id}/events"), true);
    assert_eq!(code, 200);
    assert!(headers.contains("text/event-stream"));
    let ev = sse_data(&body);
    let names: Vec<&str> = ev.iter().filter_map(|e| e["event"].as_str()).collect();
    assert!(names.contains(&"message.delta"));
    assert_eq!(*names.last().unwrap(), "run.completed");
    let done = ev.last().unwrap();
    assert_eq!(done["run_id"], run_id.as_str());
    assert_eq!(done["output"], "echo: research this");
    assert!(body.contains(": stream closed"));
    let (_, _, body) = get(h.addr, &format!("/v1/runs/{run_id}"), true);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["object"], "hermes.run");
    assert_eq!(v["status"], "completed");
    let calls = h.mock.calls.lock().unwrap();
    let (_, r) = &calls[0];
    assert_eq!(r.reasoning, Some(Reasoning::Effort("high".into())));
    assert!(r.messages[0]["content"]
        .as_str()
        .unwrap()
        .contains("You are Hermes Research."));
    assert_eq!(r.messages[1]["content"], "board");
    drop(calls);
    // approvals are off on Galion: 409 like upstream with no pending approval
    let (code, _, _) = post(
        h.addr,
        &format!("/v1/runs/{run_id}/approval"),
        &[("Authorization", &bearer())],
        &json!({"choice": "once"}),
    );
    assert_eq!(code, 409);
    let (code, _, _) = post(
        h.addr,
        "/v1/runs/run_missing/stop",
        &[("Authorization", &bearer())],
        &json!({}),
    );
    assert_eq!(code, 404);
}

#[test]
fn concurrency_cap_returns_429() {
    let mut mock = MockProvider::echo();
    mock.delay_ms = 600;
    let h = harness_with(mock, |c| c.max_concurrent_runs = 1);
    let (code, _, _) = post(
        h.addr,
        "/v1/runs",
        &[("Authorization", &bearer())],
        &json!({"input": "slow"}),
    );
    assert_eq!(code, 202);
    let (code, _, body) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", &bearer())],
        &json!({"messages": [{"role": "user", "content": "x"}]}),
    );
    assert_eq!(code, 429, "{body}");
}

#[test]
fn spend_cap_blocks_with_402() {
    let mock = MockProvider::new(|_, _| {
        let mut r = ChatResult {
            content: "costly".into(),
            finish_reason: "stop".into(),
            ..Default::default()
        };
        r.usage.cost_usd = 0.5;
        r
    });
    let h = harness_with(mock, |c| {
        c.spend.session_usd_cap = 0.4;
        c.spend.daily_usd_cap = 0.0;
    });
    let hdr = [
        ("Authorization", bearer()),
        ("X-Hermes-Session-Id", "spendy".to_string()),
    ];
    let hdr: Vec<(&str, &str)> = hdr.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let body = json!({"messages": [{"role": "user", "content": "x"}]});
    assert_eq!(post(h.addr, "/v1/chat/completions", &hdr, &body).0, 200);
    let (code, _, b) = post(h.addr, "/v1/chat/completions", &hdr, &body);
    assert_eq!(code, 402, "{b}");
    assert!(b.contains("session spend cap"));
    // another session is not blocked by this one's cap
    let (code, _, _) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", &bearer())],
        &body,
    );
    assert_eq!(code, 200);
}

#[test]
fn tool_calls_stream_progress_events() {
    let h = harness();
    let (code, _, body) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", &bearer())],
        &json!({"stream": true, "messages": [{"role": "user", "content": "tool:clock {}"}]}),
    );
    assert_eq!(code, 200);
    assert!(body.contains("event: hermes.tool.progress"));
    assert!(streamed_text(&sse_data(&body)).starts_with("done: "));
}

#[test]
fn bad_requests() {
    let h = harness();
    let (code, _, _) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", &bearer())],
        &json!({"messages": []}),
    );
    assert_eq!(code, 400);
    let long = "x".repeat(300);
    let (code, _, _) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", &bearer()), ("X-Hermes-Session-Id", &long)],
        &json!({"messages": [{"role": "user", "content": "x"}]}),
    );
    assert_eq!(code, 400);
    assert_eq!(get(h.addr, "/nope", true).0, 404);
    assert_eq!(
        get(h.addr, "/p/default/v1/models", true).0,
        200,
        "multiplex default profile mirror"
    );
    assert_eq!(get(h.addr, "/p/other/v1/models", true).0, 404);
    let _ = std::fs::remove_dir_all(&h.home);
}

#[test]
fn delegate_task_end_to_end_with_caps() {
    let h = harness();
    let (code, _, body) = post(
        h.addr,
        "/v1/chat/completions",
        &[
            ("Authorization", &bearer()),
            ("X-Hermes-Session-Id", "boss-1"),
        ],
        &json!({"stream": true, "messages": [{"role": "user", "content": "delegate_task: find a | find b"}]}),
    );
    assert_eq!(code, 200);
    assert!(body.contains("\"tool\":\"delegate_task\""));
    let text = streamed_text(&sse_data(&body));
    assert!(
        text.contains("echo: find a") && text.contains("echo: find b"),
        "{text}"
    );
    let calls = h.mock.calls.lock().unwrap();
    assert_eq!(calls.len(), 4, "boss, two children, boss again");
    assert_eq!(calls[0].0.model, "anthropic/claude-opus-5.5");
    assert!(calls[0]
        .1
        .tools
        .iter()
        .any(|t| t["function"]["name"] == "delegate_task"));
    let kids: Vec<_> = calls[1..3].iter().collect();
    assert!(kids
        .iter()
        .all(|(t, r)| t.model == "anthropic/claude-sonnet-5.5"
            && r.tools
                .iter()
                .all(|x| x["function"]["name"] != "delegate_task")));
    drop(calls);
    // four tasks exceed max_concurrent_children=3: the tool refuses, the boss answers
    let (code, _, body) = post(
        h.addr,
        "/v1/chat/completions",
        &[("Authorization", &bearer())],
        &json!({"messages": [{"role": "user", "content": "delegate_task: 1|2|3|4"}]}),
    );
    assert_eq!(code, 200);
    assert!(body.contains("Too many tasks"), "{body}");
}

#[test]
fn persona_plex_on_galion_one_when_configured() {
    let plex = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../openmind-persona/personas/persona-plex-v1.md"
    );
    let h = harness_with(MockProvider::echo(), |c| {
        c.persona_file = Some(PathBuf::from(plex));
        c.system_prompt = Some("Reply in exactly four sentences.".into());
    });
    for (sid, rule) in [
        ("one-voice", "Spoken form only"),
        ("one-admin", "Text channel rules"),
    ] {
        let (code, _, _) = post(
            h.addr,
            "/v1/chat/completions",
            &[
                ("Authorization", &bearer()),
                ("X-Hermes-Session-Id", sid),
                ("X-Hermes-Session-Key", "galion-one"),
            ],
            &json!({"model": "galion", "messages": [{"role": "system", "content": "client voice setting"}, {"role": "user", "content": "hi"}]}),
        );
        assert_eq!(code, 200);
        let calls = h.mock.calls.lock().unwrap();
        let sys = calls.last().unwrap().1.messages[0]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(sys.starts_with("You are Hermes."));
        assert!(sys.contains("The voice in the room."));
        assert!(sys.contains("You are Persona Plex, the voice of Galion One"));
        assert!(sys.contains(rule), "{sid}");
        assert!(
            !sys.contains("four sentences"),
            "persona channel drops the global length rule"
        );
        assert!(
            sys.find("client voice setting").unwrap() < sys.find("You are Persona Plex").unwrap(),
            "persona goes last"
        );
    }
    // other channels are untouched
    let (code, _, _) = post(
        h.addr,
        "/v1/chat/completions",
        &[
            ("Authorization", &bearer()),
            ("X-Hermes-Session-Key", "console:user:u:project:p"),
        ],
        &json!({"messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(code, 200);
    let calls = h.mock.calls.lock().unwrap();
    let sys = calls.last().unwrap().1.messages[0]["content"]
        .as_str()
        .unwrap();
    assert!(!sys.contains("Persona Plex") && sys.contains("four sentences"));
}
