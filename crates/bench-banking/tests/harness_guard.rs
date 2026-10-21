// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Cross-check on the shared galion-bench MockServer (swarm/bench-harness):
//! the draft paths run against static Revolut-shaped fixtures and every
//! money-moving fixture (`pay-refused`, ...) must answer zero times.

use bench_banking::{audit::Audit, client::BankClient, mock::REFRESH_TOKEN, tools::BankingTools};
use galion_bench::{client, load_jsonl, MockServer};
use serde_json::json;

const REFUSED: &[&str] = &[
    "pay-refused",
    "transfer-refused",
    "exchange-refused",
    "card-freeze-refused",
    "card-unfreeze-refused",
    "disputes-refused",
];

#[test]
fn drafts_never_hit_money_endpoints_on_shared_harness() {
    let set = load_jsonl(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/revolut-readonly.jsonl"
    ))
    .unwrap();
    let mock = MockServer::start("127.0.0.1:0", set).unwrap();
    let mut t = BankingTools::new(
        BankClient::new(&mock.base_url(), REFRESH_TOKEN).unwrap(),
        Audit::new(),
    );
    let calls = [
        ("bank_session", json!({})),
        ("bank_balance", json!({})),
        ("bank_statement", json!({})),
        ("payee_list", json!({})),
        (
            "payment_draft",
            json!({"from_account": "acc-eur-main", "payee_id": "cp-001", "amount": "50", "currency": "EUR", "reference": "Dinner"}),
        ),
        (
            "fx_quote",
            json!({"from": "EUR", "to": "USD", "amount": "100"}),
        ),
        (
            "card_freeze_draft",
            json!({"card_id": "card-1", "action": "freeze", "reason": "lost"}),
        ),
        (
            "dispute_draft",
            json!({"transaction_id": "tx-008", "reason": "not mine"}),
        ),
    ];
    for (tool, args) in &calls {
        let r = t.dispatch(tool, args);
        assert!(!r.is_error, "{tool}: {}", r.content);
    }
    assert_eq!(t.drafts.len(), 3);
    for id in REFUSED {
        assert_eq!(mock.count(id), 0, "{id} was hit");
    }
    assert!(
        mock.unmatched().is_empty(),
        "unscripted calls: {:?}",
        mock.unmatched()
    );

    // The guard is live: a direct (harness, not Hermes) /pay is refused and counted.
    let r = client::request(mock.addr(), "POST", "/api/1.0/pay", &[], "{}").unwrap();
    assert_eq!(r.status, 403);
    assert_eq!(mock.count("pay-refused"), 1);
}
