// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Stdin bytes in, response bytes out. No child process.

use openmind_mcp_stack::{StackError, StackServer, DEFAULT_PROTOCOL, SERVER_NAME, TOOL_NAME};
use serde_json::{json, Value};

const STAMP: &str = "2026-10-19T16:39:10+02:00";

fn server(crates: &[&str]) -> StackServer {
    StackServer::with_timestamp(crates, STAMP).unwrap()
}

fn responses(raw: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(raw).unwrap();
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn session(parts: &[&str]) -> Vec<u8> {
    let mut buf = String::new();
    for part in parts {
        buf.push_str(part);
        if !part.ends_with('\n') {
            buf.push('\n');
        }
    }
    buf.into_bytes()
}

#[test]
fn initialize_echoes_protocol_and_server_info() {
    let raw = session(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0.0.0"}}}"#,
    ]);
    let got = responses(&server(&["openmind-schema"]).handle_bytes(&raw));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["id"], json!(1));
    assert_eq!(got[0]["result"]["protocolVersion"], json!("2025-03-26"));
    assert_eq!(got[0]["result"]["serverInfo"]["name"], json!(SERVER_NAME));
    assert_eq!(got[0]["result"]["serverInfo"]["version"], json!("0.1.0"));
    assert_eq!(
        got[0]["result"]["capabilities"]["tools"]["listChanged"],
        json!(false)
    );
}

#[test]
fn unknown_protocol_falls_back_without_zulu() {
    let raw = br#"{"jsonrpc":"2.0","id":"abc","method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#;
    let got = responses(&server(&[]).handle_bytes(raw));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0]["id"], json!("abc"));
    assert_eq!(got[0]["result"]["protocolVersion"], json!(DEFAULT_PROTOCOL));
}

#[test]
fn tools_list_is_only_stack_status() {
    let raw = br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
    let got = responses(&server(&["openmind-schema"]).handle_bytes(raw));
    let tools = got[0]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], json!(TOOL_NAME));
    assert_eq!(tools[0]["inputSchema"]["type"], json!("object"));
    assert_eq!(
        tools[0]["inputSchema"]["additionalProperties"],
        json!(false)
    );
}

#[test]
fn stack_status_returns_given_crates_and_zagreb_stamp() {
    let names = ["zagreb-time", "openmind-schema", "openmind-config"];
    let raw = br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"stack_status","arguments":{}}}"#;
    let got = responses(&server(&names).handle_bytes(raw));
    assert_eq!(got[0]["result"]["isError"], json!(false));
    let text = got[0]["result"]["content"][0]["text"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(text).unwrap();
    assert_eq!(parsed, server(&names).fixture());
    assert_eq!(
        got[0]["result"]["structuredContent"]["crates"],
        json!(names)
    );
    assert_eq!(
        got[0]["result"]["structuredContent"]["observed_at"],
        json!(STAMP)
    );
    let stamp = parsed["observed_at"].as_str().unwrap();
    assert!(stamp.ends_with("+01:00") || stamp.ends_with("+02:00"));
    assert!(!stamp.contains('Z'));
    assert!(!stamp.contains("+00:00"));
}

#[test]
fn session_skips_notifications_and_keeps_order() {
    let raw = session(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"stack_status"}}"#,
    ]);
    let got = responses(&server(&["openmind-schema"]).handle_bytes(&raw));
    assert_eq!(got.len(), 3);
    assert_eq!(got[0]["id"], json!(1));
    assert!(got[0]["result"]["serverInfo"].is_object());
    assert_eq!(got[1]["id"], json!(2));
    assert_eq!(got[1]["result"]["tools"][0]["name"], json!(TOOL_NAME));
    assert_eq!(got[2]["id"], json!(3));
    assert_eq!(
        got[2]["result"]["structuredContent"]["crates"],
        json!(["openmind-schema"])
    );
}

