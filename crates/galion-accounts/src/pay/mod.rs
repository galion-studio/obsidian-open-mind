// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Payment adapters behind one trait. An adapter never sees an account
//! number: it gets an opaque payment reference and reports normalised
//! [`PayEvent`]s, which [`apply`] turns into paid-until changes.

pub mod cash;
pub mod crypto;
pub mod json_transport;
pub mod stripe;
pub mod transport;

use crate::plans::Plan;
use crate::store::{Store, StoreError, DAY};

/// Which way a payment came in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Stripe,
    Crypto,
    Cash,
}

impl Method {
    pub fn id(self) -> &'static str {
        match self {
            Method::Stripe => "stripe",
            Method::Crypto => "crypto",
            Method::Cash => "cash",
        }
    }
}

/// Connected, or not, with a reason a person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Connected,
    NotConnected(String),
}

impl Status {
    pub fn is_connected(&self) -> bool {
        matches!(self, Status::Connected)
    }
}

/// Start a payment for `plan`, tagged with `pay_ref`.
#[derive(Debug, Clone)]
pub struct StartRequest {
    pub pay_ref: String,
    pub plan: Plan,
    /// The account's existing Stripe customer, so a second checkout does not
    /// create a second customer.
    pub existing_customer: Option<String>,
}

/// What the browser should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    /// Send the browser to the processor's page.
    Redirect(String),
    /// Show a reference to write down (cash) or pay to (crypto).
    Reference {
        reference: String,
        instructions: String,
    },
}

/// A normalised payment event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayEvent {
    /// Checkout finished: tie the processor's customer/subscription to the
    /// account behind `pay_ref`.
    Linked {
        pay_ref: String,
        customer: Option<String>,
        subscription: Option<String>,
    },
    /// A subscription period was paid: paid_until moves to `paid_until`.
    Renewed {
        subscription: Option<String>,
        customer: Option<String>,
        pay_ref: Option<String>,
        plan: Option<Plan>,
        paid_until: i64,
        amount_cents: i64,
        currency: String,
    },
    /// The plan of a running subscription changed (upgrade/downgrade).
    PlanChanged { subscription: String, plan: Plan },
    /// The subscription is over.
    Ended { subscription: String, at: i64 },
    /// A one-off top-up (cash, crypto): add time for `amount_cents`.
    Credit {
        pay_ref: String,
        amount_cents: i64,
        currency: String,
    },
}

/// A verified webhook: its id (for replay protection) and its events.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Webhook {
    pub id: Option<String>,
    pub kind: String,
    pub events: Vec<PayEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayError {
    NotConnected(String),
    Unsupported,
    /// Signature or timestamp check failed. Never retried.
    BadSignature(&'static str),
    BadPayload(String),
    /// The processor answered with an error (message from the processor,
    /// never a key).
    Api(u16, String),
    Transport(String),
}

impl std::fmt::Display for PayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PayError::NotConnected(r) => write!(f, "not connected: {r}"),
            PayError::Unsupported => f.write_str("not supported by this payment method"),
            PayError::BadSignature(w) => write!(f, "webhook signature rejected: {w}"),
            PayError::BadPayload(w) => write!(f, "bad payload: {w}"),
            PayError::Api(s, m) => write!(f, "processor error {s}: {m}"),
            PayError::Transport(m) => write!(f, "transport: {m}"),
        }
    }
}

/// One payment method.
pub trait PaymentAdapter: Send + Sync {
    fn method(&self) -> Method;
    fn status(&self) -> Status;
    /// Begin a payment.
    fn start(&self, req: &StartRequest) -> Result<StartOutcome, PayError>;
    /// Verify and parse a webhook delivery (raw body, as received).
    fn webhook(
        &self,
        headers: &[(String, String)],
        body: &[u8],
        now: i64,
    ) -> Result<Webhook, PayError>;
}

/// Grace added to a subscription's period end, so a renewal that is paid a
/// few hours late does not flicker the account to Free.
pub const RENEW_GRACE: i64 = DAY;

/// How applying a webhook went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// All events applied (or nothing to do).
    Done(usize),
    /// Already processed this webhook id.
    Replay,
    /// An event names a Galion payment we cannot match yet; ask the
    /// processor to retry later.
    Retry,
}

