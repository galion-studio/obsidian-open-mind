// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Stripe, over its REST API (no SDK): Checkout Sessions (mode=subscription,
//! EUR prices), Customer Portal sessions, webhook signature checks and the
//! four events that move paid-until:
//!
//! * `checkout.session.completed` -> link customer + subscription to the
//!   account behind the opaque `client_reference_id`;
//! * `invoice.paid` -> paid_until = period end (+ 1 day grace);
//! * `customer.subscription.updated` -> plan change (time moves only on a
//!   paid invoice);
//! * `customer.subscription.deleted` -> time stops at `ended_at`, Stripe ids
//!   forgotten.
//!
//! Stripe only ever gets the opaque reference (`client_reference_id`,
//! `metadata[galion_ref]`), never the account number, and no email is sent.
//!
//! Signature scheme (https://docs.stripe.com/webhooks#verify-manually):
//! header `Stripe-Signature: t=<unix>,v1=<hex>[,v1=...][,v0=...]`;
//! `v1 = hex(HMAC-SHA256(endpoint_secret, "<t>.<raw body>"))`, compared in
//! constant time; `t` must be within the tolerance (default 300 s).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::transport::{ApiRequest, Transport};
use super::{
    Method, PayError, PayEvent, PaymentAdapter, StartOutcome, StartRequest, Status, Webhook,
};
use crate::plans::Plan;

pub const DEFAULT_API_BASE: &str = "https://api.stripe.com";
pub const DEFAULT_TOLERANCE: i64 = 300;
/// Pinned so request/response shapes do not drift with the account default.
/// Webhook payloads follow the endpoint's own version; the parser accepts
/// both the pre-2025 and the 2025+ ("basil") shapes.
pub const API_VERSION: &str = "2025-03-31.basil";
/// Events the webhook endpoint must subscribe to.
pub const EVENTS: [&str; 4] = [
    "checkout.session.completed",
    "invoice.paid",
    "customer.subscription.updated",
    "customer.subscription.deleted",
];

/// Stripe configuration (from the environment).
#[derive(Clone, Default)]
pub struct StripeConfig {
    pub secret_key: Option<String>,
    pub webhook_secret: Option<String>,
    /// Price ids from config; when absent the price is looked up by
    /// `lookup_key` (galion_plus_monthly / galion_pro_monthly).
    pub price_plus: Option<String>,
    pub price_pro: Option<String>,
    pub api_base: String,
    pub tolerance: i64,
    /// Public site, for success/cancel/return URLs (e.g. https://galion.app).
    pub public_url: String,
    /// Let Stripe compute VAT (needs Stripe Tax and a billing address).
    pub automatic_tax: bool,
}

impl std::fmt::Debug for StripeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StripeConfig")
            .field("secret_key", &self.secret_key.as_ref().map(|_| "<set>"))
            .field(
                "webhook_secret",
                &self.webhook_secret.as_ref().map(|_| "<set>"),
            )
            .field("price_plus", &self.price_plus)
            .field("price_pro", &self.price_pro)
            .field("api_base", &self.api_base)
            .field("tolerance", &self.tolerance)
            .field("public_url", &self.public_url)
            .field("automatic_tax", &self.automatic_tax)
            .finish()
    }
}

fn nonempty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

