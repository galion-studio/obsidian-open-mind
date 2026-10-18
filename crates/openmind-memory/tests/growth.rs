// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Growth features: batch capture, keyset pagination, search offsets,
//! bounded caches with exact invalidation, reader pool, /stats.

use openmind_memory::store::{L0Record, Store};
use openmind_memory::{api::App, config::Config, engine::Engine, http::Request};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const KEY: &str = "test-key-growth";
const AUTH: (&str, &str) = ("authorization", "Bearer test-key-growth");

fn tmpdir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("growth-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(dir: &Path, extra: &[(&str, &str)]) -> App {
    let d = dir.to_string_lossy().to_string();
    let extra: Vec<(String, String)> = extra
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let cfg = Config::from_env(&move |k| match k {
        "TDAI_DATA_DIR" => Some(d.clone()),
        "TDAI_INSTANCE_ID" => Some("galion".into()),
        "TDAI_GATEWAY_API_KEY" => Some(KEY.into()),
        _ => extra.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone()),
    });
    App::new(Engine::new(cfg))
}

fn call(app: &App, method: &str, path: &str, body: Value) -> (u16, Value) {
    let req = Request {
        method: method.into(),
        path: path.into(),
        headers: vec![(AUTH.0.into(), AUTH.1.into())],
        body: if body.is_null() {
            Vec::new()
        } else {
            body.to_string().into_bytes()
        },
    };
    let r = app.handle(req);
    (
        r.status,
        serde_json::from_str(&r.body).unwrap_or(Value::Null),
    )
}

fn cap(session: &str, u: &str, a: &str) -> Value {
    json!({"user_content": u, "assistant_content": a, "session_key": session})
}

