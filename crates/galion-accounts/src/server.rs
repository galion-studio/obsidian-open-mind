// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! HTTP: the public account API + page, and the internal API.
//!
//! Public (behind the galion.app reverse proxy):
//!
//! | route                          | what                                    |
//! |--------------------------------|-----------------------------------------|
//! | GET  /account                  | the page                                |
//! | POST /api/account/create       | new number (shown once) + session       |
//! | POST /api/account/login        | `{"number"}` -> session cookie          |
//! | POST /api/account/logout       |                                         |
//! | GET  /api/account/me           | plan, paid-until, which payments work   |
//! | POST /api/pay/stripe/checkout  | `{"plan"}` -> Stripe Checkout URL       |
//! | POST /api/pay/stripe/portal    | -> Customer Portal URL                  |
//! | POST /api/pay/stripe/cancel    | cancel at the end of the paid period    |
//! | POST /api/pay/stripe/resume    | undo that                               |
//! | POST /api/pay/cash             | `{"plan"}` -> cash reference + address  |
//! | GET  /account/cash-slip?ref=   | printable slip for one's own reference  |
//! | POST /webhooks/stripe          | Stripe webhook (signature checked)      |
//! | POST /api/pay/crypto           | `{"plan","backend"?}` -> BTCPay link or XMR address |
//! | POST /webhooks/crypto          | BTCPay webhook (BTCPay-Sig checked)     |
//!
//! Internal (loopback only, bearer token):
//!
//! | route                          | what                                    |
//! |--------------------------------|-----------------------------------------|
//! | POST /internal/v1/entitlement  | `{"session"}` or `{"number"}` -> plan   |
//! | POST /internal/v1/cash/credit  | operator credits a cash reference       |
//! | POST /internal/v1/crypto/poll  | Monero + BTCPay reconcile (admin token) |
//! | GET  /internal/v1/cash/lookup?ref= | what a cash reference expects (admin) |
//! | GET  /internal/v1/admin/status | counts, ledger, duplicates (admin token) |
//! | GET  /internal/v1/ready        | which methods are configured (booleans) |
//! | GET  /internal/v1/health       | counts + payment method status          |

use std::sync::Arc;

use galion_common::http::{Request, Response};
use serde_json::{json, Value};

use crate::hash::Hasher;
use crate::number::AccountNumber;
use crate::pay::cash::{self, Cash};
use crate::pay::crypto::{self, Crypto};
use crate::pay::stripe::{Stripe, CHECKOUT_TTL, MIN_TRIAL};
use crate::pay::{self, Applied, Method, PayError, PaymentAdapter, StartOutcome, StartRequest};
use crate::plans::Plan;
use crate::ratelimit::{Limiter, Refused};
use crate::session;
use crate::store::{Account, Billing, Store, StoreError, REF_TTL_CASH, REF_TTL_ONLINE};

/// Server settings that are not payment config.
#[derive(Clone, Default)]
pub struct Settings {
    /// `Domain=` for the session cookie (e.g. `galion.app`), so Galion One
    /// and the console on sub-domains see it too.
    pub cookie_domain: Option<String>,
    /// `Secure` cookies (off only for loopback tests).
    pub secure_cookies: bool,
    /// Take the client address from `X-Forwarded-For` (last hop) because a
    /// trusted reverse proxy sits in front. Used for rate limiting only.
    pub trust_proxy: bool,
    /// Bearer token for /internal/v1/entitlement and /health.
    pub internal_token: Option<String>,
    /// Bearer token for operator actions (cash credit).
    pub admin_token: Option<String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("cookie_domain", &self.cookie_domain)
            .field("secure_cookies", &self.secure_cookies)
            .field("trust_proxy", &self.trust_proxy)
            .field(
                "internal_token",
                &self.internal_token.as_ref().map(|_| "<set>"),
            )
            .field("admin_token", &self.admin_token.as_ref().map(|_| "<set>"))
            .finish()
    }
}

pub struct App {
    pub store: Store,
    pub hasher: Hasher,
    pub limiter: Limiter,
    pub stripe: Stripe,
    pub cash: Cash,
    pub crypto: Crypto,
    pub settings: Settings,
    /// Clock (tests replace it).
    pub clock: Box<dyn Fn() -> i64 + Send + Sync>,
}

fn json_resp(status: u16, v: Value) -> Response {
    Response::json(status, v.to_string())
        .with_header("cache-control", "no-store")
        .with_header("x-content-type-options", "nosniff")
}

fn err(status: u16, code: &str, message: &str) -> Response {
    json_resp(
        status,
        json!({"ok": false, "error": code, "message": message}),
    )
}

fn db_err(_: StoreError) -> Response {
    err(500, "storage", "Storage error. Nothing was changed.")
}

fn refused(r: Refused) -> Response {
    let (code, secs, msg) = match r {
        Refused::Locked(s) => ("locked", s, "Too many attempts. Wait and try again."),
        Refused::Busy(s) => (
            "busy",
            s,
            "Too many failed logins right now. Try again in a minute.",
        ),
    };
    err(429, code, msg).with_header("retry-after", &secs.max(1).to_string())
}

