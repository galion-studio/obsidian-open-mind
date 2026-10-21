// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! In-process test of the refusal-counter sidecar on loopback port 20311
//! (wave-4 slot 161 range 20310-20319): quota and budget refusals, a
//! restart without compaction, a compaction, a deleted sidecar and a torn
//! sidecar. The real `kill -9` loop is `real_refusal_persist.rs`.

use openmind_queue::refusals::{Sidecar, SLOT};
use openmind_queue::{http, server};
use serde_json::{json, Value};
use std::time::Duration;

fn call(addr: &str, m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(addr, m, p, body.as_deref(), Duration::from_secs(10)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

fn counters(a: &str) -> (u64, u64, Value) {
    let (_, s) = call(a, "GET", "/stats", None);
    (
        s["stats"]["quota_refusals"].as_u64().unwrap(),
        s["stats"]["budget_refusals"].as_u64().unwrap(),
        s["refusals_sidecar"].clone(),
    )
}

/// One quota refusal (agent `q` limited to 1 pending) and one budget
/// refusal (agent `b` capped at 5, job costs 9) per call.
fn refuse_once(a: &str) {
    let (c, _) = call(a, "POST", "/jobs", Some(json!({"agent":"q","kind":"echo"})));
    assert_eq!(c, 429, "quota refusal");
    let (c, _) = call(
        a,
        "POST",
        "/jobs",
        Some(json!({"agent":"b","kind":"echo","cost":9})),
    );
    assert_eq!(c, 402, "budget refusal");
}

#[test]
fn refusal_counters_survive_restart_compaction_and_damage() {
    let dir = std::env::temp_dir().join(format!("poseidon-refusals-http-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let opts = server::Options {
        bind: "127.0.0.1:20311".into(),
        data: dir.clone(),
        ..server::Options::default()
    };

    // --- run 1: set limits, refuse 3 + 3
    let h = server::start(opts.clone()).unwrap();
    let a = h.addr.to_string();
    let (_, _, sc) = counters(&a);
    assert_eq!(sc["fresh"], true);
    let (c, _) = call(
        &a,
        "POST",
        "/quotas",
        Some(json!({"agent":"q","max_pending":1,"max_inflight":1})),
    );
    assert_eq!(c, 200);
    let (c, _) = call(&a, "POST", "/caps", Some(json!({"scope":"b","cap":5})));
    assert_eq!(c, 200);
    let (c, _) = call(
        &a,
        "POST",
        "/jobs",
        Some(json!({"agent":"q","kind":"echo"})),
    );
    assert_eq!(c, 201);
    for _ in 0..3 {
        refuse_once(&a);
    }
    assert_eq!(counters(&a).0, 3);
    assert_eq!(counters(&a).1, 3);
    h.stop(); // no compaction, no sidecar sync: same files a kill -9 leaves

    // --- run 2: counters come back from the sidecar alone
    let h = server::start(opts.clone()).unwrap();
    let a = h.addr.to_string();
    let (q, b, sc) = counters(&a);
    assert_eq!((q, b), (3, 3));
    assert_eq!(sc["restored"], true);
    assert_eq!(sc["bad_slots"], 0);
    refuse_once(&a);
    let (c, v) = call(&a, "POST", "/admin/compact", None);
    assert_eq!(c, 200, "{v}");
    refuse_once(&a);
    assert_eq!((counters(&a).0, counters(&a).1), (5, 5));
    h.stop();

    // --- run 3: delete the sidecar; the compaction's Counters record
    // still holds 4 + 4 (older, but not zero)
    std::fs::remove_file(Sidecar::path(&dir)).unwrap();
    let h = server::start(opts.clone()).unwrap();
    let a = h.addr.to_string();
    let (q, b, sc) = counters(&a);
    assert_eq!((q, b), (4, 4));
    assert_eq!(sc["fresh"], true);
    refuse_once(&a);
    h.stop();

    // --- run 4: tear the newest slot; the other slot (one step older)
    // wins, and the log's Counters (4 + 4) never pulls it backwards
    let p = Sidecar::path(&dir);
    let mut bytes = std::fs::read(&p).unwrap();
    let (_, rep) = Sidecar::open(&dir).unwrap();
    let newest = (rep.seq % 2) as usize * SLOT;
    for x in &mut bytes[newest + 30..newest + SLOT] {
        *x = 0;
    }
    std::fs::write(&p, &bytes).unwrap();
    let h = server::start(opts).unwrap();
    let a = h.addr.to_string();
    let (q, b, sc) = counters(&a);
    assert_eq!(sc["bad_slots"], 1);
    // the previous save held 5 quota + 4 budget (one call is two saves)
    assert_eq!((q, b), (5, 4));
    // and the damaged slot is rewritten on the next change
    refuse_once(&a);
    h.stop();
    let (_, rep) = Sidecar::open(&dir).unwrap();
    assert_eq!(rep.bad_slots, 0);
    let c = rep.restored.unwrap();
    assert_eq!((c.quota_refusals, c.budget_refusals), (6, 5));
    let _ = std::fs::remove_dir_all(&dir);
}
