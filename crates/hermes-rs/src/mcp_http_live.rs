// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use super::{discover_with, EnvFn};
use crate::config::{McpServerCfg, ToolAllow};
use crate::tools::ToolOutput;
use galion_common::http::{self, Response};
use serde_json::{json, Value};
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

fn call(t: &Arc<dyn crate::tools::Tool>, args: Value) -> ToolOutput {
    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = ToolCtx {
        session_id: "s",
        spend_session: "s",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &|_| {},
    };
    t.call(&args, &ctx)
}

struct Hit {
    method: String,
    session: Option<String>,
    cursor: Option<String>,
    tool: Option<String>,
}

struct Mock {
    hits: Mutex<Vec<Hit>>,
    fail_calls: AtomicUsize,
}

struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    state: Arc<Mock>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(2));
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Server {
    fn fail_calls_one(&self) {
        self.state.fail_calls.store(1, Ordering::SeqCst);
    }
}

fn sse_result(id: u64, result: Value) -> Vec<u8> {
    let note = r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info"}}"#;
    let msg = json!({"jsonrpc":"2.0","id":id,"result":result});
    format!("event: message\ndata: {note}\n\nevent: message\ndata: {msg}\n\n").into_bytes()
}

fn mock_dispatch(req: &http::Request, state: &Mock) -> Response {
    if req.method() != "POST" || req.path() != "/mcp" {
        return Response::text(404, "not found");
    }
    if req.header("authorization") != Some("Bearer secret-token") {
        return Response::text(401, "unauthorized");
    }
    if req.header("accept").is_none()
        || req.header("content-type") != Some("application/json")
        || req.header("mcp-protocol-version").is_none()
    {
        return Response::text(400, "missing mcp headers");
    }
    let body: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => return Response::text(400, "bad json"),
    };
    let method = body["method"].as_str().unwrap_or("").to_string();
    state.hits.lock().expect("hits").push(Hit {
        method: method.clone(),
        session: req.header("mcp-session-id").map(str::to_string),
        cursor: body["params"]["cursor"].as_str().map(str::to_string),
        tool: body["params"]["name"].as_str().map(str::to_string),
    });
    if method == "initialize" {
        let id = body["id"].as_u64().unwrap_or(0);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mock", "version": "1"}
            }
        });
        return Response::json(200, payload.to_string()).with_header("Mcp-Session-Id", "sess-1");
    }
    if req.header("mcp-session-id") != Some("sess-1") {
        return Response::text(400, "missing session");
    }
    if method == "notifications/initialized" {
        return Response::new(202, "application/json", Vec::<u8>::new());
    }
    let id = body["id"].as_u64().unwrap_or(0);
    if method == "tools/list" {
        if body["params"]["cursor"].as_str() == Some("p2") {
            let result = json!({
                "tools": [{
                    "name": "search_code",
                    "description": "Search code",
                    "inputSchema": {"type": "object", "properties": {}}
                }]
            });
            return Response::new(200, "text/event-stream", sse_result(id, result))
                .with_header("Mcp-Session-Id", "sess-1");
        }
        let result = json!({
            "tools": [
                {
                    "name": "get_file_contents",
                    "description": "Get a file",
                    "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}}
                },
                {
                    "name": "push_files",
                    "description": "Write files",
                    "inputSchema": {"type": "object", "properties": {}}
                }
            ],
            "nextCursor": "p2"
        });
        let payload = json!({"jsonrpc":"2.0","id":id,"result":result});
        return Response::json(200, payload.to_string()).with_header("Mcp-Session-Id", "sess-1");
    }
    if method == "tools/call" {
        if state.fail_calls.load(Ordering::SeqCst) > 0 {
            state.fail_calls.fetch_sub(1, Ordering::SeqCst);
            return Response::json(404, br#"{"error":"session gone"}"#.to_vec())
                .with_header("Mcp-Session-Id", "sess-1");
        }
        let name = body["params"]["name"].as_str().unwrap_or("");
        let leaked = concat!("readme ", "gh", "p_abcdefghijklmno");
        let text = if name == "get_file_contents" {
            leaked
        } else {
            "unexpected tool"
        };
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {"content": [{"type": "text", "text": text}], "isError": false}
        });
        return Response::json(200, payload.to_string()).with_header("Mcp-Session-Id", "sess-1");
    }
    Response::text(400, "unknown method")
}

fn serve_mock() -> Server {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let addr = listener.local_addr().expect("addr");
    let stop = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mock {
        hits: Mutex::new(Vec::new()),
        fail_calls: AtomicUsize::new(0),
    });
    let stop_t = stop.clone();
    let state_t = state.clone();
    let join = std::thread::spawn(move || {
        let limits = http::Limits {
            head_timeout: Duration::from_secs(5),
            body_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            ..http::Limits::default()
        };
        while !stop_t.load(Ordering::SeqCst) {
            let Ok((mut sock, _)) = listener.accept() else {
                break;
            };
            if stop_t.load(Ordering::SeqCst) {
                break;
            }
            match http::read_request(&mut sock, &limits) {
                Ok(Some(req)) => {
                    let resp = mock_dispatch(&req, &state_t);
                    let _ = sock.write_all(&resp.to_bytes(false));
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = sock.write_all(&http::error_reply(e));
                }
            }
        }
    });
    Server {
        addr,
        stop,
        join: Some(join),
        state,
    }
}

#[test]
fn streamable_http_allowlist_session_sse_and_reconnect() {
    let server = serve_mock();
    let url = format!("http://{}/mcp", server.addr);
    let env: EnvFn = Arc::new(|k| match k {
        "HERMES_RS_TEST_MCP_TOKEN" => Some("secret-token".into()),
        _ => None,
    });
    let cfg = McpServerCfg {
        timeout_s: 5,
        connect_timeout_s: 5,
        allow: ToolAllow::Only(vec!["get_file_contents".into(), "search_code".into()]),
        token_env: Some("HERMES_RS_TEST_MCP_TOKEN".into()),
        ..McpServerCfg::http("mock", url)
    };
    let (tools, errors) = discover_with(&[cfg], env);
    assert!(errors.is_empty(), "{errors:?}");
    let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(
        names,
        ["mcp__mock__get_file_contents", "mcp__mock__search_code"]
    );
    assert!(tools.iter().all(|t| !t.name().contains("push_files")));
    let o = call(&tools[0], json!({"path": "README.md"}));
    assert!(!o.is_error, "{o:?}");
    assert!(o.content.contains("readme"), "{o:?}");
    assert!(o.content.contains("[REDACTED]"), "{o:?}");
    assert!(!o.content.contains(concat!("gh", "p_")), "{o:?}");
    server.fail_calls_one();
    let o = call(&tools[0], json!({"path": "README.md"}));
    assert!(!o.is_error, "reconnected after HTTP 404: {o:?}");
    let hits = server.state.hits.lock().expect("hits");
    let methods: Vec<&str> = hits.iter().map(|h| h.method.as_str()).collect();
    assert!(methods.iter().filter(|m| **m == "initialize").count() >= 2);
    assert!(hits.iter().any(|h| h.method == "initialize" && h.session.is_none()));
    assert!(hits
        .iter()
        .any(|h| h.method != "initialize" && h.session.as_deref() == Some("sess-1")));
    assert!(hits.iter().any(|h| h.method == "tools/list" && h.cursor.as_deref() == Some("p2")));
    assert!(hits.iter().all(|h| h.tool.as_deref() != Some("push_files")));
}
