// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Golden tests: recorded live shapes (no data) + source-derived shapes,
//! synthetic data only. Calls App::handle directly (no sockets).

use openmind_memory::{api::App, config::Config, engine::Engine, http::Request, store::Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn tmpdir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("golden-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app(dir: &Path, key: Option<&str>, extra: &[(&str, &str)]) -> App {
    let d = dir.to_string_lossy().to_string();
    let key = key.map(String::from);
    let extra: Vec<(String, String)> = extra
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let cfg = Config::from_env(&move |k| match k {
        "TDAI_DATA_DIR" => Some(d.clone()),
        "TDAI_INSTANCE_ID" => Some("galion".into()),
        "TDAI_GATEWAY_API_KEY" => key.clone(),
        _ => extra.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone()),
    });
    App::new(Engine::new(cfg))
}

fn call(
    app: &App,
    method: &str,
    path: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> (u16, Value) {
    let req = Request {
        method: method.into(),
        path: path.into(),
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: if body.is_null() {
            Vec::new()
        } else {
            body.to_string().into_bytes()
        },
    };
    let r = app.handle(req);
    (
        r.status,
        if r.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&r.body).unwrap()
        },
    )
}

fn recorded() -> Value {
    serde_json::from_str(include_str!("fixtures/recorded.json")).unwrap()
}

/// Same keys (recursively for objects), ignoring values.
fn same_shape(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => x.keys().all(|k| {
            y.get(k)
                .is_some_and(|v| same_shape(&x[k], v) || x[k].is_object() && v.is_null())
        }),
        _ => true,
    }
}

const AUTH: (&str, &str) = ("authorization", "Bearer s3cret");
const SVC: (&str, &str) = ("x-tdai-service-id", "galion");

#[test]
fn recorded_health_and_auth_shapes() {
    let dir = tmpdir("auth");
    let a = app(&dir, Some("s3cret"), &[]);
    let rec = recorded();
    let (s, h) = call(&a, "GET", "/health", Value::Null, &[]);
    assert_eq!(s, 200);
    assert!(same_shape(&rec["health"], &h), "{h}");
    assert_eq!(h["stores"]["embeddingService"], json!(false));

    let (s, b) = call(&a, "POST", "/recall", json!({}), &[]);
    assert_eq!((s, &b), (401, &rec["unauth_v1"]["body"]));
    let (s, b) = call(&a, "GET", "/nope", Value::Null, &[]);
    assert_eq!((s, &b), (401, &rec["unauth_v1"]["body"]));
    let (s, b) = call(&a, "POST", "/v2/offload/query-mmd", json!({}), &[]);
    assert_eq!(s, 401);
    assert_eq!(b["message"], rec["unauth_v2"]["body"]["message"]);
    assert!(
        b["request_id"].as_str().unwrap().starts_with("req-")
            && b["request_id"].as_str().unwrap().len() == 20
    );
    let (s, _) = call(&a, "OPTIONS", "/recall", Value::Null, &[]);
    assert_eq!(s, 204);
    let (s, b) = call(
        &a,
        "POST",
        "/recall",
        json!({}),
        &[("authorization", "Bearer wrong")],
    );
    assert_eq!(
        (s, b["error"].as_str().unwrap()),
        (401, "Unauthorized: invalid token")
    );
    let (s, b) = call(&a, "GET", "/nope", Value::Null, &[AUTH]);
    assert_eq!((s, b), (404, json!({"error": "Not found: GET /nope"})));
}

