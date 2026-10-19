// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

#![forbid(unsafe_code)]

use openmind_memory::{parse_bind, Index, Kind, PutRequest, SearchRequest, Server};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn assert_zagreb(ts: &str) {
    assert!(
        ts.ends_with("+01:00") || ts.ends_with("+02:00"),
        "missing Zagreb offset: {ts}"
    );
    assert!(!ts.contains('Z'), "{ts}");
    assert!(!ts.contains("+00:00"), "{ts}");
}

fn scratch() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("openmind-memory-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn put() {
    let mut index = Index::open_memory().unwrap();
    let note = index
        .put(&PutRequest::memory("Galion stores the spare note offline."))
        .unwrap();
    assert!(note.id.starts_with('m'), "{}", note.id);
    assert_zagreb(&note.recorded_at);

    let talk = index
        .put(&PutRequest::conversation(
            "sess-put",
            "user",
            "Please remember the blue folder.",
        ))
        .unwrap();
    assert!(talk.id.starts_with('c'), "{}", talk.id);
    assert_zagreb(&talk.recorded_at);
    assert_ne!(note.id, talk.id);

    let again = Index::open_memory().unwrap();
    let miss = again.search(&SearchRequest::new("blue")).unwrap();
    assert!(miss.is_empty());
}

#[test]
fn search() {
    let mut index = Index::open_memory().unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/notes.json");
    let loaded = index.load_fixture_path(&fixture).unwrap();
    assert_eq!(loaded, 2);

    let mut memories = SearchRequest::new("Zagreb notes");
    memories.kind = Some(Kind::Memory);
    let hits = index.search(&memories).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].kind, Kind::Memory);
    assert_eq!(hits[0].scene, "desk");
    assert!(hits[0].content.contains("Zagreb"));
    assert_zagreb(&hits[0].recorded_at);
    assert!(hits[0].score > 0.0);

    let mut desk_only = memories.clone();
    desk_only.scene = Some("desk".to_string());
    assert_eq!(index.search(&desk_only).unwrap().len(), 1);
    desk_only.scene = Some("kitchen".to_string());
    assert!(index.search(&desk_only).unwrap().is_empty());

    let mut talks = SearchRequest::new("spare port");
    talks.kind = Some(Kind::Conversation);
    talks.session_key = Some("fixture-session".to_string());
    let found = index.search(&talks).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].role, "user");
    assert!(found[0].content.contains("spare memory port"));

    talks.session_key = Some("other-session".to_string());
    assert!(index.search(&talks).unwrap().is_empty());

    let mut miss = SearchRequest::new("submarine");
    miss.kind = Some(Kind::Memory);
    assert!(index.search(&miss).unwrap().is_empty());

    let err = index.search(&SearchRequest::new("   ")).unwrap_err();
    assert!(err.to_string().contains("query"));
}

