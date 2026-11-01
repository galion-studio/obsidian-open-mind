// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Both crypto backends end to end through the real curl transport against
//! LOCAL mock servers (ephemeral loopback ports, one pair per test): a mock
//! BTCPay Server Greenfield API and a mock monero-wallet-rpc. No network, no
//! real keys, no coins.

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::*;
use galion_accounts::pay::crypto::{Crypto, CryptoConfig};
use galion_accounts::pay::transport::CurlTransport;
use galion_accounts::store::{CRYPTO_SETTLE_WINDOW, DAY};
use galion_common::http::{self, Limits, Request, Response};
use serde_json::{json, Value};

const KEY: &str = "btcpay_test_key_not_real";
const STORE: &str = "STORE1";
const WHS: &str = "btcpay_webhook_test_secret";

#[derive(Default)]
struct Mock {
    /// (method, path, query, auth, body)
    seen: Vec<(String, String, String, String, String)>,
    /// BTCPay invoices: id -> (orderId, amount, status)
    invoices: HashMap<String, (String, String, String)>,
    /// wallet-rpc `in` transfers returned by get_transfers.
    transfers: Vec<Value>,
    next: u64,
    rate: String,
    /// Wallet answers only with an HTTP digest `Authorization` (`--rpc-login`).
    digest: bool,
    /// BTCPay `additionalStatus` per invoice (default "None").
    extra: HashMap<String, String>,
    /// BTCPay payment methods per invoice: (amount due in BTC, [(value, status)]).
    payments: HashMap<String, (String, Vec<(String, String)>)>,
}

type Shared = Arc<Mutex<Mock>>;

fn serve(h: Arc<http::Handler>) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = http::serve(l, Limits::default(), h);
    });
    format!("http://{addr}")
}

