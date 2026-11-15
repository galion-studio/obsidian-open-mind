// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! CPU timings for Hermes request handling and the MCP client parser on main.
//! No sockets and no child process. `cargo bench -p hermes-rs --bench cpu`.

use galion_common::http::{parse_head, Limits, Request as CommonRequest};
use hermes_rs::hdr_value;
use hermes_rs::http::{full_bytes, Request, Response};
use hermes_rs::mcp::{parse_line, prefixed_name, sanitize, tool_result_text};
use serde_json::json;
use std::hint::black_box;
use std::net::SocketAddr;
use std::time::Instant;

fn measure(name: &str, iters: u64, mut body: impl FnMut()) {
    for _ in 0..32 {
        body();
    }
    let mut samples = [0u128; 5];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..iters {
            body();
        }
        *sample = start.elapsed().as_nanos() / u128::from(iters);
    }
    samples.sort_unstable();
    println!(
        "BENCH {name} iters={iters} median_ns={} min_ns={} samples_ns={samples:?}",
        samples[2], samples[0]
    );
}

#[test]
fn cpu_hot_paths() {
    let limits = Limits {
        max_body_bytes: 8 * 1024 * 1024,
        max_head_bytes: 64 * 1024,
        max_headers: 256,
        accept_chunked: true,
        ..Limits::default()
    };
    let raw = b"POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8642\r\nContent-Type: application/json\r\nAuthorization: Bearer local\r\nContent-Length: 2\r\nConnection: close\r\n\r\n";
    let head = parse_head(raw, &limits).expect("head");
    assert_eq!(head.method, "POST");
    measure("request_parse_head", 80_000, || {
        black_box(parse_head(black_box(raw), &limits).expect("head"));
    });

    let peer = SocketAddr::from(([127, 0, 0, 1], 9));
    let body = b"{}".to_vec();
    let view = Request::from_common(CommonRequest {
        head: head.clone(),
        raw_head: raw.to_vec(),
        body: body.clone(),
        peer: Some(peer),
    });
    assert_eq!(view.header("authorization"), Some("Bearer local"));
    assert_eq!(view.path, "/v1/chat/completions");
    assert!(view.json().is_ok());
    measure("request_from_common_header_json", 40_000, || {
        let parsed = parse_head(raw, &limits).expect("head");
        let view = Request::from_common(CommonRequest {
            head: parsed,
            raw_head: raw.to_vec(),
            body: body.clone(),
            peer: Some(peer),
        });
        black_box(view.header("authorization"));
        black_box(view.header("content-type"));
        black_box(view.json().expect("json"));
    });

    let reply = Response::json(
        200,
        &json!({"id":"chatcmpl_local","object":"chat.completion"}),
    );
    let bytes = full_bytes(&reply).expect("bytes");
    assert!(bytes.windows(15).any(|w| w == b"HTTP/1.1 200 OK"));
    measure("request_full_bytes", 40_000, || {
        black_box(full_bytes(black_box(&reply)).expect("bytes"));
    });

    assert_eq!(hdr_value("session key", 200), "session key");
    measure("request_hdr_value", 100_000, || {
        black_box(hdr_value(black_box("session key"), 200));
    });
    assert_eq!(hdr_value("session_key_local", 200), "session_key_local");
    measure("request_hdr_value_plain", 100_000, || {
        black_box(hdr_value(black_box("session_key_local"), 200));
    });
    let messy = "a\n  b\tc";
    assert_eq!(hdr_value(messy, 200), "a b c");
    measure("request_hdr_value_ws", 100_000, || {
        black_box(hdr_value(black_box(messy), 200));
    });

    assert_eq!(sanitize("stack.status"), "stack_status");
    measure("mcp_sanitize", 100_000, || {
        black_box(sanitize(black_box("stack.status")));
    });
    assert_eq!(
        prefixed_name("galion-stack", "stack.status"),
        "mcp__galion_stack__stack_status"
    );
    measure("mcp_prefixed_name", 100_000, || {
        black_box(prefixed_name(
            black_box("galion-stack"),
            black_box("stack.status"),
        ));
    });
    let long_server = "s".repeat(40);
    let long_tool = "t".repeat(40);
    assert_eq!(prefixed_name(&long_server, &long_tool).len(), 64);
    measure("mcp_prefixed_name_long", 40_000, || {
        black_box(prefixed_name(
            black_box(&long_server),
            black_box(&long_tool),
        ));
    });

    let list_line = r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"stack.status","description":"Read stack status","inputSchema":{"type":"object","properties":{"verbose":{"type":"boolean"}}}},{"name":"notes.search","description":"Search the vault","inputSchema":{"type":"object","properties":{"q":{"type":"string"}}}}],"nextCursor":"page-2"}}"#;
    let list = parse_line(list_line).expect("list");
    assert_eq!(list["result"]["tools"][0]["name"], "stack.status");
    measure("mcp_parse_tools_list", 20_000, || {
        black_box(parse_line(black_box(list_line)).expect("list"));
    });

    let call_line = r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"nerve up"},{"type":"text","text":"cortex dry-run"},{"type":"image","mimeType":"image/png"}],"isError":false}}"#;
    let call = parse_line(call_line).expect("call");
    assert!(tool_result_text(&call["result"]).starts_with("nerve up\n"));
    measure("mcp_parse_tools_call", 40_000, || {
        black_box(parse_line(black_box(call_line)).expect("call"));
    });
    measure("mcp_tool_result_text", 40_000, || {
        black_box(tool_result_text(black_box(&call["result"])));
    });

    assert!(parse_line("   ").is_none());
    assert!(parse_line("not-json").is_none());
    measure("mcp_parse_skip", 100_000, || {
        black_box(parse_line(black_box("not-json")));
    });
}
