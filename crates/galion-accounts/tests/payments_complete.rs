// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The switch-on checklist as tests: `stripe-setup` (dry run, apply,
//! idempotent re-run, webhook event update, secret never printed),
//! checkout reuse/expiry/trial, cancel/resume, failed payment + grace,
//! refunds, foreign and duplicate subscriptions, banked cash time, the
//! printable cash slip, the admin/readiness endpoints and the CLI.
//!
//! Stripe is a STATEFUL Rust mock on an ephemeral loopback port, reached
//! through the real curl transport. No network, no real keys.

mod common;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use common::*;
use galion_accounts::hash::{Cost, Hasher};
use galion_accounts::pay::cash::{Cash, CashConfig};
use galion_accounts::pay::crypto::Crypto;
use galion_accounts::pay::stripe::{sign, Stripe, StripeConfig, EVENTS};
use galion_accounts::pay::transport::CurlTransport;
use galion_accounts::ratelimit::Limiter;
use galion_accounts::server::{App, Settings};
use galion_accounts::store::{Store, DAY, PAYMENT_GRACE};
use galion_common::http::{self, Limits, Request, Response};
use serde_json::{json, Value};

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    idem: Option<String>,
    form: Vec<(String, String)>,
}

#[derive(Default)]
struct StripeState {
    seen: Vec<Seen>,
    /// lookup_key -> (price id, product id, cents)
    prices: HashMap<String, (String, String, i64)>,
    products: u32,
    portal: Vec<String>,
    /// (id, url, events)
    hooks: Vec<(String, String, Vec<String>)>,
    sessions: u32,
}

type Shared = Arc<Mutex<StripeState>>;

fn unpct(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

fn decode(s: &str) -> Vec<(String, String)> {
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (unpct(k), unpct(v))
        })
        .collect()
}

fn get<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
    f.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
}

