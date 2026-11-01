// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The real curl transport against a LOCAL mock of the Stripe REST API on
//! 127.0.0.1:19972 (no network, no real keys). Checks what goes over the
//! wire: bearer key, Idempotency-Key, Stripe-Version, form fields.

mod common;

use std::sync::{Arc, Mutex, OnceLock};

use common::*;
use galion_accounts::pay::stripe::{Stripe, StripeConfig, API_VERSION};
use galion_accounts::pay::transport::CurlTransport;
use galion_accounts::pay::{PayError, StartRequest};
use galion_accounts::plans::Plan;
use galion_common::http::{self, Limits, Request, Response};

const PORT: u16 = 19972;

#[derive(Clone, Debug)]
struct Seen {
    method: String,
    path: String,
    query: String,
    auth: String,
    idem: Option<String>,
    version: Option<String>,
    body: String,
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

fn mock() -> &'static Mutex<Vec<Seen>> {
    static SEEN: OnceLock<Arc<Mutex<Vec<Seen>>>> = OnceLock::new();
    SEEN.get_or_init(|| {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let listener = std::net::TcpListener::bind(("127.0.0.1", PORT)).expect("port 19972 free");
        let h: Arc<http::Handler> = Arc::new(move |r: &Request| {
            let body = String::from_utf8_lossy(&r.body).into_owned();
            let rec = Seen {
                method: r.method().into(),
                path: r.path().into(),
                query: r.query().into(),
                auth: r.header("authorization").unwrap_or("").into(),
                idem: r.header("idempotency-key").map(str::to_string),
                version: r.header("stripe-version").map(str::to_string),
                body: body.clone(),
            };
            s2.lock().unwrap().push(rec.clone());
            if rec.auth != format!("Bearer {SK}") {
                return Response::json(401, r#"{"error":{"message":"Invalid API Key provided"}}"#);
            }
            let f = decode(if rec.method == "GET" { &rec.query } else { &rec.body });
            let get = |k: &str| f.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()).unwrap_or_default();
            match (rec.method.as_str(), rec.path.as_str()) {
                ("GET", "/v1/prices") => {
                    if get("lookup_keys[]") == "galion_plus_monthly" && get("active") == "true" {
                        Response::json(200, r#"{"object":"list","data":[{"id":"price_plus_http"}]}"#)
                    } else {
                        Response::json(200, r#"{"object":"list","data":[]}"#)
                    }
                }
                ("POST", "/v1/checkout/sessions") => {
                    if get("line_items[0][price]") == "price_pro_none" {
                        return Response::json(402, r#"{"error":{"message":"No such price: 'price_pro_none'"}}"#);
                    }
                    Response::json(
                        200,
                        format!(r#"{{"id":"cs_http","url":"https://checkout.stripe.com/c/pay/cs_http#{}"}}"#, get("client_reference_id")),
                    )
                }
                ("POST", "/v1/billing_portal/sessions") => {
                    Response::json(200, r#"{"url":"https://billing.stripe.com/p/session/http"}"#)
                }
                ("POST", "/v1/products") => Response::json(200, format!(r#"{{"id":"prod_{}"}}"#, get("metadata[galion_plan]"))),
                ("POST", "/v1/prices") => Response::json(200, format!(r#"{{"id":"price_{}_new"}}"#, get("metadata[galion_plan]"))),
                ("POST", "/v1/webhook_endpoints") => {
                    Response::json(200, r#"{"id":"we_http","secret":"whsec_from_mock"}"#)
                }
                _ => Response::json(404, r#"{"error":{"message":"unknown route"}}"#),
            }
        });
        std::thread::spawn(move || {
            let _ = http::serve(listener, Limits::default(), h);
        });
        // Leak into a 'static for the tests' lifetime.
        seen
    })
    .as_ref()
}

fn client(key: Option<&str>) -> Stripe {
    let base = format!("http://127.0.0.1:{PORT}");
    let key = key.map(str::to_string);
    let cfg = StripeConfig::from_vars(move |k| match k {
        "STRIPE_SECRET_KEY" => key.clone(),
        "STRIPE_WEBHOOK_SECRET" => Some(WHSEC.into()),
        "STRIPE_API_BASE" => Some(base.clone()),
        "STRIPE_PRICE_PRO" => Some("price_pro_none".into()),
        _ => None,
    });
    Stripe::new(cfg, Arc::new(CurlTransport::default()))
}

/// The mock is shared; tests serialise on this lock and look only at
/// requests made after they start.
static SERIAL: Mutex<()> = Mutex::new(());

fn since(n: usize) -> Vec<Seen> {
    mock().lock().unwrap()[n..].to_vec()
}

#[test]
fn checkout_and_portal_over_curl() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let start = mock().lock().unwrap().len();
    let s = client(Some(SK));
    let url = s
        .create_checkout(&StartRequest {
            pay_ref: "gp_TESTREF0000000000000000000".into(),
            plan: Plan::Plus,
            existing_customer: None,
        })
        .expect("checkout");
    assert_eq!(
        url,
        "https://checkout.stripe.com/c/pay/cs_http#gp_TESTREF0000000000000000000"
    );
    let seen = since(start);
    assert_eq!(seen.len(), 2);
    assert_eq!(
        (seen[0].method.as_str(), seen[0].path.as_str()),
        ("GET", "/v1/prices")
    );
    assert!(seen[0]
        .query
        .contains("lookup_keys%5B%5D=galion_plus_monthly"));
    let co = &seen[1];
    assert_eq!(co.path, "/v1/checkout/sessions");
    assert_eq!(
        co.idem.as_deref(),
        Some("galion-checkout-gp_TESTREF0000000000000000000")
    );
    assert_eq!(co.version.as_deref(), Some(API_VERSION));
    let f = decode(&co.body);
    let get = |k: &str| f.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(get("mode"), Some("subscription"));
    assert_eq!(get("line_items[0][price]"), Some("price_plus_http"));
    assert_eq!(
        get("metadata[galion_ref]"),
        Some("gp_TESTREF0000000000000000000")
    );
    assert_eq!(
        get("cancel_url"),
        Some("https://galion.app/account?checkout=cancelled")
    );
    assert!(f.iter().all(|(k, _)| !k.contains("email")));

    // The price id is cached: a second checkout makes one call only.
    let n = mock().lock().unwrap().len();
    s.create_checkout(&StartRequest {
        pay_ref: "gp_TESTREF0000000000000000001".into(),
        plan: Plan::Plus,
        existing_customer: Some("cus_old".into()),
    })
    .unwrap();
    let seen = since(n);
    assert_eq!(seen.len(), 1);
    assert!(decode(&seen[0].body).contains(&("customer".into(), "cus_old".into())));

    let p = s.create_portal("cus_old").unwrap();
    assert_eq!(p, "https://billing.stripe.com/p/session/http");
}

#[test]
fn api_errors_surface_without_leaking_the_key() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _ = mock(); // start the local mock first
    let s = client(Some(SK));
    // Configured price id that Stripe does not know -> 402 from the mock.
    match s.create_checkout(&StartRequest {
        pay_ref: "gp_X".into(),
        plan: Plan::Pro,
        existing_customer: None,
    }) {
        Err(PayError::Api(402, m)) => assert!(m.contains("No such price")),
        other => panic!("{other:?}"),
    }
    // Wrong key -> 401, message passed on, key not in it.
    let bad = client(Some("sk_test_wrong"));
    match bad.create_portal("cus_1") {
        Err(PayError::Api(401, m)) => assert!(!m.contains("sk_test_wrong")),
        other => panic!("{other:?}"),
    }
    // Nothing listening -> transport error, not a panic.
    let cfg = StripeConfig::from_vars(|k| match k {
        "STRIPE_SECRET_KEY" => Some(SK.into()),
        "STRIPE_WEBHOOK_SECRET" => Some(WHSEC.into()),
        "STRIPE_API_BASE" => Some("http://127.0.0.1:19979".into()),
        _ => None,
    });
    let dead = Stripe::new(cfg, Arc::new(CurlTransport::default()));
    assert!(matches!(
        dead.create_portal("cus_1"),
        Err(PayError::Transport(_))
    ));
}

#[test]
fn setup_dry_run_sends_nothing_and_apply_creates_eur_prices() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let start = mock().lock().unwrap().len();
    // Dry run: works without any key and makes no request.
    let dry = client(None)
        .setup(false, Some("https://galion.app/webhooks/stripe"))
        .unwrap();
    assert_eq!(since(start).len(), 0);
    assert!(dry
        .iter()
        .any(|l| l.contains("would create plus") && l.contains("EUR 20.00/month")));
    assert!(dry
        .iter()
        .any(|l| l.contains("would create pro") && l.contains("EUR 220.00/month")));
    assert!(dry
        .iter()
        .any(|l| l.contains("would create webhook endpoint")));
    // --apply without a key refuses.
    assert!(matches!(
        client(None).setup(true, None),
        Err(PayError::NotConnected(_))
    ));

    // Apply against the mock: plus exists? The mock only lists plus when
    // active=true is asked, and setup asks without it, so both get created.
    let out = client(Some(SK))
        .setup(true, Some("https://galion.app/webhooks/stripe"))
        .unwrap();
    let seen = since(start);
    let prices: Vec<_> = seen
        .iter()
        .filter(|s| s.method == "POST" && s.path == "/v1/prices")
        .collect();
    assert_eq!(prices.len(), 2);
    for (p, (cents, lk)) in prices.iter().zip([
        ("2000", "galion_plus_monthly"),
        ("22000", "galion_pro_monthly"),
    ]) {
        let f = decode(&p.body);
        assert!(f.contains(&("currency".into(), "eur".into())));
        assert!(f.contains(&("unit_amount".into(), cents.into())));
        assert!(f.contains(&("recurring[interval]".into(), "month".into())));
        assert!(f.contains(&("lookup_key".into(), lk.into())));
        assert!(f.contains(&("tax_behavior".into(), "inclusive".into())));
        assert_eq!(
            p.idem.as_deref(),
            Some(format!("galion-setup-price-{lk}").as_str())
        );
    }
    let wh = seen
        .iter()
        .find(|s| s.path == "/v1/webhook_endpoints")
        .expect("webhook created");
    assert!(wh
        .idem
        .as_deref()
        .is_some_and(|k| k.starts_with("galion-setup-webhook-")));
    let f = decode(&wh.body);
    for e in [
        "checkout.session.completed",
        "invoice.paid",
        "customer.subscription.updated",
        "customer.subscription.deleted",
    ] {
        assert!(
            f.iter()
                .any(|(k, v)| k.starts_with("enabled_events[") && v == e),
            "{e}"
        );
    }
    assert!(out.iter().any(|l| l.starts_with("created plus")));
    assert!(out
        .iter()
        .any(|l| l == "STRIPE_WEBHOOK_SECRET=whsec_from_mock"));
}
