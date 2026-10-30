// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! JSON-RPC bytes in, response bytes out (no child process). Temp vault
//! and a mock memory gateway.

mod common;

use common::{make_vault, mock_memory, snapshot};
use openmind_mcp_stack::memory::{key_from_env_text, parse_response, MemoryClient};
use openmind_mcp_stack::vault::Vault;
use openmind_mcp_stack::{Config, Server, DEFAULT_PROTOCOL, SERVER_NAME, TOOL_NAMES};
use serde_json::{json, Value};
use std::time::Duration;

const STAMP: &str = "2026-10-29T14:00:00+01:00";

fn server(vault: Option<&std::path::Path>, mem_url: Option<&str>, key: Option<&str>) -> Server {
    Server::new(Config {
        memory: mem_url
            .map(|u| MemoryClient::new(u, key.map(String::from), Duration::from_secs(3)).unwrap()),
        vault: vault.map(|p| Vault::open(p).unwrap()),
        save_session: String::new(),
        fixed_time: Some(STAMP.into()),
        stack: None,
        notes: None,
    })
}

fn rpc(s: &Server, line: &str) -> Value {
    let out = s.handle_bytes(format!("{line}\n").as_bytes());
    serde_json::from_slice(&out).unwrap()
}

fn call(s: &Server, name: &str, args: &Value) -> (bool, Value) {
    let req = json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}});
    let r = rpc(s, &req.to_string());
    let res = &r["result"];
    let text = res["content"][0]["text"].as_str().unwrap_or("").to_string();
    let is_err = res["isError"] == json!(true);
    let v = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (is_err, v)
}

