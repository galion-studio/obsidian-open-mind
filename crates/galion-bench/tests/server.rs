// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! In-process mock server tests (always run; ephemeral loopback port).

use galion_bench::client::{get, post_json, request};
use galion_bench::{load_jsonl, MockServer};
use serde_json::json;

fn bank() -> MockServer {
    let set = load_jsonl(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/example-bank.jsonl"
    ))
    .unwrap();
    MockServer::start("127.0.0.1:0", set).unwrap()
}

#[test]
fn scripted_bank_session() {
    let mut s = bank();
    let a = s.addr();
    let auth = [("Authorization", "Bearer mock-token")];

    let r = request(a, "GET", "/api/1.0/accounts", &auth, "").unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.header("x-bench-fixture"), Some("accounts"));
    let v = r.json().unwrap();
    let stamp = v[0]["updated_at"].as_str().unwrap();
    zagreb_ok(stamp);

    assert_eq!(get(a, "/api/1.0/accounts").unwrap().status, 401);

    // flaky FX quote: 503 once, then 200
    assert_eq!(get(a, "/api/1.0/rate?from=EUR&to=USD").unwrap().status, 503);
    let r = get(a, "/api/1.0/rate?to=USD&from=EUR").unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.json().unwrap()["rate"], 1.08);
    assert_eq!(get(a, "/api/1.0/rate?from=EUR&to=GBP").unwrap().status, 404);

    // `after`: card shows pending draft only once the draft was posted
    assert_eq!(
        get(a, "/api/1.0/cards/card-mock-1")
            .unwrap()
            .json()
            .unwrap()["pending_draft"],
        json!(null)
    );
    let r = post_json(
        a,
        "/api/1.0/cards/card-mock-1/freeze-draft",
        &json!({"reason":"lost","note":"x"}),
    )
    .unwrap();
    assert_eq!(r.status, 202);
    assert_eq!(
        get(a, "/api/1.0/cards/card-mock-1")
            .unwrap()
            .json()
            .unwrap()["pending_draft"],
        "freeze"
    );

    // money move refused + journaled
    let r = post_json(a, "/api/1.0/pay", &json!({"amount": 10})).unwrap();
    assert_eq!(r.status, 403);
    assert_eq!(s.count("pay-refused"), 1);

    let hits = s.hits();
    assert_eq!(hits.len(), 9);
    assert_eq!(s.unmatched().len(), 1);
    for h in &hits {
        zagreb_ok(&h.at);
    }
    assert_eq!(hits.last().unwrap().body, r#"{"amount":10}"#);

    // admin journal over HTTP mirrors the in-process view
    let j = get(a, "/__bench/journal").unwrap().json().unwrap();
    assert_eq!(j["hits"].as_array().unwrap().len(), 9);
    assert_eq!(
        get(a, "/__bench/health").unwrap().json().unwrap()["fixtures"],
        9
    );
    assert_eq!(
        post_json(a, "/__bench/reset", &json!({})).unwrap().status,
        200
    );
    assert!(s.hits().is_empty());
    assert_eq!(
        get(a, "/api/1.0/rate?from=EUR&to=USD").unwrap().status,
        503,
        "reset restores `times`"
    );

    s.shutdown();
    assert!(
        get(a, "/__bench/health").is_err(),
        "port must be closed after shutdown"
    );
}

#[test]
fn malformed_request_gets_400() {
    use std::io::{Read, Write};
    let s = bank();
    let mut c = std::net::TcpStream::connect(s.addr()).unwrap();
    c.write_all(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
        .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 400"), "{out}");
}

#[test]
fn refuses_non_loopback_bind() {
    let e = MockServer::start("0.0.0.0:0", Default::default())
        .err()
        .unwrap();
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn all_example_fixtures_load() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "jsonl") {
            load_jsonl(&p).unwrap_or_else(|e| panic!("{e}"));
            n += 1;
        }
    }
    assert!(n >= 3);
}

fn zagreb_ok(s: &str) {
    assert!(
        zagreb_time::require_rfc3339(s).is_ok(),
        "not a Zagreb stamp: {s}"
    );
}
