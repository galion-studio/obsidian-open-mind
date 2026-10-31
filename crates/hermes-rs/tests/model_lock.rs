// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Model lock (HERMES_RS_MODEL_LOCK=x-ai/grok-4.7) end to end on a loopback
//! gateway with the mock provider and the live config shape of 2026-10-31
//! (boss -> Opus alias, galion -> Haiku route, delegation -> river-flash).
//! No network beyond 127.0.0.1, no spend.

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::provider::MockProvider;
use hermes_rs::session::{MemStore, SessionRow, SessionStore};
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{ClockTool, Registry};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const KEY: &str = "test-key-0123456789abcdef";
const LOCK: &str = "x-ai/grok-4.7";

const LIVE: &str = r#"
model:
  provider: openrouter
  default: x-ai/grok-4.7
  context_length: 65536
  base_url: https://openrouter.ai/api/v1
model_aliases:
  boss:
    model: anthropic/claude-opus-5.5
    provider: openrouter
  worker:
    model: anthropic/claude-sonnet-5.5
    provider: openrouter
  river-flash:
    model: deepseek-ai/DeepSeek-V4.1-Flash
    provider: custom
    base_url: "https://api.river.ai/v1"
providers:
  river:
    api: https://api.river.ai/v1
    default_model: deepseek-ai/DeepSeek-V4.1-Flash
cron:
  model: deepseek-ai/DeepSeek-V4.1-Flash
  model_provider: river
delegation:
  max_concurrent_children: 3
  max_spawn_depth: 1
  model: "anthropic/claude-sonnet-5.5"
  provider: "openrouter"
platforms:
  api_server:
    extra:
      model_routes:
        galion:
          model: "anthropic/claude-haiku-5.5"
          provider: "openrouter"
"#;

struct H {
    addr: SocketAddr,
    mock: Arc<MockProvider>,
    store: Arc<MemStore>,
    cfg: Arc<Config>,
}

