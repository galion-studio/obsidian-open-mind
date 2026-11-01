// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end Stripe subscription flow through the HTTP handler, with a
//! mocked Stripe API and webhooks signed like Stripe signs them. Covers both
//! invoice shapes (pre-2025 `subscription` and 2025 "basil" `parent`).

mod common;

use common::*;
use galion_accounts::pay::stripe::sign;
use serde_json::{json, Value};

const DAY: i64 = 86_400;

fn hook(t: &TestApp, body: &Value) -> galion_common::http::Response {
    let raw = body.to_string();
    let now = (t.app.clock)();
    let sig = format!("t={now},v1={}", sign(WHSEC, now, raw.as_bytes()));
    t.app.handle(&req(
        "POST",
        "/webhooks/stripe",
        &[
            ("Content-Type", "application/json"),
            ("Stripe-Signature", &sig),
        ],
        raw.as_bytes(),
    ))
}

fn new_account(t: &TestApp) -> (String, String) {
    let r = t.app.handle(&post_json("/api/account/create", None, "{}"));
    assert_eq!(r.status, 200);
    let number = json(&r)["number"].as_str().unwrap().to_string();
    (cookie_of(&r), number)
}

fn me(t: &TestApp, cookie: &str) -> Value {
    json(
        &t.app
            .handle(&req("GET", "/api/account/me", &[("Cookie", cookie)], b"")),
    )
}

fn checkout(t: &TestApp, cookie: &str, plan: &str) -> String {
    let r = t.app.handle(&post_json(
        "/api/pay/stripe/checkout",
        Some(cookie),
        &json!({ "plan": plan }).to_string(),
    ));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let url = json(&r)["url"].as_str().unwrap().to_string();
    url.split('#').nth(1).unwrap().to_string()
}

fn completed(id: &str, r: &str, cus: &str, sub: &str) -> Value {
    json!({"id": id, "object": "event", "type": "checkout.session.completed",
        "data": {"object": {"id": "cs_test_1", "object": "checkout.session", "mode": "subscription",
            "client_reference_id": r, "metadata": {"galion_ref": r},
            "customer": cus, "subscription": sub, "payment_status": "paid"}}})
}

/// 2025 "basil" invoice shape.
fn invoice_basil(id: &str, r: &str, cus: &str, sub: &str, end: i64, cents: i64) -> Value {
    json!({"id": id, "object": "event", "type": "invoice.paid",
        "data": {"object": {"id": "in_1", "object": "invoice", "customer": cus,
            "amount_paid": cents, "currency": "eur",
            "parent": {"type": "subscription_details",
                "subscription_details": {"subscription": sub, "metadata": {"galion_ref": r}}},
            "lines": {"data": [{"period": {"start": end - 30 * DAY, "end": end},
                "pricing": {"price_details": {"price": "price_plus_mock"}}}]}}}})
}

/// Pre-2025 invoice shape.
fn invoice_classic(id: &str, r: &str, cus: &str, sub: &str, end: i64, lk: &str) -> Value {
    json!({"id": id, "object": "event", "type": "invoice.paid",
        "data": {"object": {"id": "in_2", "object": "invoice", "customer": cus,
            "subscription": sub, "amount_paid": 2000, "currency": "eur",
            "subscription_details": {"metadata": {"galion_ref": r}},
            "lines": {"data": [{"period": {"start": end - 30 * DAY, "end": end},
                "price": {"id": "price_x", "lookup_key": lk}}]}}}})
}

