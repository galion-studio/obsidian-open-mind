// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test: run the built `openmind-compact` binary on a real box log
//! (default /workspace/swarm/CALL-RECORD.md, or $COMPACT_REAL_LOG), feed it
//! as a JSONL transcript, and check the output. Gated: GALION_REAL=1.
//! No network, no ports, nothing live.

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn gated() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn log_path() -> String {
    std::env::var("COMPACT_REAL_LOG").unwrap_or_else(|_| "/workspace/swarm/CALL-RECORD.md".into())
}

#[test]
fn real_cli_compacts_box_log_within_budget() {
    if !gated() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let raw = std::fs::read_to_string(log_path()).expect("real log readable");
    let mut jsonl = String::new();
    jsonl.push_str(
        &json!({"role":"system","content":"You are the Gaia swarm record keeper."}).to_string(),
    );
    jsonl.push('\n');
    let entries: Vec<&str> = raw.split("\n- 20").collect();
    for (i, e) in entries.iter().enumerate() {
        let id = format!("call_{i:05}");
        for v in [
            json!({"role":"user","content":"next part"}),
            json!({"role":"assistant","content":"reading","tool_calls":[{"id": id}]}),
            json!({"role":"tool","content": e, "tool_call_id": id}),
        ] {
            jsonl.push_str(&v.to_string());
            jsonl.push('\n');
        }
    }
    jsonl.push_str(
        &json!({"role":"user","content":"Which branch did the context-compact work push?"})
            .to_string(),
    );
    jsonl.push('\n');
    let in_tokens = jsonl.len() / 4;
    let budget = (in_tokens / 10).max(2000);
    let mut child = Command::new(env!("CARGO_BIN_EXE_openmind-compact"))
        .args(["compact", "--budget", &budget.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(jsonl.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    let stats: Value = serde_json::from_slice(&out.stderr).expect("stats JSON on stderr");
    eprintln!("stats: {stats}");
    let lines: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).expect("valid JSONL"))
        .collect();
    assert!(!lines.is_empty());
    assert_eq!(lines[0]["role"], "system");
    assert_eq!(lines.last().unwrap()["role"], "user");
    assert!(stats["output_tokens"].as_u64().unwrap() <= budget as u64);
    assert!(
        stats["elapsed_us"].as_u64().unwrap() < 2_000_000,
        "well under 2 s"
    );
    // every tool result still has its call
    let calls: Vec<String> = lines
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c["id"].as_str().map(str::to_string))
        .collect();
    for m in &lines {
        if let Some(id) = m["tool_call_id"].as_str() {
            assert!(calls.iter().any(|c| c == id), "orphan tool result {id}");
        }
    }
}

#[test]
fn real_cli_bench_reports_retention() {
    if !gated() {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let out = Command::new(env!("CARGO_BIN_EXE_openmind-compact"))
        .args(["bench", "--reps", "3", "--real", &log_path()])
        .output()
        .expect("run bench");
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("## real box logs"));
    assert!(s.contains("| synth 100k s2 /10 |"));
    eprintln!("{s}");
}
