// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Stack tools against mock loopback services and temp files: shapes,
//! redaction (no token, no VPS-style IP), GET-only, and the off-by-default
//! write tool that only ever creates new notes.

mod common;

use common::{make_vault, snapshot, TempDir};
use openmind_mcp_stack::memory::MemoryClient;
use openmind_mcp_stack::notes::NoteCreator;
use openmind_mcp_stack::stack::{SpendRules, Stack};
use openmind_mcp_stack::vault::Vault;
use openmind_mcp_stack::{Config, Server, STACK_TOOL_NAMES, WRITE_TOOL_NAMES};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const STAMP: &str = "2026-10-30T12:00:00+01:00";
const LEAK_IP: &str = "203.0.113.77";
const LEAK_TOKEN: &str = "sk-or-v1-0123456789abcdef0123456789abcdef";

type Seen = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

/// Mock HTTP service: exact path -> JSON body (200), else 404.
fn mock(routes: Vec<(&'static str, Value)>) -> (String, Seen) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for stream in l.incoming().flatten() {
            let mut stream = stream;
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut tmp).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            let head = String::from_utf8_lossy(&buf).to_string();
            let mut first = head.lines().next().unwrap_or("").split_whitespace();
            let method = first.next().unwrap_or("").to_string();
            let path = first.next().unwrap_or("").to_string();
            let auth = head.lines().find_map(|l| {
                l.strip_prefix("Authorization: Bearer ")
                    .map(|v| v.trim().to_string())
            });
            s2.lock().unwrap().push((method, path.clone(), auth));
            let (code, body) = routes
                .iter()
                .find(|(p, _)| *p == path)
                .map_or((404, json!({"error":"not found"})), |(_, b)| {
                    (200, b.clone())
                });
            let body = body.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (url, seen)
}

fn client(url: &str, key: Option<&str>) -> MemoryClient {
    MemoryClient::new(url, key.map(String::from), Duration::from_secs(3)).unwrap()
}

struct Fixture {
    server: Server,
    seen: Vec<Seen>,
    files: TempDir,
    _docs: TempDir,
    _o: TempDir,
}