fn body_json(req: &Request) -> Option<Value> {
    if req.body.is_empty() {
        return Some(json!({}));
    }
    serde_json::from_slice(&req.body).ok()
}

fn bearer(req: &Request) -> Option<&str> {
    req.header("authorization")?.strip_prefix("Bearer ")
}

fn token_ok(req: &Request, want: &Option<String>) -> bool {
    match (want, bearer(req)) {
        (Some(w), Some(got)) => crate::ct_eq(w.as_bytes(), got.as_bytes()),
        _ => false,
    }
}

/// Paid-until for people: Zagreb label plus the RFC 3339 stamp.
pub fn entitlement_json(a: &Account, now: i64) -> Value {
    let e = a.entitlement(now);
    json!({
        "ok": true,
        "plan": e.plan.id(),
        "plan_name": e.plan.name(),
        "active": e.active,
        "paid_until": if a.paid_until > 0 { json!(a.paid_until) } else { Value::Null },
        "paid_until_local": if a.paid_until > 0 { json!(galion_common::time::from_unix(a.paid_until)) } else { Value::Null },
        "paid_until_label": if a.paid_until > 0 { json!(galion_common::time::label(a.paid_until)) } else { Value::Null },
        "renewing": e.renewing,
        // ok | past_due (card failed, access kept until paid_until) |
        // canceling (ends at cancel_at, no more charges)
        "billing": e.billing.id(),
        "cancel_at": a.cancel_at.filter(|_| e.billing == Billing::Canceling),
        "cancel_at_label": a.cancel_at.filter(|_| e.billing == Billing::Canceling).map(galion_common::time::label),
        // Cash/crypto time waiting behind a card subscription.
        "banked_days": a.banked_secs / crate::store::DAY,
        "banked_plan": a.banked_plan.filter(|_| a.banked_secs > 0).map(|p| p.id()),
    })
}

impl App {
    fn now(&self) -> i64 {
        (self.clock)()
    }

    fn client(&self, req: &Request) -> String {
        if self.settings.trust_proxy {
            if let Some(x) = req.header("x-forwarded-for") {
                if let Some(last) = x.rsplit(',').next() {
                    return last.trim().to_string();
                }
            }
        }
        req.peer.map(|p| p.ip().to_string()).unwrap_or_default()
    }

    fn session_token(req: &Request) -> Option<String> {
        req.header("cookie").and_then(session::from_cookie_header)
    }

    fn current(&self, req: &Request) -> Result<Option<Account>, StoreError> {
        match Self::session_token(req) {
            Some(t) => self
                .store
                .session_account(&session::token_hash(&t), self.now()),
            None => Ok(None),
        }
    }

    /// Browser POSTs must be JSON and, when an Origin is sent, come from our
    /// own site (CSRF guard on top of SameSite cookies).
    fn browser_post_ok(&self, req: &Request) -> bool {
        let ct = req.header("content-type").unwrap_or("");
        if !ct.starts_with("application/json") {
            return false;
        }
        match req.header("origin") {
            None => true,
            Some(o) => {
                let host = req.header("host").unwrap_or("");
                o.strip_prefix("https://")
                    .or_else(|| o.strip_prefix("http://"))
                    == Some(host)
                    || o == self.stripe.cfg.public_url
            }
        }
    }

    fn start_session(&self, account_id: i64) -> Result<String, StoreError> {
        let t = session::new_token();
        self.store.create_session(
            &session::token_hash(&t),
            account_id,
            self.now() + session::TTL,
        )?;
        Ok(session::set_cookie(
            &t,
            self.settings.cookie_domain.as_deref(),
            self.settings.secure_cookies,
        ))
    }

    fn methods_json(&self) -> Value {
        // The detailed reason (which env var is missing) goes to the server
        // log at start-up only; visitors get a plain sentence.
        let st = |a: &dyn PaymentAdapter, name: &str| {
            let s = a.status();
            json!({"connected": s.is_connected(), "note": match s {
                pay::Status::Connected => Value::Null,
                pay::Status::NotConnected(_) => json!(format!("{name} is not connected yet.")),
            }})
        };
        let mut crypto_st = st(&self.crypto, "Crypto payment");
        crypto_st["backends"] = Value::Array(
            self.crypto
                .cfg
                .ready()
                .iter()
                .map(|b| json!({"id": b.id(), "label": b.label()}))
                .collect(),
        );
        json!({
            "stripe": st(&self.stripe, "Card payment"),
            "crypto": crypto_st,
            "cash": st(&self.cash, "Cash by post"),
        })
    }