#[test]
fn batch_capture_one_transaction_and_per_item_errors() {
    let dir = tmpdir("batch");
    let a = app(&dir, &[]);
    let (s, b) = call(
        &a,
        "POST",
        "/capture/batch",
        json!({"items": [
            cap("s1", "the walrus plan", "noted the walrus plan"),
            {"user_content": "missing session"},
            cap("s2", "otter logistics", "otter logistics ok"),
        ]}),
    );
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["count"], 3);
    assert_eq!(b["accepted"], 2);
    assert_eq!(b["l0_recorded"], 4);
    assert_eq!(b["results"][1]["status"], 400);
    assert!(b["results"][1]["error"]
        .as_str()
        .unwrap()
        .contains("Missing required"));
    let (_, f) = call(
        &a,
        "POST",
        "/search/conversations",
        json!({"query": "walrus"}),
    );
    assert_eq!(f["total"], 2, "{f}");
    // JSONL shard got all four lines in one append.
    let shard = std::fs::read_dir(dir.join("conversations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(std::fs::read_to_string(shard).unwrap().lines().count(), 4);
    // Missing items / too many items.
    assert_eq!(call(&a, "POST", "/capture/batch", json!({})).0, 400);
    let a2 = app(&dir, &[("OPENMIND_MEMORY_BATCH_MAX", "1")]);
    let (s, _) = call(
        &a2,
        "POST",
        "/capture/batch",
        json!({"items": [cap("x","a b","c d"), cap("y","e f","g h")]}),
    );
    assert_eq!(s, 413);
}

#[test]
fn keyset_pagination_covers_every_row_once() {
    let dir = tmpdir("page");
    let a = app(&dir, &[]);
    let items: Vec<Value> = (0..7)
        .map(|i| {
            json!({"user_content": format!("question {i}"), "assistant_content": format!("answer {i}"),
                   "session_key": "pg", "messages": [
                       {"role": "user", "content": format!("question number {i}"), "timestamp": 4_000_000_000_000_i64 + i},
                       {"role": "assistant", "content": format!("answer number {i}"), "timestamp": 4_000_000_000_000_i64 + i},
                   ]})
        })
        .collect();
    // Future timestamps (the per-session cursor skips older ones); user and
    // assistant share a timestamp, so ties are broken by record_id.
    let items: Vec<Value> = items
        .into_iter()
        .enumerate()
        .map(|(i, mut v)| {
            v["session_key"] = json!(format!("pg{i}"));
            v
        })
        .collect();
    let (s, b) = call(&a, "POST", "/capture/batch", json!({ "items": items }));
    assert_eq!(s, 200);
    let recorded = b["l0_recorded"].as_u64().unwrap();
    assert!(recorded > 0, "{b}");
    let mut seen = std::collections::HashSet::new();
    let mut cursor = Value::Null;
    let mut pages = 0;
    let mut last_ts = i64::MAX;
    loop {
        let (s, p) = call(
            &a,
            "POST",
            "/conversations/list",
            json!({"limit": 3, "cursor": cursor}),
        );
        assert_eq!(s, 200, "{p}");
        for it in p["items"].as_array().unwrap() {
            assert!(seen.insert(it["record_id"].as_str().unwrap().to_string()));
            let ts = it["timestamp"].as_i64().unwrap();
            assert!(ts <= last_ts, "newest first");
            last_ts = ts;
        }
        pages += 1;
        if !p["has_more"].as_bool().unwrap() {
            assert!(p["next_cursor"].is_null());
            break;
        }
        cursor = p["next_cursor"].clone();
    }
    assert_eq!(seen.len() as u64, recorded);
    assert!(pages >= 2);
    // Session filter.
    let (_, p) = call(
        &a,
        "POST",
        "/conversations/list",
        json!({"session_key": "pg3"}),
    );
    assert!(p["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i["session_key"] == "pg3"));
    // Bad cursor.
    assert_eq!(
        call(&a, "POST", "/conversations/list", json!({"cursor": "zz"})).0,
        400
    );
    // L1 listing.
    {
        let st = Store::open(&dir.join("instances/galion/vectors.db"), false).unwrap();
        for i in 0..5 {
            st.insert_l1(
                &format!("r{i}"),
                &format!("fact {i}"),
                "persona",
                50,
                "",
                "s1",
                "",
                "{}",
            )
            .unwrap();
        }
    }
    let (_, p1) = call(&a, "POST", "/memories/list", json!({"limit": 2}));
    assert_eq!(p1["count"], 2);
    let (_, p2) = call(
        &a,
        "POST",
        "/memories/list",
        json!({"limit": 10, "cursor": p1["next_cursor"]}),
    );
    assert_eq!(p2["count"], 3);
    assert_eq!(p2["has_more"], false);
}

#[test]
fn indexes_are_used_for_listing() {
    let dir = tmpdir("idx");
    let st = Store::open(&dir.join("v.db"), false).unwrap();
    let plan = st.explain(
        "SELECT record_id FROM l0_conversations WHERE session_key = 'a' ORDER BY timestamp DESC, record_id DESC LIMIT 5",
    );
    assert!(
        plan.iter().any(|l| l.contains("idx_l0_session_ts")),
        "{plan:?}"
    );
    let plan = st.explain(
        "SELECT record_id FROM l0_conversations ORDER BY timestamp DESC, record_id DESC LIMIT 5",
    );
    assert!(plan.iter().any(|l| l.contains("idx_l0_ts_id")), "{plan:?}");
    assert!(!plan.iter().any(|l| l.contains("TEMP B-TREE")), "{plan:?}");
}

#[test]
fn reupsert_keeps_one_fts_row() {
    let dir = tmpdir("reup");
    let st = Store::open(&dir.join("v.db"), false).unwrap();
    let r = L0Record {
        id: "l0_fixed".into(),
        session_key: "s".into(),
        session_id: "s".into(),
        role: "user".into(),
        message_text: "first text".into(),
        recorded_at: "2026-10-18T20:00:00.000Z".into(),
        timestamp: 1,
    };
    st.upsert_l0(&r).unwrap();
    let mut r2 = r.clone();
    r2.message_text = "second text".into();
    st.upsert_l0(&r2).unwrap();
    assert_eq!(st.count("l0_fts"), 1);
    assert_eq!(st.count("l0_conversations"), 1);
    assert!(st.l0_has_rows());
    assert_eq!(st.search_l0_fts("\"second\"", 5).len(), 1);
    assert_eq!(st.search_l0_fts("\"first\"", 5).len(), 0);
}

#[test]
fn recall_cache_hits_and_invalidates_on_write_and_persona_change() {
    let dir = tmpdir("cache");
    std::fs::write(dir.join("persona.md"), "# Persona\n\nlikes tea\n").unwrap();
    let a = app(&dir, &[]);
    let q = json!({"query": "zebra", "session_key": "c1"});
    let (_, r1) = call(&a, "POST", "/recall", q.clone());
    let (_, r2) = call(&a, "POST", "/recall", q.clone());
    assert_eq!(r1, r2);
    let (_, st) = call(&a, "GET", "/stats", Value::Null);
    assert_eq!(st["engine"]["recall_cache_hits"], 1, "{st}");
    assert!(!r1["context"].as_str().unwrap().contains("zebra"));
    // A write bumps the generation: the next recall sees the new L0 row.
    call(
        &a,
        "POST",
        "/capture",
        cap("c1", "the zebra crossing", "zebra noted"),
    );
    let (_, r3) = call(&a, "POST", "/recall", q.clone());
    assert!(r3["context"].as_str().unwrap().contains("zebra"), "{r3}");
    // Persona change (size differs) is picked up without restart.
    std::fs::write(
        dir.join("persona.md"),
        "# Persona\n\nlikes coffee very much\n",
    )
    .unwrap();
    let (_, r4) = call(&a, "POST", "/recall", q.clone());
    assert!(r4["context"].as_str().unwrap().contains("coffee"), "{r4}");
    // Cache off still answers the same.
    let off = app(
        &dir,
        &[
            ("OPENMIND_MEMORY_RECALL_CACHE", "0"),
            ("OPENMIND_MEMORY_READERS", "0"),
        ],
    );
    let (_, r5) = call(&off, "POST", "/recall", q);
    assert_eq!(r5, r4);
}

#[test]
fn search_offset_only_when_asked() {
    let dir = tmpdir("off");
    let a = app(&dir, &[]);
    call(&a, "POST", "/capture", cap("o1", "kiwi one", "kiwi two"));
    let (_, plain) = call(
        &a,
        "POST",
        "/search/conversations",
        json!({"query": "kiwi", "limit": 1}),
    );
    assert!(plain.get("offset").is_none() && plain.get("next_offset").is_none());
    assert_eq!(plain["total"], 1);
    let (_, p0) = call(
        &a,
        "POST",
        "/search/conversations",
        json!({"query": "kiwi", "limit": 1, "offset": 0}),
    );
    assert_eq!(p0["offset"], 0);
    assert_eq!(p0["next_offset"], 1);
    let (_, p1) = call(
        &a,
        "POST",
        "/search/conversations",
        json!({"query": "kiwi", "limit": 1, "offset": 1}),
    );
    assert_eq!(p1["total"], 1);
    assert!(p1["next_offset"].is_null());
    assert_ne!(p0["results"], p1["results"]);
}

#[test]
fn stats_needs_auth_and_reports_pool() {
    let dir = tmpdir("stats");
    let a = app(&dir, &[("OPENMIND_MEMORY_READERS", "2")]);
    let r = a.handle(Request {
        method: "GET".into(),
        path: "/stats".into(),
        headers: vec![],
        body: vec![],
    });
    assert_eq!(r.status, 401);
    let (s, st) = call(&a, "GET", "/stats", Value::Null);
    assert_eq!(s, 200);
    assert_eq!(st["readers"], 2);
    assert_eq!(st["rows"]["l0_conversations"], 0);
    // /health keeps its keys.
    let (_, h) = call(&a, "GET", "/health", Value::Null);
    assert_eq!(h["implementation"], "openmind-memory");
}

#[test]
fn concurrent_reads_and_writes() {
    let dir = tmpdir("conc");
    let a = Arc::new(app(&dir, &[]));
    let hs: Vec<_> = (0..8)
        .map(|t| {
            let a = a.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    if t % 2 == 0 {
                        let (s, _) = call(
                            &a,
                            "POST",
                            "/capture",
                            cap(&format!("t{t}"), &format!("note {i} about lynx"), "ok lynx"),
                        );
                        assert_eq!(s, 200);
                    } else {
                        let (s, _) = call(
                            &a,
                            "POST",
                            "/recall",
                            json!({"query": "lynx", "session_key": format!("t{t}")}),
                        );
                        assert_eq!(s, 200);
                        let (s, _) = call(
                            &a,
                            "POST",
                            "/search/conversations",
                            json!({"query": "lynx"}),
                        );
                        assert_eq!(s, 200);
                    }
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    let (_, st) = call(&a, "GET", "/stats", Value::Null);
    assert!(st["rows"]["l0_conversations"].as_i64().unwrap() > 0);
}