impl StripeConfig {
    /// Read STRIPE_SECRET_KEY, STRIPE_WEBHOOK_SECRET, STRIPE_PRICE_PLUS,
    /// STRIPE_PRICE_PRO, STRIPE_API_BASE, STRIPE_WEBHOOK_TOLERANCE_SECS,
    /// STRIPE_AUTOMATIC_TAX and ACCOUNTS_PUBLIC_URL through `get`.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> StripeConfig {
        StripeConfig {
            secret_key: nonempty(get("STRIPE_SECRET_KEY")),
            webhook_secret: nonempty(get("STRIPE_WEBHOOK_SECRET")),
            price_plus: nonempty(get("STRIPE_PRICE_PLUS")),
            price_pro: nonempty(get("STRIPE_PRICE_PRO")),
            api_base: nonempty(get("STRIPE_API_BASE"))
                .unwrap_or_else(|| DEFAULT_API_BASE.into())
                .trim_end_matches('/')
                .to_string(),
            tolerance: nonempty(get("STRIPE_WEBHOOK_TOLERANCE_SECS"))
                .and_then(|s| s.parse().ok())
                .filter(|&t: &i64| t > 0)
                .unwrap_or(DEFAULT_TOLERANCE),
            public_url: nonempty(get("ACCOUNTS_PUBLIC_URL"))
                .unwrap_or_else(|| "https://galion.app".into())
                .trim_end_matches('/')
                .to_string(),
            automatic_tax: matches!(
                nonempty(get("STRIPE_AUTOMATIC_TAX")).as_deref(),
                Some("1" | "true" | "yes")
            ),
        }
    }

    pub fn from_env() -> StripeConfig {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    /// Connected only with both the API key and the webhook secret.
    pub fn status(&self) -> Status {
        match (&self.secret_key, &self.webhook_secret) {
            (Some(k), Some(w)) => {
                if !(k.starts_with("sk_") || k.starts_with("rk_")) {
                    Status::NotConnected("STRIPE_SECRET_KEY does not look like a Stripe secret or restricted key (sk_/rk_)".into())
                } else if !w.starts_with("whsec_") {
                    Status::NotConnected("STRIPE_WEBHOOK_SECRET does not look like a webhook signing secret (whsec_)".into())
                } else {
                    Status::Connected
                }
            }
            (None, None) => Status::NotConnected(
                "Stripe is not connected: set STRIPE_SECRET_KEY and STRIPE_WEBHOOK_SECRET".into(),
            ),
            (None, _) => {
                Status::NotConnected("Stripe is not connected: STRIPE_SECRET_KEY is not set".into())
            }
            (_, None) => Status::NotConnected(
                "Stripe is not connected: STRIPE_WEBHOOK_SECRET is not set".into(),
            ),
        }
    }

    fn configured_price(&self, plan: Plan) -> Option<&str> {
        match plan {
            Plan::Plus => self.price_plus.as_deref(),
            Plan::Pro => self.price_pro.as_deref(),
            Plan::Free => None,
        }
    }
}

// ------------------------------------------------------------- signatures

/// Why a `Stripe-Signature` was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigError {
    Missing,
    Malformed,
    NoV1,
    OutsideTolerance,
    Mismatch,
}

impl SigError {
    pub fn as_str(self) -> &'static str {
        match self {
            SigError::Missing => "missing Stripe-Signature header",
            SigError::Malformed => "malformed Stripe-Signature header",
            SigError::NoV1 => "no v1 signature",
            SigError::OutsideTolerance => "timestamp outside tolerance",
            SigError::Mismatch => "no signature matches",
        }
    }
}

/// `v1` signature for `payload` at `t` (what Stripe sends). Public so tests
/// and the local mock can sign.
pub fn sign(secret: &str, t: i64, payload: &[u8]) -> String {
    let mut msg = format!("{t}.").into_bytes();
    msg.extend_from_slice(payload);
    crate::hmac::hmac_sha256_hex(secret.as_bytes(), &msg)
}

/// Check a `Stripe-Signature` header against the raw body.
pub fn verify_signature(
    header: Option<&str>,
    payload: &[u8],
    secret: &str,
    tolerance: i64,
    now: i64,
) -> Result<i64, SigError> {
    let h = header.ok_or(SigError::Missing)?;
    let mut t: Option<i64> = None;
    let mut v1: Vec<&str> = Vec::new();
    for part in h.split(',') {
        let (k, v) = part.trim().split_once('=').ok_or(SigError::Malformed)?;
        match k {
            "t" => t = Some(v.parse().map_err(|_| SigError::Malformed)?),
            "v1" => v1.push(v),
            _ => {} // v0 (test-mode legacy) and future schemes are ignored
        }
    }
    let t = t.ok_or(SigError::Malformed)?;
    if v1.is_empty() {
        return Err(SigError::NoV1);
    }
    if (now - t).abs() > tolerance {
        return Err(SigError::OutsideTolerance);
    }
    let want = sign(secret, t, payload);
    // Every candidate is compared (no early exit) in constant time.
    let mut ok = false;
    for s in v1 {
        ok |= crate::ct_eq(s.as_bytes(), want.as_bytes());
    }
    if ok {
        Ok(t)
    } else {
        Err(SigError::Mismatch)
    }
}

