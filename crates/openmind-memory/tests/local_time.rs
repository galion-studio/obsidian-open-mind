// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! F-354-1 (slot 367): storage stays UTC (`recorded_at` = JS `toISOString()`,
//! 8420 wire parity, lexical `ORDER BY`/`>` compares), `/search/conversations`
//! renders it in Europe/Zagreb with an explicit offset, incl. the 2026-10-25
//! autumn DST switch where 02:30 local happens twice.

use openmind_memory::{config::Config, engine::Engine, store::L0Record};
use serde_json::json;
use std::path::{Path, PathBuf};

fn tmpdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("om-localtime-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn engine(dir: &Path) -> Engine {
    let dir = dir.to_string_lossy().to_string();
    Engine::new(Config::from_env(&move |k| match k {
        "OPENMIND_MEMORY_DATA_DIR" => Some(dir.clone()),
        _ => None,
    }))
}

fn rec(id: &str, text: &str, recorded_at: &str, ts: i64) -> L0Record {
    L0Record {
        id: id.into(),
        session_key: "dst".into(),
        session_id: "dst".into(),
        role: "user".into(),
        message_text: text.into(),
        recorded_at: recorded_at.into(),
        timestamp: ts,
    }
}

#[test]
fn search_conversations_renders_stored_utc_in_zagreb_across_dst() {
    let dir = tmpdir("dst");
    let e = engine(&dir);
    let store = e.store.as_ref().expect("store");
    let rows = [
        // first 02:30 (CEST) and second 02:30 (CET) on 2026-10-25
        ("a", "plitvice first pass", "2026-10-25T00:30:00.000Z", 1), // repo-check: allow (stored UTC row under test)
        ("b", "plitvice second pass", "2026-10-25T01:30:00.000Z", 2), // repo-check: allow (stored UTC row under test)
        // the F-354-1 sample from slot 354 (CET, after the switch)
        ("c", "plitvice winter", "2026-10-28T03:08:06.277Z", 3), // repo-check: allow (stored UTC row under test)
        // already-offset row and legacy junk are tolerated
        ("d", "plitvice offset", "2026-07-01T12:00:00+02:00", 4),
        ("e", "plitvice legacy", "last tuesday", 5),
    ];
    for (id, text, at, ts) in rows {
        store.upsert_l0(&rec(id, text, at, ts)).unwrap();
    }
    let (s, r) = e.search_conversations(&json!({"query": "plitvice", "limit": 10}));
    assert_eq!(s, 200);
    assert_eq!(r["total"], 5, "{r}");
    let text = r["results"].as_str().unwrap();
    for want in [
        "Session: dst [2026-10-25T02:30:00+02:00]",
        "Session: dst [2026-10-25T02:30:00+01:00]",
        "Session: dst [2026-10-28T04:08:06+01:00]",
        "Session: dst [2026-07-01T12:00:00+02:00]",
        "Session: dst [last tuesday]",
    ] {
        assert!(text.contains(want), "missing {want:?} in:\n{text}");
    }
    assert!(
        !text.contains("Z]"),
        "no Z stamps in search output:\n{text}"
    );
    assert!(!text.contains("+00:00"), "no UTC offset:\n{text}");

    // storage is unchanged UTC (display-only fix)
    let conn = rusqlite::Connection::open(dir.join("vectors.db")).unwrap();
    let v: String = conn
        .query_row(
            "SELECT recorded_at FROM l0_conversations WHERE record_id='c'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "2026-10-28T03:08:06.277Z"); // repo-check: allow (stored UTC row under test)
    let _ = std::fs::remove_dir_all(&dir);
}