    /// Public routes.
    pub fn handle(&self, req: &Request) -> Response {
        let path = req.path();
        match (req.method(), path) {
            ("GET" | "HEAD", "/account" | "/account/") => crate::page::html(),
            ("GET" | "HEAD", "/account/account.css") => crate::page::css(),
            ("GET" | "HEAD", "/account/account.js") => crate::page::js(),
            ("GET", "/account/cash-slip") => self.cash_slip(req),
            ("GET", "/api/account/plans") => self.plans(),
            // Contract with the galion.app home page (swarm/galion-home-20261101,
            // docs/payments/galion-home-checkout.md): HEAD probes, GET starts.
            ("HEAD", "/api/checkout") => self.checkout_probe(req),
            ("GET", "/api/checkout") => self.checkout_link(req),
            ("POST", "/webhooks/stripe") => self.webhook(&self.stripe, req),
            ("POST", "/webhooks/crypto") => self.crypto_webhook(req),
            ("POST", p) if p.starts_with("/api/") && !self.browser_post_ok(req) => {
                err(415, "bad_request", "Send JSON from the Galion site.")
            }
            ("POST", "/api/account/create") => self.create(req),
            ("POST", "/api/account/login") => self.login(req),
            ("POST", "/api/account/logout") => self.logout(req),
            ("GET", "/api/account/me") => self.me(req),
            ("POST", "/api/pay/stripe/checkout") => self.checkout(req),
            ("POST", "/api/pay/stripe/portal") => self.portal(req),
            ("POST", "/api/pay/stripe/cancel") => self.cancel(req, true),
            ("POST", "/api/pay/stripe/resume") => self.cancel(req, false),
            ("POST", "/api/pay/cash") => self.cash_ref(req),
            ("POST", "/api/pay/crypto") => self.crypto_start(req),
            _ => err(404, "not_found", "Not found."),
        }
    }

    fn plans(&self) -> Response {
        let plans: Vec<Value> = [Plan::Free, Plan::Plus, Plan::Pro]
            .iter()
            .map(|p| {
                json!({"id": p.id(), "name": p.name(), "eur_cents": p.eur_cents(),
                       "price": cash::fmt_eur(p.eur_cents()), "blurb": p.blurb()})
            })
            .collect();
        json_resp(
            200,
            json!({"ok": true, "plans": plans, "methods": self.methods_json()}),
        )
    }

    fn create(&self, req: &Request) -> Response {
        let now = self.now();
        if let Err(r) = self.limiter.create(&self.client(req), now) {
            return refused(r);
        }
        // 9e15 numbers: a collision is ~never, but loop rather than fail.
        for _ in 0..4 {
            let n = AccountNumber::generate();
            match self.store.create_account(&self.hasher.lookup(&n)) {
                Ok(id) => {
                    let cookie = match self.start_session(id) {
                        Ok(c) => c,
                        Err(e) => return db_err(e),
                    };
                    return json_resp(
                        200,
                        json!({"ok": true, "number": n.grouped(),
                               "message": "This number is your account. Write it down or keep it in a password manager. We cannot show it again or recover it."}),
                    )
                    .with_header("set-cookie", &cookie);
                }
                Err(StoreError::Exists) => continue,
                Err(e) => return db_err(e),
            }
        }
        err(500, "storage", "Could not create an account. Try again.")
    }

    fn login(&self, req: &Request) -> Response {
        let now = self.now();
        let client = self.client(req);
        if let Err(r) = self.limiter.login_allowed(&client, now) {
            return refused(r);
        }
        let Some(body) = body_json(req) else {
            return err(
                400,
                "bad_request",
                "Send {\"number\": \"1234 5678 9012 3456\"}.",
            );
        };
        let number = body
            .get("number")
            .and_then(|n| n.as_str())
            .and_then(AccountNumber::parse);
        let account = match number {
            Some(n) => match self.store.find_by_lookup(&self.hasher.lookup(&n)) {
                Ok(a) => a,
                Err(e) => return db_err(e),
            },
            None => None,
        };
        let Some(a) = account else {
            self.limiter.login_failed(&client, now);
            return err(
                401,
                "no_account",
                "That number is not an account. Check the 16 digits.",
            );
        };
        self.limiter.login_ok(&client, now);
        match self.start_session(a.id) {
            Ok(cookie) => {
                json_resp(200, entitlement_json(&a, now)).with_header("set-cookie", &cookie)
            }
            Err(e) => db_err(e),
        }
    }

    fn logout(&self, req: &Request) -> Response {
        if let Some(t) = Self::session_token(req) {
            if let Err(e) = self.store.end_session(&session::token_hash(&t)) {
                return db_err(e);
            }
        }
        json_resp(200, json!({"ok": true})).with_header(
            "set-cookie",
            &session::clear_cookie(
                self.settings.cookie_domain.as_deref(),
                self.settings.secure_cookies,
            ),
        )
    }