#[test]
fn full_subscription_lifecycle() {
    let mock = mock_transport();
    let t = app(true, false, mock.clone());
    let (cookie, number) = new_account(&t);
    assert_eq!(me(&t, &cookie)["plan"], "free");

    let r = checkout(&t, &cookie, "plus");
    assert!(r.starts_with("gp_") && r.len() == 29);
    {
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "price lookup + checkout");
        assert_eq!(seen[0].method, "GET");
        assert!(seen[0]
            .form
            .contains(&("lookup_keys[]".into(), "galion_plus_monthly".into())));
        let co = &seen[1];
        assert!(co.url.ends_with("/v1/checkout/sessions"));
        assert_eq!(
            co.idempotency_key.as_deref(),
            Some(format!("galion-checkout-{r}").as_str())
        );
        let f = |k: &str| {
            co.form
                .iter()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(f("mode"), Some("subscription"));
        assert_eq!(f("line_items[0][price]"), Some("price_plus_mock"));
        assert_eq!(f("client_reference_id"), Some(r.as_str()));
        assert_eq!(
            f("subscription_data[metadata][galion_ref]"),
            Some(r.as_str())
        );
        assert_eq!(
            f("success_url"),
            Some("https://galion.app/account?checkout=done")
        );
        // Nothing personal and never the account number.
        let digits: String = number.chars().filter(|c| c.is_ascii_digit()).collect();
        for (k, v) in &co.form {
            assert!(!k.contains("email"), "{k}");
            assert!(!v.contains(&digits) && !v.contains(&number), "{k}");
        }
    }

    // checkout.session.completed links the customer + subscription.
    let ev = completed("evt_c1", &r, "cus_A", "sub_A");
    let res = hook(&t, &ev);
    assert_eq!(res.status, 200);
    assert_eq!(json(&res)["applied"], 1);
    // Replay of the same event id is a no-op.
    let res = hook(&t, &ev);
    assert_eq!(json(&res)["replay"], true);
    let m = me(&t, &cookie);
    assert_eq!(m["can_manage"], true);
    assert_eq!(m["active"], false, "no paid time before invoice.paid");

    // invoice.paid (basil shape) grants the period + 1 day grace.
    let end = common::T0 + 30 * DAY;
    assert_eq!(
        hook(
            &t,
            &invoice_basil("evt_i1", &r, "cus_A", "sub_A", end, 2000)
        )
        .status,
        200
    );
    let m = me(&t, &cookie);
    assert_eq!(m["plan"], "plus");
    assert_eq!(m["active"], true);
    assert_eq!(m["renewing"], true);
    assert_eq!(m["paid_until"], end + DAY);
    assert_eq!(t.app.store.ledger_count().unwrap(), 1);

    // A second checkout while subscribed is refused (use the portal).
    let r2 = t.app.handle(&post_json(
        "/api/pay/stripe/checkout",
        Some(&cookie),
        r#"{"plan":"pro"}"#,
    ));
    assert_eq!(r2.status, 409);
    let p = t
        .app
        .handle(&post_json("/api/pay/stripe/portal", Some(&cookie), "{}"));
    assert_eq!(p.status, 200);
    assert!(json(&p)["url"]
        .as_str()
        .unwrap()
        .starts_with("https://billing.stripe.com/"));
    {
        let seen = mock.seen.lock().unwrap();
        let last = seen.last().unwrap();
        assert!(last.form.contains(&("customer".into(), "cus_A".into())));
        assert!(last
            .form
            .contains(&("return_url".into(), "https://galion.app/account".into())));
    }

    // Upgrade in the portal: plan changes, paid time does not move.
    let up = json!({"id": "evt_u1", "type": "customer.subscription.updated",
        "data": {"object": {"id": "sub_A", "object": "subscription", "status": "active",
            "items": {"data": [{"price": {"id": "price_pro_mock", "lookup_key": "galion_pro_monthly"}}]}}}});
    assert_eq!(hook(&t, &up).status, 200);
    let m = me(&t, &cookie);
    assert_eq!(m["plan"], "pro");
    assert_eq!(m["paid_until"], end + DAY);

    // Renewal next month (classic shape), matched by subscription.
    t.set_now(end);
    // The 30-day session has expired by now: the number logs in again.
    assert_eq!(me(&t, &cookie)["error"], "no_session");
    let li = t.app.handle(&post_json(
        "/api/account/login",
        None,
        &json!({ "number": number }).to_string(),
    ));
    assert_eq!(li.status, 200);
    let cookie = cookie_of(&li);
    let end2 = end + 30 * DAY;
    assert_eq!(
        hook(
            &t,
            &invoice_classic("evt_i2", &r, "cus_A", "sub_A", end2, "galion_pro_monthly")
        )
        .status,
        200
    );
    assert_eq!(me(&t, &cookie)["paid_until"], end2 + DAY);

    // Cancelled: time stops at ended_at and the Stripe link is dropped.
    let at = end + 5 * DAY;
    t.set_now(at);
    let del = json!({"id": "evt_d1", "type": "customer.subscription.deleted",
        "data": {"object": {"id": "sub_A", "object": "subscription", "status": "canceled", "ended_at": at}}});
    assert_eq!(hook(&t, &del).status, 200);
    let m = me(&t, &cookie);
    assert_eq!(m["paid_until"], at);
    assert_eq!(m["can_manage"], false);
    assert_eq!(m["renewing"], false);
    assert!(t.app.store.find_by_subscription("sub_A").unwrap().is_none());
    assert!(t.app.store.find_by_customer("cus_A").unwrap().is_none());

    // The entitlement API answers for the session.
    let session = cookie.split_once('=').unwrap().1;
    let e = t.app.handle_internal(&req(
        "POST",
        "/internal/v1/entitlement",
        &[
            ("Authorization", "Bearer internal-test-token"),
            ("Content-Type", "application/json"),
        ],
        json!({ "session": session }).to_string().as_bytes(),
    ));
    assert_eq!(e.status, 200);
    // Paid time ended exactly now, so the session is back on Free.
    assert_eq!(json(&e)["plan"], "free");
    assert_eq!(json(&e)["active"], false);
    assert_eq!(json(&e)["paid_until"], at);

    // Nothing linking this account to Stripe is left in the database.
    let dump = t.app.store.debug_dump();
    assert!(
        dump.iter()
            .all(|row| !row.contains("cus_A") && !row.contains("sub_A")),
        "{dump:?}"
    );
    assert!(dump
        .iter()
        .filter(|row| row.starts_with("pay_refs:"))
        .all(|row| row.contains(" NULL ")));
}

