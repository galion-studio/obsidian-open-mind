// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! `/capture` import mode (swarm/memory-schema): opt-in, old timestamps kept,
//! idempotent dedupe on (session_key, timestamp, message_text). Default
//! behaviour (cursor drops old timestamps) is asserted unchanged.

use openmind_memory::{config::Config, engine::Engine};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("om-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn engine(dir: &Path, allow: bool) -> Engine {
    let dir = dir.to_string_lossy().to_string();
    Engine::new(Config::from_env(&move |k| match k {
        "OPENMIND_MEMORY_DATA_DIR" => Some(dir.clone()),
        "OPENMIND_MEMORY_ALLOW_IMPORT" if allow => Some("1".into()),
        _ => None,
    }))
}

/// A replayed session from 2026-10-01 (old timestamps), CJK + emoji text.
fn replay(import: bool) -> Value {
    let t0 = 1_759_312_800_000i64; // 2025-10-01T12:00:00+02:00, well in the past
    json!({
        "user_content": "x", "assistant_content": "y", "session_key": "replay-1",
        "import": import,
        "messages": [
            {"role": "user", "content": "\u{6211}\u{4eec}\u{660e}\u{5929}\u{53bb}\u{8428}\u{683c}\u{52d2}\u{5e03}\u{5417}？🚆", "timestamp": t0},
            {"role": "assistant", "content": "\u{597d}\u{7684}！Zagreb tomorrow 👍🏽 \u{5bb6}\u{65cf} 👨‍👩‍👧", "timestamp": t0 + 1},
            {"role": "user", "content": "no timestamp here, so import refuses it"},
        ]
    })
}

fn jsonl_lines(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("conversations"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| std::fs::read_to_string(e.path()).unwrap().lines().count())
                .sum()
        })
        .unwrap_or(0)
}

#[test]
fn default_drops_old_timestamps_and_ignores_import_flag() {
    let dir = tmpdir("default");
    let e = engine(&dir, false);
    let (s, v) = e.capture(&replay(true));
    // only the message without a timestamp (stamped "now") passes the cursor
    assert_eq!((s, v["l0_recorded"].clone()), (200, json!(1)));
    assert!(
        v.get("import").is_none(),
        "flag must be ignored when not allowed"
    );
}

#[test]
fn import_keeps_old_timestamps_and_is_idempotent() {
    let dir = tmpdir("import");
    let e = engine(&dir, true);
    let (s, v) = e.capture(&replay(true));
    assert_eq!(s, 200, "{v}");
    assert_eq!(
        v["import"],
        json!({"inserted": 2, "duplicates": 0, "skipped_no_timestamp": 1})
    );
    assert_eq!(v["l0_recorded"], 2);
    assert_eq!(jsonl_lines(&dir), 2);
    for _ in 0..3 {
        let (_, v) = e.capture(&replay(true));
        assert_eq!(
            v["import"],
            json!({"inserted": 0, "duplicates": 2, "skipped_no_timestamp": 1})
        );
    }
    assert_eq!(
        jsonl_lines(&dir),
        2,
        "duplicates must not reach the JSONL shard"
    );
    let store = e.store.as_ref().unwrap();
    assert_eq!(store.count("l0_conversations"), 2);
    assert_eq!(store.count("l0_fts"), 2);
    // round trip + FTS: CJK n-gram and Latin term
    let (_, r) = e.search_conversations(
        &json!({"query": "\u{8428}\u{683c}\u{52d2}\u{5e03}", "session_key": "replay-1"}),
    );
    assert_eq!(r["total"], 1);
    assert!(r["results"].as_str().unwrap().contains(
        "\u{6211}\u{4eec}\u{660e}\u{5929}\u{53bb}\u{8428}\u{683c}\u{52d2}\u{5e03}\u{5417}？🚆"
    ));
    let (_, r) = e.search_conversations(&json!({"query": "zagreb"}));
    assert!(r["results"]
        .as_str()
        .unwrap()
        .contains("\u{597d}\u{7684}！Zagreb tomorrow 👍🏽 \u{5bb6}\u{65cf} 👨‍👩‍👧"));

    // allowed server, but a normal request without the flag: default path
    let (_, v) = e.capture(&replay(false));
    assert_eq!(v["l0_recorded"], 1);
    assert!(v.get("import").is_none());
}