// ------------------------------------------------------------- events

fn s(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = match p.parse::<usize>() {
            Ok(i) => cur.get(i)?,
            Err(_) => cur.get(*p)?,
        };
    }
    match cur {
        Value::String(x) if !x.is_empty() => Some(x.clone()),
        // Expanded objects: take their id.
        Value::Object(o) => o.get("id").and_then(|i| i.as_str()).map(str::to_string),
        _ => None,
    }
}

fn n(v: &Value, path: &[&str]) -> Option<i64> {
    let mut cur = v;
    for p in path {
        cur = match p.parse::<usize>() {
            Ok(i) => cur.get(i)?,
            Err(_) => cur.get(*p)?,
        };
    }
    cur.as_i64()
}

impl StripeConfig {
    /// Map a Stripe Price (id or lookup_key) to our plan.
    pub fn plan_for_price(&self, price_id: Option<&str>, lookup_key: Option<&str>) -> Option<Plan> {
        for p in Plan::PAID {
            if lookup_key.is_some() && lookup_key == p.stripe_lookup_key() {
                return Some(p);
            }
            if price_id.is_some() && price_id == self.configured_price(p) {
                return Some(p);
            }
        }
        None
    }
}

/// Turn a verified Stripe event body into [`PayEvent`]s.
pub fn parse_event(cfg: &StripeConfig, ev: &Value, now: i64) -> Result<Webhook, PayError> {
    let id = s(ev, &["id"]).ok_or_else(|| PayError::BadPayload("event without id".into()))?;
    let kind = s(ev, &["type"]).unwrap_or_default();
    let o = ev
        .get("data")
        .and_then(|d| d.get("object"))
        .ok_or_else(|| PayError::BadPayload("event without data.object".into()))?;
    let mut events = Vec::new();
    match kind.as_str() {
        "checkout.session.completed" => {
            let pay_ref =
                s(o, &["client_reference_id"]).or_else(|| s(o, &["metadata", "galion_ref"]));
            // Only our own sessions; a paid subscription checkout.
            if let Some(pay_ref) = pay_ref.filter(|r| r.starts_with("gp_")) {
                if s(o, &["mode"]).as_deref() == Some("subscription") {
                    events.push(PayEvent::Linked {
                        pay_ref,
                        customer: s(o, &["customer"]),
                        subscription: s(o, &["subscription"]),
                    });
                }
            }
        }
        "invoice.paid" => {
            let subscription = s(o, &["subscription"])
                .or_else(|| s(o, &["parent", "subscription_details", "subscription"]));
            let pay_ref = s(o, &["subscription_details", "metadata", "galion_ref"]).or_else(|| {
                s(
                    o,
                    &["parent", "subscription_details", "metadata", "galion_ref"],
                )
            });
            // The latest period end over the invoice lines.
            let mut end: Option<i64> = None;
            let mut plan = None;
            if let Some(lines) = o.pointer("/lines/data").and_then(|l| l.as_array()) {
                for l in lines {
                    if let Some(e) = n(l, &["period", "end"]) {
                        end = Some(end.map_or(e, |x: i64| x.max(e)));
                    }
                    let price = s(l, &["price", "id"])
                        .or_else(|| s(l, &["pricing", "price_details", "price"]));
                    let lk = s(l, &["price", "lookup_key"]);
                    plan = plan.or_else(|| cfg.plan_for_price(price.as_deref(), lk.as_deref()));
                }
            }
            let end = end.or_else(|| n(o, &["period_end"]));
            if subscription.is_some() || pay_ref.is_some() {
                let Some(end) = end else {
                    return Err(PayError::BadPayload(
                        "invoice.paid without a period end".into(),
                    ));
                };
                events.push(PayEvent::Renewed {
                    subscription,
                    customer: s(o, &["customer"]),
                    pay_ref: pay_ref.filter(|r| r.starts_with("gp_")),
                    plan,
                    paid_until: end,
                    amount_cents: n(o, &["amount_paid"]).unwrap_or(0),
                    currency: s(o, &["currency"]).unwrap_or_default().to_ascii_uppercase(),
                });
            }
        }
        "customer.subscription.updated" => {
            let status = s(o, &["status"]).unwrap_or_default();
            if let (Some(sub), true) = (
                s(o, &["id"]),
                matches!(status.as_str(), "active" | "trialing"),
            ) {
                let price = s(o, &["items", "data", "0", "price", "id"]);
                let lk = s(o, &["items", "data", "0", "price", "lookup_key"]);
                if let Some(plan) = cfg.plan_for_price(price.as_deref(), lk.as_deref()) {
                    events.push(PayEvent::PlanChanged {
                        subscription: sub,
                        plan,
                    });
                }
            }
        }
        "customer.subscription.deleted" => {
            if let Some(sub) = s(o, &["id"]) {
                let at = n(o, &["ended_at"])
                    .or_else(|| n(o, &["canceled_at"]))
                    .unwrap_or(now);
                events.push(PayEvent::Ended {
                    subscription: sub,
                    at,
                });
            }
        }
        _ => {}
    }
    Ok(Webhook {
        id: Some(id),
        kind,
        events,
    })
}