#[test]
fn invoice_before_checkout_completed_and_unknown_refs_retry() {
    let t = app(true, false, mock_transport());
    let (cookie, _) = new_account(&t);
    let r = checkout(&t, &cookie, "plus");
    let end = common::T0 + 30 * DAY;

    // The invoice arrives first: matched by the reference in the
    // subscription metadata, and the subscription gets linked.
    let res = hook(
        &t,
        &invoice_classic(
            "evt_early",
            &r,
            "cus_B",
            "sub_B",
            end,
            "galion_plus_monthly",
        ),
    );
    assert_eq!(res.status, 200, "{}", String::from_utf8_lossy(&res.body));
    assert_eq!(me(&t, &cookie)["active"], true);
    assert!(t.app.store.find_by_subscription("sub_B").unwrap().is_some());
    // Then checkout.session.completed: still fine.
    assert_eq!(
        hook(&t, &completed("evt_late", &r, "cus_B", "sub_B")).status,
        200
    );
    // Its reference is now used up and unlinked.
    assert!(t.app.store.peek_pay_ref(&r, common::T0).unwrap().is_none());

    // Unknown reference / subscription: 409 so Stripe retries, and the
    // event id is NOT remembered (the retry must get in).
    let ghost = invoice_basil(
        "evt_ghost",
        "gp_UNKNOWNUNKNOWNUNKNOWN0000",
        "cus_X",
        "sub_X",
        end,
        2000,
    );
    assert_eq!(hook(&t, &ghost).status, 409);
    assert_eq!(hook(&t, &ghost).status, 409, "not swallowed as a replay");
    let ghost2 = completed(
        "evt_ghost2",
        "gp_UNKNOWNUNKNOWNUNKNOWN0000",
        "cus_X",
        "sub_X",
    );
    assert_eq!(hook(&t, &ghost2).status, 409);

    // Events we do not handle are acknowledged.
    let other =
        json!({"id": "evt_o", "type": "charge.succeeded", "data": {"object": {"id": "ch_1"}}});
    assert_eq!(hook(&t, &other).status, 200);
}

