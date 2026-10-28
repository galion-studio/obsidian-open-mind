// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The supplier fixture a Hermes adapter can be pointed at (galion-bench
//! MockServer on an ephemeral loopback port) must agree with the sim's
//! catalog, and must refuse anything that looks like a payment.

use galion_bench::{client, load_jsonl, MockServer};
use galion_vending::world::{CATALOG, DELIVERY_FEE};
use serde_json::json;

const FIX: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/vending-supplier.jsonl"
);

#[test]
fn supplier_fixture_matches_catalog_and_refuses_payments() {
    let mock = MockServer::start("127.0.0.1:0", load_jsonl(FIX).unwrap()).unwrap();
    let cat = client::get(mock.addr(), "/supplier/catalog")
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(cat["delivery_fee_cents"], DELIVERY_FEE);
    let items = cat["items"].as_array().unwrap();
    assert_eq!(items.len(), CATALOG.len());
    for (it, p) in items.iter().zip(CATALOG) {
        assert_eq!(it["sku"], p.sku);
        assert_eq!(it["unit_cost_cents"], p.unit_cost);
        assert_eq!(it["capacity"], p.capacity);
    }
    let order = json!({"lines": [{"sku": "cola", "qty": 3}]});
    // first order: supplier busy (recovery path), then accepted
    assert_eq!(
        client::post_json(mock.addr(), "/supplier/orders", &order)
            .unwrap()
            .status,
        503
    );
    let ok = client::post_json(mock.addr(), "/supplier/orders", &order).unwrap();
    assert_eq!(ok.status, 201);
    assert_eq!(ok.json().unwrap()["simulated"], true);
    let pay = client::post_json(mock.addr(), "/supplier/pay/now", &json!({"cents": 100})).unwrap();
    assert_eq!(pay.status, 403);
    assert_eq!(mock.count("pay-refused"), 1);
    assert!(mock.unmatched().is_empty());
}