fn harness(locked: bool) -> H {
    let home = std::env::temp_dir().join(hermes_rs::new_id("hermes-rs-model-lock-"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("SOUL.md"), "You are Hermes.").unwrap();
    let mut cfg = Config::defaults(&home);
    cfg.apply_yaml(&hermes_rs::yaml::parse(LIVE).unwrap());
    cfg.apply_env(&|k| match k {
        "HERMES_RS_MODEL_LOCK" if locked => Some(LOCK.into()),
        "HERMES_RS_DELEGATION_MODEL" => Some("river-flash".into()),
        _ => None,
    })
    .unwrap();
    let cfg = Arc::new(cfg);
    let mock = Arc::new(MockProvider::echo());
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let mut tools = Registry::new();
    tools.add(Arc::new(ClockTool));
    let agent = hermes_rs::agent::boss(cfg.clone(), mock.clone(), spend, tools);
    let store = Arc::new(MemStore::default());
    let gw = Arc::new(Gateway::new(
        cfg.clone(),
        agent,
        store.clone(),
        Some(KEY.into()),
    ));
    let addr = hermes_rs::api::spawn(gw, "127.0.0.1:0").unwrap();
    H {
        addr,
        mock,
        store,
        cfg,
    }
}

fn post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let b = body.to_string();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        b.len()
    );
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(b.as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let text = String::from_utf8_lossy(&out).to_string();
    let (h, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = h
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (code, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn chat(h: &H, model: Option<&str>, text: &str) -> (u16, Value) {
    let mut b = json!({"messages": [{"role": "user", "content": text}]});
    if let Some(m) = model {
        b["model"] = json!(m);
    }
    post(h.addr, "/v1/chat/completions", &b)
}

fn models_called(h: &H) -> Vec<String> {
    h.mock
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(t, _)| t.model.clone())
        .collect()
}

/// Baseline (lock off): the Friday path. 'boss' reaches Opus.
#[test]
fn without_lock_boss_reaches_opus() {
    let h = harness(false);
    let (code, _) = chat(&h, Some("boss"), "hi");
    assert_eq!(code, 200);
    assert_eq!(models_called(&h), vec!["anthropic/claude-opus-5.5"]);
}

#[test]
fn boss_and_opus_requests_are_refused() {
    let h = harness(true);
    for m in [
        "boss",
        "anthropic/claude-opus-5.5",
        "worker",
        "galion",
        "river-flash",
        "deepseek-ai/DeepSeek-V4.1-Flash",
    ] {
        let (code, v) = chat(&h, Some(m), "hi");
        assert_eq!(code, 400, "{m}: {v}");
        assert_eq!(v["error"]["code"], "model_locked", "{m}: {v}");
        assert!(v["error"]["message"].as_str().unwrap().contains(LOCK));
    }
    let (code, v) = post(h.addr, "/v1/runs", &json!({"model": "boss", "input": "hi"}));
    assert_eq!(code, 400, "{v}");
    assert_eq!(v["error"]["code"], "model_locked");
    let (code, _) = post(
        h.addr,
        "/v1/chat/completions",
        &json!({"provider": "river", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(code, 400);
    assert!(models_called(&h).is_empty(), "nothing reached a provider");
}

#[test]
fn default_and_locked_id_run_on_grok() {
    let h = harness(true);
    assert_eq!(chat(&h, None, "hi").0, 200);
    assert_eq!(chat(&h, Some("hermes-agent"), "hi").0, 200);
    assert_eq!(chat(&h, Some(LOCK), "hi").0, 200);
    assert_eq!(models_called(&h), vec![LOCK, LOCK, LOCK]);
}

#[test]
fn old_opus_session_pin_is_ignored_and_rewritten() {
    let h = harness(true);
    let mut row = SessionRow::new("sess-opus", "api_server", Some("cortex-escalation"));
    row.model = Some("anthropic/claude-opus-5.5".into());
    row.provider = Some("openrouter".into());
    h.store.upsert(&row).unwrap();
    let b = json!({"messages": [{"role": "user", "content": "resume"}]});
    let mut s = TcpStream::connect(h.addr).unwrap();
    let body = b.to_string();
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {KEY}\r\nX-Hermes-Session-Id: sess-opus\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    assert_eq!(models_called(&h), vec![LOCK]);
    assert_eq!(
        h.store.get("sess-opus").unwrap().model.as_deref(),
        Some(LOCK)
    );
    // and the pin endpoint refuses Opus
    let (code, v) = post(
        h.addr,
        "/api/sessions/sess-opus/model",
        &json!({"model": "boss"}),
    );
    assert_eq!(code, 403, "{v}");
    assert_eq!(v["error"]["code"], "model_locked");
    let (code, v) = post(
        h.addr,
        "/api/sessions/sess-opus/model",
        &json!({"model": LOCK}),
    );
    assert_eq!(code, 200, "{v}");
    assert_eq!(v["model"], LOCK);
}

/// The agent's own tool calls cannot switch the model: delegate_task with a
/// smuggled model/provider runs its child on the lock, and no tool offered
/// to the model has a model or provider parameter.
#[test]
fn agent_tool_calls_cannot_switch_model() {
    let h = harness(true);
    let (code, v) = chat(
        &h,
        None,
        r#"tool:delegate_task {"tasks":[{"goal":"g"}],"model":"anthropic/claude-opus-5.5","provider":"river"}"#,
    );
    assert_eq!(code, 200, "{v}");
    let called = models_called(&h);
    assert!(called.len() >= 3, "parent, child, parent again: {called:?}");
    assert!(called.iter().all(|m| m == LOCK), "{called:?}");
    let calls = h.mock.calls.lock().unwrap();
    for (_, req) in calls.iter() {
        for t in &req.tools {
            let name = t["function"]["name"].as_str().unwrap_or("");
            assert!(!name.to_lowercase().contains("model"), "tool {name}");
            let props = &t["function"]["parameters"]["properties"];
            assert!(
                props.get("model").is_none() && props.get("provider").is_none(),
                "tool {name}: {props}"
            );
        }
    }
    assert_eq!(
        h.cfg.delegation.model.as_deref(),
        Some(LOCK),
        "river-flash delegation is locked too"
    );
}

#[test]
fn models_list_shows_only_the_lock() {
    let h = harness(true);
    let mut s = TcpStream::connect(h.addr).unwrap();
    s.write_all(
        format!("GET /v1/models HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {KEY}\r\n\r\n")
            .as_bytes(),
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let body: Value = serde_json::from_str(out.split_once("\r\n\r\n").unwrap().1).unwrap();
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["hermes-agent", LOCK]);
}

#[test]
fn self_modify_cannot_touch_the_lock() {
    use hermes_rs::self_modify::{check_patch, checks_ok};
    let edit_file = "diff --git a/crates/hermes-rs/src/model_lock.rs b/crates/hermes-rs/src/model_lock.rs\n--- a/crates/hermes-rs/src/model_lock.rs\n+++ b/crates/hermes-rs/src/model_lock.rs\n@@ -1 +1 @@\n-a\n+b\n";
    assert!(!checks_ok(&check_patch(edit_file)));
    let remove_use = "diff --git a/crates/hermes-rs/src/api.rs b/crates/hermes-rs/src/api.rs\n--- a/crates/hermes-rs/src/api.rs\n+++ b/crates/hermes-rs/src/api.rs\n@@ -1 +1 @@\n-        if let Some(l) = &self.cfg.model_lock {\n+        if false {\n";
    assert!(!checks_ok(&check_patch(remove_use)));
    let clear = "diff --git a/crates/hermes-rs/src/provider.rs b/crates/hermes-rs/src/provider.rs\n--- a/crates/hermes-rs/src/provider.rs\n+++ b/crates/hermes-rs/src/provider.rs\n@@ -1 +1,2 @@\n x\n+    cfg.model_lock = None;\n";
    assert!(!checks_ok(&check_patch(clear)));
}