#[test]
fn capture_then_recall_and_search() {
    let dir = tmpdir("flow");
    let a = app(&dir, None, &[]);
    let (s, b) = call(&a, "POST", "/capture", json!({"user_content": "x"}), &[]);
    assert_eq!(
        (s, b),
        (
            400,
            json!({"error": "Missing required fields: user_content, assistant_content, session_key"})
        )
    );
    let (s, b) = call(
        &a,
        "POST",
        "/capture",
        json!({"user_content": "I like hiking near Velebit", "assistant_content": "Noted: hiking.\n```sh\nrm -rf /\n```", "session_key": "s1", "user_id": "u"}),
        &[],
    );
    assert_eq!(
        (s, b),
        (200, json!({"l0_recorded": 2, "scheduler_notified": false}))
    );
    // JSONL shard written in the gateway's flat format.
    let shard = std::fs::read_dir(dir.join("conversations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let first: Value = serde_json::from_str(
        std::fs::read_to_string(shard)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    let keys: Vec<&str> = first
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "sessionKey",
            "sessionId",
            "recordedAt",
            "id",
            "role",
            "content",
            "timestamp"
        ]
    );

    // Recall: L0 fallback keeps only texts containing the exact query.
    let (s, b) = call(
        &a,
        "POST",
        "/recall",
        json!({"query": "hiking near Velebit", "session_key": "s1"}),
        &[],
    );
    assert_eq!(s, 200);
    assert_eq!(
        b,
        json!({"context": "I like hiking near Velebit", "memory_count": 0, "code": 0, "message": "ok", "retryable": false})
    );
    let (s, b) = call(&a, "POST", "/recall", json!({"query": "x"}), &[]);
    assert_eq!(
        (s, b),
        (
            400,
            json!({"error": "Missing required fields: query, session_key"})
        )
    );

    let (s, b) = call(
        &a,
        "POST",
        "/search/conversations",
        json!({"query": "hiking", "limit": 5}),
        &[],
    );
    assert_eq!(s, 200);
    assert_eq!(b["total"], json!(2));
    let text = b["results"].as_str().unwrap();
    assert!(
        text.starts_with("Found 2 matching message(s):\n\n---\n**["),
        "{text}"
    );
    assert!(
        !text.contains("rm -rf"),
        "assistant code blocks are stripped"
    );
    let (_, b) = call(
        &a,
        "POST",
        "/search/conversations",
        json!({"query": "hiking", "session_key": "other"}),
        &[],
    );
    assert_eq!(
        b,
        json!({"results": "No matching conversation messages found.", "total": 0})
    );

    let (s, b) = call(
        &a,
        "POST",
        "/session/end",
        json!({"session_key": "s1"}),
        &[],
    );
    assert_eq!((s, b), (200, json!({"flushed": true})));
}

