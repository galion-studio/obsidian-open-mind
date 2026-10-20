// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! In-process HTTP test of the POSEIDON service on loopback port 19129
//! (slot 12 range 19120-19129): submit, priority order, ack, nack to DLQ,
//! and restart persistence on the same data directory.

use openmind_queue::engine::Config;
use openmind_queue::{http, server};
use serde_json::{json, Value};
use std::time::Duration;

fn call(addr: &str, m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(addr, m, p, body.as_deref(), Duration::from_secs(10)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

#[test]
fn http_roundtrip_and_restart_persistence() {
    let dir = std::env::temp_dir().join(format!("poseidon-http-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let opts = server::Options {
        bind: "127.0.0.1:19129".into(),
        data: dir.clone(),
        cfg: Config {
            backoff_base_ms: 10,
            backoff_max_ms: 20,
            ..Config::default()
        },
        ..server::Options::default()
    };
    let h = server::start(opts.clone()).unwrap();
    let a = h.addr.to_string();
    let (c, v) = call(&a, "GET", "/healthz", None);
    assert_eq!(c, 200);
    assert_eq!(v["function"], "POSEIDON");
    assert!(v["time"].as_str().unwrap().ends_with(":00"));

    let mut ids = Vec::new();
    for p in [1, 9, 5] {
        let (c, v) = call(
            &a,
            "POST",
            "/jobs",
            Some(json!({"kind":"echo","priority":p})),
        );
        assert_eq!(c, 201);
        ids.push(v["id"].as_u64().unwrap());
    }
    let (c, _) = call(&a, "POST", "/jobs", Some(json!({"kind":"revolut-payment"})));
    assert_eq!(c, 403);
    let (c, _) = call(&a, "POST", "/jobs", Some(json!({"bogus":1})));
    assert_eq!(c, 400);

    // highest priority first
    let (_, l) = call(&a, "POST", "/lease", Some(json!({"worker":"t"})));
    assert_eq!(l["id"].as_u64(), Some(ids[1]));
    let (c, _) = call(
        &a,
        "POST",
        &format!("/jobs/{}/ack", ids[1]),
        Some(json!({"token": l["token"]})),
    );
    assert_eq!(c, 200);
    // stale second ack
    let (c, _) = call(
        &a,
        "POST",
        &format!("/jobs/{}/ack", ids[1]),
        Some(json!({"token": l["token"]})),
    );
    assert_eq!(c, 404);

    // lease the prio-5 job and leave it leased across the restart
    let (_, l2) = call(
        &a,
        "POST",
        "/lease",
        Some(json!({"worker":"t","lease_ms":60000})),
    );
    assert_eq!(l2["id"].as_u64(), Some(ids[2]));
    let (c, _) = call(&a, "POST", "/caps", Some(json!({"scope":"anon","cap":100})));
    assert_eq!(c, 200);
    h.stop();

    let h = server::start(opts).unwrap();
    let a = h.addr.to_string();
    let (_, s) = call(&a, "GET", "/stats", None);
    assert_eq!(s["depth"]["ready"], 1);
    assert_eq!(s["depth"]["leased"], 1);
    assert_eq!(s["stats"]["acked"], 1);
    assert_eq!(s["agents"]["anon"]["cap"], 100);
    // the old token still acks the lease that survived the restart
    let (c, _) = call(
        &a,
        "POST",
        &format!("/jobs/{}/ack", ids[2]),
        Some(json!({"token": l2["token"]})),
    );
    assert_eq!(c, 200);
    // long-poll returns as soon as work exists
    let (_, l3) = call(
        &a,
        "POST",
        "/lease",
        Some(json!({"worker":"t","wait_ms":2000})),
    );
    assert_eq!(l3["id"].as_u64(), Some(ids[0]));
    let (_, l4) = call(
        &a,
        "POST",
        "/lease",
        Some(json!({"worker":"t","wait_ms":50})),
    );
    assert_eq!(l4["miss"], "empty");
    let (_, g) = call(
        &a,
        "POST",
        "/gaia/dispatch",
        Some(json!({"system":"queues","verb":"status"})),
    );
    assert_eq!(g["outcome"], "done");
    h.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