    fn me(&self, req: &Request) -> Response {
        match self.current(req) {
            Ok(Some(a)) => {
                let mut v = entitlement_json(&a, self.now());
                v["methods"] = self.methods_json();
                v["can_manage"] = json!(a.stripe_customer.is_some());
                v["can_cancel"] = json!(a.stripe_subscription.is_some());
                // Open cash references, so the slip can be printed again.
                let now = self.now();
                v["cash_refs"] = match self.store.open_refs_of(a.id, Method::Cash.id(), now) {
                    Ok(refs) => Value::Array(
                        refs.into_iter()
                            .map(|(r, p, exp)| {
                                json!({"reference": r, "plan": p.id(),
                                       "expires_label": galion_common::time::label(exp),
                                       "slip_url": format!("/account/cash-slip?ref={r}")})
                            })
                            .collect(),
                    ),
                    Err(e) => return db_err(e),
                };
                json_resp(200, v)
            }
            Ok(None) => {
                let mut v =
                    json!({"ok": false, "error": "no_session", "message": "Not logged in."});
                v["methods"] = self.methods_json();
                json_resp(401, v)
            }
            Err(e) => db_err(e),
        }
    }

    fn paid_plan(req: &Request) -> Option<Plan> {
        body_json(req)?
            .get("plan")
            .and_then(|p| p.as_str())
            .and_then(Plan::parse)
            .filter(|p| *p != Plan::Free)
    }

    fn query_plan(req: &Request) -> Option<Plan> {
        req.query()
            .split('&')
            .find_map(|kv| kv.strip_prefix("plan="))
            .and_then(Plan::parse)
            .filter(|p| *p != Plan::Free)
    }

    /// `HEAD /api/checkout?plan=P`: 204 when card checkout is connected,
    /// 503 otherwise. Creates nothing.
    fn checkout_probe(&self, req: &Request) -> Response {
        if Self::query_plan(req).is_none() {
            return Response::new(400, "text/plain; charset=utf-8", "unknown plan");
        }
        match self.stripe.status() {
            pay::Status::Connected => Response::new(204, "text/plain; charset=utf-8", ""),
            pay::Status::NotConnected(_) => {
                Response::new(503, "text/plain; charset=utf-8", "not connected")
            }
        }
        .with_header("cache-control", "no-store")
    }