/// Start the mock; returns its state and base URL.
fn stripe_mock() -> (Shared, String) {
    let st: Shared = Arc::new(Mutex::new(StripeState::default()));
    let s2 = st.clone();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let h: Arc<http::Handler> = Arc::new(move |r: &Request| {
        let mut s = s2.lock().unwrap();
        let body = String::from_utf8_lossy(&r.body).into_owned();
        let form = decode(if r.method() == "GET" {
            r.query()
        } else {
            &body
        });
        s.seen.push(Seen {
            method: r.method().into(),
            path: r.path().into(),
            idem: r.header("idempotency-key").map(str::to_string),
            form: form.clone(),
        });
        if r.header("authorization") != Some(&format!("Bearer {SK}")) {
            return Response::json(401, r#"{"error":{"message":"Invalid API Key provided"}}"#);
        }
        let list =
            |v: Vec<Value>| Response::json(200, json!({"object": "list", "data": v}).to_string());
        match (r.method(), r.path()) {
            ("GET", "/v1/prices") => {
                let lk = get(&form, "lookup_keys[]").unwrap_or("");
                match s.prices.get(lk) {
                    Some((id, prod, cents)) => list(vec![json!({"id": id, "product": prod,
                        "unit_amount": cents, "currency": "eur", "lookup_key": lk})]),
                    None => list(vec![]),
                }
            }
            ("POST", "/v1/products") => {
                s.products += 1;
                Response::json(200, json!({"id": format!("prod_{}", s.products)}).to_string())
            }
            ("POST", "/v1/prices") => {
                let lk = get(&form, "lookup_key").unwrap().to_string();
                let id = format!("price_{lk}");
                let cents: i64 = get(&form, "unit_amount").unwrap().parse().unwrap();
                let prod = get(&form, "product").unwrap().to_string();
                s.prices.insert(lk, (id.clone(), prod, cents));
                Response::json(200, json!({"id": id}).to_string())
            }
            ("GET", "/v1/billing_portal/configurations") => list(
                s.portal
                    .iter()
                    .map(|id| json!({"id": id, "active": true, "metadata": {"galion": "accounts"}}))
                    .collect(),
            ),
            ("POST", "/v1/billing_portal/configurations") => {
                let id = format!("bpc_{}", s.portal.len() + 1);
                s.portal.push(id.clone());
                Response::json(200, json!({"id": id}).to_string())
            }
            ("GET", "/v1/webhook_endpoints") => list(
                s.hooks
                    .iter()
                    .map(|(id, url, ev)| json!({"id": id, "url": url, "enabled_events": ev}))
                    .collect(),
            ),
            ("POST", "/v1/webhook_endpoints") => {
                let id = format!("we_{}", s.hooks.len() + 1);
                let ev: Vec<String> = form
                    .iter()
                    .filter(|(k, _)| k.starts_with("enabled_events["))
                    .map(|(_, v)| v.clone())
                    .collect();
                s.hooks.push((id.clone(), get(&form, "url").unwrap().into(), ev));
                Response::json(200, json!({"id": id, "secret": "whsec_mock_created_once"}).to_string())
            }
            ("POST", p) if p.starts_with("/v1/webhook_endpoints/") => {
                let id = &p["/v1/webhook_endpoints/".len()..];
                let ev: Vec<String> = form
                    .iter()
                    .filter(|(k, _)| k.starts_with("enabled_events["))
                    .map(|(_, v)| v.clone())
                    .collect();
                if let Some(h) = s.hooks.iter_mut().find(|h| h.0 == id) {
                    h.2 = ev;
                }
                // An update never returns the secret.
                Response::json(200, json!({"id": id}).to_string())
            }
            ("POST", "/v1/checkout/sessions") => {
                s.sessions += 1;
                let r = get(&form, "client_reference_id").unwrap_or("");
                Response::json(
                    200,
                    json!({"id": format!("cs_{}", s.sessions),
                        "url": format!("https://checkout.stripe.com/c/pay/cs_{}#{r}", s.sessions)})
                    .to_string(),
                )
            }
            ("POST", "/v1/billing_portal/sessions") => Response::json(
                200,
                json!({"url": format!("https://billing.stripe.com/p/session/{}", get(&form, "configuration").unwrap_or("default"))}).to_string(),
            ),
            ("POST", p) if p.starts_with("/v1/subscriptions/") => {
                let cancel = get(&form, "cancel_at_period_end") == Some("true");
                Response::json(
                    200,
                    json!({"id": &p["/v1/subscriptions/".len()..], "status": "active",
                        "cancel_at_period_end": cancel,
                        "cancel_at": if cancel { json!(T0 + 30 * DAY) } else { Value::Null },
                        "items": {"data": [{"current_period_end": T0 + 30 * DAY}]}})
                    .to_string(),
                )
            }
            _ => Response::json(404, r#"{"error":{"message":"unknown route"}}"#),
        }
    });
    std::thread::spawn(move || {
        let _ = http::serve(l, Limits::default(), h);
    });
    (st, format!("http://{addr}"))
}

fn stripe_client(base: &str, key: Option<&str>, extra: &[(&'static str, &str)]) -> Stripe {
    let base = base.to_string();
    let key = key.map(str::to_string);
    let extra: Vec<(&'static str, String)> =
        extra.iter().map(|(k, v)| (*k, v.to_string())).collect();
    let cfg = StripeConfig::from_vars(move |k| match k {
        "STRIPE_SECRET_KEY" => key.clone(),
        "STRIPE_WEBHOOK_SECRET" => Some(WHSEC.into()),
        "STRIPE_API_BASE" => Some(base.clone()),
        "ACCOUNTS_PUBLIC_URL" => Some("https://galion.app".into()),
        other => extra
            .iter()
            .find(|(a, _)| *a == other)
            .map(|(_, v)| v.clone()),
    });
    Stripe::new(cfg, Arc::new(CurlTransport::default()))
}

fn full_app(base: &str) -> TestApp {
    // Prices exist as after `stripe-setup --apply`; looked up by lookup_key.
    let st = stripe_client(base, Some(SK), &[]);
    st.setup(true, None).unwrap();
    let clock = Arc::new(AtomicI64::new(T0));
    let c2 = clock.clone();
    let app = App {
        store: Store::in_memory().unwrap(),
        hasher: Hasher::new(vec![9u8; 32], Cost::TEST).unwrap(),
        limiter: Limiter::new(),
        stripe: stripe_client(base, Some(SK), &[("STRIPE_PORTAL_CONFIG", "bpc_1")]),
        cash: Cash {
            cfg: CashConfig::from_vars(|k| {
                (k == "ACCOUNTS_CASH_ADDRESS")
                    .then(|| "Galion <Box>|PO Box 1|10000 Zagreb".to_string())
            }),
        },
        crypto: Crypto::disabled(),
        settings: Settings {
            cookie_domain: None,
            secure_cookies: true,
            trust_proxy: false,
            internal_token: Some("internal-test-token".into()),
            admin_token: Some("admin-test-token".into()),
        },
        clock: Box::new(move || c2.load(Ordering::SeqCst)),
    };
    TestApp {
        app: Arc::new(app),
        clock,
    }
}

fn since(st: &Shared, n: usize) -> Vec<Seen> {
    st.lock().unwrap().seen[n..].to_vec()
}

fn mark(st: &Shared) -> usize {
    st.lock().unwrap().seen.len()
}

fn hook(t: &TestApp, body: &Value) -> Response {
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
    (
        cookie_of(&r),
        json(&r)["number"].as_str().unwrap().to_string(),
    )
}

/// Sessions last 30 days; after a long clock jump log in again.
fn relogin(t: &TestApp, number: &str) -> String {
    let r = t.app.handle(&post_json(
        "/api/account/login",
        None,
        &json!({ "number": number }).to_string(),
    ));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    cookie_of(&r)
}

fn me(t: &TestApp, c: &str) -> Value {
    json(
        &t.app
            .handle(&req("GET", "/api/account/me", &[("Cookie", c)], b"")),
    )
}

fn checkout_url(t: &TestApp, c: &str, plan: &str) -> String {
    let r = t.app.handle(&post_json(
        "/api/pay/stripe/checkout",
        Some(c),
        &json!({ "plan": plan }).to_string(),
    ));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    json(&r)["url"].as_str().unwrap().to_string()
}

fn ref_of(url: &str) -> String {
    url.split('#').nth(1).unwrap().to_string()
}

fn ev(id: &str, kind: &str, obj: Value) -> Value {
    json!({"id": id, "object": "event", "type": kind, "data": {"object": obj}})
}

fn completed(id: &str, r: &str, cus: &str, sub: &str) -> Value {
    ev(
        id,
        "checkout.session.completed",
        json!({"id": "cs_x", "mode": "subscription",
        "client_reference_id": r, "metadata": {"galion_ref": r}, "customer": cus, "subscription": sub}),
    )
}

#[allow(clippy::too_many_arguments)]
fn invoice(
    id: &str,
    r: Option<&str>,
    cus: &str,
    sub: &str,
    start: i64,
    end: i64,
    lk: &str,
    cents: i64,
) -> Value {
    ev(
        id,
        "invoice.paid",
        json!({"id": format!("in_{id}"), "customer": cus, "amount_paid": cents,
        "currency": "eur", "billing_reason": "subscription_cycle",
        "parent": {"type": "subscription_details", "subscription_details":
            {"subscription": sub, "metadata": r.map(|r| json!({"galion_ref": r})).unwrap_or(json!({}))}},
        "lines": {"data": [{"period": {"start": start, "end": end}, "price": {"id": "price_x", "lookup_key": lk}}]}}),
    )
}

fn admin(t: &TestApp, path: &str, token: &str) -> Response {
    t.app.handle_internal(&req(
        "GET",
        path,
        &[("Authorization", &format!("Bearer {token}"))],
        b"",
    ))
}

/// Link an account to a Plus card subscription paid until `end`.
fn subscribe(t: &TestApp, c: &str, cus: &str, sub: &str, plan: &str, end: i64) {
    let url = checkout_url(t, c, plan);
    let r = ref_of(&url);
    assert_eq!(
        hook(t, &completed(&format!("evc_{sub}"), &r, cus, sub)).status,
        200
    );
    let lk = if plan == "pro" {
        "galion_pro_monthly"
    } else {
        "galion_plus_monthly"
    };
    let cents = if plan == "pro" { 22000 } else { 2000 };
    let resp = hook(
        t,
        &invoice(
            &format!("evi_{sub}"),
            Some(&r),
            cus,
            sub,
            end - 30 * DAY,
            end,
            lk,
            cents,
        ),
    );
    assert_eq!(resp.status, 200, "{}", String::from_utf8_lossy(&resp.body));
}

// ---------------------------------------------------------------- setup

#[test]
fn setup_dry_run_apply_rerun_and_event_update() {
    let (st, base) = stripe_mock();
    let url = "https://galion.app/webhooks/stripe";

    // Dry run WITH a key: reads only.
    let n = mark(&st);
    let dry = stripe_client(&base, Some(SK), &[])
        .setup(false, Some(url))
        .unwrap();
    let seen = since(&st, n);
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|s| s.method == "GET"), "{seen:?}");
    assert!(dry.webhook_secret.is_none());
    let text = dry.lines.join("\n");
    assert!(text.contains("dry run (test key)"), "{text}");
    assert!(text.contains("would create plus") && text.contains("EUR 20.00/month"));
    assert!(text.contains("would create pro") && text.contains("EUR 220.00/month"));
    assert!(text.contains("would create billing portal configuration"));
    assert!(text.contains("would create webhook endpoint"));

    // Apply.
    let n = mark(&st);
    let rep = stripe_client(&base, Some(SK), &[])
        .setup(true, Some(url))
        .unwrap();
    let posts: Vec<Seen> = since(&st, n)
        .into_iter()
        .filter(|s| s.method == "POST")
        .collect();
    let paths: Vec<&str> = posts.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/v1/products",
            "/v1/prices",
            "/v1/products",
            "/v1/prices",
            "/v1/billing_portal/configurations",
            "/v1/webhook_endpoints"
        ]
    );
    assert!(
        posts.iter().all(|p| p.idem.is_some()),
        "every create is idempotent"
    );
    let price_plus = &posts[1].form;
    assert_eq!(get(price_plus, "currency"), Some("eur"));
    assert_eq!(get(price_plus, "unit_amount"), Some("2000"));
    assert_eq!(get(&posts[3].form, "unit_amount"), Some("22000"));
    assert_eq!(get(&posts[3].form, "recurring[interval]"), Some("month"));
    let portal = &posts[4].form;
    assert_eq!(
        get(portal, "features[subscription_cancel][mode]"),
        Some("at_period_end")
    );
    assert_eq!(
        get(
            portal,
            "features[subscription_update][products][0][prices][0]"
        ),
        Some("price_galion_plus_monthly")
    );
    assert_eq!(
        get(
            portal,
            "features[subscription_update][products][1][prices][0]"
        ),
        Some("price_galion_pro_monthly")
    );
    assert_eq!(
        get(
            portal,
            "features[subscription_update][products][1][product]"
        ),
        Some("prod_2")
    );
    assert_eq!(
        get(portal, "features[customer_update][enabled]"),
        Some("false")
    );
    let hook_form = &posts[5].form;
    for e in EVENTS {
        assert!(
            hook_form
                .iter()
                .any(|(k, v)| k.starts_with("enabled_events[") && v == e),
            "{e}"
        );
    }
    assert!(EVENTS.contains(&"invoice.payment_failed") && EVENTS.contains(&"charge.refunded"));
    assert_eq!(
        rep.webhook_secret.as_deref(),
        Some("whsec_mock_created_once")
    );
    let text = rep.lines.join("\n");
    assert!(!text.contains("whsec_"), "secret printed: {text}");
    assert!(!format!("{rep:?}").contains("whsec_mock"));
    assert!(text.contains("set STRIPE_PORTAL_CONFIG=bpc_1"), "{text}");

    // Re-run: nothing new.
    let n = mark(&st);
    let again = stripe_client(&base, Some(SK), &[])
        .setup(true, Some(url))
        .unwrap();
    assert!(since(&st, n).iter().all(|s| s.method == "GET"));
    assert!(again.webhook_secret.is_none());
    let text = again.lines.join("\n");
    assert!(
        text.contains("exists plus") && text.contains("exists pro"),
        "{text}"
    );
    assert!(text.contains("exists billing portal configuration bpc_1"));
    assert!(text.contains("with all events"));

    // An older endpoint with only 4 events gets the missing ones, no new secret.
    st.lock().unwrap().hooks[0].2.truncate(4);
    let n = mark(&st);
    let upd = stripe_client(&base, Some(SK), &[])
        .setup(true, Some(url))
        .unwrap();
    let posts: Vec<Seen> = since(&st, n)
        .into_iter()
        .filter(|s| s.method == "POST")
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].path, "/v1/webhook_endpoints/we_1");
    assert_eq!(st.lock().unwrap().hooks[0].2.len(), EVENTS.len());
    assert!(upd.webhook_secret.is_none());
    assert!(upd
        .lines
        .join("\n")
        .contains("updated webhook endpoint we_1"));

    // Plain http webhook URLs are refused; no key + apply refused.
    assert!(stripe_client(&base, Some(SK), &[])
        .setup(true, Some("http://galion.app/x"))
        .is_err());
    assert!(stripe_client(&base, None, &[]).setup(true, None).is_err());
}