/// Mock BTCPay (Greenfield) + mock monero-wallet-rpc. Returns their base URLs.
fn mocks() -> (Shared, String, String) {
    let m: Shared = Arc::new(Mutex::new(Mock {
        rate: "200.00".into(),
        ..Mock::default()
    }));
    let m1 = m.clone();
    let btcpay = serve(Arc::new(move |r: &Request| {
        let mut m = m1.lock().unwrap();
        let body = String::from_utf8_lossy(&r.body).into_owned();
        let auth = r.header("authorization").unwrap_or("").to_string();
        m.seen.push((
            r.method().into(),
            r.path().into(),
            r.query().into(),
            auth.clone(),
            body.clone(),
        ));
        if auth != format!("token {KEY}") {
            return Response::json(
                401,
                r#"{"code":"unauthenticated","message":"Authentication is required"}"#,
            );
        }
        let inv = format!("/api/v1/stores/{STORE}/invoices");
        match (r.method(), r.path()) {
            ("POST", p) if p == inv => {
                let v: Value = serde_json::from_str(&body).unwrap();
                assert_eq!(v["currency"], "EUR");
                assert_eq!(v["checkout"]["requiresRefundEmail"], false);
                assert_eq!(
                    v["metadata"].as_object().unwrap().len(),
                    1,
                    "only orderId goes to BTCPay"
                );
                m.next += 1;
                let id = format!("Inv{}", m.next);
                let order = v["metadata"]["orderId"].as_str().unwrap().to_string();
                let amount = v["amount"].as_str().unwrap().to_string();
                m.invoices
                    .insert(id.clone(), (order, amount.clone(), "New".into()));
                Response::json(
                    200,
                    json!({"id": id, "checkoutLink": format!("https://pay.example/i/{id}"),
                    "status": "New", "amount": amount, "currency": "EUR"})
                    .to_string(),
                )
            }
            ("GET", p) if p.starts_with(&format!("{inv}/")) && p.ends_with("/payment-methods") => {
                let id = &p[inv.len() + 1..p.len() - "/payment-methods".len()];
                match m.payments.get(id) {
                    Some((due, pays)) => Response::json(
                        200,
                        json!([{"paymentMethodId": "BTC-CHAIN", "currency": "BTC", "amount": due,
                            "payments": pays.iter().map(|(v, s)| json!({"id": "tx", "value": v, "status": s})).collect::<Vec<_>>()}])
                        .to_string(),
                    ),
                    None => Response::json(200, "[]"),
                }
            }
            ("GET", p) if p.starts_with(&format!("{inv}/")) => {
                let id = &p[inv.len() + 1..];
                let extra = m.extra.get(id).cloned().unwrap_or_else(|| "None".into());
                match m.invoices.get(id) {
                    Some((o, a, s)) => Response::json(
                        200,
                        json!({"id": id, "status": s, "additionalStatus": extra, "amount": a,
                        "currency": "EUR", "metadata": {"orderId": o}})
                        .to_string(),
                    ),
                    None => {
                        Response::json(404, r#"{"code":"invoice-not-found","message":"not found"}"#)
                    }
                }
            }
            ("GET", p) if p == format!("/api/v1/stores/{STORE}/rates") => {
                assert_eq!(r.query(), "currencyPair=XMR_EUR");
                Response::json(
                    200,
                    json!([{"currencyPair": "XMR_EUR", "errors": [], "rate": m.rate}]).to_string(),
                )
            }
            _ => Response::json(404, r#"{"message":"no route"}"#),
        }
    }));
    let m2 = m.clone();
    let wallet = serve(Arc::new(move |r: &Request| {
        let mut m = m2.lock().unwrap();
        let auth = r.header("authorization").unwrap_or("").to_string();
        if m.digest && !auth.starts_with("Digest username=\"galion\"") {
            // monero-wallet-rpc --rpc-login answers 401 with a digest challenge first.
            return Response::json(401, "{}").with_header(
                "www-authenticate",
                "Digest qop=\"auth\",algorithm=MD5,realm=\"monero-rpc\",nonce=\"bW9jaw==\",stale=false",
            );
        }
        let body = String::from_utf8_lossy(&r.body).into_owned();
        m.seen.push((
            "RPC".into(),
            r.path().into(),
            String::new(),
            r.header("authorization").unwrap_or("").into(),
            body.clone(),
        ));
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        let result = match v["method"].as_str().unwrap() {
            "create_address" => {
                assert!(v["params"].get("label").is_none(), "no wallet label");
                m.next += 1;
                json!({"address": format!("8MockSubaddress{}", m.next), "address_index": m.next,
                       "address_indices": [m.next]})
            }
            "make_integrated_address" => {
                m.next += 1;
                json!({"integrated_address": format!("4MockIntegrated{}", m.next),
                       "payment_id": format!("{:016x}", 0xabc0_0000u64 + m.next)})
            }
            "get_transfers" => json!({"in": m.transfers}),
            _ => {
                return Response::json(
                    200,
                    r#"{"jsonrpc":"2.0","id":"0","error":{"code":-32601,"message":"Method not found"}}"#,
                )
            }
        };
        Response::json(
            200,
            json!({"jsonrpc": "2.0", "id": "0", "result": result}).to_string(),
        )
    }));
    (m, btcpay, format!("{wallet}/json_rpc"))
}

fn crypto(vars: Vec<(&'static str, String)>) -> Crypto {
    let cfg = CryptoConfig::from_vars(move |k| {
        vars.iter().find(|(a, _)| *a == k).map(|(_, v)| v.clone())
    });
    Crypto::new(cfg, Arc::new(CurlTransport::default()))
}

fn btcpay_vars(base: &str) -> Vec<(&'static str, String)> {
    vec![
        ("BTCPAY_URL", base.to_string()),
        ("BTCPAY_STORE_ID", STORE.into()),
        ("BTCPAY_API_KEY", KEY.into()),
        ("BTCPAY_WEBHOOK_SECRET", WHS.into()),
    ]
}

fn account(t: &TestApp) -> (String, String) {
    let r = t.app.handle(&post_json("/api/account/create", None, "{}"));
    assert_eq!(r.status, 200);
    (
        json(&r)["number"].as_str().unwrap().replace(' ', ""),
        cookie_of(&r),
    )
}

fn me(t: &TestApp, c: &str) -> Value {
    json(
        &t.app
            .handle(&req("GET", "/api/account/me", &[("Cookie", c)], b"")),
    )
}

fn hook(t: &TestApp, body: &Value, secret: &str) -> Response {
    let b = body.to_string();
    let sig = format!(
        "sha256={}",
        galion_accounts::hmac::hmac_sha256_hex(secret.as_bytes(), b.as_bytes())
    );
    t.app.handle(&req(
        "POST",
        "/webhooks/crypto",
        &[("Content-Type", "application/json"), ("BTCPay-Sig", &sig)],
        b.as_bytes(),
    ))
}

fn settled(inv: &str) -> Value {
    json!({"deliveryId": format!("d-{inv}"), "webhookId": "wh1", "type": "InvoiceSettled",
           "timestamp": T0, "storeId": STORE, "invoiceId": inv,
           // An attacker-chosen amount in the body is never read.
           "amount": "999999", "metadata": {"orderId": "gp_whatever"}})
}

fn no_secrets_or_numbers(t: &TestApp, number: &str) {
    for line in t.app.store.debug_dump() {
        assert!(!line.contains(number), "account number stored: {line}");
        assert!(!line.contains("Mock"), "address stored: {line}");
    }
}

#[test]
fn off_by_default_and_page_hides_the_button() {
    let t = app(false, false, mock_transport());
    let (_, c) = account(&t);
    let m = me(&t, &c);
    assert_eq!(m["methods"]["crypto"]["connected"], false);
    assert_eq!(m["methods"]["crypto"]["backends"], json!([]));
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus"}"#,
    ));
    assert_eq!(r.status, 503);
    let r = hook(&t, &settled("Inv1"), WHS);
    assert_eq!(r.status, 503);
    let page = String::from_utf8(t.app.handle(&req("GET", "/account", &[], b"")).body).unwrap();
    assert_eq!(
        page.matches(r#"data-m="crypto" hidden disabled"#).count(),
        2
    );
    assert!(page.contains("Pay with crypto"));
}

#[test]
fn configured_but_not_enabled_stays_off() {
    let (_, b, w) = mocks();
    let mut v = btcpay_vars(&b);
    v.push(("MONERO_WALLET_RPC_URL", w));
    v.push(("MONERO_EUR_RATE", "200".into()));
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (_, c) = account(&t);
    assert_eq!(me(&t, &c)["methods"]["crypto"]["connected"], false);
}

#[test]
fn btcpay_invoice_webhook_credit_and_cleanup() {
    let (m, b, _) = mocks();
    let mut v = btcpay_vars(&b);
    v.push(("ACCOUNTS_CRYPTO_BACKENDS", "btcpay".into()));
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (number, c) = account(&t);
    let mm = me(&t, &c);
    assert_eq!(mm["methods"]["crypto"]["connected"], true);
    assert_eq!(
        mm["methods"]["crypto"]["backends"],
        json!([{"id": "btcpay", "label": "Bitcoin (BTCPay)"}])
    );

    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus"}"#,
    ));
    assert_eq!(r.status, 200, "{:?}", json(&r));
    assert_eq!(json(&r)["url"], "https://pay.example/i/Inv1");
    {
        let m = m.lock().unwrap();
        let (_, _, _, _, body) = &m.seen[0];
        assert!(body.contains(r#""amount":"20.00""#));
        assert!(body.contains(r#""orderId":"gp_"#));
        assert!(!body.contains(&number));
    }

    // Bad signature, wrong secret.
    assert_eq!(hook(&t, &settled("Inv1"), "wrong").status, 400);
    // Not our event / unknown invoice: acknowledged, nothing happens.
    let mut other = settled("Inv1");
    other["type"] = json!("InvoiceCreated");
    assert_eq!(json(&hook(&t, &other, WHS))["ignored"], "event type");
    assert_eq!(
        json(&hook(&t, &settled("Nope"), WHS))["ignored"],
        "unknown invoice"
    );
    // BTCPay itself does not say Settled yet: retry later.
    assert_eq!(hook(&t, &settled("Inv1"), WHS).status, 409);
    assert_eq!(me(&t, &c)["active"], false);

    m.lock().unwrap().invoices.get_mut("Inv1").unwrap().2 = "Settled".into();
    let r = hook(&t, &settled("Inv1"), WHS);
    assert_eq!(r.status, 200);
    assert_eq!(json(&r)["eur_cents"], 2000);
    let mm = me(&t, &c);
    assert_eq!(mm["active"], true);
    assert_eq!(mm["plan"], "plus");
    assert_eq!(mm["paid_until"], T0 + 30 * DAY);
    // Redelivery: no second credit.
    assert_eq!(json(&hook(&t, &settled("Inv1"), WHS))["replay"], true);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 30 * DAY);

    // The link is cut on credit; only reference + invoice id remain.
    let dump = t.app.store.debug_dump();
    let pr: Vec<_> = dump.iter().filter(|l| l.starts_with("pay_refs:")).collect();
    assert_eq!(pr.len(), 1);
    assert!(pr[0].contains(" NULL crypto plus"), "{pr:?}");
    assert!(dump
        .iter()
        .any(|l| l.starts_with("crypto_invoices:") && l.contains("Inv1")));
    no_secrets_or_numbers(&t, &number);
    // After the settlement window everything about the invoice is gone.
    t.app.store.sweep(T0 + CRYPTO_SETTLE_WINDOW + 1).unwrap();
    let dump = t.app.store.debug_dump();
    assert!(
        !dump
            .iter()
            .any(|l| l.starts_with("crypto_invoices:") || l.starts_with("pay_refs:")),
        "{dump:?}"
    );
    // Bookkeeping line has no account.
    assert!(dump
        .iter()
        .any(|l| l.starts_with("ledger:") && l.contains("crypto 2000 EUR")));
}

#[test]
fn btcpay_order_id_must_match() {
    let (m, b, _) = mocks();
    let mut v = btcpay_vars(&b);
    v.push(("ACCOUNTS_CRYPTO_BACKENDS", "btcpay".into()));
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (_, c) = account(&t);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"pro","backend":"btcpay"}"#,
    ));
    assert_eq!(r.status, 200);
    {
        let mut m = m.lock().unwrap();
        let i = m.invoices.get_mut("Inv1").unwrap();
        assert_eq!(i.1, "220.00");
        i.0 = "gp_SOMEONEELSE".into();
        i.2 = "Settled".into();
    }
    assert_eq!(hook(&t, &settled("Inv1"), WHS).status, 400);
    assert_eq!(me(&t, &c)["active"], false);
}