    /// `GET /api/checkout?plan=P` (the home page "Subscribe" link): logged in
    /// and connected -> 303 to Stripe Checkout; otherwise 303 to /account,
    /// where a number is created or entered first.
    fn checkout_link(&self, req: &Request) -> Response {
        let see = |loc: &str| {
            Response::new(303, "text/plain; charset=utf-8", "")
                .with_header("location", loc)
                .with_header("cache-control", "no-store")
        };
        let Some(plan) = Self::query_plan(req) else {
            return see("/account");
        };
        let back = format!("/account?plan={}", plan.id());
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return see(&back),
            Err(e) => return db_err(e),
        };
        if !self.stripe.status().is_connected() || a.stripe_subscription.is_some() {
            return see(&back);
        }
        match self.start_checkout(&a, plan) {
            Ok(url) => see(&url),
            Err(_) => see(&format!("{back}&checkout=error")),
        }
    }

    fn start_checkout(&self, a: &Account, plan: Plan) -> Result<String, Response> {
        let now = self.now();
        // Double click / second tab: hand out the same open Checkout page,
        // so one account cannot end up with two subscriptions.
        if let Some(url) = self.store.open_checkout(a.id, plan, now).map_err(db_err)? {
            return Ok(url);
        }
        let r = pay::new_online_ref();
        self.store
            .put_pay_ref(&r, a.id, Method::Stripe.id(), plan, now + REF_TTL_ONLINE)
            .map_err(db_err)?;
        // Prepaid cash/crypto time of the same plan: first charge when it
        // runs out (Stripe needs at least 48 h for a trial).
        let trial_end = (a.plan == plan && a.paid_until > now + MIN_TRIAL).then_some(a.paid_until);
        let sr = StartRequest {
            pay_ref: r.clone(),
            plan,
            existing_customer: a.stripe_customer.clone(),
            trial_end,
            expires_at: Some(now + CHECKOUT_TTL),
        };
        match self.stripe.start(&sr) {
            Ok(StartOutcome::Redirect(url)) => {
                // Reuse until 5 minutes before Stripe closes the page.
                self.store
                    .set_ref_url(&r, &url, now + CHECKOUT_TTL - 300)
                    .map_err(db_err)?;
                Ok(url)
            }
            Ok(_) => Err(err(500, "unexpected", "Unexpected payment answer.")),
            Err(e) => Err(pay_err(e)),
        }
    }

    fn checkout(&self, req: &Request) -> Response {
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return err(401, "no_session", "Log in first."),
            Err(e) => return db_err(e),
        };
        let Some(plan) = Self::paid_plan(req) else {
            return err(400, "bad_plan", "Choose plus or pro.");
        };
        if let pay::Status::NotConnected(_) = self.stripe.status() {
            return err(503, "not_connected", NOT_CONNECTED);
        }
        if a.stripe_subscription.is_some() {
            return err(
                409,
                "has_subscription",
                "This account already has a card subscription. Use Manage to change or cancel it.",
            );
        }
        match self.start_checkout(&a, plan) {
            Ok(url) => json_resp(200, json!({"ok": true, "url": url})),
            Err(r) => r,
        }
    }

    fn portal(&self, req: &Request) -> Response {
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return err(401, "no_session", "Log in first."),
            Err(e) => return db_err(e),
        };
        let Some(c) = a.stripe_customer else {
            return err(
                404,
                "no_customer",
                "There is no card subscription on this account.",
            );
        };
        match self.stripe.create_portal(&c) {
            Ok(url) => json_resp(200, json!({"ok": true, "url": url})),
            Err(e) => pay_err(e),
        }
    }

    /// `POST /api/pay/stripe/cancel|resume`: stop (or restart) renewals at
    /// the end of the paid period, without the portal. Paid time is kept.
    fn cancel(&self, req: &Request, cancel: bool) -> Response {
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return err(401, "no_session", "Log in first."),
            Err(e) => return db_err(e),
        };
        let Some(sub) = a.stripe_subscription.clone() else {
            return err(
                404,
                "no_subscription",
                "There is no card subscription on this account.",
            );
        };
        let now = self.now();
        match self.stripe.set_cancel_at_period_end(&sub, cancel, now) {
            Ok(at) => {
                let (billing, at) = if cancel {
                    (Billing::Canceling, at.or(Some(a.paid_until)))
                } else {
                    (Billing::Ok, None)
                };
                if let Err(e) = self.store.set_billing(a.id, billing, at) {
                    return db_err(e);
                }
                match self.store.get(a.id) {
                    Ok(Some(a)) => {
                        let mut v = entitlement_json(&a, now);
                        v["message"] = json!(if cancel {
                            "Cancelled. No more charges; your plan runs until the end of the paid period."
                        } else {
                            "Renewal is back on."
                        });
                        json_resp(200, v)
                    }
                    Ok(None) => err(404, "no_account", "Account not found."),
                    Err(e) => db_err(e),
                }
            }
            Err(e) => pay_err(e),
        }
    }

    /// `GET /account/cash-slip?ref=GC-...`: a printable page for one of the
    /// logged-in account's own open cash references. No script.
    fn cash_slip(&self, req: &Request) -> Response {
        let page = |status: u16, body: String| {
            Response::new(status, "text/html; charset=utf-8", body)
                .with_header("content-security-policy", cash::SLIP_CSP)
                .with_header("x-content-type-options", "nosniff")
                .with_header("referrer-policy", "no-referrer")
                .with_header("cache-control", "no-store")
        };
        let not_found = || {
            page(
                404,
                "<!doctype html><meta charset=utf-8><title>Not found</title><p>No open cash reference with that code on this account. <a href=\"/account\">Back</a></p>".into(),
            )
        };
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return not_found(),
            Err(e) => return db_err(e),
        };
        let Some(r) = req
            .query()
            .split('&')
            .find_map(|kv| kv.strip_prefix("ref="))
            .and_then(cash::normalise_reference)
        else {
            return not_found();
        };
        let now = self.now();
        let refs = match self.store.open_refs_of(a.id, Method::Cash.id(), now) {
            Ok(v) => v,
            Err(e) => return db_err(e),
        };
        let Some((_, plan, exp)) = refs.into_iter().find(|(x, _, _)| *x == r) else {
            return not_found();
        };
        let addr = self.cash.cfg.address.clone().unwrap_or_default();
        page(
            200,
            cash::slip_html(&r, plan, &addr, &galion_common::time::label(exp)),
        )
    }

    fn cash_ref(&self, req: &Request) -> Response {
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return err(401, "no_session", "Log in first."),
            Err(e) => return db_err(e),
        };
        let Some(plan) = Self::paid_plan(req) else {
            return err(400, "bad_plan", "Choose plus or pro.");
        };
        if let pay::Status::NotConnected(_) = self.cash.status() {
            return err(503, "not_connected", NOT_CONNECTED);
        }
        let now = self.now();
        let mut r = cash::new_reference();
        let mut tries = 0;
        loop {
            match self
                .store
                .put_pay_ref(&r, a.id, Method::Cash.id(), plan, now + REF_TTL_CASH)
            {
                Ok(()) => break,
                Err(StoreError::Exists) if tries < 4 => {
                    tries += 1;
                    r = cash::new_reference();
                }
                Err(e) => return db_err(e),
            }
        }
        let sr = StartRequest {
            pay_ref: r,
            plan,
            existing_customer: None,
            trial_end: None,
            expires_at: None,
        };
        match self.cash.start(&sr) {
            Ok(StartOutcome::Reference {
                reference,
                instructions,
            }) => json_resp(
                200,
                json!({"ok": true, "reference": reference, "instructions": instructions,
                       "slip_url": format!("/account/cash-slip?ref={reference}")}),
            ),
            Ok(_) => err(500, "unexpected", "Unexpected payment answer."),
            Err(e) => pay_err(e),
        }
    }

    /// `POST /api/pay/crypto {"plan", "backend"?}`: a BTCPay checkout link,
    /// or a Monero address + amount. `backend` defaults to the first ready one.
    fn crypto_start(&self, req: &Request) -> Response {
        let a = match self.current(req) {
            Ok(Some(a)) => a,
            Ok(None) => return err(401, "no_session", "Log in first."),
            Err(e) => return db_err(e),
        };
        let Some(plan) = Self::paid_plan(req) else {
            return err(400, "bad_plan", "Choose plus or pro.");
        };
        let ready = self.crypto.cfg.ready();
        let asked = body_json(req).and_then(|b| {
            b.get("backend")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        });
        let backend = match asked.as_deref() {
            None | Some("") => ready.first().copied(),
            Some(s) => crypto::Backend::parse(s).filter(|b| ready.contains(b)),
        };
        let Some(backend) = backend else {
            return err(503, "not_connected", NOT_CONNECTED);
        };
        match crypto::start(&self.crypto, &self.store, backend, a.id, plan, self.now()) {
            Ok(crypto::CryptoStart::Redirect { url }) => json_resp(
                200,
                json!({"ok": true, "backend": backend.id(), "url": url}),
            ),
            Ok(crypto::CryptoStart::Monero {
                address,
                amount_xmr,
                uri,
                expires_at,
                confirmations,
            }) => json_resp(
                200,
                json!({"ok": true, "backend": backend.id(), "address": address,
                       "amount_xmr": amount_xmr, "uri": uri,
                       "expires_at": expires_at,
                       "expires_label": galion_common::time::label(expires_at),
                       "instructions": format!(
                           "Send exactly {amount_xmr} XMR to this address before {} (Zagreb time). \
                            This address is for this payment only. {} days of {} are added after \
                            {confirmations} confirmations (about {} minutes). Paying less adds time \
                            pro rata. Do not reuse the address.",
                           galion_common::time::label(expires_at),
                           crate::plans::days_for(plan, plan.eur_cents()),
                           plan.name(),
                           confirmations * 2)}),
            ),
            Err(crypto::CryptoError::Pay(e)) => pay_err(e),
            Err(crypto::CryptoError::Store(e)) => db_err(e),
        }
    }

    /// `POST /webhooks/crypto`: BTCPay Server webhook.
    fn crypto_webhook(&self, req: &Request) -> Response {
        match crypto::btcpay_hook(
            &self.crypto,
            &self.store,
            &req.head.headers,
            &req.body,
            self.now(),
        ) {
            Ok(crypto::HookOutcome::Credited(c)) => {
                json_resp(200, json!({"ok": true, "applied": 1, "eur_cents": c}))
            }
            Ok(crypto::HookOutcome::Replay) => json_resp(200, json!({"ok": true, "replay": true})),
            Ok(crypto::HookOutcome::Ignored(why)) => {
                json_resp(200, json!({"ok": true, "ignored": why}))
            }
            Ok(crypto::HookOutcome::Retry) => {
                err(409, "retry", "Payment not matched yet; retry later.")
            }
            Err(crypto::CryptoError::Pay(e)) => pay_err(e),
            Err(crypto::CryptoError::Store(e)) => db_err(e),
        }
    }

    fn webhook(&self, adapter: &dyn PaymentAdapter, req: &Request) -> Response {
        let now = self.now();
        let hook = match adapter.webhook(&req.head.headers, &req.body, now) {
            Ok(h) => h,
            Err(e) => return pay_err(e),
        };
        match pay::apply(&self.store, adapter.method(), &hook, now) {
            Ok(Applied::Done(n)) => json_resp(200, json!({"ok": true, "applied": n})),
            Ok(Applied::Replay) => json_resp(200, json!({"ok": true, "replay": true})),
            // Not matched yet (events can arrive out of order): the
            // processor retries a non-2xx delivery later.
            Ok(Applied::Retry) => err(409, "retry", "Payment not matched yet; retry later."),
            Err(e) => db_err(e),
        }
    }

    // ---------------------------------------------------------- internal

    /// Internal routes (bind to loopback only).
    pub fn handle_internal(&self, req: &Request) -> Response {
        match (req.method(), req.path()) {
            ("GET", "/internal/v1/health") => {
                if !token_ok(req, &self.settings.internal_token) {
                    return err(401, "unauthorized", "Bearer token required.");
                }
                match self.store.counts() {
                    Ok((a, s, p)) => json_resp(
                        200,
                        json!({"ok": true, "accounts": a, "sessions": s, "open_payment_refs": p,
                               "methods": self.methods_json()}),
                    ),
                    Err(e) => db_err(e),
                }
            }
            ("POST", "/internal/v1/entitlement") => {
                if !token_ok(req, &self.settings.internal_token) {
                    return err(401, "unauthorized", "Bearer token required.");
                }
                self.entitlement(req)
            }
            ("POST", "/internal/v1/cash/credit") => {
                if !token_ok(req, &self.settings.admin_token) {
                    return err(401, "unauthorized", "Admin token required.");
                }
                self.cash_credit(req)
            }
            ("POST", "/internal/v1/crypto/poll") => {
                if !token_ok(req, &self.settings.admin_token) {
                    return err(401, "unauthorized", "Admin token required.");
                }
                if self.crypto.cfg.ready().is_empty() {
                    return pay_err(PayError::NotConnected("no crypto backend".into()));
                }
                match crypto::poll_all(&self.crypto, &self.store, self.now()) {
                    Ok(r) => json_resp(
                        200,
                        json!({"ok": true, "open": r.open, "credited": r.credited,
                               "credited_eur_cents": r.credited_cents}),
                    ),
                    Err(crypto::CryptoError::Pay(e)) => pay_err(e),
                    Err(crypto::CryptoError::Store(e)) => db_err(e),
                }
            }
            ("GET", "/internal/v1/cash/lookup") => {
                if !token_ok(req, &self.settings.admin_token) {
                    return err(401, "unauthorized", "Admin token required.");
                }
                self.cash_lookup(req)
            }
            ("GET", "/internal/v1/admin/status") => {
                if !token_ok(req, &self.settings.admin_token) {
                    return err(401, "unauthorized", "Admin token required.");
                }
                self.admin_status()
            }
            // Loopback only and booleans only: safe without a token, so a
            // deploy script can check it before any token exists.
            ("GET", "/internal/v1/ready") => json_resp(200, self.readiness()),
            _ => err(404, "not_found", "Not found."),
        }
    }

    /// Which methods are configured, as booleans (never a value).
    pub fn readiness(&self) -> Value {
        let ready = self.crypto.cfg.ready();
        let stripe = self.stripe.status().is_connected();
        let cash = self.cash.status().is_connected();
        json!({
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
            "stripe": {
                "configured": stripe,
                "mode": self.stripe.cfg.mode(),
                "secret_key": self.stripe.cfg.secret_key.is_some(),
                "webhook_secret": self.stripe.cfg.webhook_secret.is_some(),
                "portal_config": self.stripe.cfg.portal_config.is_some(),
                "automatic_tax": self.stripe.cfg.automatic_tax,
            },
            "btcpay": {"configured": ready.contains(&crypto::Backend::BtcPay)},
            "monero": {"configured": ready.contains(&crypto::Backend::Monero)},
            "cash": {"configured": cash},
            "internal_token": self.settings.internal_token.is_some(),
            "admin_token": self.settings.admin_token.is_some(),
            "checkout_ready": stripe,
        })
    }

    fn admin_status(&self) -> Response {
        let now = self.now();
        match self.store.admin_stats(now) {
            Ok(s) => {
                let pairs = |v: &[(String, i64)]| {
                    Value::Object(v.iter().map(|(k, n)| (k.clone(), json!(n))).collect())
                };
                json_resp(
                    200,
                    json!({"ok": true, "now_label": galion_common::time::label(now),
                        "accounts": s.accounts, "active_plus": s.active_plus, "active_pro": s.active_pro,
                        "subscriptions": s.subscriptions, "past_due": s.past_due, "canceling": s.canceling,
                        "sessions": s.sessions,
                        "open_refs": pairs(&s.open_refs), "open_crypto": pairs(&s.open_crypto),
                        "duplicate_subscriptions": s.duplicate_subscriptions,
                        "duplicate_subscription_ids": self.store.duplicates().unwrap_or_default(),
                        "ledger": s.ledger.iter().map(|(m, me, c, n)| json!({"month": m, "method": me, "currency": c, "cents": n})).collect::<Vec<_>>(),
                        "readiness": self.readiness()}),
                )
            }
            Err(e) => db_err(e),
        }
    }

    /// `GET /internal/v1/cash/lookup?ref=`: what an envelope with this
    /// reference should hold. Never says which account.
    fn cash_lookup(&self, req: &Request) -> Response {
        let Some(r) = req
            .query()
            .split('&')
            .find_map(|kv| kv.strip_prefix("ref="))
            .and_then(cash::normalise_reference)
        else {
            return err(400, "bad_request", "Need ?ref=GC-XXXX-XXXX-XXXX.");
        };
        match self.store.pay_ref_info(&r, self.now()) {
            Ok(Some((m, plan, exp))) if m == Method::Cash.id() => json_resp(
                200,
                json!({"ok": true, "reference": r, "plan": plan.id(),
                       "eur_cents_per_month": plan.eur_cents(),
                       "expires_at": exp, "expires_label": galion_common::time::label(exp)}),
            ),
            Ok(_) => err(
                404,
                "unknown_reference",
                "No open cash reference with that code (used, expired or mistyped).",
            ),
            Err(e) => db_err(e),
        }
    }

    fn entitlement(&self, req: &Request) -> Response {
        let now = self.now();
        let Some(body) = body_json(req) else {
            return err(
                400,
                "bad_request",
                "Send {\"session\": \"...\"} or {\"number\": \"...\"}.",
            );
        };
        let found = if let Some(t) = body.get("session").and_then(|s| s.as_str()) {
            self.store.session_account(&session::token_hash(t), now)
        } else if let Some(n) = body.get("number").and_then(|s| s.as_str()) {
            // Same limiter as the public login, keyed on the caller.
            let key = format!(
                "internal:{}",
                body.get("client").and_then(|c| c.as_str()).unwrap_or("")
            );
            if let Err(r) = self.limiter.login_allowed(&key, now) {
                return refused(r);
            }
            match AccountNumber::parse(n) {
                Some(n) => {
                    let r = self.store.find_by_lookup(&self.hasher.lookup(&n));
                    if matches!(r, Ok(None)) {
                        self.limiter.login_failed(&key, now);
                    }
                    r
                }
                None => {
                    self.limiter.login_failed(&key, now);
                    Ok(None)
                }
            }
        } else {
            return err(
                400,
                "bad_request",
                "Send {\"session\": \"...\"} or {\"number\": \"...\"}.",
            );
        };
        match found {
            Ok(Some(a)) => json_resp(200, entitlement_json(&a, now)),
            Ok(None) => json_resp(
                200,
                json!({"ok": true, "plan": "free", "active": false, "known": false}),
            ),
            Err(e) => db_err(e),
        }
    }

    fn cash_credit(&self, req: &Request) -> Response {
        let Some(body) = body_json(req) else {
            return err(400, "bad_request", "JSON body required.");
        };
        let reference = body
            .get("reference")
            .and_then(|r| r.as_str())
            .and_then(cash::normalise_reference);
        let amount = body
            .get("amount_cents")
            .and_then(|a| a.as_i64())
            .filter(|a| *a > 0);
        let currency = body
            .get("currency")
            .and_then(|c| c.as_str())
            .unwrap_or("EUR")
            .to_ascii_uppercase();
        let (Some(reference), Some(amount)) = (reference, amount) else {
            return err(
                400,
                "bad_request",
                "Need reference (GC-XXXX-XXXX-XXXX) and amount_cents > 0.",
            );
        };
        match credit_cash(&self.store, &reference, amount, &currency, self.now()) {
            Ok(Some(v)) => json_resp(200, v),
            Ok(None) => err(
                404,
                "unknown_reference",
                "No open cash reference with that code (used, expired or mistyped).",
            ),
            Err(e) => db_err(e),
        }
    }
}

