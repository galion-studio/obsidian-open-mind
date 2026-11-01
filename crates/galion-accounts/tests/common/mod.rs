// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Shared test helpers. No network: Stripe is a MockTransport or a local
//! loopback server.
#![allow(dead_code)]

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use galion_accounts::hash::{Cost, Hasher};
use galion_accounts::pay::cash::{Cash, CashConfig};
use galion_accounts::pay::crypto::Crypto;
use galion_accounts::pay::stripe::{Stripe, StripeConfig};
use galion_accounts::pay::transport::{ApiRequest, ApiResponse, MockTransport, Transport};
use galion_accounts::ratelimit::Limiter;
use galion_accounts::server::{App, Settings};
use galion_accounts::store::Store;
use galion_common::http::{parse_head, Limits, Request, Response};

pub const WHSEC: &str = "whsec_test_galion_fixed_vector_secret";
pub const SK: &str = "sk_test_galion_mock_not_a_real_key";
pub const T0: i64 = 1_761_955_200; // 2025-11-01 01:00 CET

pub fn stripe_vars(base: Option<&str>) -> impl Fn(&str) -> Option<String> {
    let base = base.map(str::to_string);
    move |k: &str| match k {
        "STRIPE_SECRET_KEY" => Some(SK.into()),
        "STRIPE_WEBHOOK_SECRET" => Some(WHSEC.into()),
        "STRIPE_API_BASE" => base.clone(),
        "ACCOUNTS_PUBLIC_URL" => Some("https://galion.app".into()),
        _ => None,
    }
}

/// A canned Stripe API: prices by lookup_key, checkout and portal sessions.
pub fn fake_stripe(req: &ApiRequest) -> ApiResponse {
    let path = req
        .url
        .split("://")
        .nth(1)
        .and_then(|r| r.find('/').map(|i| &r[i..]))
        .unwrap_or("");
    let path = path.split('?').next().unwrap_or("");
    let form = |k: &str| {
        req.form
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
    };
    let body = match (req.method, path) {
        ("GET", "/v1/prices") => match form("lookup_keys[]").as_deref() {
            Some("galion_plus_monthly") => r#"{"object":"list","data":[{"id":"price_plus_mock","lookup_key":"galion_plus_monthly"}]}"#.to_string(),
            Some("galion_pro_monthly") => r#"{"object":"list","data":[{"id":"price_pro_mock","lookup_key":"galion_pro_monthly"}]}"#.to_string(),
            _ => r#"{"object":"list","data":[]}"#.to_string(),
        },
        ("POST", "/v1/checkout/sessions") => {
            let r = form("client_reference_id").unwrap_or_default();
            format!(r#"{{"id":"cs_test_mock","url":"https://checkout.stripe.com/c/pay/cs_test_mock#{r}"}}"#)
        }
        ("POST", "/v1/billing_portal/sessions") => {
            r#"{"id":"bps_mock","url":"https://billing.stripe.com/p/session/mock"}"#.to_string()
        }
        _ => {
            return ApiResponse {
                status: 404,
                body: r#"{"error":{"message":"no such route in the mock"}}"#.into(),
            }
        }
    };
    ApiResponse { status: 200, body }
}

pub fn mock_transport() -> Arc<MockTransport> {
    Arc::new(MockTransport {
        seen: Default::default(),
        reply: Some(Box::new(fake_stripe)),
    })
}

pub struct TestApp {
    pub app: Arc<App>,
    pub clock: Arc<AtomicI64>,
}

impl TestApp {
    pub fn set_now(&self, t: i64) {
        self.clock.store(t, Ordering::SeqCst);
    }
}

pub fn app(stripe_on: bool, cash_on: bool, transport: Arc<dyn Transport>) -> TestApp {
    app_with_crypto(stripe_on, cash_on, transport, Crypto::disabled())
}

/// Same as [`app`] with a given crypto adapter.
pub fn app_with_crypto(
    stripe_on: bool,
    cash_on: bool,
    transport: Arc<dyn Transport>,
    crypto: Crypto,
) -> TestApp {
    let clock = Arc::new(AtomicI64::new(T0));
    let c2 = clock.clone();
    let cfg = if stripe_on {
        StripeConfig::from_vars(stripe_vars(None))
    } else {
        StripeConfig::from_vars(|_| None)
    };
    let cash_cfg = CashConfig::from_vars(|k| {
        (cash_on && k == "ACCOUNTS_CASH_ADDRESS")
            .then(|| "Galion|PO Box 1|10000 Zagreb".to_string())
    });
    let app = App {
        store: Store::in_memory().unwrap(),
        hasher: Hasher::new(vec![7u8; 32], Cost::TEST).unwrap(),
        limiter: Limiter::new(),
        stripe: Stripe::new(cfg, transport),
        cash: Cash { cfg: cash_cfg },
        crypto,
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

/// Build a Request as the server would parse it.
pub fn req(method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Request {
    let mut raw = format!("{method} {path} HTTP/1.1\r\nHost: galion.app\r\n");
    for (k, v) in headers {
        raw.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() {
        raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    raw.push_str("\r\n");
    let head = parse_head(raw.as_bytes(), &Limits::default()).expect("valid head");
    Request {
        head,
        raw_head: raw.into_bytes(),
        body: body.to_vec(),
        peer: Some("192.0.2.10:5555".parse().unwrap()),
    }
}

pub fn json(r: &Response) -> serde_json::Value {
    serde_json::from_slice(&r.body).unwrap_or(serde_json::Value::Null)
}

pub fn header<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
    galion_common::http::header(&r.headers, name)
}

/// Session cookie value from a Set-Cookie header.
pub fn cookie_of(r: &Response) -> String {
    let sc = header(r, "set-cookie").expect("set-cookie");
    let v = sc.split(';').next().unwrap();
    v.to_string()
}

pub fn post_json(path: &str, cookie: Option<&str>, body: &str) -> Request {
    let mut h = vec![
        ("Content-Type", "application/json"),
        ("Origin", "https://galion.app"),
    ];
    if let Some(c) = cookie {
        h.push(("Cookie", c));
    }
    req("POST", path, &h, body.as_bytes())
}