fn fixture(readiness: bool) -> Fixture {
    let (nerve, s1) = mock(vec![
        (
            "/health",
            json!({"status":"ok","score":91,"commit":"18a5a72","state_ts":"2026-10-30T12:16:01+01:00","state_age_s":30,"state_fresh":true}),
        ),
        (
            "/state",
            json!({"ts":"2026-10-30T12:16:01+01:00","system":{"status":"degrading","score":91,"verdict":"1 open incident"},
            "components":[
                {"name":"openmind-memory","status":"healthy","unit":"openmind-memory.service","unit_state":"active","unit_sub":"running","port":8420,"http":200,"restarts_1h":0,"reasons":[],"last_error":null,"host":LEAK_IP},
                {"name":"console","status":"degrading","unit":"console-rust-primary.service","unit_state":"active","unit_sub":"running","port":18080,"http":200,"restarts_1h":1,"reasons":["1 restart"],"last_error":format!("probe of {LEAK_IP} failed")}]}),
        ),
        (
            "/alerts",
            json!([
            {"ts":"2026-10-30T10:31:02+01:00","severity":"warn","component":"console","key":"incident:a","msg":"older"},
            {"ts":"2026-10-30T11:51:32+01:00","severity":"warn","component":"console","key":"incident:b","msg":format!("newer token {LEAK_TOKEN}")}]),
        ),
        (
            "/incidents",
            json!({"open":[{"id":"inc-1","title":"console degrading"}],"recent":[]}),
        ),
    ]);
    let (cortex, s2) = mock(vec![
        (
            "/health",
            json!({"ok":true,"mode":"live","commit":"0cdb778","heal_verify":true,"heals_pending":0,"incidents_open":2,"escalations_open":1,"hermes_handoff":true,"api_token":"should-not-leak-1234"}),
        ),
        (
            "/heals",
            json!({"enabled":true,"verified":3,"failed":1,"pending":0,"success_rate":0.75,"recent":[{"unit":"tuner","outcome":"verified"}]}),
        ),
        (
            "/escalations",
            json!({"open":[{"id":"esc-1","ts":"2026-10-30T09:00:00+01:00","rule":"R2","severity":"error","subject":"unit:x","reason":"r","events":2,"handoff":"handed-to-hermes","lead_event":{"data":{"big":"x"}}}],"closed_total":4}),
        ),
        (
            "/alerts",
            json!([{"id":"al-1","ts":"2026-10-30T12:21:16+01:00","rule":"R99","msg":"http:other signal.error"}]),
        ),
    ]);
    let (queue, s3) = mock(vec![
        (
            "/healthz",
            json!({"ok":true,"function":"POSEIDON","service":"openmind-queue"}),
        ),
        (
            "/stats",
            json!({"depth":{"dead":0,"delayed":0,"leased":1,"ready":2},"load":0,"global":{"cap":null},"agents":{"deploy-check":{"pending":0}},"started":"2026-10-22T23:46:47+02:00"}),
        ),
        (
            "/gaia/status",
            json!({"function":"POSEIDON","health":{"integrity":100,"load":0},"systems":[]}),
        ),
    ]);
    let files = TempDir::new("stackfiles");
    let f = files.path();
    std::fs::write(
        f.join("rs-spend.json"),
        json!({"day":"2026-10-30","day_usd":2.5193,"day_calls":229,
               "sessions":{"a":4.0,"b":3.117,"c":0.0}})
        .to_string(),
    )
    .unwrap();
    let bdir = f.join("backups");
    std::fs::create_dir_all(bdir.join("offsite")).unwrap();
    std::fs::write(
        bdir.join("galion-20261030-072008+0100.tar.gz"),
        vec![0u8; 2 * 1024 * 1024],
    )
    .unwrap();
    std::fs::write(
        bdir.join("offsite/galion-20261030-072008+0100.tar.gz.age"),
        b"age",
    )
    .unwrap();
    std::fs::write(bdir.join("notes.txt"), b"ignored").unwrap();
    if readiness {
        std::fs::write(
            f.join("readiness.json"),
            json!({"score":"12/15","as_of":"2026-10-30T11:41:00+01:00","checks":[{"id":"G9","result":"FAIL"}]}).to_string(),
        )
        .unwrap();
    }
    let (docs, o) = make_vault();
    std::fs::create_dir_all(docs.path().join("platform")).unwrap();
    std::fs::write(
        docs.path().join("platform/plan.md"),
        "Platform plan: universal environment\n",
    )
    .unwrap();
    let stack = Stack {
        nerve: Some(client(&nerve, None).with_label("nerve")),
        cortex: Some(client(&cortex, None).with_label("cortex")),
        queue: Some(client(&queue, None).with_label("queue")),
        spend_file: Some(f.join("rs-spend.json")),
        spend: SpendRules::default(),
        backup_dir: Some(bdir),
        docs: Some(Vault::open(docs.path()).unwrap().with_excluded(&["one"])),
        readiness_file: Some(f.join("readiness.json")),
    };
    let server = Server::new(Config {
        fixed_time: Some(STAMP.into()),
        stack: Some(stack),
        ..Config::default()
    });
    Fixture {
        server,
        seen: vec![s1, s2, s3],
        files,
        _docs: docs,
        _o: o,
    }
}

fn call(s: &Server, name: &str, args: &Value) -> (bool, String, Value) {
    let req = json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":name,"arguments":args}});
    let out = s.handle_bytes(format!("{req}\n").as_bytes());
    let r: Value = serde_json::from_slice(&out).unwrap();
    let res = &r["result"];
    let text = res["content"][0]["text"].as_str().unwrap_or("").to_string();
    let v = serde_json::from_str(&text).unwrap_or(Value::Null);
    (res["isError"] == json!(true), text, v)
}