// ------------------------------------------------------------- checkout

#[test]
fn checkout_is_reused_expires_and_uses_prepaid_time_as_trial() {
    let (st, base) = stripe_mock();
    let t = full_app(&base);
    let (c, _) = new_account(&t);
    let n = mark(&st);
    let u1 = checkout_url(&t, &c, "plus");
    let u2 = checkout_url(&t, &c, "plus");
    assert_eq!(u1, u2, "double click gets the same page");
    let sessions: Vec<Seen> = since(&st, n)
        .into_iter()
        .filter(|s| s.path == "/v1/checkout/sessions")
        .collect();
    assert_eq!(sessions.len(), 1);
    let expires = (T0 + 3600).to_string();
    assert_eq!(get(&sessions[0].form, "expires_at"), Some(expires.as_str()));
    assert_eq!(get(&sessions[0].form, "subscription_data[trial_end]"), None);
    // Another plan is another page.
    assert_ne!(checkout_url(&t, &c, "pro"), u1);
    // Close to Stripe's expiry: a fresh page.
    t.set_now(T0 + 3400);
    assert_ne!(checkout_url(&t, &c, "plus"), u1);

    // Prepaid cash time on the same plan becomes the trial.
    t.set_now(T0);
    let (c2, _) = new_account(&t);
    let r = t
        .app
        .handle(&post_json("/api/pay/cash", Some(&c2), r#"{"plan":"plus"}"#));
    let cref = json(&r)["reference"].as_str().unwrap().to_string();
    let r = t.app.handle_internal(&req(
        "POST",
        "/internal/v1/cash/credit",
        &[
            ("Authorization", "Bearer admin-test-token"),
            ("Content-Type", "application/json"),
        ],
        json!({"reference": cref, "amount_cents": 2000})
            .to_string()
            .as_bytes(),
    ));
    assert_eq!(r.status, 200);
    let n = mark(&st);
    checkout_url(&t, &c2, "plus");
    let s = since(&st, n)
        .into_iter()
        .find(|s| s.path == "/v1/checkout/sessions")
        .unwrap();
    let trial = (T0 + 30 * DAY).to_string();
    assert_eq!(
        get(&s.form, "subscription_data[trial_end]"),
        Some(trial.as_str())
    );
    // ... but not for another plan.
    let n = mark(&st);
    checkout_url(&t, &c2, "pro");
    let s = since(&st, n)
        .into_iter()
        .find(|s| s.path == "/v1/checkout/sessions")
        .unwrap();
    assert_eq!(get(&s.form, "subscription_data[trial_end]"), None);
}

// --------------------------------------------------------- cancel/resume

#[test]
fn cancel_and_resume_without_the_portal() {
    let (st, base) = stripe_mock();
    let t = full_app(&base);
    let (c, _) = new_account(&t);
    // Nothing to cancel yet.
    assert_eq!(
        t.app
            .handle(&post_json("/api/pay/stripe/cancel", Some(&c), "{}"))
            .status,
        404
    );
    subscribe(&t, &c, "cus_c", "sub_c", "plus", T0 + 30 * DAY);
    let m = me(&t, &c);
    assert_eq!(m["renewing"], true);
    assert_eq!(m["can_cancel"], true);
    assert_eq!(m["billing"], "ok");

    let n = mark(&st);
    let r = t
        .app
        .handle(&post_json("/api/pay/stripe/cancel", Some(&c), "{}"));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    let s = since(&st, n);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].path, "/v1/subscriptions/sub_c");
    assert_eq!(get(&s[0].form, "cancel_at_period_end"), Some("true"));
    assert!(s[0]
        .idem
        .as_deref()
        .unwrap()
        .starts_with("galion-sub-sub_c-true-"));
    let m = me(&t, &c);
    assert_eq!(m["billing"], "canceling");
    assert_eq!(m["renewing"], false);
    assert_eq!(m["active"], true, "paid time is kept");
    assert_eq!(m["cancel_at"], T0 + 30 * DAY);

    let r = t
        .app
        .handle(&post_json("/api/pay/stripe/resume", Some(&c), "{}"));
    assert_eq!(r.status, 200);
    let m = me(&t, &c);
    assert_eq!(m["billing"], "ok");
    assert_eq!(m["renewing"], true);
    assert_eq!(m["cancel_at"], Value::Null);

    // Cancelling in the portal arrives as subscription.updated.
    let upd = ev(
        "ev_upd1",
        "customer.subscription.updated",
        json!({"id": "sub_c", "status": "active",
        "cancel_at_period_end": true, "cancel_at": null,
        "items": {"data": [{"current_period_end": T0 + 30 * DAY, "price": {"id": "p", "lookup_key": "galion_plus_monthly"}}]}}),
    );
    assert_eq!(hook(&t, &upd).status, 200);
    let m = me(&t, &c);
    assert_eq!(m["billing"], "canceling");
    assert_eq!(m["cancel_at"], T0 + 30 * DAY);
    // Portal sessions use the configured portal configuration.
    let r = t
        .app
        .handle(&post_json("/api/pay/stripe/portal", Some(&c), "{}"));
    assert_eq!(
        json(&r)["url"],
        "https://billing.stripe.com/p/session/bpc_1"
    );
}