fn transfer(minor: u64, amount: u64, conf: u64) -> Value {
    json!({"address": format!("8MockSubaddress{minor}"), "amount": amount, "confirmations": conf,
           "double_spend_seen": false, "unlock_time": 0, "payment_id": "0000000000000000",
           "subaddr_index": {"major": 0, "minor": minor}, "txid": format!("tx{minor}{amount}{conf}"), "type": "in"})
}

fn poll(t: &TestApp) -> Value {
    json(&t.app.handle_internal(&req(
        "POST",
        "/internal/v1/crypto/poll",
        &[("Authorization", "Bearer admin-test-token")],
        b"",
    )))
}

#[test]
fn monero_subaddress_confirmations_and_single_credit() {
    let (m, _, w) = mocks();
    m.lock().unwrap().digest = true;
    let v = vec![
        ("ACCOUNTS_CRYPTO_BACKENDS", "monero".to_string()),
        ("MONERO_WALLET_RPC_URL", w),
        ("MONERO_WALLET_RPC_LOGIN", "galion:rpcpw".into()),
        ("MONERO_EUR_RATE", "200".into()),
    ];
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (number, c) = account(&t);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus","backend":"monero"}"#,
    ));
    assert_eq!(r.status, 200, "{:?}", json(&r));
    let j = json(&r);
    assert_eq!(j["address"], "8MockSubaddress1");
    assert_eq!(j["amount_xmr"], "0.1");
    assert_eq!(j["uri"], "monero:8MockSubaddress1?tx_amount=0.1");
    assert!(j["instructions"]
        .as_str()
        .unwrap()
        .contains("10 confirmations"));
    assert_eq!(poll(&t)["credited"], 0);
    // Not enough confirmations yet; a double spend and a time-locked output never count.
    {
        let mut m = m.lock().unwrap();
        m.transfers.push(transfer(1, 100_000_000_000, 9));
        let mut d = transfer(1, 100_000_000_000, 50);
        d["double_spend_seen"] = json!(true);
        m.transfers.push(d);
        let mut l = transfer(1, 100_000_000_000, 50);
        l["unlock_time"] = json!(9_999_999);
        m.transfers.push(l);
        // Someone else's subaddress.
        m.transfers.push(transfer(7, 100_000_000_000, 50));
    }
    assert_eq!(poll(&t)["credited"], 0);
    assert_eq!(me(&t, &c)["active"], false);
    m.lock().unwrap().transfers[0]["confirmations"] = json!(10);
    let p = poll(&t);
    assert_eq!(p["credited"], 1, "{p:?}");
    assert_eq!(p["credited_eur_cents"], 2000);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 30 * DAY);
    // Polling again credits nothing more.
    assert_eq!(poll(&t)["open"], 0);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 30 * DAY);
    {
        let m = m.lock().unwrap();
        let gt: Vec<_> = m
            .seen
            .iter()
            .filter(|s| s.4.contains("get_transfers"))
            .collect();
        assert!(
            gt[0].4.contains(r#""subaddr_indices":[1]"#),
            "{:?}",
            gt[0].4
        );
        assert!(
            m.seen.iter().all(|s| s.4.find("rpcpw").is_none()),
            "password never in a body"
        );
    }
    no_secrets_or_numbers(&t, &number);
    t.app.store.sweep(T0 + CRYPTO_SETTLE_WINDOW + 1).unwrap();
    assert!(!t
        .app
        .store
        .debug_dump()
        .iter()
        .any(|l| l.starts_with("crypto_invoices:")));
}

