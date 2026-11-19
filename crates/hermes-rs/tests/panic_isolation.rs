// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! One bad request or a panicking turn returns an error. The next call still runs.

use hermes_rs::agent::boss;
use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::http::{Request, Response};
use hermes_rs::provider::{ChatRequest, ChatResult, MockProvider, Target};
use hermes_rs::session::{MemStore, SessionRow, SessionStore};
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::Registry;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const SAMPLES: &[&str] = &[
    "Črnomerec, Šibenik",
    "Zażółć gęślą jaźń",
    "Češljuga u Đakovu",
    "中文",
    "日本語",
    "مرحبا",
    "नमस्ते",
    "👩‍👩‍👧‍👦",
    "e\u{301}",
];

fn text_of(resp: Response) -> (u16, String) {
    match resp {
        Response::Full { status, body, .. } => (status, String::from_utf8_lossy(&body).into_owned()),
        Response::Stream { status, .. } => (status, String::new()),
    }
}

fn req(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Request {
    Request {
        method: method.into(),
        path: path.into(),
        query: String::new(),
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        body: body.to_vec(),
    }
}

fn gateway(store: Arc<dyn SessionStore>, provider: Arc<dyn hermes_rs::provider::Provider>) -> Arc<Gateway> {
    let home = std::env::temp_dir().join(hermes_rs::new_id("hermes-panic-"));
    std::fs::create_dir_all(&home).unwrap();
    let cfg = Arc::new(Config::defaults(&home));
    let spend = Arc::new(SpendTracker::new(cfg.spend.clone(), None));
    let agent = boss(cfg.clone(), provider, spend, Registry::new());
    Arc::new(Gateway::new(cfg, agent, store, None))
}

struct BoomStore;
impl SessionStore for BoomStore {
    fn get(&self, _: &str) -> Option<SessionRow> {
        let s = "Ž";
        let _ = &s[..1];
        None
    }
    fn find_by_key(&self, _: &str, _: &str) -> Option<SessionRow> {
        None
    }
    fn upsert(&self, _: &SessionRow) -> Result<(), String> {
        Ok(())
    }
    fn messages(&self, _: &str) -> Vec<Value> {
        Vec::new()
    }
    fn append(&self, _: &str, _: &[Value]) -> Result<(), String> {
        Ok(())
    }
    fn list(&self, _: usize) -> Vec<SessionRow> {
        Vec::new()
    }
}

fn chat_body(text: &str) -> Vec<u8> {
    json!({"messages": [{"role": "user", "content": text}]}).to_string().into_bytes()
}

#[test]
fn a_panicking_session_is_an_error_and_the_next_request_works() {
    let gw = gateway(Arc::new(BoomStore), Arc::new(MockProvider::echo()));
    let bad = gw.handle(req(
        "POST",
        "/v1/chat/completions",
        &[("x-hermes-session-id", "sess-č")],
        &chat_body("Črnomerec"),
    ));
    let (status, body) = text_of(bad);
    assert_eq!(status, 500, "{body}");
    assert!(body.contains("session_panic"), "{body}");
    let (health, _) = text_of(gw.handle(req("GET", "/health", &[], b"")));
    assert_eq!(health, 200);
}

#[test]
fn multilingual_chat_round_trips_and_invalid_bytes_do_not_abort() {
    let gw = gateway(Arc::new(MemStore::default()), Arc::new(MockProvider::echo()));
    let text = SAMPLES.join(" ");
    let (status, body) = text_of(gw.handle(req(
        "POST",
        "/v1/chat/completions",
        &[],
        &chat_body(&text),
    )));
    assert_eq!(status, 200, "{body}");
    for s in SAMPLES {
        assert!(body.contains(s), "missing {s} in {body}");
    }
    let junk = gw.handle(req(
        "POST",
        "/v1/chat/completions",
        &[],
        &[0xff, 0xfe, b'{', 0xc3, 0x28],
    ));
    let (status, _) = text_of(junk);
    assert_eq!(status, 400);
    let (health, _) = text_of(gw.handle(req("GET", "/health", &[], b"")));
    assert_eq!(health, 200);
}

#[test]
fn a_panicking_turn_is_an_error_and_the_next_turn_runs() {
    let once = Arc::new(AtomicBool::new(false));
    let flag = once.clone();
    let mock = MockProvider::new(move |_t: &Target, r: &ChatRequest| {
        if !flag.swap(true, Ordering::SeqCst) {
            let s = "Ž";
            let _ = &s[..1];
        }
        let user = r
            .messages
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("")
            .to_string();
        ChatResult {
            content: user,
            finish_reason: "stop".into(),
            ..ChatResult::default()
        }
    });
    let gw = gateway(Arc::new(MemStore::default()), Arc::new(mock));
    let (status, body) = text_of(gw.handle(req(
        "POST",
        "/v1/chat/completions",
        &[],
        &chat_body("first"),
    )));
    assert_eq!(status, 502, "{body}");
    assert!(body.contains("agent.turn"), "{body}");
    let (status, body) = text_of(gw.handle(req(
        "POST",
        "/v1/chat/completions",
        &[],
        &chat_body("Zażółć"),
    )));
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("Zażółć"), "{body}");
}
