// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Integration: hermes-rs's own MCP client spawns the real
//! `openmind-mcp-stack` binary over stdio, discovers its tools and calls
//! them against a temp vault and a mock memory gateway.

mod common;

use common::{make_vault, mock_memory, snapshot, TempDir};
use hermes_rs::config::McpServerCfg;
use hermes_rs::tools::{Event, ToolCtx};
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn cfg(args: Vec<String>) -> McpServerCfg {
    McpServerCfg {
        name: "openmind".into(),
        command: env!("CARGO_BIN_EXE_openmind-mcp-stack").into(),
        args,
        timeout_s: 10,
        connect_timeout_s: 10,
        lazy: false,
    }
}

#[test]
fn hermes_mcp_client_discovers_and_calls_tools() {
    let (v, _o) = make_vault();
    let m = mock_memory(Some("file-key-1"));
    let kd = TempDir::new("key");
    let key_file = kd.path().join("hermes.env");
    std::fs::write(
        &key_file,
        "OPENROUTER_API_KEY=x\nTDAI_GATEWAY_API_KEY=file-key-1\n",
    )
    .unwrap();
    let before = snapshot(v.path());
    let (tools, errors) = hermes_rs::mcp::discover(&[cfg(vec![
        "--vault".into(),
        v.path().display().to_string(),
        "--memory-url".into(),
        m.url.clone(),
        "--key-file".into(),
        key_file.display().to_string(),
    ])]);
    assert!(errors.is_empty(), "{errors:?}");
    let names: Vec<String> = tools.iter().map(|t| t.name()).collect();
    for want in [
        "mcp__openmind__memory_search",
        "mcp__openmind__memory_save",
        "mcp__openmind__vault_read",
        "mcp__openmind__vault_search",
    ] {
        assert!(
            names.contains(&want.to_string()),
            "{want} missing from {names:?}"
        );
    }
    let tool = |n: &str| tools.iter().find(|t| t.name() == n).unwrap().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let emit = |_e: Event| {};
    let ctx = ToolCtx {
        session_id: "s-test",
        spend_session: "s-test",
        depth: 0,
        user_text: "",
        cancel: &cancel,
        emit: &emit,
    };
    let out = tool("mcp__openmind__memory_save").call(&json!({"text":"round trip fact 42"}), &ctx);
    assert!(!out.is_error, "{}", out.content);
    let out = tool("mcp__openmind__memory_search").call(&json!({"query":"fact 42"}), &ctx);
    assert!(!out.is_error, "{}", out.content);
    assert!(
        out.content.contains("round trip fact 42"),
        "{}",
        out.content
    );
    let out = tool("mcp__openmind__vault_read").call(&json!({"path":"23-vault.md"}), &ctx);
    assert!(!out.is_error);
    assert!(out.content.contains("Galion memory vault"));
    let out = tool("mcp__openmind__vault_read").call(&json!({"path":"../x"}), &ctx);
    assert!(out.is_error);
    assert!(m
        .seen
        .lock()
        .unwrap()
        .iter()
        .all(|s| s.auth.as_deref() == Some("file-key-1")));
    assert_eq!(before, snapshot(v.path()));
}

#[test]
fn binary_refuses_non_loopback_memory_and_missing_vault() {
    let bin = env!("CARGO_BIN_EXE_openmind-mcp-stack");
    let st = std::process::Command::new(bin)
        .args(["--memory-url", "http://10.1.2.3:8420", "--check"])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(2));
    let st = std::process::Command::new(bin)
        .args(["--vault", "/no/such/vault", "--no-memory", "--check"])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(2));
    let st = std::process::Command::new(bin)
        .args(["--no-memory", "--no-stack", "--check"])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&st.stdout).trim(),
        "tools=openmind_status"
    );
    // Stack tools are on by default (loopback defaults); the write tool is not.
    let st = std::process::Command::new(bin)
        .args(["--no-memory", "--check"])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(0));
    let out = String::from_utf8_lossy(&st.stdout).to_string();
    for t in [
        "stack_health",
        "stack_alerts",
        "cortex_heals",
        "spend_status",
        "backup_status",
        "queue_status",
        "hermes_readiness",
    ] {
        assert!(out.contains(t), "{t} missing: {out}");
    }
    assert!(!out.contains("vault_note_create"), "{out}");
    assert!(
        !out.contains("one_threads"),
        "no Hermes key, no thread tools: {out}"
    );
    // A typo in the stack conf is refused (exit 2), not silently ignored.
    let kd = TempDir::new("conf");
    let conf = kd.path().join("stack.conf");
    std::fs::write(&conf, "vault_note_creat = on\n").unwrap();
    let st = std::process::Command::new(bin)
        .args([
            "--no-memory",
            "--stack-conf",
            conf.to_str().unwrap(),
            "--check",
        ])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(2));
    std::fs::write(&conf, "vault_note_create = maybe\n").unwrap();
    let st = std::process::Command::new(bin)
        .args([
            "--no-memory",
            "--stack-conf",
            conf.to_str().unwrap(),
            "--check",
        ])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(2));
    // Non-loopback stack URL refused.
    std::fs::write(&conf, "nerve_url = http://10.1.2.3:8670\n").unwrap();
    let st = std::process::Command::new(bin)
        .args([
            "--no-memory",
            "--stack-conf",
            conf.to_str().unwrap(),
            "--check",
        ])
        .output()
        .unwrap();
    assert_eq!(st.status.code(), Some(2));
}