// ------------------------------------------------- failed payment, grace

#[test]
fn failed_renewal_keeps_access_for_the_grace_then_recovers() {
    let (_st, base) = stripe_mock();
    let t = full_app(&base);
    let (c, number) = new_account(&t);
    let e1 = T0 + 30 * DAY;
    subscribe(&t, &c, "cus_f", "sub_f", "plus", e1);
    assert_eq!(me(&t, &c)["paid_until"], e1 + DAY);

    t.set_now(e1 + 3600);
    let c = relogin(&t, &number);
    let failed = ev(
        "ev_fail1",
        "invoice.payment_failed",
        json!({"id": "in_f2", "customer": "cus_f",
        "billing_reason": "subscription_cycle", "attempt_count": 1,
        "parent": {"subscription_details": {"subscription": "sub_f"}},
        "lines": {"data": [{"period": {"start": e1, "end": e1 + 30 * DAY}}]}}),
    );
    let r = hook(&t, &failed);
    assert_eq!(r.status, 200);
    assert_eq!(json(&r)["applied"], 1);
    let m = me(&t, &c);
    assert_eq!(m["billing"], "past_due");
    assert_eq!(m["active"], true);
    assert_eq!(m["paid_until"], e1 + PAYMENT_GRACE);
    // Replay of the same event changes nothing.
    assert_eq!(json(&hook(&t, &failed))["replay"], true);

    // Stripe's retry succeeds: billing back to ok, a full new period.
    t.set_now(e1 + 3 * DAY);
    let r = hook(
        &t,
        &invoice(
            "ev_paid2",
            None,
            "cus_f",
            "sub_f",
            e1,
            e1 + 30 * DAY,
            "galion_plus_monthly",
            2000,
        ),
    );
    assert_eq!(r.status, 200);
    let m = me(&t, &c);
    assert_eq!(m["billing"], "ok");
    assert_eq!(m["paid_until"], e1 + 31 * DAY);

    // Retries exhausted: subscription goes unpaid, then is deleted.
    let e2 = e1 + 30 * DAY;
    t.set_now(e2 + DAY);
    let c = relogin(&t, &number);
    let upd = ev(
        "ev_unpaid",
        "customer.subscription.updated",
        json!({"id": "sub_f", "status": "unpaid",
        "cancel_at_period_end": false, "items": {"data": [{"current_period_end": e2}]}}),
    );
    assert_eq!(hook(&t, &upd).status, 200);
    assert_eq!(me(&t, &c)["billing"], "past_due");
    t.set_now(e2 + 5 * DAY);
    let del = ev(
        "ev_del",
        "customer.subscription.deleted",
        json!({"id": "sub_f", "status": "canceled",
        "ended_at": e2 + 5 * DAY, "canceled_at": e2 + 5 * DAY}),
    );
    assert_eq!(hook(&t, &del).status, 200);
    let m = me(&t, &c);
    assert_eq!(m["active"], false);
    assert_eq!(m["billing"], "ok");
    assert_eq!(m["renewing"], false);

    // A first payment that never went through (no account linked) is acked.
    let first = ev(
        "ev_fail_first",
        "invoice.payment_failed",
        json!({"id": "in_new", "customer": "cus_new",
        "billing_reason": "subscription_create", "parent": {"subscription_details": {"subscription": "sub_new"}}}),
    );
    let r = hook(&t, &first);
    assert_eq!(r.status, 200);
    assert_eq!(json(&r)["applied"], 0);
}