#[test]
fn notification_and_blank_lines_emit_nothing() {
    let server = server(&["openmind-schema"]);
    let silent = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert!(server.handle_bytes(silent).is_empty());
    assert!(server.handle_bytes(b"\n\n").is_empty());
}

#[test]
fn unknown_method_and_bad_json_keep_protocol_errors() {
    let server = server(&["openmind-schema"]);
    let missing =
        responses(&server.handle_bytes(br#"{"jsonrpc":"2.0","id":9,"method":"resources/list"}"#));
    assert_eq!(missing[0]["id"], json!(9));
    assert_eq!(missing[0]["error"]["code"], json!(-32601));
    let parsed = responses(&server.handle_bytes(b"not-json\n"));
    assert_eq!(parsed[0]["id"], Value::Null);
    assert_eq!(parsed[0]["error"]["code"], json!(-32700));
    let ping = responses(&server.handle_bytes(br#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#));
    assert_eq!(ping[0]["result"], json!({}));
}

#[test]
fn rejects_paths_zulu_and_empty_names() {
    let zulu = format!("2026-10-19T14:39:10{}", "Z");
    assert_eq!(
        StackServer::with_timestamp(["openmind-schema"], zulu).unwrap_err(),
        StackError::BadTimestamp
    );
    assert_eq!(
        StackServer::with_timestamp(["openmind-schema"], "2026-10-19T14:39:10+00:00").unwrap_err(),
        StackError::BadTimestamp
    );
    let foreign = format!("/opt/galion/{}", "hermes");
    assert_eq!(
        StackServer::with_timestamp([foreign.as_str()], STAMP).unwrap_err(),
        StackError::BadCrate
    );
    let db = format!("state.{}", "db");
    assert_eq!(
        StackServer::with_timestamp([db.as_str()], STAMP).unwrap_err(),
        StackError::BadCrate
    );
    assert_eq!(
        StackServer::with_timestamp([""], STAMP).unwrap_err(),
        StackError::BadCrate
    );
}

#[test]
fn live_stamp_is_zagreb_not_zulu() {
    let server = StackServer::new(["openmind-schema"]).unwrap();
    let stamp = server.observed_at();
    assert_eq!(stamp.len(), 25);
    assert!(stamp.ends_with("+01:00") || stamp.ends_with("+02:00"));
    assert!(!stamp.contains('Z'));
    assert!(!stamp.contains("+00:00"));
    assert_eq!(server.crates(), &["openmind-schema".to_owned()]);
}

#[test]
fn implementation_names_no_foreign_tree_database_or_socket_api() {
    let files = [
        include_str!("../src/lib.rs"),
        include_str!("../src/main.rs"),
        include_str!("../Cargo.toml"),
    ];
    for src in files {
        let folded = src.to_ascii_lowercase();
        assert!(!folded.contains("hermes"));
        assert!(!folded.contains("state.db"));
        assert!(!folded.contains("tcplistener"));
        assert!(!folded.contains("std::net"));
        assert!(!folded.contains("std::fs"));
        assert!(!folded.contains("reqwest"));
        assert!(!folded.contains("tokio"));
        assert!(!folded.contains("hyper"));
        assert!(!src.contains("Command::new"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn handle_bytes_opens_no_descriptor() {
    let before = fd_targets();
    let raw = session(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"stack_status","arguments":{}}}"#,
    ]);
    let _ = server(&["openmind-schema", "zagreb-time"]).handle_bytes(&raw);
    let after = fd_targets();
    assert_eq!(before, after);
    assert!(after
        .iter()
        .all(|target| { !target.contains("/opt/galion/hermes") && !target.contains("state.db") }));
}

#[cfg(target_os = "linux")]
fn fd_targets() -> Vec<String> {
    let mut names = Vec::new();
    let dir = std::fs::read_dir("/proc/self/fd").unwrap();
    for entry in dir.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            names.push(target.to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}