/// Operator credit for a cash envelope (HTTP and CLI share this).
/// Returns `None` when the reference is unknown, used or expired.
pub fn credit_cash(
    store: &Store,
    reference: &str,
    amount_cents: i64,
    currency: &str,
    now: i64,
) -> Result<Option<Value>, StoreError> {
    if currency != "EUR" {
        return Ok(Some(json!({"ok": false, "error": "currency",
            "message": "Only EUR is priced. Convert by hand and credit the EUR value."})));
    }
    if amount_cents <= 0 {
        return Ok(Some(
            json!({"ok": false, "error": "amount", "message": "amount_cents must be above 0."}),
        ));
    }
    let Some(reference) = cash::normalise_reference(reference) else {
        return Ok(None);
    };
    let reference = reference.as_str();
    let Some(p) = store.peek_pay_ref(reference, now)? else {
        return Ok(None);
    };
    if p.method != Method::Cash.id() {
        return Ok(None);
    }
    let hook = pay::Webhook {
        id: None,
        kind: "cash".into(),
        events: vec![pay::PayEvent::Credit {
            pay_ref: reference.to_string(),
            amount_cents,
            currency: currency.into(),
        }],
    };
    match pay::apply(store, Method::Cash, &hook, now)? {
        Applied::Done(_) => {
            let days = crate::plans::days_for(p.plan, amount_cents);
            Ok(Some(
                json!({"ok": true, "plan": p.plan.id(), "days_added": days,
                "message": format!("Added {days} days of {}. Shred the envelope and slip.", p.plan.name())}),
            ))
        }
        _ => Ok(None),
    }
}

