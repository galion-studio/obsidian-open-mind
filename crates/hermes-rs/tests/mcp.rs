// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use hermes_rs::config::McpServerCfg;
use hermes_rs::tools::{ToolCtx, ToolOutput};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn call(t: &Arc<dyn hermes_rs::tools::Tool>, args: serde_json::Value) -> ToolOutput {
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

#[test]
fn stdio_server_discovery_paging_call_and_reconnect() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-mcp.sh");
    let cfg = McpServerCfg {
        name: "galion-stack".into(),
        command: "bash".into(),
        args: vec![script.into()],
        timeout_s: 5,
        connect_timeout_s: 5,
        lazy: true,
        // keep the runner's uid: a sandbox "root" in a user namespace cannot
        // switch to the default 65534 (not mapped there)
        run_as: Some("root".into()),
        ..McpServerCfg::default()
    };
    let broken = McpServerCfg {
        name: "gone".into(),
        command: "/nonexistent/mcp".into(),
        ..cfg.clone()
    };
    let (tools, errors) = hermes_rs::mcp::discover(&[cfg, broken]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(
        names,
        [
            "mcp__galion_stack__stack_status",
            "mcp__galion_stack__crash"
        ]
    );
    assert_eq!(
        tools[0].parameters()["properties"]["unit"]["type"],
        "string"
    );
    assert_eq!(tools[1].parameters()["type"], "object");
    let o = call(&tools[0], json!({"unit": "hermes.service"}));
    assert!(
        !o.is_error && o.content.ends_with("\nhermes.service active"),
        "{o:?}"
    );
    assert!(o
        .content
        .starts_with("[MCP server galion-stack output: untrusted"));
    let o = call(&tools[1], json!({}));
    assert!(o.is_error, "server exit is a tool error: {o:?}");
    let o = call(&tools[0], json!({"unit": "console.service"}));
    assert!(
        o.content.ends_with("console.service active"),
        "reconnected after the crash: {o:?}"
    );
}
