// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Strict `/health` JSON rule (gaia-fault-inject, slot 189): only a JSON
//! object with top-level `status: "ok"` or `ok: true` is healthy. The three
//! live VPS bodies (captured read-only 2026-10-21 13:24 +02:00) still pass.

use gaia_live::{json_ok, JsonVerdict};

#[test]
fn live_vps_bodies_still_pass() {
    // 8420 openmind-memory, 8642 hermes-rs, 8650 (`{"ok":true}`).
    let memory = r#"{"status":"ok","version":"0.1.0","uptime":13168,"stores":{"vectorStore":true,"embeddingService":false},"services":{"timerScanner":null,"pipelineWorker":null,"stateBackend":"none"},"implementation":"openmind-memory","implementation_version":"0.1.0","read_only":false,"fts":true}"#;
    let hermes = r#"{"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}"#;
    for b in [
        memory,
        hermes,
        r#"{"ok":true}"#,
        "  {\"status\" : \"ok\"}\n",
    ] {
        assert_eq!(json_ok(b), JsonVerdict::Ok, "{b}");
    }
}

#[test]
fn misleading_bodies_fail() {
    for (b, want) in [
        (r#"{"status":"ok""#, JsonVerdict::NotJson),
        (
            r#"{"status":"down","last":{"status":"ok"}}"#,
            JsonVerdict::NotOk,
        ),
        (
            r#"health: "status":"ok" (plain text)"#,
            JsonVerdict::NotJson,
        ),
        (
            r#"<html><body>"ok":true</body></html>"#,
            JsonVerdict::NotJson,
        ),
        (r#"{"status":"o k"}"#, JsonVerdict::NotOk),
        (r#"[{"status":"ok"}]"#, JsonVerdict::NotOk),
        ("", JsonVerdict::NotJson),
        (r#"{"ok":false,"checks":{"ok":true}}"#, JsonVerdict::NotOk),
        (r#"{"status":"OK"}"#, JsonVerdict::NotOk),
        (r#"{"ok":"true"}"#, JsonVerdict::NotOk),
        (r#"{"status":"ok"}{"status":"down"}"#, JsonVerdict::NotJson),
        (
            "f\r\n{\"status\":\"ok\"}\r\n0\r\n\r\n",
            JsonVerdict::NotJson,
        ),
    ] {
        assert_eq!(json_ok(b), want, "{b:?}");
    }
}

#[test]
fn verdict_text_keeps_status_not_ok_wording() {
    // `gaia status --live` notes and older tests match on "status not ok".
    assert!(JsonVerdict::NotJson.text().starts_with("status not ok"));
    assert_eq!(JsonVerdict::NotOk.text(), "status not ok");
    assert_eq!(JsonVerdict::Ok.text(), "status ok");
}

/// The rule before slot 189 (whitespace removed, then a substring search),
/// kept here only to pin what it got wrong.
fn old_rule(body: &str) -> bool {
    let squeezed: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.contains("\"status\":\"ok\"") || squeezed.contains("\"ok\":true")
}

#[test]
fn old_rule_failed_open() {
    // The 8 `gaia-dummy --mode badjson --bad KIND` bodies.
    let bodies = [
        r#"{"status":"ok""#,
        r#"{"status":"down","last":{"status":"ok"}}"#,
        r#"health: "status":"ok" (plain text)"#,
        r#"<html><body>"ok":true</body></html>"#,
        r#"{"status":"o k"}"#,
        r#"[{"status":"ok"}]"#,
        "",
        r#"{"ok":false,"checks":{"ok":true}}"#,
    ];
    let old_ok = bodies.iter().filter(|b| old_rule(b)).count();
    let new_ok = bodies
        .iter()
        .filter(|b| json_ok(b) == JsonVerdict::Ok)
        .count();
    assert_eq!((old_ok, new_ok), (7, 0));
}
