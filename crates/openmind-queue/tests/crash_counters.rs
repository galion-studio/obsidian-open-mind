// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 206 (always-on, in-process, loopback 127.0.0.1:20760): every
//! refusal counter, spend total, cap and quota must come back unchanged
//! after a restart on the same data directory, both from the raw log and
//! after a compaction. The five global refusal counters live in slot 161's
//! `refusals.dat` sidecar. Per-agent `budget_blocked` is NOT persisted by
//! the sidecar (still process-local): it is left out of the compared view
//! and printed instead (open item D-206-2 in docs/real-tests/poseidon-crash-soak.md).

use openmind_queue::engine::Config;
use openmind_queue::{http, server};
use serde_json::{json, Value};
use std::time::Duration;

const ADDR: &str = "127.0.0.1:20760";

fn call(m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(ADDR, m, p, body.as_deref(), Duration::from_secs(10)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

/// The parts of /stats that must survive a restart.
fn durable_view() -> Value {
    let (c, s) = call("GET", "/stats", None);
    assert_eq!(c, 200);
    let mut agents = serde_json::Map::new();
    for (n, a) in s["agents"].as_object().unwrap() {
        agents.insert(
            n.clone(),
            json!({"cap": a["cap"], "spent": a["spent"], "quota": a["quota"],
                   "pending": a["pending"],
                   "inflight": a["inflight"], "reserved": a["reserved"]}),
        );
    }
    json!({"stats": s["stats"], "global": s["global"], "depth": s["depth"], "agents": agents})
}

#[test]
fn refusal_counters_budgets_caps_quotas_survive_restart_and_compaction() {
    let dir = std::env::temp_dir().join(format!("poseidon-206-counters-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let opts = server::Options {
        bind: ADDR.into(),
        data: dir.clone(),
        cfg: Config {
            max_depth: 6,
            ..Config::default()
        },
        ..server::Options::default()
    };
    let h = server::start(opts.clone()).unwrap();

    // quota: agent q may have 2 pending
    let q = json!({"agent":"q","max_pending":2,"max_inflight":1});
    assert_eq!(call("POST", "/quotas", Some(q)).0, 200);
    for _ in 0..2 {
        assert_eq!(call("POST", "/jobs", Some(json!({"agent":"q"}))).0, 201);
    }
    assert_eq!(call("POST", "/jobs", Some(json!({"agent":"q"}))).0, 429);
    assert_eq!(call("POST", "/jobs", Some(json!({"agent":"q"}))).0, 429);

    // budget: agent b cap 10; a cost-11 job is refused at the door
    let cap = json!({"scope":"b","cap":10});
    assert_eq!(call("POST", "/caps", Some(cap)).0, 200);
    let big = json!({"agent":"b","cost":11,"queue":"bq"});
    assert_eq!(call("POST", "/jobs", Some(big)).0, 402);
    // two cost-6 jobs fit at the door, but only one can be reserved:
    // leasing the second is skipped (budget_blocked) while the first is out
    for _ in 0..2 {
        let j = json!({"agent":"b","cost":6,"queue":"bq"});
        assert_eq!(call("POST", "/jobs", Some(j)).0, 201);
    }
    let (_, l1) = call("POST", "/lease", Some(json!({"worker":"w","queue":"bq"})));
    assert!(l1["id"].is_u64());
    let (_, l2) = call("POST", "/lease", Some(json!({"worker":"w","queue":"bq"})));
    assert_eq!(l2["miss"], "budget_hard_stop");

    // stale ack: wrong token
    let path = format!("/jobs/{}/ack", l1["id"]);
    let bad = json!({"token": l1["token"].as_u64().unwrap() + 999});
    assert_eq!(call("POST", &path, Some(bad)).0, 409);
    // real ack, charged 6
    let good = json!({"token": l1["token"]});
    assert_eq!(call("POST", &path, Some(good)).0, 200);

    // backpressure: max_depth 6 live jobs (q:2, b:1 left) -> fill to 6, then 503
    for _ in 0..3 {
        assert_eq!(call("POST", "/jobs", Some(json!({}))).0, 201);
    }
    assert_eq!(call("POST", "/jobs", Some(json!({}))).0, 503);

    let blocked_b = call("GET", "/stats", None).1["agents"]["b"]["budget_blocked"].clone();
    let before = durable_view();
    let st = &before["stats"];
    assert_eq!(st["quota_refusals"], 2, "{before}");
    assert!(st["budget_refusals"].as_u64().unwrap() >= 2, "{before}");
    assert_eq!(st["stale_acks"], 1, "{before}");
    assert_eq!(st["backpressure_refusals"], 1, "{before}");
    assert!(blocked_b.as_u64().unwrap() >= 1);
    assert_eq!(before["agents"]["b"]["spent"], 6);
    h.stop();

    // 1. replay of the raw log
    let h = server::start(opts.clone()).unwrap();
    let (_, s2) = call("GET", "/stats", None);
    eprintln!(
        "budget_blocked[b] before restart {} after {} (process-local, D-206-2)",
        blocked_b, s2["agents"]["b"]["budget_blocked"]
    );
    assert_eq!(
        durable_view(),
        before,
        "raw-log replay changed durable state"
    );
    // 2. after a compaction
    let (c, _) = call("POST", "/admin/compact", None);
    assert_eq!(c, 200);
    h.stop();
    let h = server::start(opts.clone()).unwrap();
    assert_eq!(
        durable_view(),
        before,
        "compacted replay changed durable state"
    );
    // 3. counters keep counting from the restored values
    assert_eq!(call("POST", "/jobs", Some(json!({}))).0, 503); // still full
    let after = durable_view();
    assert_eq!(after["stats"]["backpressure_refusals"], 2);
    h.stop();
    let h = server::start(opts).unwrap();
    assert_eq!(durable_view(), after);
    h.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Slot 206: a compaction that fails after the change was synced must not
/// turn that change into an error (the client would retry and enqueue or
/// charge twice). Compaction is made to fail by putting a directory where
/// `queue.log.tmp` goes. Loopback 127.0.0.1:20769.
#[test]
fn failed_compaction_does_not_fail_a_durable_change() {
    const A2: &str = "127.0.0.1:20769";
    let c2 = |m: &str, p: &str, b: Option<Value>| -> (u16, Value) {
        let body = b.map(|v| v.to_string());
        let (c, s) = http::request(A2, m, p, body.as_deref(), Duration::from_secs(10)).unwrap();
        (c, serde_json::from_str(&s).unwrap_or(Value::Null))
    };
    let dir = std::env::temp_dir().join(format!("poseidon-206-compactfail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("queue.log.tmp")).unwrap();
    let opts = server::Options {
        bind: A2.into(),
        data: dir.clone(),
        compact_min: 4,
        ..server::Options::default()
    };
    let h = server::start(opts.clone()).unwrap();
    // enqueue + lease + ack cycles: once the log has more than 4x
    // (live + 16) lines every commit tries (and fails) to compact
    for _ in 0..40 {
        let (c, v) = c2("POST", "/jobs", Some(json!({})));
        assert_eq!(c, 201, "a durable enqueue was answered {c} {v}");
        let (_, l) = c2("POST", "/lease", Some(json!({"worker":"w"})));
        let (c, _) = c2(
            "POST",
            &format!("/jobs/{}/ack", l["id"]),
            Some(json!({"token": l["token"]})),
        );
        assert_eq!(c, 200);
    }
    let (_, st) = c2("GET", "/stats", None);
    assert!(
        st["store"]["compaction_errors"].as_u64().unwrap() > 0,
        "{st}"
    );
    assert_eq!(st["store"]["compactions"], 0);
    assert_eq!(st["stats"]["enqueued"], 40);
    // the explicit admin call still reports the failure
    assert_eq!(c2("POST", "/admin/compact", None).0, 500);
    h.stop();
    // everything answered is in the log
    std::fs::remove_dir(dir.join("queue.log.tmp")).unwrap();
    let h = server::start(opts).unwrap();
    let (_, st) = c2("GET", "/stats", None);
    assert_eq!(st["stats"]["enqueued"], 40);
    assert_eq!(st["stats"]["acked"], 40);
    // and compaction works again once the obstacle is gone
    assert_eq!(c2("POST", "/admin/compact", None).0, 200);
    h.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