#[test]
fn monero_partial_payment_is_pro_rata_after_the_grace() {
    let (m, _, w) = mocks();
    let v = vec![
        ("ACCOUNTS_CRYPTO_BACKENDS", "monero".to_string()),
        ("MONERO_WALLET_RPC_URL", w),
        ("MONERO_EUR_RATE", "200".into()),
        ("MONERO_CONFIRMATIONS", "5".into()),
    ];
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (_, c) = account(&t);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus"}"#,
    ));
    assert_eq!(r.status, 200);
    m.lock()
        .unwrap()
        .transfers
        .push(transfer(1, 50_000_000_000, 6));
    assert_eq!(
        poll(&t)["credited"],
        0,
        "waits while the price lock + grace runs"
    );
    t.set_now(T0 + 60 * 60 + galion_accounts::pay::crypto::MONERO_PARTIAL_GRACE);
    let p = poll(&t);
    assert_eq!(p["credited_eur_cents"], 1000);
    let mm = me(&t, &c);
    assert_eq!(
        mm["paid_until"],
        T0 + 60 * 60 + galion_accounts::pay::crypto::MONERO_PARTIAL_GRACE + 15 * DAY
    );
}

#[test]
fn monero_rate_from_btcpay_and_integrated_addresses() {
    let (m, b, w) = mocks();
    m.lock().unwrap().rate = "125.5".into();
    let mut v = btcpay_vars(&b);
    v.push(("ACCOUNTS_CRYPTO_BACKENDS", "btcpay,monero".into()));
    v.push(("MONERO_WALLET_RPC_URL", w));
    v.push(("MONERO_RATE_SOURCE", "btcpay".into()));
    v.push(("MONERO_ADDRESS_MODE", "integrated".into()));
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (_, c) = account(&t);
    let bs = me(&t, &c)["methods"]["crypto"]["backends"].clone();
    assert_eq!(bs.as_array().unwrap().len(), 2);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus","backend":"monero"}"#,
    ));
    assert_eq!(r.status, 200, "{:?}", json(&r));
    let j = json(&r);
    // 20 / 125.5 = 0.159362549800796... XMR, rounded up to the piconero.
    assert_eq!(j["amount_xmr"], "0.159362549801");
    assert_eq!(j["address"], "4MockIntegrated1");
    let pid = format!("{:016x}", 0xabc0_0001u64);
    let mut tr = transfer(0, 159_362_549_801, 10);
    tr["payment_id"] = json!(pid);
    tr["subaddr_index"] = json!({"major": 0, "minor": 0});
    m.lock().unwrap().transfers.push(tr);
    assert_eq!(poll(&t)["credited_eur_cents"], 2000);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 30 * DAY);
    let m = m.lock().unwrap();
    let gt = m
        .seen
        .iter()
        .find(|s| s.4.contains("get_transfers"))
        .unwrap();
    assert!(!gt.4.contains("subaddr_indices"));
}