// ------------------------------------------------------------- client

/// Stripe REST client + [`PaymentAdapter`].
pub struct Stripe {
    pub cfg: StripeConfig,
    transport: Arc<dyn Transport>,
    price_cache: Mutex<HashMap<&'static str, String>>,
}

impl Stripe {
    pub fn new(cfg: StripeConfig, transport: Arc<dyn Transport>) -> Stripe {
        Stripe {
            cfg,
            transport,
            price_cache: Mutex::new(HashMap::new()),
        }
    }

    fn key(&self) -> Result<&str, PayError> {
        match self.cfg.status() {
            Status::Connected => Ok(self.cfg.secret_key.as_deref().unwrap_or_default()),
            Status::NotConnected(r) => Err(PayError::NotConnected(r)),
        }
    }

    /// Key only (setup does not need the webhook secret yet).
    fn key_only(&self) -> Result<&str, PayError> {
        match self.cfg.secret_key.as_deref() {
            Some(k) if k.starts_with("sk_") || k.starts_with("rk_") => Ok(k),
            _ => Err(PayError::NotConnected(
                "STRIPE_SECRET_KEY is not set".into(),
            )),
        }
    }

    fn call(
        &self,
        key: &str,
        method: &'static str,
        path: &str,
        form: Vec<(String, String)>,
        idem: Option<String>,
    ) -> Result<Value, PayError> {
        let req = ApiRequest {
            method,
            url: format!("{}{}", self.cfg.api_base, path),
            bearer: key.to_string(),
            idempotency_key: idem,
            stripe_version: Some(API_VERSION.into()),
            form,
        };
        let resp = self.transport.send(&req)?;
        let v: Value = serde_json::from_str(&resp.body)
            .map_err(|_| PayError::Api(resp.status, "response is not JSON".into()))?;
        if !(200..300).contains(&resp.status) {
            let msg = v
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("error")
                .chars()
                .take(300)
                .collect();
            return Err(PayError::Api(resp.status, msg));
        }
        Ok(v)
    }