// ---------------------------------------------------------------- refunds

#[test]
fn refunds_are_booked_and_a_full_refund_ends_the_paid_time() {
    let (_st, base) = stripe_mock();
    let t = full_app(&base);
    let (c, _) = new_account(&t);
    subscribe(&t, &c, "cus_r", "sub_r", "plus", T0 + 30 * DAY);
    t.set_now(T0 + DAY);
    // Partial refund (goodwill): time kept, ledger -500.
    let part = ev(
        "ev_ref1",
        "charge.refunded",
        json!({"id": "ch_1", "customer": "cus_r",
        "amount": 2000, "amount_refunded": 500, "refunded": false, "currency": "eur"}),
    );
    let mut part = part;
    part["data"]["previous_attributes"] = json!({"amount_refunded": 0});
    assert_eq!(hook(&t, &part).status, 200);
    assert_eq!(me(&t, &c)["paid_until"], T0 + 31 * DAY);
    // The rest refunded: full, time ends now; only the delta is booked.
    let mut full = ev(
        "ev_ref2",
        "charge.refunded",
        json!({"id": "ch_1", "customer": "cus_r",
        "amount": 2000, "amount_refunded": 2000, "refunded": true, "currency": "eur"}),
    );
    full["data"]["previous_attributes"] = json!({"amount_refunded": 500});
    assert_eq!(hook(&t, &full).status, 200);
    let m = me(&t, &c);
    assert_eq!(m["paid_until"], T0 + DAY);
    assert_eq!(m["active"], false);
    let s = json(&admin(&t, "/internal/v1/admin/status", "admin-test-token"));
    let cents: i64 = s["ledger"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["method"] == "stripe")
        .map(|l| l["cents"].as_i64().unwrap())
        .sum();
    assert_eq!(cents, 0, "2000 paid - 500 - 1500 refunded: {s}");
    // A refund for a customer we do not know is acknowledged.
    let other = ev(
        "ev_ref3",
        "charge.refunded",
        json!({"id": "ch_x", "customer": "cus_unknown",
        "amount_refunded": 100, "refunded": false, "currency": "eur"}),
    );
    assert_eq!(hook(&t, &other).status, 200);
}