#[test]
fn unknown_or_disabled_backend_is_refused() {
    let (_, b, _) = mocks();
    let mut v = btcpay_vars(&b);
    v.push(("ACCOUNTS_CRYPTO_BACKENDS", "btcpay".into()));
    let t = app_with_crypto(false, false, mock_transport(), crypto(v));
    let (_, c) = account(&t);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus","backend":"monero"}"#,
    ));
    assert_eq!(r.status, 503);
    let r = t
        .app
        .handle(&post_json("/api/pay/crypto", None, r#"{"plan":"plus"}"#));
    assert_eq!(r.status, 401);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"free"}"#,
    ));
    assert_eq!(r.status, 400);
    // The poll covers only the backends that are on (BTCPay reconcile here,
    // no Monero call).
    let p = poll(&t);
    assert_eq!(p["ok"], true, "{p}");
    assert_eq!(p["open"], 0);
    // With no crypto backend at all the poll says so.
    let t = app(false, false, mock_transport());
    assert_eq!(poll(&t)["error"], "not_connected");
}

fn expired(inv: &str) -> Value {
    json!({"deliveryId": format!("x-{inv}"), "webhookId": "wh1", "type": "InvoiceExpired",
           "timestamp": T0, "storeId": STORE, "invoiceId": inv,
           "partiallyPaid": true, "metadata": {"orderId": "gp_whatever"}})
}