#[test]
fn initialize_and_protocol_fallback() {
    let s = server(None, None, None);
    let r = rpc(
        &s,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
    );
    assert_eq!(r["result"]["protocolVersion"], json!("2025-03-26"));
    assert_eq!(r["result"]["serverInfo"]["name"], json!(SERVER_NAME));
    let r = rpc(
        &s,
        r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
    );
    assert_eq!(r["result"]["protocolVersion"], json!(DEFAULT_PROTOCOL));
    assert!(s
        .handle_bytes(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_empty());
    let bad = rpc(&s, "not-json");
    assert_eq!(bad["error"]["code"], json!(-32700));
    let nf = rpc(&s, r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#);
    assert_eq!(nf["error"]["code"], json!(-32601));
}

#[test]
fn tools_list_depends_on_configuration() {
    let (v, _o) = make_vault();
    let m = mock_memory(None);
    let names = |s: &Server| -> Vec<String> {
        let r = rpc(s, r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#);
        r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names(&server(None, None, None)), vec!["openmind_status"]);
    assert_eq!(
        names(&server(Some(v.path()), Some(&m.url), None)).len(),
        TOOL_NAMES.len()
    );
    let only_vault = names(&server(Some(v.path()), None, None));
    assert!(only_vault.iter().all(|n| !n.starts_with("memory_")));
    // a hidden tool cannot be called either
    let (err, _) = call(
        &server(Some(v.path()), None, None),
        "memory_save",
        &json!({"text":"x"}),
    );
    assert!(err);
    let r = rpc(
        &server(None, None, None),
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
    );
    for t in r["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], json!("object"));
        assert_eq!(t["inputSchema"]["additionalProperties"], json!(false));
    }
}

#[test]
fn vault_list_read_search() {
    let (v, _o) = make_vault();
    let s = server(Some(v.path()), None, None);
    let (err, l) = call(&s, "vault_list", &json!({}));
    assert!(!err, "{l}");
    let paths: Vec<&str> = l["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["23-vault.md", "big.md", "daily/2026-10-29.md"]);
    let stamp = l["notes"][0]["modified"].as_str().unwrap();
    assert!(stamp.ends_with("+01:00") || stamp.ends_with("+02:00"));
    let (err, l) = call(&s, "vault_list", &json!({"folder":"daily"}));
    assert!(!err);
    assert_eq!(l["count"], json!(1));

    let (err, r) = call(&s, "vault_read", &json!({"path":"daily/2026-10-29.md"}));
    assert!(!err);
    assert_eq!(r["text"], json!("Line one\nThe Hermes MCP test note\n"));
    assert_eq!(r["truncated"], json!(false));

    let (err, r) = call(&s, "vault_read", &json!({"path":"big.md"}));
    assert!(!err);
    assert_eq!(r["truncated"], json!(true));
    assert_eq!(r["text"].as_str().unwrap().len(), 32 * 1024);
    let (_, r) = call(
        &s,
        "vault_read",
        &json!({"path":"big.md","max_bytes": 10_000_000}),
    );
    assert_eq!(r["text"].as_str().unwrap().len(), 64 * 1024);

    let (err, r) = call(&s, "vault_search", &json!({"query":"hermes mcp"}));
    assert!(!err);
    assert_eq!(r["count"], json!(1));
    assert_eq!(r["hits"][0]["path"], json!("daily/2026-10-29.md"));
    assert_eq!(r["hits"][0]["line"], json!(2));
    // hidden folders and links are never searched
    let (_, r) = call(&s, "vault_search", &json!({"query":"hidden"}));
    assert_eq!(r["count"], json!(0));
    let (_, r) = call(&s, "vault_search", &json!({"query":"OUTSIDE-SECRET"}));
    assert_eq!(r["count"], json!(0));
    let (err, _) = call(&s, "vault_search", &json!({"query":"x"}));
    assert!(err, "1-char query refused");
}

#[test]
fn vault_refuses_traversal_hidden_links_and_non_notes() {
    let (v, outside) = make_vault();
    let s = server(Some(v.path()), None, None);
    let abs = outside.path().join("secret.md").display().to_string();
    for bad in [
        "../outside/secret.md",
        "daily/../../secret.md",
        "./23-vault.md",
        abs.as_str(),
        "/etc/passwd",
        ".git/config",
        ".obsidian/app.json",
        "escape.md",
        "escape-dir/secret.md",
        "image.png",
        "daily",
        "missing.md",
        "",
        "a\\..\\b.md",
    ] {
        let (err, r) = call(&s, "vault_read", &json!({"path": bad}));
        assert!(err, "{bad} should be refused, got {r}");
        assert!(!r.to_string().contains("OUTSIDE-SECRET"), "{bad}");
    }
    let (err, _) = call(&s, "vault_list", &json!({"folder":"../"}));
    assert!(err);
    let (err, _) = call(&s, "vault_list", &json!({"folder":"escape-dir"}));
    assert!(err);
    let long = "a/".repeat(300) + "x.md";
    let (err, _) = call(&s, "vault_read", &json!({"path": long}));
    assert!(err);
}

#[test]
fn vault_tools_never_write() {
    let (v, _o) = make_vault();
    let before = snapshot(v.path());
    let s = server(Some(v.path()), None, None);
    for (name, args) in [
        ("vault_list", json!({})),
        ("vault_read", json!({"path":"23-vault.md"})),
        ("vault_search", json!({"query":"vault"})),
        ("vault_read", json!({"path":"../x.md"})),
        ("openmind_status", json!({})),
    ] {
        let _ = call(&s, name, &args);
    }
    assert_eq!(before, snapshot(v.path()));
    // no write API in the sources
    for src in [
        include_str!("../src/vault.rs"),
        include_str!("../src/lib.rs"),
    ] {
        for word in [
            "fs::write",
            "OpenOptions",
            "create_dir",
            "remove_file",
            "rename(",
            "set_permissions",
        ] {
            assert!(!src.contains(word), "{word} found in source");
        }
    }
}

#[test]
fn vault_root_must_be_a_real_folder() {
    assert!(Vault::open("/").is_err());
    assert!(Vault::open("/definitely/not/here").is_err());
}

#[test]
fn memory_tools_round_trip_against_mock_with_key() {
    let m = mock_memory(Some("k-test-123"));
    let s = server(None, Some(&m.url), Some("k-test-123"));
    let (err, r) = call(
        &s,
        "memory_save",
        &json!({"text":"Mate prefers CET times in reports"}),
    );
    assert!(!err, "{r}");
    assert_eq!(r["saved"], json!(true));
    assert_eq!(r["session_key"], json!("hermes-mcp-notes"));
    let (err, r) = call(
        &s,
        "memory_search",
        &json!({"query":"cet times","limit":99}),
    );
    assert!(!err, "{r}");
    assert_eq!(r["total"], json!(1));
    let (err, r) = call(&s, "memory_recall", &json!({"query":"cet times"}));
    assert!(!err);
    assert_eq!(r["scope"], json!("all"));
    let (_, r) = call(
        &s,
        "memory_recall",
        &json!({"query":"cet","session_key":"s1"}),
    );
    assert_eq!(r["scope"], json!("session"));
    let (err, _) = call(&s, "memory_conversations", &json!({"query":"cet"}));
    assert!(!err);
    // no structured hit -> conversation matches attached
    let (err, r) = call(&s, "memory_search", &json!({"query":"nl1 cet"}));
    assert!(!err, "{r}");
    assert_eq!(r["total"], json!(0));
    assert!(r.get("conversations").is_some(), "{r}");
    let seen = m.seen.lock().unwrap().clone();
    let paths: Vec<&str> = seen.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "/capture",
            "/search/memories",
            "/recall",
            "/recall",
            "/search/conversations",
            "/search/memories",
            "/search/conversations"
        ]
    );
    assert!(seen
        .iter()
        .all(|x| x.auth.as_deref() == Some("k-test-123") && x.method == "POST"));
    assert_eq!(seen[1].body["limit"], json!(20), "limit clamped");
    assert_eq!(
        seen[0].body["user_content"],
        json!("Mate prefers CET times in reports")
    );
    // status shows key=true but never the key
    let (_, st) = call(&s, "openmind_status", &json!({}));
    assert_eq!(st["memory"]["key"], json!(true));
    assert!(!st.to_string().contains("k-test-123"));
    assert_eq!(st["observed_at"], json!(STAMP));
}

#[test]
fn memory_errors_are_tool_errors() {
    let m = mock_memory(Some("right"));
    let s = server(None, Some(&m.url), Some("wrong"));
    let (err, r) = call(&s, "memory_search", &json!({"query":"x"}));
    assert!(err);
    assert!(r.as_str().unwrap().contains("401"));
    assert!(!r.as_str().unwrap().contains("right") && !r.as_str().unwrap().contains("wrong"));
    let (err, _) = call(&s, "memory_save", &json!({"text": "y".repeat(9000)}));
    assert!(err);
    let (err, _) = call(&s, "memory_save", &json!({}));
    assert!(err);
    // nothing listening
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let s = server(None, Some(&url), None);
    let (err, r) = call(&s, "memory_search", &json!({"query":"x"}));
    assert!(err);
    assert!(r.as_str().unwrap().contains("unreachable"));
}

#[test]
fn memory_url_must_be_loopback_http() {
    let t = Duration::from_secs(1);
    assert!(MemoryClient::new("http://127.0.0.1:8420", None, t).is_ok());
    assert!(MemoryClient::new("http://localhost:8420", None, t).is_ok());
    assert!(MemoryClient::new("https://127.0.0.1:8420", None, t).is_err());
    assert!(MemoryClient::new("http://192.0.2.1:8420", None, t).is_err());
    assert!(MemoryClient::new("http://memory.example:8420", None, t).is_err());
    assert!(MemoryClient::new("http://127.0.0.1:8420/x", None, t).is_err());
    assert!(MemoryClient::new("http://u@127.0.0.1:8420", None, t).is_err());
    let dbg = format!(
        "{:?}",
        MemoryClient::new("http://127.0.0.1:1", Some("sekrit".into()), t).unwrap()
    );
    assert!(!dbg.contains("sekrit"));
}

#[test]
fn key_file_parsing_takes_only_known_names() {
    let text = "OPENROUTER_API_KEY=nope\nexport TDAI_GATEWAY_API_KEY=\"abc\"\n# c\n";
    assert_eq!(key_from_env_text(text).as_deref(), Some("abc"));
    let both = "TDAI_GATEWAY_API_KEY=b\nOPENMIND_MEMORY_API_KEY=a\n";
    assert_eq!(key_from_env_text(both).as_deref(), Some("a"));
    assert_eq!(key_from_env_text("OTHER=1\n"), None);
}

#[test]
fn http_parsing_handles_length_and_chunked() {
    let (s, b) = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}xx").unwrap();
    assert_eq!((s, b.as_slice()), (200, b"{}".as_slice()));
    let (s, b) = parse_response(
        b"HTTP/1.1 201 C\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n",
    )
    .unwrap();
    assert_eq!(s, 201);
    assert_eq!(b, br#"{"a":1}"#);
    assert!(parse_response(b"garbage").is_err());
}
