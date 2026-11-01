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
use crate::store::Billing;

pub const DEFAULT_API_BASE: &str = "https://api.stripe.com";
pub const DEFAULT_TOLERANCE: i64 = 300;
/// Pinned so request/response shapes do not drift with the account default.
/// Webhook payloads follow the endpoint's own version; the parser accepts
/// both the pre-2025 and the 2025+ ("basil") shapes.
pub const API_VERSION: &str = "2025-03-31.basil";
/// Events the webhook endpoint must subscribe to.
pub const EVENTS: [&str; 6] = [
    "checkout.session.completed",
    "invoice.paid",
    "invoice.payment_failed",
    "customer.subscription.updated",
    "customer.subscription.deleted",
    "charge.refunded",
];

/// What `stripe-setup` did or would do. `Debug` hides the secret.
#[derive(Default)]
pub struct SetupReport {
    pub lines: Vec<String>,
    /// The new webhook endpoint's signing secret (only on creation).
    pub webhook_secret: Option<String>,
}

impl std::fmt::Debug for SetupReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SetupReport")
            .field("lines", &self.lines)
            .field(
                "webhook_secret",
                &self.webhook_secret.as_ref().map(|_| "<set>"),
            )
            .finish()
    }
}

/// How long a Checkout page stays open (Stripe allows 30 min to 24 h). The
/// same page is handed out again until shortly before it closes.
pub const CHECKOUT_TTL: i64 = 3600;
/// Stripe refuses a `trial_end` less than 48 h ahead.
pub const MIN_TRIAL: i64 = 2 * 86_400 + 600;

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
    /// Billing Portal configuration id (`bpc_...`, not a secret) made by
    /// `stripe-setup --apply`. Unset = the account's default configuration.
    pub portal_config: Option<String>,
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
            .field("portal_config", &self.portal_config)
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
            portal_config: nonempty(get("STRIPE_PORTAL_CONFIG")),
        }
    }

    /// `test` or `live` from the key prefix (never the key).
    pub fn mode(&self) -> Option<&'static str> {
        let k = self.secret_key.as_deref()?;
        if k.starts_with("sk_live_") || k.starts_with("rk_live_") {
            Some("live")
        } else if k.starts_with("sk_test_") || k.starts_with("rk_test_") {
            Some("test")
        } else {
            None
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
        "invoice.payment_failed" => {
            let subscription = s(o, &["subscription"])
                .or_else(|| s(o, &["parent", "subscription_details", "subscription"]));
            // The first invoice of a new subscription failing means checkout
            // never completed: there is no paid time to protect.
            let first = s(o, &["billing_reason"]).as_deref() == Some("subscription_create");
            let mut start: Option<i64> = None;
            if let Some(lines) = o.pointer("/lines/data").and_then(|l| l.as_array()) {
                for l in lines {
                    if let Some(st) = n(l, &["period", "start"]) {
                        start = Some(start.map_or(st, |x: i64| x.min(st)));
                    }
                }
            }
            if !first && (subscription.is_some() || s(o, &["customer"]).is_some()) {
                events.push(PayEvent::PaymentFailed {
                    subscription,
                    customer: s(o, &["customer"]),
                    paid_through: start.or_else(|| n(o, &["period_start"])),
                });
            }
        }
        "customer.subscription.updated" => {
            let status = s(o, &["status"]).unwrap_or_default();
            if let Some(sub) = s(o, &["id"]) {
                if matches!(status.as_str(), "active" | "trialing") {
                    let price = s(o, &["items", "data", "0", "price", "id"]);
                    let lk = s(o, &["items", "data", "0", "price", "lookup_key"]);
                    if let Some(plan) = cfg.plan_for_price(price.as_deref(), lk.as_deref()) {
                        events.push(PayEvent::PlanChanged {
                            subscription: sub.clone(),
                            plan,
                        });
                    }
                }
                let period_end = n(o, &["current_period_end"])
                    .or_else(|| n(o, &["items", "data", "0", "current_period_end"]));
                let cancel_flag = o
                    .get("cancel_at_period_end")
                    .and_then(|b| b.as_bool())
                    .unwrap_or(false);
                let cancel_at = n(o, &["cancel_at"]);
                let (billing, at) = match status.as_str() {
                    "past_due" | "unpaid" => (Billing::PastDue, None),
                    _ if cancel_flag => (Billing::Canceling, cancel_at.or(period_end)),
                    _ if cancel_at.is_some() => (Billing::Canceling, cancel_at),
                    _ => (Billing::Ok, None),
                };
                if matches!(
                    status.as_str(),
                    "active" | "trialing" | "past_due" | "unpaid"
                ) {
                    events.push(PayEvent::BillingChanged {
                        subscription: sub,
                        billing,
                        cancel_at: at,
                    });
                }
            }
        }
        "charge.refunded" => {
            let total = n(o, &["amount_refunded"]).unwrap_or(0);
            let before = ev
                .pointer("/data/previous_attributes/amount_refunded")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let full = o.get("refunded").and_then(|b| b.as_bool()).unwrap_or(false);
            events.push(PayEvent::Refunded {
                customer: s(o, &["customer"]),
                refunded_cents: (total - before).max(0),
                currency: s(o, &["currency"]).unwrap_or_default().to_ascii_uppercase(),
                full,
            });
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
        if let Some(t) = req.expires_at {
            form.push(("expires_at".into(), t.to_string()));
        }
        if let Some(t) = req.trial_end {
            // Prepaid cash/crypto time of the same plan: the card is saved
            // now and first charged when that time runs out.
            form.push(("subscription_data[trial_end]".into(), t.to_string()));
            form.push((
                "subscription_data[trial_settings][end_behavior][missing_payment_method]".into(),
                "cancel".into(),
            ));
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
        let mut form: Vec<(String, String)> = vec![
            ("customer".into(), customer.into()),
            (
                "return_url".into(),
                format!("{}/account", self.cfg.public_url),
            ),
        ];
        if let Some(c) = &self.cfg.portal_config {
            form.push(("configuration".into(), c.clone()));
        }
        let v = self.call(key, "POST", "/v1/billing_portal/sessions", form, None)?;
        v.get("url")
            .and_then(|u| u.as_str())
            .map(str::to_string)
            .ok_or_else(|| PayError::Api(200, "portal session without url".into()))
    }

    /// Cancel at the end of the paid period (`cancel = true`) or undo that
    /// (`false`). Returns the subscription's `cancel_at` (or period end).
    pub fn set_cancel_at_period_end(
        &self,
        subscription: &str,
        cancel: bool,
        now: i64,
    ) -> Result<Option<i64>, PayError> {
        let key = self.key()?;
        if subscription.is_empty()
            || !subscription
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(PayError::BadPayload("bad subscription id".into()));
        }
        // Per minute: a cancel/resume/cancel sequence is not swallowed by
        // Stripe's 24 h idempotency replay, a double click is.
        let idem = format!("galion-sub-{subscription}-{cancel}-{}", now / 60);
        let v = self.call(
            key,
            "POST",
            &format!("/v1/subscriptions/{subscription}"),
            vec![("cancel_at_period_end".into(), cancel.to_string())],
            Some(idem),
        )?;
        Ok(n(&v, &["cancel_at"])
            .or_else(|| n(&v, &["current_period_end"]))
            .or_else(|| n(&v, &["items", "data", "0", "current_period_end"])))
    }

    /// The setup steps (`stripe-setup`): the two EUR products and monthly
    /// prices with their lookup keys, a Billing Portal configuration limited
    /// to those prices, and the webhook endpoint. Anything that exists is
    /// left alone (an existing endpoint only gets missing events added).
    ///
    /// Dry run (`apply == false`): with no key nothing is sent; with a key
    /// only GETs are sent, to say what exists. The webhook signing secret is
    /// never put in `lines`; it comes back in [`SetupReport::webhook_secret`]
    /// for the caller to write to a 0600 file.
    pub fn setup(&self, apply: bool, webhook_url: Option<&str>) -> Result<SetupReport, PayError> {
        let mut rep = SetupReport::default();
        let out = &mut rep.lines;
        let key = match (&self.cfg.secret_key, apply) {
            (_, true) => Some(self.key_only()?),
            (Some(_), false) => Some(self.key_only()?),
            (None, false) => None,
        };
        let verb = if apply { "created" } else { "would create" };
        out.push(format!(
            "mode: {}{}",
            if apply { "APPLY" } else { "dry run" },
            match (key, self.cfg.mode()) {
                (None, _) => " (no key: nothing sent)".to_string(),
                (Some(_), Some(m)) => format!(" ({m} key)"),
                (Some(_), None) => " (key of unknown kind)".to_string(),
            }
        ));
        // (product id, price id) per plan, for the portal configuration.
        let mut catalog: Vec<(String, String)> = Vec::new();
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
                let amount = existing
                    .pointer("/data/0/unit_amount")
                    .and_then(|a| a.as_i64());
                let cur = existing
                    .pointer("/data/0/currency")
                    .and_then(|a| a.as_str());
                if amount.is_some_and(|a| a != plan.eur_cents()) || cur.is_some_and(|c| c != "eur")
                {
                    out.push(format!(
                        "WARNING {} price {id} under lookup_key {lk} is not EUR {}.{:02}: fix it in the Dashboard",
                        plan.id(),
                        plan.eur_cents() / 100,
                        plan.eur_cents() % 100
                    ));
                }
                let prod = s(&existing, &["data", "0", "product"]).unwrap_or_default();
                catalog.push((prod, id.to_string()));
                out.push(format!("exists {line} ({id})"));
                continue;
            }
            if !apply {
                out.push(format!("would create {line}"));
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
            catalog.push((pid.clone(), id.to_string()));
            out.push(format!("created {line} ({pid}, {id})"));
        }

        // Billing Portal: card update, invoices, cancel at period end, and
        // switching between exactly our two prices (prorated).
        match key {
            None => out.push(
                "would create billing portal configuration (update card, invoices, cancel at period end, switch Plus/Pro)".into(),
            ),
            Some(key) => {
                let list = self.call(
                    key,
                    "GET",
                    "/v1/billing_portal/configurations",
                    vec![
                        ("active".into(), "true".into()),
                        ("limit".into(), "100".into()),
                    ],
                    None,
                )?;
                let found = list
                    .get("data")
                    .and_then(|d| d.as_array())
                    .and_then(|d| {
                        d.iter().find(|c| {
                            c.pointer("/metadata/galion").and_then(|m| m.as_str()) == Some("accounts")
                        })
                    })
                    .and_then(|c| s(c, &["id"]));
                match found {
                    Some(id) => out.push(format!(
                        "exists billing portal configuration {id}: set STRIPE_PORTAL_CONFIG={id}"
                    )),
                    None if !apply || catalog.len() != Plan::PAID.len() => out.push(format!(
                        "{verb} billing portal configuration (update card, invoices, cancel at period end, switch Plus/Pro)"
                    )),
                    None => {
                        let mut form: Vec<(String, String)> = vec![
                            ("business_profile[headline]".into(), "Galion".into()),
                            ("default_return_url".into(), format!("{}/account", self.cfg.public_url)),
                            ("features[payment_method_update][enabled]".into(), "true".into()),
                            ("features[invoice_history][enabled]".into(), "true".into()),
                            ("features[customer_update][enabled]".into(), "false".into()),
                            ("features[subscription_cancel][enabled]".into(), "true".into()),
                            ("features[subscription_cancel][mode]".into(), "at_period_end".into()),
                            ("features[subscription_update][enabled]".into(), "true".into()),
                            ("features[subscription_update][default_allowed_updates][0]".into(), "price".into()),
                            ("features[subscription_update][proration_behavior]".into(), "create_prorations".into()),
                            ("metadata[galion]".into(), "accounts".into()),
                        ];
                        for (i, (prod, price)) in catalog.iter().enumerate() {
                            form.push((format!("features[subscription_update][products][{i}][product]"), prod.clone()));
                            form.push((format!("features[subscription_update][products][{i}][prices][0]"), price.clone()));
                        }
                        let v = self.call(
                            key,
                            "POST",
                            "/v1/billing_portal/configurations",
                            form,
                            Some("galion-setup-portal-v1".into()),
                        )?;
                        let id = s(&v, &["id"]).unwrap_or_else(|| "?".into());
                        out.push(format!(
                            "created billing portal configuration {id}: set STRIPE_PORTAL_CONFIG={id}"
                        ));
                    }
                }
            }
        }

        if let Some(url) = webhook_url {
            if !url.starts_with("https://") {
                return Err(PayError::BadPayload("webhook url must be https://".into()));
            }
            match key {
                None => out.push(format!(
                    "would create webhook endpoint {url} for {}",
                    EVENTS.join(", ")
                )),
                Some(key) => {
                    let list = self.call(
                        key,
                        "GET",
                        "/v1/webhook_endpoints",
                        vec![("limit".into(), "100".into())],
                        None,
                    )?;
                    let existing = list
                        .get("data")
                        .and_then(|d| d.as_array())
                        .and_then(|d| d.iter().find(|e| s(e, &["url"]).as_deref() == Some(url)))
                        .cloned();
                    if let Some(e) = existing {
                        let id = s(&e, &["id"]).unwrap_or_else(|| "?".into());
                        let have: Vec<String> = e
                            .get("enabled_events")
                            .and_then(|v| v.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let all = have.iter().any(|h| h == "*");
                        let missing: Vec<&str> = EVENTS
                            .iter()
                            .copied()
                            .filter(|e| !all && !have.iter().any(|h| h == e))
                            .collect();
                        if missing.is_empty() {
                            out.push(format!(
                                "exists webhook endpoint {id} for {url} with all events"
                            ));
                        } else if !apply {
                            out.push(format!(
                                "would add events to webhook endpoint {id}: {}",
                                missing.join(", ")
                            ));
                        } else {
                            let mut form: Vec<(String, String)> = Vec::new();
                            let mut evs: Vec<String> = have.clone();
                            evs.extend(missing.iter().map(|m| m.to_string()));
                            for (i, e) in evs.iter().enumerate() {
                                form.push((format!("enabled_events[{i}]"), e.clone()));
                            }
                            self.call(
                                key,
                                "POST",
                                &format!("/v1/webhook_endpoints/{id}"),
                                form,
                                None,
                            )?;
                            out.push(format!(
                                "updated webhook endpoint {id}: added {} (signing secret unchanged)",
                                missing.join(", ")
                            ));
                        }
                    } else if !apply {
                        out.push(format!(
                            "would create webhook endpoint {url} for {}",
                            EVENTS.join(", ")
                        ));
                    } else {
                        let mut form = vec![
                            ("url".into(), url.to_string()),
                            ("api_version".into(), API_VERSION.into()),
                            ("description".into(), "galion-accounts".into()),
                        ];
                        for (i, e) in EVENTS.iter().enumerate() {
                            form.push((format!("enabled_events[{i}]"), e.to_string()));
                        }
                        // Fixed key: a second --apply within 24 h does not
                        // create a second endpoint.
                        let digest = galion_common::sha256::hex(&galion_common::sha256::digest(
                            url.as_bytes(),
                        ));
                        let idem = format!("galion-setup-webhook-{}", &digest[..16]);
                        let v =
                            self.call(key, "POST", "/v1/webhook_endpoints", form, Some(idem))?;
                        let id = s(&v, &["id"]).unwrap_or_else(|| "?".into());
                        out.push(format!("created webhook endpoint {id} for {url}"));
                        rep.webhook_secret = v
                            .get("secret")
                            .and_then(|s| s.as_str())
                            .filter(|s| !s.is_empty())
                            .map(str::to_string);
                    }
                }
            }
        }
        Ok(rep)
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