#[test]
fn put_then_search_temp_file() {
    let dir = scratch();
    let path = dir.join("index.db");
    let recorded = {
        let mut index = Index::open_path(&path).unwrap();
        let stored = index
            .put(&PutRequest::memory("Temp file keeps the harbour note."))
            .unwrap();
        assert_zagreb(&stored.recorded_at);
        stored.recorded_at
    };
    let index = Index::open_path(&path).unwrap();
    let mut req = SearchRequest::new("harbour");
    req.kind = Some(Kind::Memory);
    let hits = index.search(&req).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].recorded_at, recorded);
    assert_zagreb(&hits[0].recorded_at);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn http_put_and_search_on_ephemeral_port() {
    let server = Server::spawn(Arc::new(Mutex::new(Index::open_memory().unwrap()))).unwrap();
    let addr = server.addr();
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert_ne!(addr.port(), 0);
    assert_ne!(addr.port(), 8420);

    let (status, health) = exchange(addr, "GET", "/health", "", "");
    assert_eq!(status, 200, "{health}");
    assert_eq!(health["status"], "ok");
    assert_eq!(health["version"], "0.1.0");
    assert_eq!(health["implementation"], "openmind-memory");
    assert_eq!(health["embeddingService"], false);
    assert_eq!(health["stores"]["embeddingService"], false);
    assert!(!health.to_string().contains('Z'));

    let capture = r#"{"user_content":"Store the cedar key.","assistant_content":"Cedar key stored.","session_key":"http-sess"}"#;
    let (status, body) = exchange(addr, "POST", "/capture", "", capture);
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["l0_recorded"], 2);
    assert_eq!(body["scheduler_notified"], false);

    let (status, body) = exchange(
        addr,
        "POST",
        "/search/memories",
        "",
        r#"{"query":"cedar","limit":5}"#,
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["strategy"], "fts");
    assert_eq!(body["total"], 1);
    let text = body["results"].as_str().unwrap();
    assert!(text.contains("cedar"), "{text}");
    assert!(text.contains("episodic"), "{text}");

    let (status, body) = exchange(
        addr,
        "POST",
        "/search/conversations",
        "",
        r#"{"query":"cedar","session_key":"http-sess","limit":5}"#,
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["total"], 2);
    assert!(body["results"]
        .as_str()
        .unwrap()
        .contains("Store the cedar key."));

    let (status, miss) = exchange(
        addr,
        "POST",
        "/search/memories",
        "",
        r#"{"query":"submarine"}"#,
    );
    assert_eq!(status, 200, "{miss}");
    assert_eq!(miss["total"], 0);
    assert_eq!(miss["results"], "No matching memories found.");

    let (status, missing) = exchange(addr, "POST", "/search/conversations", "", "{}");
    assert_eq!(status, 400, "{missing}");
    assert!(missing["error"].as_str().unwrap().contains("query"));

    let v2_headers = "Authorization: Bearer local\r\nx-tdai-service-id: default\r\n";
    let add = r#"{"session_id":"v2-sess","messages":[{"role":"user","content":"Remember the linen map."}]}"#;
    let (status, body) = exchange(addr, "POST", "/v2/conversation/add", v2_headers, add);
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["code"], 0);
    assert_eq!(body["data"]["total_count"], 1);
    let accepted = body["data"]["accepted_ids"].as_array().unwrap();
    assert_eq!(accepted.len(), 1);

    let (status, body) = exchange(
        addr,
        "POST",
        "/v2/atomic/search",
        v2_headers,
        r#"{"query":"linen","limit":5}"#,
    );
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["code"], 0);
    let items = body["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0]["content"].as_str().unwrap().contains("linen"));
    assert_zagreb(items[0]["created_at"].as_str().unwrap());
    assert!(!items[0].to_string().contains('Z'));

    let (status, body) = exchange(
        addr,
        "POST",
        "/v2/conversation/search",
        v2_headers,
        r#"{"query":"linen","session_id":"v2-sess"}"#,
    );
    assert_eq!(status, 200, "{body}");
    let messages = body["data"]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_zagreb(messages[0]["timestamp"].as_str().unwrap());

    let (status, denied) = exchange(
        addr,
        "POST",
        "/v2/atomic/search",
        "",
        r#"{"query":"linen"}"#,
    );
    assert_eq!(status, 401, "{denied}");
    assert_eq!(denied["code"], 401);

    drop(server);
    let closed = TcpStream::connect_timeout(&addr, Duration::from_millis(200));
    assert!(closed.is_err(), "listener still open on {addr}");
}

#[test]
fn refuses_public_and_live_port() {
    assert!(parse_bind("127.0.0.1:0").is_ok());
    assert!(parse_bind("127.0.0.1:8439").is_ok());
    assert!(parse_bind("0.0.0.0:9").is_err());
    assert!(parse_bind("127.0.0.1:8420").is_err());
    let err = parse_bind("192.168.1.9:9").unwrap_err();
    assert!(err.to_string().contains("127.0.0.1"), "{err}");
}

fn exchange(
    addr: SocketAddr,
    method: &str,
    path: &str,
    extra_headers: &str,
    body: &str,
) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let text = String::from_utf8(buf).unwrap();
    let (head, raw_body) = text.split_once("\r\n\r\n").unwrap_or(("", &text));
    let status: u16 = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    let value: Value = serde_json::from_str(raw_body).unwrap_or(Value::Null);
    (status, value)
}
