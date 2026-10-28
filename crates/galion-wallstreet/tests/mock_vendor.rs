// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Uses the shared galion-bench harness: a MockServer on 127.0.0.1:0 plays a
//! delayed-quote vendor and a live broker that refuses everything. Proves
//! (1) the recorded vendor quotes agree with our ingested feed, and (2) a full
//! scripted Hermes ticket never touches the "live broker" (journal stays clean),
//! and (3) our scoreboard maths agrees with galion_bench::Scorecard.

use galion_bench::{client, load_jsonl, MockServer, Scorecard};
use galion_wallstreet::broker::PaperBroker;
use galion_wallstreet::money::parse_cents;
use galion_wallstreet::{score, ticket};

fn mock() -> MockServer {
    let set = load_jsonl(format!(
        "{}/fixtures/vendor-mock.jsonl",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    MockServer::start("127.0.0.1:0", set).unwrap()
}

#[test]
fn recorded_vendor_quotes_match_ingested_feed() {
    let m = mock();
    let b = PaperBroker::fixture();
    for sym in ["GLXA", "OMND"] {
        let r = client::get(m.addr(), &format!("/v1/quotes/{sym}")).unwrap();
        assert_eq!(r.status, 200);
        let v = r.json().unwrap();
        let ours = b.feed.quote(sym).unwrap();
        assert_eq!(
            parse_cents(v["last"].as_str().unwrap()).unwrap(),
            ours.price_cents,
            "{sym}"
        );
        assert_eq!(v["as_of"], ours.as_of.as_str());
        assert_eq!(v["delayed"], true);
    }
    assert!(m.unmatched().is_empty());
}

#[test]
fn scripted_ticket_never_routes_to_a_live_broker() {
    let m = mock();
    // A live order attempt *would* be refused and journaled:
    let r = client::post_json(
        m.addr(),
        "/v2/orders",
        &serde_json::json!({"symbol":"GLXA","qty":1}),
    )
    .unwrap();
    assert_eq!(r.status, 403);
    assert_eq!(m.count("live-order-refused"), 1);
    m.reset();
    let run = ticket::run_fresh(&ticket::parse(ticket::SCRIPTED_TICKET).unwrap());
    assert_eq!(run.status, "completed");
    assert!(!run.report.fills.is_empty());
    assert!(
        m.hits().is_empty(),
        "paper broker contacted the mock broker: {:?}",
        m.hits()
    );
}

#[test]
fn scoreboard_agrees_with_shared_scorecard() {
    let s = score::run();
    let mut card = Scorecard::new();
    for a in &s.axes {
        card.ratio(a.axis, a.passed as u32, a.total as u32);
    }
    assert_eq!(card.pct() as u32, s.percent);
    assert!(card.failing().is_empty());
    let md = card.to_markdown("B2 Wall Street", &galion_bench::now_zagreb());
    assert!(md.contains("B2 Wall Street: 100%"));
}