#[test]
fn every_stack_tool_listed_and_answers_without_leaks() {
    let fx = fixture(true);
    let r: Value = serde_json::from_slice(
        &fx.server
            .handle_bytes(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n"),
    )
    .unwrap();
    let names: Vec<&str> = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for t in STACK_TOOL_NAMES {
        assert!(names.contains(&t), "{t} not listed: {names:?}");
    }
    for t in WRITE_TOOL_NAMES {
        assert!(!names.contains(&t), "write tool {t} must be off by default");
    }
    let args: [(&str, Value); 11] = [
        ("stack_health", json!({})),
        ("stack_health", json!({"component":"console"})),
        ("stack_alerts", json!({"limit":5})),
        ("cortex_heals", json!({})),
        ("spend_status", json!({})),
        ("backup_status", json!({})),
        ("queue_status", json!({})),
        ("docs_list", json!({"folder":"platform"})),
        ("docs_read", json!({"path":"platform/plan.md"})),
        ("docs_search", json!({"query":"universal environment"})),
        ("hermes_readiness", json!({})),
    ];
    for (name, a) in &args {
        let (err, text, _) = call(&fx.server, name, a);
        assert!(!err, "{name} failed: {text}");
        assert!(!text.contains(LEAK_IP), "{name} leaked an IP: {text}");
        assert!(!text.contains("sk-or-v1-"), "{name} leaked a token: {text}");
        assert!(!text.contains("should-not-leak"), "{name} leaked: {text}");
        assert!(
            !text.contains("hermes-test-key"),
            "{name} leaked the key: {text}"
        );
    }
    // only GET was ever sent, and no key to any stack service
    for s in &fx.seen {
        for (m, p, auth) in s.lock().unwrap().iter() {
            assert_eq!(m, "GET", "{p}");
            assert!(auth.is_none(), "{p}");
        }
    }
}

#[test]
fn stack_tool_shapes() {
    let fx = fixture(true);
    let (_, _, h) = call(&fx.server, "stack_health", &json!({}));
    assert_eq!(h["system"]["score"], json!(91));
    assert_eq!(h["components"].as_array().unwrap().len(), 2);
    assert_eq!(h["components"][1]["restarts_1h"], json!(1));
    assert_eq!(h["cortex"]["mode"], json!("live"));
    let (err, text, _) = call(&fx.server, "stack_health", &json!({"component":"nope"}));
    assert!(err && text.contains("console"), "{text}");
    let (_, _, a) = call(&fx.server, "stack_alerts", &json!({"limit":1}));
    assert_eq!(a["nerve_alerts"].as_array().unwrap().len(), 1);
    assert!(a["nerve_alerts"][0]["msg"]
        .as_str()
        .unwrap()
        .starts_with("newer"));
    assert_eq!(a["nerve_incidents"]["open"][0]["id"], json!("inc-1"));
    let (_, _, c) = call(&fx.server, "cortex_heals", &json!({}));
    assert_eq!(c["heals"]["success_rate"], json!(0.75));
    assert_eq!(c["escalations_open"][0]["rule"], json!("R2"));
    assert!(c["escalations_open"][0].get("lead_event").is_none());
    let (_, _, s) = call(&fx.server, "spend_status", &json!({}));
    assert_eq!(s["ledger_total_usd"], json!(7.117));
    assert_eq!(s["fresh_usd"], json!(3.668));
    assert_eq!(s["left_to_stop_line_usd"], json!(0.832));
    assert_eq!(s["state"], json!("ok"));
    assert_eq!(s["top_sessions"].as_array().unwrap().len(), 2);
    let (_, _, b) = call(&fx.server, "backup_status", &json!({}));
    assert_eq!(b["archives"], json!(1));
    assert_eq!(b["offsite_copies"], json!(1));
    assert_eq!(b["state"], json!("ok"), "{b}");
    let (_, _, q) = call(&fx.server, "queue_status", &json!({}));
    assert_eq!(q["depth"]["ready"], json!(2));
    // Galion One is out of scope: its docs folder is never listed, searched or read
    for t in ["one_threads", "one_thread_read"] {
        let (err, text, _) = call(&fx.server, t, &json!({}));
        assert!(err && text.contains("unknown tool"), "{text}");
    }
    let (_, text, _) = call(&fx.server, "docs_list", &json!({}));
    assert!(
        text.contains("platform/plan.md") && !text.contains("one/app.md"),
        "{text}"
    );
    let (_, text, _) = call(
        &fx.server,
        "docs_search",
        &json!({"query":"universal environment"}),
    );
    assert!(!text.contains("one/app.md"), "{text}");
    for p in ["one/app.md", "one"] {
        let (err, _, _) = call(&fx.server, "docs_read", &json!({"path":p}));
        assert!(err, "{p} must be refused");
    }
    let (err, _, _) = call(&fx.server, "docs_list", &json!({"folder":"one"}));
    assert!(err);
    let (_, _, d) = call(
        &fx.server,
        "docs_search",
        &json!({"query":"Galion memory vault"}),
    );
    assert_eq!(d["hits"][0]["path"], json!("23-vault.md"));
    let (err, _, _) = call(&fx.server, "docs_read", &json!({"path":"escape.md"}));
    assert!(err, "symlink out of the docs tree must be refused");
    let (_, _, r) = call(&fx.server, "hermes_readiness", &json!({}));
    assert_eq!(r["live"], json!(false));
    assert_eq!(r["snapshot"]["score"], json!("12/15"));
}

#[test]
fn spend_states_and_missing_parts() {
    let fx = fixture(false);
    let (err, text, _) = call(&fx.server, "hermes_readiness", &json!({}));
    assert!(err && text.contains("no readiness snapshot"), "{text}");
    std::fs::write(
        fx.files.path().join("rs-spend.json"),
        json!({"sessions":{"a":8.0}}).to_string(),
    )
    .unwrap();
    let (_, _, s) = call(&fx.server, "spend_status", &json!({}));
    assert_eq!(s["state"], json!("past the stop line: no new paid calls"));
    std::fs::write(
        fx.files.path().join("rs-spend.json"),
        json!({"sessions":{"a":9.0}}).to_string(),
    )
    .unwrap();
    let (_, _, s) = call(&fx.server, "spend_status", &json!({}));
    assert_eq!(s["state"], json!("over limit: stop all paid calls"));
    assert_eq!(s["left_to_limit_usd"], json!(0.0));
    // nothing configured: no stack tools at all
    let bare = Server::new(Config::default());
    let (err, text, _) = call(&bare, "stack_health", &json!({}));
    assert!(err && text.contains("unknown tool"), "{text}");
    // unreachable service: an error result, not a panic
    let dead = Server::new(Config {
        stack: Some(Stack {
            nerve: Some(client("http://127.0.0.1:9", None).with_label("nerve")),
            ..Stack::default()
        }),
        ..Config::default()
    });
    let (err, text, _) = call(&dead, "stack_health", &json!({}));
    assert!(err && text.starts_with("nerve unreachable"), "{text}");
}

#[test]
fn write_tool_is_off_unless_switched_on_and_never_overwrites() {
    let (v, _o) = make_vault();
    std::fs::create_dir_all(v.path().join("hermes-inbox")).unwrap();
    let before = snapshot(v.path());
    let off = Server::new(Config {
        vault: Some(Vault::open(v.path()).unwrap()),
        fixed_time: Some(STAMP.into()),
        ..Config::default()
    });
    let (err, text, _) = call(&off, "vault_note_create", &json!({"title":"x","text":"y"}));
    assert!(err && text.contains("unknown tool"), "{text}");
    assert_eq!(before, snapshot(v.path()));
    let on = Server::new(Config {
        vault: Some(Vault::open(v.path()).unwrap()),
        fixed_time: Some(STAMP.into()),
        notes: Some(NoteCreator::new(v.path(), "hermes-inbox").unwrap()),
        ..Config::default()
    });
    let (err, text, r) = call(
        &on,
        "vault_note_create",
        &json!({"title":"Daily check","text":"all green"}),
    );
    assert!(!err, "{text}");
    let rel = r["path"].as_str().unwrap().to_string();
    assert!(rel.starts_with("hermes-inbox/"));
    let (err, _, _) = call(
        &on,
        "vault_note_create",
        &json!({"title":"Daily check","text":"again"}),
    );
    assert!(err, "same name must not overwrite");
    // every note that existed before is byte-identical
    let after = snapshot(v.path());
    for (p, bytes) in &before {
        let now = after.iter().find(|(q, _)| q == p).map(|(_, b)| b);
        assert_eq!(now, Some(bytes), "{} changed", p.display());
    }
    assert_eq!(after.len(), before.len() + 1);
}