// ------------------------------------------------- foreign and duplicates

#[test]
fn foreign_invoices_are_acked_and_duplicate_subscriptions_flagged() {
    let (_st, base) = stripe_mock();
    let t = full_app(&base);
    // Another product on the same Stripe account: 200, not 409 forever.
    let r = hook(
        &t,
        &invoice(
            "ev_foreign",
            None,
            "cus_z",
            "sub_z",
            T0,
            T0 + 30 * DAY,
            "someone_elses_price",
            999,
        ),
    );
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    assert_eq!(json(&r)["applied"], 0);

    // Two checkouts finished for one account (second opened after the
    // first page closed): the first wins, the second is flagged.
    let (c, _) = new_account(&t);
    let r1 = ref_of(&checkout_url(&t, &c, "plus"));
    t.set_now(T0 + 3500);
    let r2 = ref_of(&checkout_url(&t, &c, "plus"));
    assert_ne!(r1, r2);
    assert_eq!(
        hook(&t, &completed("ev_d1", &r1, "cus_d1", "sub_d1")).status,
        200
    );
    assert_eq!(
        hook(
            &t,
            &invoice(
                "ev_di1",
                Some(&r1),
                "cus_d1",
                "sub_d1",
                T0,
                T0 + 30 * DAY,
                "galion_plus_monthly",
                2000
            )
        )
        .status,
        200
    );
    assert_eq!(
        hook(&t, &completed("ev_d2", &r2, "cus_d2", "sub_d2")).status,
        200
    );
    let r = hook(
        &t,
        &invoice(
            "ev_di2",
            Some(&r2),
            "cus_d2",
            "sub_d2",
            T0,
            T0 + 30 * DAY,
            "galion_plus_monthly",
            2000,
        ),
    );
    assert_eq!(r.status, 200);
    let m = me(&t, &c);
    assert_eq!(m["paid_until"], T0 + 31 * DAY, "no time from the duplicate");
    let s = json(&admin(&t, "/internal/v1/admin/status", "admin-test-token"));
    assert_eq!(s["duplicate_subscriptions"], 1);
    assert_eq!(s["duplicate_subscription_ids"], json!(["sub_d2"]));
    assert_eq!(s["subscriptions"], 1);
    // The duplicate's own lifecycle events do not touch the account.
    let del = ev(
        "ev_dd",
        "customer.subscription.deleted",
        json!({"id": "sub_d2", "ended_at": T0 + 3600}),
    );
    assert_eq!(hook(&t, &del).status, 200);
    assert_eq!(me(&t, &c)["renewing"], true);
}

// --------------------------------------------------------------- banking