#[test]
fn bad_signatures_and_disabled_stripe() {
    let t = app(true, false, mock_transport());
    let body = br#"{"id":"evt_1","type":"invoice.paid","data":{"object":{}}}"#;
    for sig in [
        None,
        Some(format!("t={},v1={}", common::T0, "0".repeat(64))),
        Some(format!(
            "t={},v1={}",
            common::T0 - 3600,
            sign(WHSEC, common::T0 - 3600, body)
        )),
        Some(format!(
            "t={},v1={}",
            common::T0,
            sign("whsec_wrong", common::T0, body)
        )),
    ] {
        let mut h = vec![("Content-Type", "application/json")];
        if let Some(s) = &sig {
            h.push(("Stripe-Signature", s.as_str()));
        }
        let r = t.app.handle(&req("POST", "/webhooks/stripe", &h, body));
        assert_eq!(r.status, 400, "{sig:?}");
        assert_eq!(json(&r)["error"], "bad_signature");
    }

    // Not connected: clear state everywhere, no API calls.
    let mock = mock_transport();
    let off = app(false, false, mock.clone());
    let (cookie, _) = new_account(&off);
    let m = me(&off, &cookie);
    assert_eq!(m["methods"]["stripe"]["connected"], false);
    assert!(m["methods"]["stripe"]["note"]
        .as_str()
        .unwrap()
        .contains("not connected"));
    let c = off.app.handle(&post_json(
        "/api/pay/stripe/checkout",
        Some(&cookie),
        r#"{"plan":"plus"}"#,
    ));
    assert_eq!(c.status, 503);
    assert_eq!(json(&c)["error"], "not_connected");
    let w = off.app.handle(&req(
        "POST",
        "/webhooks/stripe",
        &[(
            "Stripe-Signature",
            &format!("t={},v1={}", common::T0, sign(WHSEC, common::T0, body)),
        )],
        body,
    ));
    assert_eq!(w.status, 503);
    assert!(mock.seen.lock().unwrap().is_empty());
}

#[test]
fn home_page_checkout_contract() {
    // HEAD probe: never creates anything.
    let off = app(false, false, mock_transport());
    let h = off
        .app
        .handle(&req("HEAD", "/api/checkout?plan=plus", &[], b""));
    assert_eq!(h.status, 503);
    assert_eq!(String::from_utf8_lossy(&h.body), "not connected");
    assert_eq!(
        off.app
            .handle(&req("HEAD", "/api/checkout?plan=free", &[], b""))
            .status,
        400
    );

    let mock = mock_transport();
    let on = app(true, false, mock.clone());
    assert_eq!(
        on.app
            .handle(&req("HEAD", "/api/checkout?plan=pro", &[], b""))
            .status,
        204
    );
    // GET without a session goes to /account first.
    let g = on
        .app
        .handle(&req("GET", "/api/checkout?plan=plus", &[], b""));
    assert_eq!(g.status, 303);
    assert_eq!(header(&g, "location"), Some("/account?plan=plus"));
    assert!(mock.seen.lock().unwrap().is_empty());
    // With a session: straight to Stripe Checkout.
    let (cookie, _) = new_account(&on);
    let g = on.app.handle(&req(
        "GET",
        "/api/checkout?plan=plus",
        &[("Cookie", &cookie)],
        b"",
    ));
    assert_eq!(g.status, 303);
    assert!(header(&g, "location")
        .unwrap()
        .starts_with("https://checkout.stripe.com/"));
    // Stripe not connected + session: back to /account, no API call.
    let (c2, _) = new_account(&off);
    let g = off.app.handle(&req(
        "GET",
        "/api/checkout?plan=pro",
        &[("Cookie", &c2)],
        b"",
    ));
    assert_eq!(header(&g, "location"), Some("/account?plan=pro"));
}