const NOT_CONNECTED: &str = "This payment method is not connected yet.";

fn pay_err(e: PayError) -> Response {
    match e {
        PayError::NotConnected(_) => err(503, "not_connected", NOT_CONNECTED),
        PayError::Unsupported => err(404, "unsupported", "Not supported."),
        PayError::BadSignature(w) => err(400, "bad_signature", w),
        PayError::BadPayload(w) => err(400, "bad_payload", &w),
        PayError::Api(_, m) => err(
            502,
            "processor",
            &format!("The payment processor said: {m}"),
        ),
        PayError::Transport(_) => err(
            502,
            "processor_unreachable",
            "Could not reach the payment processor. Try again.",
        ),
    }
}

/// Serve public and internal listeners (each in its own thread pool).
pub fn serve(
    app: Arc<App>,
    public: std::net::TcpListener,
    internal: std::net::TcpListener,
) -> std::io::Result<()> {
    use galion_common::http::{self, Limits};
    let limits = Limits {
        max_body_bytes: 256 * 1024,
        ..Limits::default()
    };
    let a2 = app.clone();
    let l2 = limits.clone();
    std::thread::spawn(move || {
        let h: Arc<http::Handler> = Arc::new(move |r: &Request| a2.handle_internal(r));
        let _ = http::serve(internal, l2, h);
    });
    let h: Arc<http::Handler> = Arc::new(move |r: &Request| app.handle(r));
    http::serve(public, limits, h)
}