    /// The Stripe Price id for `plan`: from config, else by lookup_key.
    pub fn price_for(&self, plan: Plan) -> Result<String, PayError> {
        if let Some(p) = self.cfg.configured_price(plan) {
            return Ok(p.to_string());
        }
        let lk = plan
            .stripe_lookup_key()
            .ok_or_else(|| PayError::BadPayload("the free plan has no price".into()))?;
        if let Some(p) = self
            .price_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(lk)
        {
            return Ok(p.clone());
        }
        let key = self.key_only()?;
        let v = self.call(
            key,
            "GET",
            "/v1/prices",
            vec![
                ("lookup_keys[]".into(), lk.into()),
                ("active".into(), "true".into()),
                ("limit".into(), "1".into()),
            ],
            None,
        )?;
        let id = v
            .pointer("/data/0/id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| {
                PayError::NotConnected(format!(
                    "no active Stripe price with lookup_key {lk}: run `galion-accounts stripe-setup --apply` or set STRIPE_PRICE_*"
                ))
            })?
            .to_string();
        self.price_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(lk, id.clone());
        Ok(id)
    }

    /// Create a Checkout Session (mode=subscription). Returns its URL.
    /// The idempotency key is derived from the opaque reference, so a
    /// retried request cannot create a second session.
    pub fn create_checkout(&self, req: &StartRequest) -> Result<String, PayError> {
        let key = self.key()?;
        let price = self.price_for(req.plan)?;
        let base = &self.cfg.public_url;
        let mut form: Vec<(String, String)> = vec![
            ("mode".into(), "subscription".into()),
            ("line_items[0][price]".into(), price),
            ("line_items[0][quantity]".into(), "1".into()),
            ("client_reference_id".into(), req.pay_ref.clone()),
            ("metadata[galion_ref]".into(), req.pay_ref.clone()),
            (
                "subscription_data[metadata][galion_ref]".into(),
                req.pay_ref.clone(),
            ),
            (
                "success_url".into(),
                format!("{base}/account?checkout=done"),
            ),
            (
                "cancel_url".into(),
                format!("{base}/account?checkout=cancelled"),
            ),
            // Card details are Stripe's to collect; we ask for nothing else.
            ("billing_address_collection".into(), "auto".into()),
        ];
        if let Some(c) = &req.existing_customer {
            form.push(("customer".into(), c.clone()));
        }
        if self.cfg.automatic_tax {
            form.push(("automatic_tax[enabled]".into(), "true".into()));
        }
        let v = self.call(
            key,
            "POST",
            "/v1/checkout/sessions",
            form,
            Some(format!("galion-checkout-{}", req.pay_ref)),
        )?;
        v.get("url")
            .and_then(|u| u.as_str())
            .map(str::to_string)
            .ok_or_else(|| PayError::Api(200, "checkout session without url".into()))
    }

    /// A Customer Portal session (manage card, switch plan, cancel).
    pub fn create_portal(&self, customer: &str) -> Result<String, PayError> {
        let key = self.key()?;
        let v = self.call(
            key,
            "POST",
            "/v1/billing_portal/sessions",
            vec![
                ("customer".into(), customer.into()),
                (
                    "return_url".into(),
                    format!("{}/account", self.cfg.public_url),
                ),
            ],
            None,
        )?;
        v.get("url")
            .and_then(|u| u.as_str())
            .map(str::to_string)
            .ok_or_else(|| PayError::Api(200, "portal session without url".into()))
    }

    /// The setup steps (`stripe-setup`): create the two EUR products and
    /// monthly prices with their lookup keys (skipping any that exist), and
    /// optionally the webhook endpoint. With `apply == false` nothing is
    /// sent; the plan is returned as text.
    pub fn setup(&self, apply: bool, webhook_url: Option<&str>) -> Result<Vec<String>, PayError> {
        let mut out = Vec::new();
        let key = if apply { Some(self.key_only()?) } else { None };
        for plan in Plan::PAID {
            let lk = plan.stripe_lookup_key().unwrap_or_default();
            let line = format!(
                "{}: product \"{}\", price EUR {}.{:02}/month, lookup_key {lk}",
                plan.id(),
                plan.name(),
                plan.eur_cents() / 100,
                plan.eur_cents() % 100
            );
            let Some(key) = key else {
                out.push(format!("would create {line}"));
                continue;
            };
            let existing = self.call(
                key,
                "GET",
                "/v1/prices",
                vec![
                    ("lookup_keys[]".into(), lk.into()),
                    ("limit".into(), "1".into()),
                ],
                None,
            )?;
            if let Some(id) = existing.pointer("/data/0/id").and_then(|i| i.as_str()) {
                out.push(format!("exists {line} ({id})"));
                continue;
            }
            let product = self.call(
                key,
                "POST",
                "/v1/products",
                vec![
                    ("name".into(), plan.name().into()),
                    ("description".into(), plan.blurb().into()),
                    ("metadata[galion_plan]".into(), plan.id().into()),
                ],
                Some(format!("galion-setup-product-{lk}")),
            )?;
            let pid = product
                .get("id")
                .and_then(|i| i.as_str())
                .ok_or_else(|| PayError::Api(200, "product without id".into()))?
                .to_string();
            let price = self.call(
                key,
                "POST",
                "/v1/prices",
                vec![
                    ("product".into(), pid.clone()),
                    ("currency".into(), "eur".into()),
                    ("unit_amount".into(), plan.eur_cents().to_string()),
                    ("recurring[interval]".into(), "month".into()),
                    ("lookup_key".into(), lk.into()),
                    ("tax_behavior".into(), "inclusive".into()),
                    ("metadata[galion_plan]".into(), plan.id().into()),
                ],
                Some(format!("galion-setup-price-{lk}")),
            )?;
            let id = price.get("id").and_then(|i| i.as_str()).unwrap_or("?");
            out.push(format!("created {line} ({pid}, {id})"));
        }
        if let Some(url) = webhook_url {
            let mut form = vec![
                ("url".into(), url.to_string()),
                ("api_version".into(), API_VERSION.into()),
                ("description".into(), "galion-accounts".into()),
            ];
            for (i, e) in EVENTS.iter().enumerate() {
                form.push((format!("enabled_events[{i}]"), e.to_string()));
            }
            match key {
                None => out.push(format!(
                    "would create webhook endpoint {url} for {}",
                    EVENTS.join(", ")
                )),
                Some(key) => {
                    // Fixed key: a second --apply within 24 h does not create a
                    // second endpoint (Stripe keeps idempotency keys for 24 h).
                    let digest =
                        galion_common::sha256::hex(&galion_common::sha256::digest(url.as_bytes()));
                    let idem = format!("galion-setup-webhook-{}", &digest[..16]);
                    let v = self.call(key, "POST", "/v1/webhook_endpoints", form, Some(idem))?;
                    let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("?");
                    out.push(format!(
                        "created webhook endpoint {id} for {url}; its signing secret (whsec_...) is shown ONCE in the API response and in the Stripe Dashboard: put it in STRIPE_WEBHOOK_SECRET"
                    ));
                    if let Some(sec) = v.get("secret").and_then(|s| s.as_str()) {
                        out.push(format!("STRIPE_WEBHOOK_SECRET={sec}"));
                    }
                }
            }
        }
        Ok(out)
    }
}

impl PaymentAdapter for Stripe {
    fn method(&self) -> Method {
        Method::Stripe
    }

    fn status(&self) -> Status {
        self.cfg.status()
    }

    fn start(&self, req: &StartRequest) -> Result<StartOutcome, PayError> {
        self.create_checkout(req).map(StartOutcome::Redirect)
    }

    fn webhook(
        &self,
        headers: &[(String, String)],
        body: &[u8],
        now: i64,
    ) -> Result<Webhook, PayError> {
        let secret = match self.cfg.status() {
            Status::Connected => self.cfg.webhook_secret.as_deref().unwrap_or_default(),
            Status::NotConnected(r) => return Err(PayError::NotConnected(r)),
        };
        let header = galion_common::http::header(headers, "stripe-signature");
        verify_signature(header, body, secret, self.cfg.tolerance, now)
            .map_err(|e| PayError::BadSignature(e.as_str()))?;
        let v: Value =
            serde_json::from_slice(body).map_err(|_| PayError::BadPayload("not JSON".into()))?;
        parse_event(&self.cfg, &v, now)
    }
}