#[test]
fn cash_time_waits_behind_a_card_subscription() {
    let (_st, base) = stripe_mock();
    let t = full_app(&base);
    let (c, number) = new_account(&t);
    let e1 = T0 + 30 * DAY;
    subscribe(&t, &c, "cus_b", "sub_b", "plus", e1);
    let r = t
        .app
        .handle(&post_json("/api/pay/cash", Some(&c), r#"{"plan":"plus"}"#));
    let cref = json(&r)["reference"].as_str().unwrap().to_string();
    let r = t.app.handle_internal(&req(
        "POST",
        "/internal/v1/cash/credit",
        &[
            ("Authorization", "Bearer admin-test-token"),
            ("Content-Type", "application/json"),
        ],
        json!({"reference": cref, "amount_cents": 4000})
            .to_string()
            .as_bytes(),
    ));
    assert_eq!(r.status, 200);
    let m = me(&t, &c);
    assert_eq!(m["paid_until"], e1 + DAY, "the card period is not touched");
    assert_eq!(m["banked_days"], 60);
    assert_eq!(m["banked_plan"], "plus");
    // Card cancelled and ended at the period end: banked days follow.
    t.set_now(e1);
    let del = ev(
        "ev_bdel",
        "customer.subscription.deleted",
        json!({"id": "sub_b", "ended_at": e1}),
    );
    assert_eq!(hook(&t, &del).status, 200);
    let c = relogin(&t, &number);
    let m = me(&t, &c);
    assert_eq!(m["paid_until"], e1 + 60 * DAY);
    assert_eq!(m["plan"], "plus");
    assert_eq!(m["banked_days"], 0);

    // Prepaid Plus time, then a Pro card subscription: the Plus days are
    // banked at link time and come back after Pro ends.
    t.set_now(T0);
    let (c2, _) = new_account(&t);
    let r = t
        .app
        .handle(&post_json("/api/pay/cash", Some(&c2), r#"{"plan":"plus"}"#));
    let cref = json(&r)["reference"].as_str().unwrap().to_string();
    t.app.handle_internal(&req(
        "POST",
        "/internal/v1/cash/credit",
        &[
            ("Authorization", "Bearer admin-test-token"),
            ("Content-Type", "application/json"),
        ],
        json!({"reference": cref, "amount_cents": 2000})
            .to_string()
            .as_bytes(),
    ));
    subscribe(&t, &c2, "cus_b2", "sub_b2", "pro", e1);
    let m = me(&t, &c2);
    assert_eq!(m["plan"], "pro");
    assert_eq!(m["paid_until"], e1 + DAY);
    assert_eq!(m["banked_days"], 30);
    assert_eq!(m["banked_plan"], "plus");
}

// ------------------------------------------------------ cash slip, admin

#[test]
fn printable_cash_slip_admin_lookup_and_readiness() {
    let (_st, base) = stripe_mock();
    let t = full_app(&base);
    let (c, number) = new_account(&t);
    let r = t
        .app
        .handle(&post_json("/api/pay/cash", Some(&c), r#"{"plan":"pro"}"#));
    assert_eq!(r.status, 200);
    let j = json(&r);
    let cref = j["reference"].as_str().unwrap().to_string();
    let slip = j["slip_url"].as_str().unwrap().to_string();
    assert_eq!(slip, format!("/account/cash-slip?ref={cref}"));
    assert_eq!(me(&t, &c)["cash_refs"][0]["reference"], cref.as_str());

    let page = t.app.handle(&req("GET", &slip, &[("Cookie", &c)], b""));
    assert_eq!(page.status, 200);
    let csp = header(&page, "content-security-policy").unwrap();
    assert!(csp.contains("default-src 'none'") && !csp.contains("script"));
    let html = String::from_utf8(page.body.clone()).unwrap();
    assert!(html.contains(&cref));
    assert!(html.contains("€220"));
    assert!(
        html.contains("Galion &lt;Box&gt;<br>PO Box 1<br>10000 Zagreb"),
        "address escaped"
    );
    assert!(!html.contains(&number) && !html.contains(&number.replace(' ', "")));
    assert!(!html.contains("<script"));
    // Someone else's session, no session, junk: all the same 404.
    let (c2, _) = new_account(&t);
    assert_eq!(
        t.app
            .handle(&req("GET", &slip, &[("Cookie", &c2)], b""))
            .status,
        404
    );
    assert_eq!(t.app.handle(&req("GET", &slip, &[], b"")).status, 404);
    assert_eq!(
        t.app
            .handle(&req(
                "GET",
                "/account/cash-slip?ref=<x>",
                &[("Cookie", &c)],
                b""
            ))
            .status,
        404
    );

    // Operator lookup: plan and amount, never the account.
    let path = format!("/internal/v1/cash/lookup?ref={}", cref.to_lowercase());
    assert_eq!(admin(&t, &path, "internal-test-token").status, 401);
    let r = admin(&t, &path, "admin-test-token");
    assert_eq!(r.status, 200);
    let l = json(&r);
    assert_eq!(l["plan"], "pro");
    assert_eq!(l["eur_cents_per_month"], 22000);
    assert!(l.get("account").is_none() && l.get("account_id").is_none());
    // After the credit the reference is gone.
    t.app.handle_internal(&req(
        "POST",
        "/internal/v1/cash/credit",
        &[
            ("Authorization", "Bearer admin-test-token"),
            ("Content-Type", "application/json"),
        ],
        json!({"reference": cref, "amount_cents": 22000})
            .to_string()
            .as_bytes(),
    ));
    assert_eq!(admin(&t, &path, "admin-test-token").status, 404);
    assert_eq!(
        t.app
            .handle(&req("GET", &slip, &[("Cookie", &c)], b""))
            .status,
        404
    );

    // Readiness: booleans only, no token needed, no values.
    let r = t
        .app
        .handle_internal(&req("GET", "/internal/v1/ready", &[], b""));
    assert_eq!(r.status, 200);
    let body = String::from_utf8(r.body.clone()).unwrap();
    let v = json(&r);
    assert_eq!(v["stripe"]["configured"], true);
    assert_eq!(v["stripe"]["mode"], "test");
    assert_eq!(v["stripe"]["portal_config"], true);
    assert_eq!(v["cash"]["configured"], true);
    assert_eq!(v["btcpay"]["configured"], false);
    for secret in [
        SK,
        WHSEC,
        "admin-test-token",
        "internal-test-token",
        "Zagreb",
        "bpc_1",
    ] {
        assert!(!body.contains(secret), "{secret} leaked");
    }
    // Admin status needs the admin token.
    assert_eq!(
        admin(&t, "/internal/v1/admin/status", "internal-test-token").status,
        401
    );
    let s = json(&admin(&t, "/internal/v1/admin/status", "admin-test-token"));
    assert_eq!(s["accounts"], 2);
    assert_eq!(s["active_pro"], 1);
    let s_text = s.to_string();
    assert!(!s_text.contains(SK) && !s_text.contains(WHSEC));
}

// ------------------------------------------------------------------- CLI

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_galion-accounts")
}

fn tmpdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("galion-pay-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn cli_readiness_and_setup_secret_file() {
    let (st, base) = stripe_mock();
    let d = tmpdir("cli");
    let envf = d.join("accounts.env");
    std::fs::write(
        &envf,
        format!(
            "# test env\nSTRIPE_SECRET_KEY={SK}\nSTRIPE_WEBHOOK_SECRET=\"{WHSEC}\"\nSTRIPE_API_BASE={base}\n\
             ACCOUNTS_PUBLIC_URL=https://galion.app\nACCOUNTS_CASH_ADDRESS=Galion|PO Box 1|10000 Zagreb\n\
             ACCOUNTS_INTERNAL_TOKEN={}\nACCOUNTS_ADMIN_TOKEN=\n",
            "i".repeat(40)
        ),
    )
    .unwrap();
    let pepper = d.join("pepper");
    std::fs::write(&pepper, "ab".repeat(32)).unwrap();
    let db = d.join("a.sqlite");
    drop(Store::open(&db).unwrap());
    let before: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();

    let out = std::process::Command::new(bin())
        .args(["readiness", "--env-file"])
        .arg(&envf)
        .arg("--pepper-file")
        .arg(&pepper)
        .arg("--db")
        .arg(&db)
        .args(["--require", "stripe,cash"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("ok   method stripe") && text.contains("ok   method cash"),
        "{text}"
    );
    assert!(text.contains("MISS ACCOUNTS_ADMIN_TOKEN"));
    assert!(text.contains("ok   database opens (read-only)"), "{text}");
    assert!(text.contains("READY"));
    let mut after: Vec<String> = std::fs::read_dir(&d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let mut before = before;
    before.sort();
    after.sort();
    assert_eq!(
        before, after,
        "readiness created no files next to the database"
    );
    for s in [
        SK,
        WHSEC,
        "ab".repeat(32).as_str(),
        "Zagreb",
        &"i".repeat(40),
    ] {
        assert!(!text.contains(s), "value printed");
    }
    // Requiring a method that is off fails.
    let out = std::process::Command::new(bin())
        .args(["readiness", "--env-file"])
        .arg(&envf)
        .arg("--pepper-file")
        .arg(&pepper)
        .arg("--db")
        .arg(&db)
        .args(["--require", "monero"])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    // --apply with a webhook URL but no --secret-out: refused before any call.
    let n = mark(&st);
    let out = std::process::Command::new(bin())
        .args([
            "stripe-setup",
            "--apply",
            "--webhook-url",
            "https://galion.app/webhooks/stripe",
            "--env-file",
        ])
        .arg(&envf)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(since(&st, n).len(), 0);

    // With it: the secret lands in a 0600 file, stdout never has it.
    let secret = d.join("whsec.txt");
    let out = std::process::Command::new(bin())
        .args([
            "stripe-setup",
            "--apply",
            "--webhook-url",
            "https://galion.app/webhooks/stripe",
            "--env-file",
        ])
        .arg(&envf)
        .arg("--secret-out")
        .arg(&secret)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!stdout.contains("whsec_mock_created_once"));
    assert!(stdout.contains("created webhook endpoint we_1"));
    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "STRIPE_WEBHOOK_SECRET=whsec_mock_created_once\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&secret).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // The file is never overwritten.
    let out = std::process::Command::new(bin())
        .args([
            "stripe-setup",
            "--apply",
            "--webhook-url",
            "https://galion.app/webhooks/stripe",
            "--env-file",
        ])
        .arg(&envf)
        .arg("--secret-out")
        .arg(&secret)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    // Leave the temp dir for inspection; it is under the OS temp dir.
    let _ = std::fs::rename(&d, d.with_extension("done"));
}