fn btcpay_app(m_b: &str) -> TestApp {
    let mut v = btcpay_vars(m_b);
    v.push(("ACCOUNTS_CRYPTO_BACKENDS", "btcpay".into()));
    app_with_crypto(false, false, mock_transport(), crypto(v))
}

#[test]
fn btcpay_underpaid_expired_credits_confirmed_part_pro_rata() {
    let (m, b, _) = mocks();
    let t = btcpay_app(&b);
    let (_, c) = account(&t);
    let r = t.app.handle(&post_json(
        "/api/pay/crypto",
        Some(&c),
        r#"{"plan":"plus"}"#,
    ));
    assert_eq!(r.status, 200);
    {
        let mut m = m.lock().unwrap();
        m.invoices.get_mut("Inv1").unwrap().2 = "Expired".into();
        m.extra.insert("Inv1".into(), "PaidPartial".into());
        // Half paid, but one of the two payments is not confirmed yet.
        m.payments.insert(
            "Inv1".into(),
            (
                "0.00040000".into(),
                vec![
                    ("0.00010000".into(), "Settled".into()),
                    ("0.00010000".into(), "Processing".into()),
                ],
            ),
        );
    }
    let r = hook(&t, &expired("Inv1"), WHS);
    assert_eq!(r.status, 200);
    assert_eq!(json(&r)["ignored"], "payment not confirmed yet");
    assert_eq!(me(&t, &c)["active"], false);
    // A later reconcile poll credits it once both are confirmed: 50 % = 15 days.
    m.lock().unwrap().payments.get_mut("Inv1").unwrap().1[1].1 = "Settled".into();
    let p = poll(&t);
    assert_eq!(p["credited"], 1, "{p}");
    assert_eq!(p["credited_eur_cents"], 1000);
    let mm = me(&t, &c);
    assert_eq!(mm["active"], true);
    assert_eq!(mm["paid_until"], T0 + 15 * DAY);
    // Webhook redelivery and another poll: no second credit.
    assert_eq!(json(&hook(&t, &expired("Inv1"), WHS))["replay"], true);
    assert_eq!(poll(&t)["credited"], 0);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 15 * DAY);
}

#[test]
fn btcpay_expired_unpaid_is_ignored_and_overpaid_gets_more_time() {
    let (m, b, _) = mocks();
    let t = btcpay_app(&b);
    let (_, c) = account(&t);
    for _ in 0..2 {
        let r = t.app.handle(&post_json(
            "/api/pay/crypto",
            Some(&c),
            r#"{"plan":"plus"}"#,
        ));
        assert_eq!(r.status, 200);
    }
    // Inv1 expired with nothing paid.
    m.lock().unwrap().invoices.get_mut("Inv1").unwrap().2 = "Expired".into();
    assert_eq!(
        json(&hook(&t, &expired("Inv1"), WHS))["ignored"],
        "expired unpaid"
    );
    // Inv2 settled with 150 % paid: 45 days.
    {
        let mut m = m.lock().unwrap();
        m.invoices.get_mut("Inv2").unwrap().2 = "Settled".into();
        m.extra.insert("Inv2".into(), "PaidOver".into());
        m.payments.insert(
            "Inv2".into(),
            (
                "0.00040000".into(),
                vec![("0.00060000".into(), "Settled".into())],
            ),
        );
    }
    let r = hook(&t, &settled("Inv2"), WHS);
    assert_eq!(r.status, 200, "{:?}", json(&r));
    assert_eq!(json(&r)["eur_cents"], 3000);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 45 * DAY);
}

#[test]
fn btcpay_missed_webhook_is_reconciled_by_the_poll() {
    let (m, b, _) = mocks();
    let t = btcpay_app(&b);
    let (_, c) = account(&t);
    let r = t
        .app
        .handle(&post_json("/api/pay/crypto", Some(&c), r#"{"plan":"pro"}"#));
    assert_eq!(r.status, 200);
    assert_eq!(poll(&t)["credited"], 0);
    m.lock().unwrap().invoices.get_mut("Inv1").unwrap().2 = "Settled".into();
    let p = poll(&t);
    assert_eq!(p["credited"], 1, "{p}");
    assert_eq!(p["credited_eur_cents"], 22000);
    let mm = me(&t, &c);
    assert_eq!(mm["plan"], "pro");
    assert_eq!(mm["paid_until"], T0 + 30 * DAY);
}