#[test]
fn l1_keyword_recall_persona_scene() {
    let dir = tmpdir("l1");
    {
        let st = Store::open(&dir.join("instances/galion/vectors.db"), false).unwrap();
        st.insert_l1(
            "r1",
            "User prefers espresso in the morning",
            "persona",
            80,
            "Coffee",
            "s1",
            "2026-10-01T08:30:00.000Z", // repo-check: allow (legacy UTC row fixture, wire parity with 8420)
            "{}",
        )
        .unwrap();
        st.insert_l1(
            "r2",
            "User plans a trip to Split",
            "episodic",
            50,
            "",
            "s1",
            "",
            r#"{"activity_start_time":"2026-11-01","activity_end_time":"2026-11-05"}"#,
        )
        .unwrap();
    }
    std::fs::write(dir.join("persona.md"), "# Persona\n- id: p1\n  likes coffee\n  session: s1\n- id: p2\n  other person\n  session: s2\n---\n## 🗺️ Scene Navigation (Scene Index)\nold nav").unwrap();
    std::fs::create_dir_all(dir.join(".metadata")).unwrap();
    std::fs::write(dir.join(".metadata/scene_index.json"), r#"[{"filename":"scene-s1.md","summary":"Coffee habits","heat":120,"created":"","updated":"2026-10-02"},{"filename":"scene-s2.md","summary":"x","heat":1}]"#).unwrap();
    let a = app(&dir, None, &[]);

    let (s, b) = call(
        &a,
        "POST",
        "/recall",
        json!({"query": "espresso", "session_key": "s1"}),
        &[],
    );
    assert_eq!(s, 200);
    assert_eq!(b["memory_count"], json!(1));
    assert_eq!(b["strategy"], json!("hybrid"));
    let ctx = b["context"].as_str().unwrap();
    let expected_nav = format!(
        "<scene-navigation>\n---\n## 🗺️ Scene Navigation (Scene Index)\n*\u{4EE5}\u{4E0B}\u{662F}\u{5F53}\u{524D}\u{573A}\u{666F}\u{8BB0}\u{5FC6}\u{7684}\u{7D22}\u{5F15}，\u{53EF}\u{6839}\u{636E}\u{9700}\u{8981} read \u{8BFB}\u{53D6}\u{8BE6}\u{7EC6}\u{5185}\u{5BB9}\u{3002}*\n\n### Path: {}\n**\u{70ED}\u{5EA6}**: 120 🔥🔥 | **\u{66F4}\u{65B0}**: 2026-10-02\nSummary: Coffee habits",
        dir.join("scene_blocks").join("scene-s1.md").display()
    );
    assert!(ctx.starts_with("<user-persona>\n# Persona\n\n- id: p1\n  likes coffee\n  session: s1\n</user-persona>\n\n"), "{ctx}");
    assert!(ctx.contains(&expected_nav), "{ctx}");
    assert!(
        !ctx.contains("other person") && !ctx.contains("espresso"),
        "parity: L1 lines are not in context"
    );

    // Opt-in: L1 lines included.
    let a2 = app(&dir, None, &[("OPENMIND_MEMORY_RECALL_INCLUDE_L1", "1")]);
    let (_, b) = call(
        &a2,
        "POST",
        "/recall",
        json!({"query": "espresso", "session_key": "s1"}),
        &[],
    );
    assert!(b["context"].as_str().unwrap().contains(
        "- [persona|Coffee] User prefers espresso in the morning (\u{6D3B}\u{52A8}\u{65F6}\u{95F4}: 2026-10-01 08:30)"
    ));

    // Unknown session: no persona/nav, but L1 hit -> strategy present, empty context.
    let (_, b) = call(
        &a,
        "POST",
        "/recall",
        json!({"query": "espresso", "session_key": "zz"}),
        &[],
    );
    assert_eq!(
        b,
        json!({"context": "", "strategy": "hybrid", "memory_count": 1, "code": 0, "message": "ok", "retryable": false})
    );
    // Nothing at all -> no strategy key (JS undefined).
    let (_, b) = call(
        &a,
        "POST",
        "/recall",
        json!({"query": "zebra", "session_key": "zz"}),
        &[],
    );
    assert_eq!(
        b,
        json!({"context": "", "memory_count": 0, "code": 0, "message": "ok", "retryable": false})
    );

    let (s, b) = call(
        &a,
        "POST",
        "/search/memories",
        json!({"query": "trip Split", "limit": 3}),
        &[],
    );
    assert_eq!(s, 200);
    assert_eq!(b["total"], json!(1));
    assert_eq!(b["strategy"], json!("fts"));
    let t = b["results"].as_str().unwrap();
    assert!(
        t.starts_with("Found 1 matching memories:\n\n- **[episodic]** (priority: 50) (score: "),
        "{t}"
    );
    assert!(t.ends_with(")\n  User plans a trip to Split\n"), "{t:?}");
    let (_, b) = call(
        &a,
        "POST",
        "/search/memories",
        json!({"query": "espresso", "type": "episodic"}),
        &[],
    );
    assert_eq!(
        b,
        json!({"results": "No matching memories found.", "total": 0, "strategy": "fts"})
    );
    let (_, b) = call(
        &a,
        "POST",
        "/search/memories",
        json!({"query": "espresso", "scene": "coff"}),
        &[],
    );
    assert_eq!(b["total"], json!(1));
}

#[test]
fn offload_ingest_mmd_read_ref() {
    let dir = tmpdir("offload");
    let a = app(&dir, Some("s3cret"), &[]);
    let (s, b) = call(
        &a,
        "POST",
        "/v2/offload/query-mmd",
        json!({"session_id": "abc"}),
        &[AUTH],
    );
    assert_eq!(
        (s, b["message"].as_str().unwrap()),
        (401, "Missing x-tdai-service-id header")
    );

    let (s, b) = call(
        &a,
        "POST",
        "/v2/offload/query-mmd",
        json!({"session_id": "abc"}),
        &[AUTH, SVC],
    );
    assert_eq!(s, 200);
    assert_eq!(b["data"], json!({"mmds": [], "currentMmd": null}));
    assert_eq!(
        (b["code"].clone(), b["message"].clone()),
        (json!(0), json!("ok"))
    );

    let pairs = json!([
        {"tool_name": "terminal", "tool_call_id": "toolu_02B", "params": {"cmd": "ls"}, "result": "a\nb", "timestamp": "2026-10-18T19:00:02Z"}, // repo-check: allow (legacy UTC tool-call fixture, wire parity with 8420)
        {"tool_name": "read \"file\"", "tool_call_id": "toolu_01A", "params": {"p": 1}, "result": {"ok": true}, "error": "warn", "timestamp": "2026-10-18T19:00:01Z", "duration_ms": 5} // repo-check: allow (legacy UTC tool-call fixture, wire parity with 8420)
    ]);
    let (s, b) = call(
        &a,
        "POST",
        "/v2/offload/ingest",
        json!({"session_id": "sess:1", "tool_pairs": pairs}),
        &[AUTH, SVC],
    );
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["data"], json!({"node_ids": ["ntoolu02B", "ntoolu01A"]}));
    let base = dir.join("offload/sess_1");
    assert_eq!(
        std::fs::read_to_string(base.join("canvas.mmd")).unwrap(),
        "%% each node drills to refs/<nodeId>.md\nflowchart TD\n  ntoolu01A[\"read 'file'\"]\n  ntoolu02B[\"terminal\"]\n  ntoolu01A --> ntoolu02B\n"
    );
    assert_eq!(
        std::fs::read_to_string(base.join("refs/ntoolu01A.md")).unwrap(),
        "# node_id: ntoolu01A\ntool: read \"file\"\ntool_call_id: toolu_01A\n```json\n{\"p\":1}\n```\n## result\n{\"ok\":true}\n## error\nwarn\n"
    );
    let pending = std::fs::read_to_string(base.join("pending.jsonl")).unwrap();
    assert!(pending.lines().nth(1).unwrap().starts_with(
        "{\"toolName\":\"read \\\"file\\\"\",\"toolCallId\":\"toolu_01A\",\"params\""
    ));

    // query-mmd falls back to canvas.mmd; version = md5[..12].
    let (_, b) = call(
        &a,
        "POST",
        "/v2/offload/query-mmd",
        json!({"session_id": "sess:1"}),
        &[AUTH, SVC],
    );
    let m = &b["data"]["mmds"][0];
    assert_eq!(m["filename"], json!("canvas.mmd"));
    assert_eq!(m["version"].as_str().unwrap().len(), 12);

    // read-ref as the plugin calls it (full=true).
    let rr = "offload/sess_1/refs/ntoolu02B.md";
    let (s, b) = call(
        &a,
        "POST",
        "/v2/offload/read-ref",
        json!({"session_id": "sess:1", "result_ref": rr, "full": true}),
        &[AUTH, SVC],
    );
    assert_eq!(s, 200);
    assert_eq!(b["data"]["result_ref"], json!(rr));
    assert_eq!(b["data"]["truncated"], json!(false));
    assert!(b["data"]["content"]
        .as_str()
        .unwrap()
        .ends_with("## result\na\nb\n"));
    let (s, _) = call(
        &a,
        "POST",
        "/v2/offload/read-ref",
        json!({"session_id": "sess:1", "result_ref": "offload/sess_1/refs/../../x.md"}),
        &[AUTH, SVC],
    );
    assert_eq!(s, 404);
    let (s, _) = call(
        &a,
        "POST",
        "/v2/offload/read-ref",
        json!({"session_id": "other", "result_ref": rr}),
        &[AUTH, SVC],
    );
    assert_eq!(s, 404);
    let (s, _) = call(
        &a,
        "POST",
        "/v2/offload/ingest",
        json!({"session_id": "bad/id", "prompt": "x"}),
        &[AUTH, SVC],
    );
    assert_eq!(s, 400);
    let (s, b) = call(
        &a,
        "POST",
        "/v2/offload/ingest",
        json!({"session_id": "s", "prompt": "hello"}),
        &[AUTH, SVC],
    );
    assert_eq!((s, b["data"].clone()), (200, json!({})));
    // Unported v2 routes answer 501 in the v2 envelope.
    let (s, b) = call(&a, "POST", "/v2/atomic/query", json!({}), &[AUTH, SVC]);
    assert_eq!((s, b["code"].clone()), (501, json!(501)));
}

#[test]
fn read_only_mode_writes_nothing() {
    let dir = tmpdir("ro");
    {
        let st = Store::open(&dir.join("instances/galion/vectors.db"), false).unwrap();
        st.insert_l1("r1", "alpha beta", "persona", 50, "", "s1", "", "{}")
            .unwrap();
    }
    let a = app(&dir, None, &[("OPENMIND_MEMORY_READ_ONLY", "1")]);
    let (s, b) = call(
        &a,
        "POST",
        "/capture",
        json!({"user_content": "u", "assistant_content": "a", "session_key": "s"}),
        &[],
    );
    assert_eq!((s, b["l0_recorded"].clone()), (200, json!(2)));
    assert!(!dir.join("conversations").exists());
    let (_, b) = call(
        &a,
        "POST",
        "/search/memories",
        json!({"query": "alpha"}),
        &[],
    );
    assert_eq!(b["total"], json!(1));
}