fn find_account(
    store: &Store,
    subscription: Option<&str>,
    customer: Option<&str>,
    pay_ref: Option<&str>,
    now: i64,
) -> Result<Option<(i64, Option<Plan>)>, StoreError> {
    if let Some(s) = subscription {
        if let Some(a) = store.find_by_subscription(s)? {
            return Ok(Some((a.id, None)));
        }
    }
    if let Some(c) = customer {
        if let Some(a) = store.find_by_customer(c)? {
            return Ok(Some((a.id, None)));
        }
    }
    if let Some(r) = pay_ref {
        if let Some(p) = store.peek_pay_ref(r, now)? {
            return Ok(Some((p.account_id, Some(p.plan))));
        }
    }
    Ok(None)
}

/// Apply a verified webhook to the store, once.
pub fn apply(
    store: &Store,
    method: Method,
    hook: &Webhook,
    now: i64,
) -> Result<Applied, StoreError> {
    if let Some(id) = &hook.id {
        if !store.mark_event(id, now)? {
            return Ok(Applied::Replay);
        }
    }
    let r = apply_events(store, method, &hook.events, now);
    match &r {
        Ok(Applied::Done(_)) => {}
        _ => {
            // Not applied: forget the id so the processor's retry gets in.
            if let Some(id) = &hook.id {
                store.unmark_event(id)?;
            }
        }
    }
    r
}

fn apply_events(
    store: &Store,
    method: Method,
    events: &[PayEvent],
    now: i64,
) -> Result<Applied, StoreError> {
    let mut n = 0;
    for ev in events {
        match ev {
            PayEvent::Linked {
                pay_ref,
                customer,
                subscription,
            } => {
                let Some(p) = store.take_pay_ref(pay_ref, now)? else {
                    // Already linked (renewal arrived first and we matched by
                    // subscription since) or expired: fine if the
                    // subscription is known.
                    if let Some(s) = subscription {
                        if store.find_by_subscription(s)?.is_some() {
                            continue;
                        }
                    }
                    return Ok(Applied::Retry);
                };
                store.link_stripe(
                    p.account_id,
                    customer.as_deref(),
                    subscription.as_deref(),
                    p.plan,
                )?;
                n += 1;
            }
            PayEvent::Renewed {
                subscription,
                customer,
                pay_ref,
                plan,
                paid_until,
                amount_cents,
                currency,
            } => {
                let Some((id, ref_plan)) = find_account(
                    store,
                    subscription.as_deref(),
                    customer.as_deref(),
                    pay_ref.as_deref(),
                    now,
                )?
                else {
                    return Ok(Applied::Retry);
                };
                if let (Some(p), true) = (ref_plan, subscription.is_some() || customer.is_some()) {
                    // Matched only by the reference: the invoice beat
                    // checkout.session.completed. Link now so the next renewal
                    // finds the account by subscription.
                    store.link_stripe(
                        id,
                        customer.as_deref(),
                        subscription.as_deref(),
                        plan.unwrap_or(p),
                    )?;
                }
                store.renew(id, plan.or(ref_plan), paid_until + RENEW_GRACE)?;
                if *amount_cents > 0 {
                    store.ledger_add(now, method.id(), *amount_cents, currency)?;
                }
                n += 1;
            }
            PayEvent::PlanChanged { subscription, plan } => {
                if let Some(a) = store.find_by_subscription(subscription)? {
                    store.renew(a.id, Some(*plan), 0)?;
                    n += 1;
                }
            }
            PayEvent::Ended { subscription, at } => {
                if let Some(a) = store.find_by_subscription(subscription)? {
                    store.end_stripe(a.id, *at)?;
                    n += 1;
                }
            }
            PayEvent::Credit {
                pay_ref,
                amount_cents,
                currency,
            } => {
                if currency != "EUR" || *amount_cents <= 0 {
                    // Only euros are priced (other currencies need an operator
                    // decision, DESIGN.md 7.3) and a credit needs a known amount.
                    return Ok(Applied::Retry);
                }
                let Some(p) = store.take_pay_ref(pay_ref, now)? else {
                    return Ok(Applied::Retry);
                };
                let days = crate::plans::days_for(p.plan, *amount_cents);
                store.add_days(p.account_id, p.plan, days, now)?;
                store.ledger_add(now, method.id(), *amount_cents, currency)?;
                n += 1;
            }
        }
    }
    Ok(Applied::Done(n))
}

/// A fresh opaque reference for online processors: `gp_` + 26 Crockford
/// characters (130 bits). Sent to Stripe instead of the account number.
pub fn new_online_ref() -> String {
    format!("gp_{}", crate::rng::crockford(26))
}
