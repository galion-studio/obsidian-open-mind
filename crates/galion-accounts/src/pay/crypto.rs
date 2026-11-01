// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cryptocurrency top-ups through two self-hosted backends, both **off by
//! default** (`ACCOUNTS_CRYPTO_BACKENDS` unset). No custodian holds the coins
//! and no third-party price API is hard-coded.
//!
//! * **BTCPay Server** (MIT), Greenfield API: `POST /api/v1/stores/{store}/
//!   invoices` in EUR with `metadata.orderId = <gp_ ref>`, the browser goes
//!   to BTCPay's checkout, and an `InvoiceSettled` webhook signed with
//!   `BTCPay-Sig: sha256=<hex HMAC-SHA256(secret, raw body)>` credits it. The
//!   amount is read back from BTCPay with the API key (`GET .../invoices/{id}`),
//!   never taken from the webhook body.
//! * **monero-wallet-rpc** (BSD-3-Clause): a new subaddress
//!   (`create_address`) or integrated address (`make_integrated_address`) per
//!   invoice, the EUR price converted to XMR at a rate from config
//!   (`MONERO_EUR_RATE`) or from the BTCPay store rate endpoint
//!   (`GET /api/v1/stores/{store}/rates?currencyPair=XMR_EUR`), and a poller
//!   that sums `get_transfers` (`in`) per address with at least
//!   `MONERO_CONFIRMATIONS` confirmations.
//!
//! Crypto tops up time like cash (no subscription): the EUR value received is
//! converted to days of the chosen plan, pro rata ([`crate::plans::days_for`]).
//! Stored per invoice: the payment reference (whose row is the only account
//! link and is cut on credit) and the processor's invoice id. Both rows are
//! deleted [`CRYPTO_SETTLE_WINDOW`] after settlement or expiry.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Value};

use super::json_transport::{Auth, JsonRequest, JsonTransport, MockJsonTransport};
use super::{
    Applied, Method, PayError, PayEvent, PaymentAdapter, StartOutcome, StartRequest, Status,
    Webhook,
};
use crate::plans::Plan;
use crate::store::{CryptoInvoice, Store, StoreError, CRYPTO_SETTLE_WINDOW};

/// A crypto backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    BtcPay,
    Monero,
}

impl Backend {
    pub fn id(self) -> &'static str {
        match self {
            Backend::BtcPay => "btcpay",
            Backend::Monero => "monero",
        }
    }

    pub fn parse(s: &str) -> Option<Backend> {
        match s.trim() {
            "btcpay" => Some(Backend::BtcPay),
            "monero" | "monero-wallet-rpc" | "xmr" => Some(Backend::Monero),
            _ => None,
        }
    }

    /// What the visitor sees on the button.
    pub fn label(self) -> &'static str {
        match self {
            Backend::BtcPay => "Bitcoin (BTCPay)",
            Backend::Monero => "Monero",
        }
    }
}

/// How the Monero wallet gives out one address per invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressMode {
    /// `create_address`: a fresh subaddress (recommended).
    Subaddress,
    /// `make_integrated_address`: primary address + random 8-byte payment id.
    Integrated,
}

/// Where the EUR -> XMR rate comes from. There is no built-in price API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmrRate {
    /// Operator-set EUR per 1 XMR, fixed-point with 8 decimals.
    Fixed(u128),
    /// The BTCPay store rate endpoint (pair `XMR_EUR`), with BTCPay's own
    /// configured rate source.
    BtcPay,
}

#[derive(Clone, Default)]
pub struct BtcPayConfig {
    /// e.g. `https://pay.galion.app` (no trailing slash).
    pub url: Option<String>,
    pub store_id: Option<String>,
    pub api_key: Option<String>,
    pub webhook_secret: Option<String>,
    /// Invoice expiry (BTCPay's price lock), minutes.
    pub invoice_minutes: i64,
}

#[derive(Clone)]
pub struct MoneroConfig {
    /// e.g. `http://127.0.0.1:18083/json_rpc`.
    pub rpc_url: Option<String>,
    /// `user:pass` matching `monero-wallet-rpc --rpc-login`.
    pub rpc_login: Option<(String, String)>,
    pub account_index: u32,
    pub mode: AddressMode,
    pub confirmations: u64,
    /// Price lock, minutes.
    pub invoice_minutes: i64,
    pub rate: Option<XmrRate>,
    pub poll_seconds: u64,
}

impl Default for MoneroConfig {
    fn default() -> Self {
        MoneroConfig {
            rpc_url: None,
            rpc_login: None,
            account_index: 0,
            mode: AddressMode::Subaddress,
            confirmations: 10,
            invoice_minutes: 60,
            rate: None,
            poll_seconds: 60,
        }
    }
}

#[derive(Clone, Default)]
pub struct CryptoConfig {
    /// Enabled backends, in the order shown. Empty = crypto is off.
    pub backends: Vec<Backend>,
    pub btcpay: BtcPayConfig,
    pub monero: MoneroConfig,
    /// Base URL for BTCPay's "back to Galion" link.
    pub public_url: String,
}

impl std::fmt::Debug for CryptoConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let set = |o: &Option<String>| o.as_ref().map(|_| "<set>");
        f.debug_struct("CryptoConfig")
            .field("backends", &self.backends)
            .field("btcpay_url", &self.btcpay.url)
            .field("btcpay_store_id", &self.btcpay.store_id)
            .field("btcpay_api_key", &set(&self.btcpay.api_key))
            .field("btcpay_webhook_secret", &set(&self.btcpay.webhook_secret))
            .field("monero_rpc_url", &self.monero.rpc_url)
            .field(
                "monero_rpc_login",
                &self.monero.rpc_login.as_ref().map(|_| "<set>"),
            )
            .field("monero_mode", &self.monero.mode)
            .field("monero_confirmations", &self.monero.confirmations)
            .field("monero_rate", &self.monero.rate)
            .finish()
    }
}

fn num<T: std::str::FromStr>(get: &impl Fn(&str) -> Option<String>, k: &str) -> Option<T> {
    get(k).and_then(|v| v.trim().parse().ok())
}

impl CryptoConfig {
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> CryptoConfig {
        let get = |k: &str| get(k).filter(|s| !s.trim().is_empty());
        let list = get("ACCOUNTS_CRYPTO_BACKENDS")
            .or_else(|| get("ACCOUNTS_CRYPTO_BACKEND"))
            .unwrap_or_default();
        let mut backends = Vec::new();
        for b in list.split(',').filter_map(Backend::parse) {
            if !backends.contains(&b) {
                backends.push(b);
            }
        }
        let rate = match get("MONERO_RATE_SOURCE").as_deref() {
            Some("btcpay") => Some(XmrRate::BtcPay),
            _ => get("MONERO_EUR_RATE")
                .and_then(|r| parse_decimal(&r, 8))
                .filter(|r| *r > 0)
                .map(XmrRate::Fixed),
        };
        CryptoConfig {
            backends,
            btcpay: BtcPayConfig {
                url: get("BTCPAY_URL").map(|u| u.trim_end_matches('/').to_string()),
                store_id: get("BTCPAY_STORE_ID"),
                api_key: get("BTCPAY_API_KEY"),
                webhook_secret: get("BTCPAY_WEBHOOK_SECRET"),
                invoice_minutes: num(&get, "BTCPAY_INVOICE_MINUTES")
                    .filter(|m: &i64| (15..=1440).contains(m))
                    .unwrap_or(60),
            },
            monero: MoneroConfig {
                rpc_url: get("MONERO_WALLET_RPC_URL"),
                rpc_login: get("MONERO_WALLET_RPC_LOGIN").and_then(|l| {
                    l.split_once(':')
                        .map(|(u, p)| (u.to_string(), p.to_string()))
                }),
                account_index: num(&get, "MONERO_ACCOUNT_INDEX").unwrap_or(0),
                mode: match get("MONERO_ADDRESS_MODE").as_deref() {
                    Some("integrated") => AddressMode::Integrated,
                    _ => AddressMode::Subaddress,
                },
                confirmations: num(&get, "MONERO_CONFIRMATIONS")
                    .filter(|c: &u64| *c >= 1)
                    .unwrap_or(10),
                invoice_minutes: num(&get, "MONERO_INVOICE_MINUTES")
                    .filter(|m: &i64| (15..=1440).contains(m))
                    .unwrap_or(60),
                rate,
                poll_seconds: num(&get, "MONERO_POLL_SECONDS")
                    .filter(|s: &u64| *s >= 10)
                    .unwrap_or(60),
            },
            public_url: get("ACCOUNTS_PUBLIC_URL")
                .unwrap_or_else(|| "https://galion.app".into())
                .trim_end_matches('/')
                .to_string(),
        }
    }

    fn btcpay_api_ready(&self) -> bool {
        self.btcpay.url.is_some() && self.btcpay.store_id.is_some() && self.btcpay.api_key.is_some()
    }

    /// Why `b` cannot take payments, or `None` when it can.
    pub fn missing(&self, b: Backend) -> Option<&'static str> {
        if !self.backends.contains(&b) {
            return Some("not enabled (ACCOUNTS_CRYPTO_BACKENDS)");
        }
        match b {
            Backend::BtcPay => {
                if !self.btcpay_api_ready() {
                    Some("BTCPAY_URL, BTCPAY_STORE_ID and BTCPAY_API_KEY are required")
                } else if self.btcpay.webhook_secret.is_none() {
                    Some("BTCPAY_WEBHOOK_SECRET is required")
                } else {
                    None
                }
            }
            Backend::Monero => {
                if self.monero.rpc_url.is_none() {
                    Some("MONERO_WALLET_RPC_URL is required")
                } else {
                    match self.monero.rate {
                        None => Some("set MONERO_EUR_RATE or MONERO_RATE_SOURCE=btcpay"),
                        Some(XmrRate::BtcPay) if !self.btcpay_api_ready() => Some(
                            "MONERO_RATE_SOURCE=btcpay needs BTCPAY_URL, BTCPAY_STORE_ID and BTCPAY_API_KEY",
                        ),
                        _ => None,
                    }
                }
            }
        }
    }

    /// Backends that are enabled and fully configured.
    pub fn ready(&self) -> Vec<Backend> {
        self.backends
            .iter()
            .copied()
            .filter(|b| self.missing(*b).is_none())
            .collect()
    }
}

pub struct Crypto {
    pub cfg: CryptoConfig,
    pub http: Arc<dyn JsonTransport>,
}

impl Crypto {
    pub fn new(cfg: CryptoConfig, http: Arc<dyn JsonTransport>) -> Crypto {
        Crypto { cfg, http }
    }

    /// Crypto switched off (the default).
    pub fn disabled() -> Crypto {
        Crypto::new(
            CryptoConfig::default(),
            Arc::new(MockJsonTransport::default()),
        )
    }
}

// ------------------------------------------------------------------ numbers

/// Parse a non-negative decimal (`"152.37"`) into an integer scaled by
/// 10^`scale`, truncating extra digits. No floats anywhere in money paths.
pub fn parse_decimal(s: &str, scale: u32) -> Option<u128> {
    let s = s.trim();
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    if !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if int.len() > 20 {
        return None;
    }
    let mut v: u128 = if int.is_empty() { 0 } else { int.parse().ok()? };
    for i in 0..scale as usize {
        let d = frac
            .as_bytes()
            .get(i)
            .map(|b| (b - b'0') as u128)
            .unwrap_or(0);
        v = v.checked_mul(10)?.checked_add(d)?;
    }
    Some(v)
}

/// Piconero due for `eur_cents` at `rate_e8` EUR per XMR, rounded up.
/// XMR = (cents / 100) / (rate_e8 / 1e8) and 1 XMR = 1e12 piconero, so
/// piconero = cents * 1e18 / rate_e8.
pub fn piconero_due(eur_cents: i64, rate_e8: u128) -> Option<i64> {
    if eur_cents <= 0 || rate_e8 == 0 {
        return None;
    }
    let n = (eur_cents as u128).checked_mul(1_000_000_000_000_000_000)?;
    let v = n.div_ceil(rate_e8);
    i64::try_from(v).ok()
}

/// `131234567890` piconero -> `"0.13123456789"`.
pub fn fmt_xmr(piconero: i64) -> String {
    let p = piconero.max(0);
    let whole = p / 1_000_000_000_000;
    let frac = p % 1_000_000_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let f = format!("{frac:012}");
    format!("{whole}.{}", f.trim_end_matches('0'))
}

/// EUR cents for `received` of `due` piconero on an invoice worth `cents`
/// (pro rata, rounded down; overpayment counts too, like cash).
pub fn pro_rata_cents(received: i64, due: i64, cents: i64) -> i64 {
    if received <= 0 || due <= 0 || cents <= 0 {
        return 0;
    }
    let v = (received as i128) * (cents as i128) / (due as i128);
    v.min(i64::MAX as i128) as i64
}

fn as_decimal_str(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// ------------------------------------------------------------------ BTCPay

/// Check `BTCPay-Sig: sha256=<hex>` over the raw body.
pub fn verify_btcpay(header: Option<&str>, body: &[u8], secret: &str) -> bool {
    let Some(h) = header.and_then(|h| h.trim().strip_prefix("sha256=")) else {
        return false;
    };
    let want = crate::hmac::hmac_sha256_hex(secret.as_bytes(), body);
    crate::ct_eq(h.to_ascii_lowercase().as_bytes(), want.as_bytes())
}

/// `InvoiceSettled` -> a [`PayEvent::Credit`] for the invoice's orderId.
/// The amount must come from the invoice itself (GET), not the webhook, so
/// it is left at 0 here and nothing is credited from an unverified amount.
pub fn parse_btcpay(v: &Value) -> Result<Webhook, PayError> {
    let id = v
        .get("deliveryId")
        .and_then(|i| i.as_str())
        .map(|s| format!("btcpay:{s}"));
    let kind = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let mut events = Vec::new();
    if kind == "InvoiceSettled" {
        if let Some(r) = v.pointer("/metadata/orderId").and_then(|r| r.as_str()) {
            if r.starts_with("gp_") {
                events.push(PayEvent::Credit {
                    pay_ref: r.to_string(),
                    amount_cents: 0,
                    currency: "EUR".into(),
                });
            }
        }
    }
    Ok(Webhook { id, kind, events })
}

/// A verified BTCPay webhook delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtcPayHook {
    pub kind: String,
    pub invoice_id: Option<String>,
    pub store_id: Option<String>,
}

/// What an invoice is worth, per [`Crypto::btcpay_value`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtcPayValue {
    Credit(BtcPaySettled),
    /// Payments seen but not confirmed yet: look again later.
    Pending,
    Nothing,
}

/// A created BTCPay invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtcPayInvoice {
    pub id: String,
    pub checkout_link: String,
    pub expires_at: i64,
}

/// What BTCPay says an invoice is worth, once settled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtcPaySettled {
    pub order_id: String,
    pub amount_cents: i64,
    pub currency: String,
}

impl Crypto {
    fn btcpay_req(
        &self,
        method: &'static str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, PayError> {
        let (Some(url), Some(key)) = (&self.cfg.btcpay.url, &self.cfg.btcpay.api_key) else {
            return Err(PayError::NotConnected("BTCPay is not configured".into()));
        };
        let r = self.http.send_json(&JsonRequest {
            method,
            url: format!("{url}{path}"),
            auth: Auth::Token(key.clone()),
            body: body.map(|b| b.to_string()),
        })?;
        let v: Value = serde_json::from_str(&r.body).unwrap_or(Value::Null);
        if !(200..300).contains(&r.status) {
            let m = v
                .get("message")
                .and_then(|m| m.as_str())
                .or_else(|| v.pointer("/0/message").and_then(|m| m.as_str()))
                .unwrap_or("request failed")
                .chars()
                .take(200)
                .collect();
            return Err(PayError::Api(r.status, m));
        }
        Ok(v)
    }

    fn store_path(&self) -> Result<String, PayError> {
        let s = self
            .cfg
            .btcpay
            .store_id
            .as_deref()
            .ok_or_else(|| PayError::NotConnected("BTCPAY_STORE_ID is not set".into()))?;
        Ok(format!("/api/v1/stores/{}", pct(s)))
    }

    /// Create a EUR invoice for one month of `plan`, tagged with `pay_ref`.
    /// Only the reference goes to BTCPay: no account number, no email (refund
    /// email is switched off).
    pub fn btcpay_create_invoice(
        &self,
        pay_ref: &str,
        plan: Plan,
        now: i64,
    ) -> Result<BtcPayInvoice, PayError> {
        let cents = plan.eur_cents();
        let minutes = self.cfg.btcpay.invoice_minutes;
        let body = json!({
            "amount": format!("{}.{:02}", cents / 100, cents % 100),
            "currency": "EUR",
            "metadata": {"orderId": pay_ref},
            "checkout": {
                "expirationMinutes": minutes,
                "redirectURL": format!("{}/account?crypto=done", self.cfg.public_url),
                "redirectAutomatically": false,
                "requiresRefundEmail": false,
            },
        });
        let v = self.btcpay_req(
            "POST",
            &format!("{}/invoices", self.store_path()?),
            Some(body),
        )?;
        let id = v
            .get("id")
            .and_then(|i| i.as_str())
            .ok_or_else(|| PayError::BadPayload("invoice without id".into()))?;
        let link = v
            .get("checkoutLink")
            .and_then(|i| i.as_str())
            .ok_or_else(|| PayError::BadPayload("invoice without checkoutLink".into()))?;
        let expires_at = v
            .get("expirationTime")
            .and_then(|t| t.as_i64())
            .unwrap_or(now + minutes * 60);
        Ok(BtcPayInvoice {
            id: id.to_string(),
            checkout_link: link.to_string(),
            expires_at,
        })
    }

    /// What a BTCPay invoice is worth now, read back from BTCPay (never from
    /// a webhook body):
    ///
    /// * `Settled` -> the invoice amount; `Settled` + `PaidOver` -> pro rata
    ///   of the confirmed payments (more time for more money).
    /// * `Expired` + `PaidPartial` -> pro rata of the *confirmed* payments;
    ///   while any payment is still unconfirmed -> [`BtcPayValue::Pending`].
    /// * anything else -> [`BtcPayValue::Nothing`] (not paid, or still open).
    pub fn btcpay_value(&self, invoice_id: &str) -> Result<BtcPayValue, PayError> {
        let path = format!("{}/invoices/{}", self.store_path()?, pct(invoice_id));
        let v = self.btcpay_req("GET", &path, None)?;
        let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
        let extra = v
            .get("additionalStatus")
            .and_then(|s| s.as_str())
            .unwrap_or("None");
        // PaidLate: money arrived after the window; credit what confirmed.
        let partial = status == "Expired" && matches!(extra, "PaidPartial" | "PaidLate");
        let over = status == "Settled" && matches!(extra, "PaidOver" | "PaidLate");
        if status == "Processing" {
            return Ok(BtcPayValue::Pending);
        }
        if status != "Settled" && !partial {
            return Ok(BtcPayValue::Nothing);
        }
        let Some(full) = self.btcpay_settled_doc(&v) else {
            return Err(PayError::BadPayload("invoice is not a EUR amount".into()));
        };
        if !(partial || over) {
            return Ok(BtcPayValue::Credit(full));
        }
        // Pro rata from the payment methods: confirmed value / amount due,
        // in the method's own currency (integer, 1e-12 units).
        let pm = self.btcpay_req("GET", &format!("{path}/payment-methods"), None)?;
        let mut paid_frac_e6: i64 = 0;
        let mut pending = false;
        for m in pm.as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            let due = as_decimal_str(m.get("amount")).and_then(|a| parse_decimal(&a, 12));
            let Some(due) = due.filter(|d| *d > 0) else {
                continue;
            };
            let mut settled: u128 = 0;
            for p in m
                .get("payments")
                .and_then(|p| p.as_array())
                .map(|a| a.as_slice())
                .unwrap_or(&[])
            {
                let val = as_decimal_str(p.get("value"))
                    .and_then(|a| parse_decimal(&a, 12))
                    .unwrap_or(0);
                match p.get("status").and_then(|s| s.as_str()) {
                    Some("Settled") => settled += val,
                    Some("Processing") => pending = true,
                    _ => {}
                }
            }
            let frac = (settled.saturating_mul(1_000_000) / due).min(i64::MAX as u128) as i64;
            paid_frac_e6 = paid_frac_e6.saturating_add(frac);
        }
        if pending {
            return Ok(BtcPayValue::Pending);
        }
        // At most a year of time for one invoice (a fat-finger overpay is
        // refunded by hand instead).
        let paid_frac_e6 = paid_frac_e6.min(12_000_000);
        let mut cents =
            (i128::from(full.amount_cents) * i128::from(paid_frac_e6) / 1_000_000) as i64;
        if over {
            cents = cents.max(full.amount_cents);
        }
        if cents <= 0 {
            return Ok(BtcPayValue::Nothing);
        }
        Ok(BtcPayValue::Credit(BtcPaySettled {
            amount_cents: cents,
            ..full
        }))
    }

    /// Read an invoice back. `Ok(None)` unless BTCPay itself says Settled.
    pub fn btcpay_settled(&self, invoice_id: &str) -> Result<Option<BtcPaySettled>, PayError> {
        let v = self.btcpay_req(
            "GET",
            &format!("{}/invoices/{}", self.store_path()?, pct(invoice_id)),
            None,
        )?;
        if v.get("status").and_then(|s| s.as_str()) != Some("Settled") {
            return Ok(None);
        }
        Ok(self.btcpay_settled_doc(&v))
    }

    fn btcpay_settled_doc(&self, v: &Value) -> Option<BtcPaySettled> {
        let order_id = v
            .pointer("/metadata/orderId")
            .and_then(|o| o.as_str())
            .unwrap_or("")
            .to_string();
        let currency = v
            .get("currency")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        let amount_cents = as_decimal_str(v.get("amount"))
            .and_then(|a| parse_decimal(&a, 2))
            .and_then(|c| i64::try_from(c).ok())
            .unwrap_or(0);
        if currency != "EUR" || amount_cents <= 0 {
            return None;
        }
        Some(BtcPaySettled {
            order_id,
            amount_cents,
            currency,
        })
    }

    /// EUR per 1 unit of `asset`, scaled by 1e8, from the BTCPay store's own
    /// rate source (`GET /api/v1/stores/{store}/rates?currencyPair=XMR_EUR`).
    pub fn btcpay_rate_e8(&self, asset: &str) -> Result<u128, PayError> {
        let pair = format!("{asset}_EUR");
        let v = self.btcpay_req(
            "GET",
            &format!("{}/rates?currencyPair={}", self.store_path()?, pct(&pair)),
            None,
        )?;
        let row = v
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|r| r.get("currencyPair").and_then(|p| p.as_str()) == Some(pair.as_str()))
            })
            .ok_or_else(|| PayError::BadPayload(format!("no {pair} rate from BTCPay")))?;
        if row
            .get("errors")
            .and_then(|e| e.as_array())
            .is_some_and(|e| !e.is_empty())
        {
            return Err(PayError::BadPayload(format!(
                "BTCPay has no usable {pair} rate"
            )));
        }
        as_decimal_str(row.get("rate"))
            .and_then(|r| parse_decimal(&r, 8))
            .filter(|r| *r > 0)
            .ok_or_else(|| PayError::BadPayload(format!("bad {pair} rate from BTCPay")))
    }

    /// Verify a BTCPay webhook (`BTCPay-Sig`) and read its fields.
    pub fn btcpay_webhook(
        &self,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<BtcPayHook, PayError> {
        if !self.cfg.backends.contains(&Backend::BtcPay) {
            return Err(PayError::NotConnected("BTCPay is not enabled".into()));
        }
        let Some(secret) = &self.cfg.btcpay.webhook_secret else {
            return Err(PayError::NotConnected(
                "BTCPAY_WEBHOOK_SECRET is not set".into(),
            ));
        };
        let h = galion_common::http::header(headers, "btcpay-sig");
        if !verify_btcpay(h, body, secret) {
            return Err(PayError::BadSignature("BTCPay-Sig mismatch"));
        }
        let v: Value =
            serde_json::from_slice(body).map_err(|_| PayError::BadPayload("not JSON".into()))?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        Ok(BtcPayHook {
            kind: s("type").unwrap_or_default(),
            invoice_id: s("invoiceId"),
            store_id: s("storeId"),
        })
    }
}

/// Percent-encode one URL path/query component.
fn pct(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

// ------------------------------------------------------------------ Monero

/// A fresh Monero address for one invoice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneroAddress {
    pub address: String,
    /// `sub:<account>/<index>` or `pid:<payment id>`: what the poller matches on.
    pub invoice_id: String,
}

impl Crypto {
    fn monero_rpc(&self, method: &str, params: Value) -> Result<Value, PayError> {
        let Some(url) = &self.cfg.monero.rpc_url else {
            return Err(PayError::NotConnected(
                "MONERO_WALLET_RPC_URL is not set".into(),
            ));
        };
        let auth = match &self.cfg.monero.rpc_login {
            Some((u, p)) => Auth::Digest {
                user: u.clone(),
                pass: p.clone(),
            },
            None => Auth::None,
        };
        let body = json!({"jsonrpc": "2.0", "id": "0", "method": method, "params": params});
        let r = self.http.send_json(&JsonRequest {
            method: "POST",
            url: url.clone(),
            auth,
            body: Some(body.to_string()),
        })?;
        if !(200..300).contains(&r.status) {
            return Err(PayError::Api(
                r.status,
                format!("wallet-rpc {method} failed"),
            ));
        }
        let v: Value = serde_json::from_str(&r.body)
            .map_err(|_| PayError::BadPayload("wallet-rpc answered non-JSON".into()))?;
        if let Some(e) = v.get("error") {
            let m: String = e
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("error")
                .chars()
                .take(200)
                .collect();
            return Err(PayError::Api(r.status, format!("wallet-rpc {method}: {m}")));
        }
        v.get("result")
            .cloned()
            .ok_or_else(|| PayError::BadPayload("wallet-rpc answer without result".into()))
    }

    /// EUR per XMR (1e8 scale) from the configured source.
    pub fn xmr_rate_e8(&self) -> Result<u128, PayError> {
        match self.cfg.monero.rate {
            Some(XmrRate::Fixed(r)) => Ok(r),
            Some(XmrRate::BtcPay) => self.btcpay_rate_e8("XMR"),
            None => Err(PayError::NotConnected("no EUR/XMR rate source".into())),
        }
    }

    /// One new address. No label is set in the wallet (nothing that could
    /// tie it to an account).
    pub fn monero_new_address(&self) -> Result<MoneroAddress, PayError> {
        let acct = self.cfg.monero.account_index;
        match self.cfg.monero.mode {
            AddressMode::Subaddress => {
                let r = self.monero_rpc("create_address", json!({"account_index": acct}))?;
                let address = r.get("address").and_then(|a| a.as_str());
                let index = r.get("address_index").and_then(|i| i.as_u64());
                match (address, index) {
                    (Some(a), Some(i)) => Ok(MoneroAddress {
                        address: a.to_string(),
                        invoice_id: format!("sub:{acct}/{i}"),
                    }),
                    _ => Err(PayError::BadPayload(
                        "create_address: missing fields".into(),
                    )),
                }
            }
            AddressMode::Integrated => {
                let r = self.monero_rpc("make_integrated_address", json!({}))?;
                let address = r.get("integrated_address").and_then(|a| a.as_str());
                let pid = r.get("payment_id").and_then(|i| i.as_str());
                match (address, pid) {
                    (Some(a), Some(p))
                        if p.len() == 16 && p.bytes().all(|b| b.is_ascii_hexdigit()) =>
                    {
                        Ok(MoneroAddress {
                            address: a.to_string(),
                            invoice_id: format!("pid:{}", p.to_ascii_lowercase()),
                        })
                    }
                    _ => Err(PayError::BadPayload(
                        "make_integrated_address: missing fields".into(),
                    )),
                }
            }
        }
    }

    /// Confirmed piconero received per invoice id, for the given open
    /// invoices. Counts only incoming transfers with at least
    /// `confirmations`, no double-spend flag and no custom unlock time.
    pub fn monero_received(
        &self,
        open: &[CryptoInvoice],
    ) -> Result<HashMap<String, i64>, PayError> {
        let acct = self.cfg.monero.account_index;
        let mut subs: Vec<u64> = Vec::new();
        let mut want_pid = false;
        for i in open {
            if let Some(rest) = i.invoice_id.strip_prefix("sub:") {
                if let Some((a, m)) = rest.split_once('/') {
                    if a.parse::<u32>().ok() == Some(acct) {
                        if let Ok(m) = m.parse() {
                            subs.push(m);
                        }
                    }
                }
            } else if i.invoice_id.starts_with("pid:") {
                want_pid = true;
            }
        }
        let mut out = HashMap::new();
        if subs.is_empty() && !want_pid {
            return Ok(out);
        }
        let mut params = json!({"in": true, "account_index": acct});
        if !want_pid {
            params["subaddr_indices"] = json!(subs);
        }
        let r = self.monero_rpc("get_transfers", params)?;
        let need = self.cfg.monero.confirmations;
        for t in r.get("in").and_then(|a| a.as_array()).into_iter().flatten() {
            let conf = t.get("confirmations").and_then(|c| c.as_u64()).unwrap_or(0);
            let dbl = t
                .get("double_spend_seen")
                .and_then(|d| d.as_bool())
                .unwrap_or(false);
            let unlock = t.get("unlock_time").and_then(|u| u.as_u64()).unwrap_or(0);
            let amount = t.get("amount").and_then(|a| a.as_u64()).unwrap_or(0);
            if conf < need || dbl || unlock != 0 || amount == 0 {
                continue;
            }
            let pid = t
                .get("payment_id")
                .and_then(|p| p.as_str())
                .map(str::to_ascii_lowercase)
                .filter(|p| p.len() == 16 && p != "0000000000000000");
            let key = match pid {
                Some(p) => format!("pid:{p}"),
                None => {
                    let major = t.pointer("/subaddr_index/major").and_then(|m| m.as_u64());
                    let minor = t.pointer("/subaddr_index/minor").and_then(|m| m.as_u64());
                    match (major, minor) {
                        (Some(a), Some(m)) if m > 0 => format!("sub:{a}/{m}"),
                        _ => continue,
                    }
                }
            };
            let e = out.entry(key).or_insert(0i64);
            *e = e.saturating_add(i64::try_from(amount).unwrap_or(i64::MAX));
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------ flows

/// What the browser gets back from starting a crypto payment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoStart {
    /// BTCPay checkout page.
    Redirect { url: String },
    /// Pay this much XMR to this address before `expires_at`.
    Monero {
        address: String,
        amount_xmr: String,
        uri: String,
        expires_at: i64,
        confirmations: u64,
    },
}

#[derive(Debug)]
pub enum CryptoError {
    Pay(PayError),
    Store(StoreError),
}

impl From<PayError> for CryptoError {
    fn from(e: PayError) -> Self {
        CryptoError::Pay(e)
    }
}

impl From<StoreError> for CryptoError {
    fn from(e: StoreError) -> Self {
        CryptoError::Store(e)
    }
}

/// Price lock + settlement grace before a partly paid Monero invoice is
/// credited pro rata (enough for 10 confirmations with margin).
pub const MONERO_PARTIAL_GRACE: i64 = 2 * 3600;

/// Start a crypto payment of one month of `plan` for `account_id` with
/// `backend`. Stores the payment reference (account link, expires after the
/// price lock + [`CRYPTO_SETTLE_WINDOW`]) and the invoice id, nothing else.
pub fn start(
    crypto: &Crypto,
    store: &Store,
    backend: Backend,
    account_id: i64,
    plan: Plan,
    now: i64,
) -> Result<CryptoStart, CryptoError> {
    if let Some(why) = crypto.cfg.missing(backend) {
        return Err(PayError::NotConnected(why.into()).into());
    }
    let pay_ref = super::new_online_ref();
    match backend {
        Backend::BtcPay => {
            let lock = now + crypto.cfg.btcpay.invoice_minutes * 60;
            store.put_pay_ref(
                &pay_ref,
                account_id,
                Method::Crypto.id(),
                plan,
                lock + CRYPTO_SETTLE_WINDOW,
            )?;
            let inv = crypto.btcpay_create_invoice(&pay_ref, plan, now)?;
            store.put_crypto_invoice(
                &pay_ref,
                backend.id(),
                &inv.id,
                0,
                inv.expires_at.max(lock),
            )?;
            Ok(CryptoStart::Redirect {
                url: inv.checkout_link,
            })
        }
        Backend::Monero => {
            let rate = crypto.xmr_rate_e8()?;
            let due = piconero_due(plan.eur_cents(), rate)
                .ok_or_else(|| PayError::BadPayload("cannot price this plan in XMR".into()))?;
            let lock = now + crypto.cfg.monero.invoice_minutes * 60;
            store.put_pay_ref(
                &pay_ref,
                account_id,
                Method::Crypto.id(),
                plan,
                lock + CRYPTO_SETTLE_WINDOW,
            )?;
            let addr = crypto.monero_new_address()?;
            store.put_crypto_invoice(&pay_ref, backend.id(), &addr.invoice_id, due, lock)?;
            let amount_xmr = fmt_xmr(due);
            Ok(CryptoStart::Monero {
                uri: format!("monero:{}?tx_amount={amount_xmr}", addr.address),
                address: addr.address,
                amount_xmr,
                expires_at: lock,
                confirmations: crypto.cfg.monero.confirmations,
            })
        }
    }
}

/// Result of handling one BTCPay webhook delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookOutcome {
    /// Credited this many EUR cents.
    Credited(i64),
    /// Already credited (redelivery).
    Replay,
    /// Not an event we act on, or not our invoice.
    Ignored(&'static str),
    /// Cannot match yet; BTCPay should retry.
    Retry,
}

/// Handle a BTCPay webhook end to end: verify the signature, ignore all but
/// `InvoiceSettled` for our store, look the invoice up by id, read the
/// amount back from BTCPay, credit pro rata and mark it settled.
pub fn btcpay_hook(
    crypto: &Crypto,
    store: &Store,
    headers: &[(String, String)],
    body: &[u8],
    now: i64,
) -> Result<HookOutcome, CryptoError> {
    let hook = crypto.btcpay_webhook(headers, body)?;
    // Settled: paid (maybe overpaid). Expired: maybe partly paid. The
    // value is always read back from BTCPay.
    if !matches!(hook.kind.as_str(), "InvoiceSettled" | "InvoiceExpired") {
        return Ok(HookOutcome::Ignored("event type"));
    }
    if hook.store_id.is_some() && hook.store_id != crypto.cfg.btcpay.store_id {
        return Ok(HookOutcome::Ignored("other store"));
    }
    let Some(inv_id) = hook.invoice_id else {
        return Ok(HookOutcome::Ignored("no invoice id"));
    };
    let Some(inv) = store.crypto_invoice(&inv_id)? else {
        return Ok(HookOutcome::Ignored("unknown invoice"));
    };
    if inv.backend != Backend::BtcPay.id() {
        return Ok(HookOutcome::Ignored("unknown invoice"));
    }
    if inv.settled_at.is_some() {
        return Ok(HookOutcome::Replay);
    }
    let s = match crypto.btcpay_value(&inv_id)? {
        BtcPayValue::Credit(s) => s,
        // The reconcile poll picks it up once confirmed.
        BtcPayValue::Pending => return Ok(HookOutcome::Ignored("payment not confirmed yet")),
        BtcPayValue::Nothing if hook.kind == "InvoiceExpired" => {
            return Ok(HookOutcome::Ignored("expired unpaid"))
        }
        BtcPayValue::Nothing => return Ok(HookOutcome::Retry),
    };
    if s.order_id != inv.pay_ref {
        return Err(PayError::BadPayload("invoice orderId does not match".into()).into());
    }
    credit(store, &inv, s.amount_cents, now)
}

/// Reconcile open BTCPay invoices by reading each back (missed webhooks,
/// late confirmations of partial payments). Runs with the Monero poll.
pub fn poll_btcpay(crypto: &Crypto, store: &Store, now: i64) -> Result<PollReport, CryptoError> {
    if let Some(why) = crypto.cfg.missing(Backend::BtcPay) {
        return Err(PayError::NotConnected(why.into()).into());
    }
    let open: Vec<CryptoInvoice> = store
        .open_crypto_invoices(Backend::BtcPay.id())?
        .into_iter()
        .filter(|i| i.expires_at + CRYPTO_SETTLE_WINDOW > now)
        .collect();
    let mut rep = PollReport {
        open: open.len(),
        ..PollReport::default()
    };
    for inv in &open {
        // Still inside the payment window and nothing to see: skip quickly.
        let s = match crypto.btcpay_value(&inv.invoice_id) {
            Ok(BtcPayValue::Credit(s)) => s,
            Ok(_) => continue,
            // One bad invoice does not stop the others.
            Err(PayError::Api(404, _)) => continue,
            Err(e) => return Err(e.into()),
        };
        if s.order_id != inv.pay_ref {
            continue;
        }
        if let HookOutcome::Credited(c) = credit(store, inv, s.amount_cents, now)? {
            rep.credited += 1;
            rep.credited_cents += c;
        }
    }
    Ok(rep)
}

/// Both crypto polls, each only when its backend is ready.
pub fn poll_all(crypto: &Crypto, store: &Store, now: i64) -> Result<PollReport, CryptoError> {
    let ready = crypto.cfg.ready();
    let mut rep = PollReport::default();
    for (b, f) in [
        (
            Backend::Monero,
            poll_monero as fn(&Crypto, &Store, i64) -> Result<PollReport, CryptoError>,
        ),
        (Backend::BtcPay, poll_btcpay),
    ] {
        if ready.contains(&b) && crypto.cfg.missing(b).is_none() {
            let r = f(crypto, store, now)?;
            rep.open += r.open;
            rep.credited += r.credited;
            rep.credited_cents += r.credited_cents;
        }
    }
    Ok(rep)
}

fn credit(
    store: &Store,
    inv: &CryptoInvoice,
    cents: i64,
    now: i64,
) -> Result<HookOutcome, CryptoError> {
    let hook = Webhook {
        id: Some(format!("{}:{}", inv.backend, inv.invoice_id)),
        kind: "crypto".into(),
        events: vec![PayEvent::Credit {
            pay_ref: inv.pay_ref.clone(),
            amount_cents: cents,
            currency: "EUR".into(),
        }],
    };
    match super::apply(store, Method::Crypto, &hook, now)? {
        Applied::Done(_) => {
            store.settle_crypto_invoice(&inv.pay_ref, now)?;
            Ok(HookOutcome::Credited(cents))
        }
        Applied::Replay => {
            store.settle_crypto_invoice(&inv.pay_ref, now)?;
            Ok(HookOutcome::Replay)
        }
        Applied::Retry => Ok(HookOutcome::Retry),
    }
}

/// What one Monero poll did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollReport {
    pub open: usize,
    pub credited: usize,
    pub credited_cents: i64,
}

/// Poll monero-wallet-rpc once: credit every open invoice that has received
/// its full amount with enough confirmations, and credit partly paid ones
/// pro rata once the price lock + [`MONERO_PARTIAL_GRACE`] is over.
pub fn poll_monero(crypto: &Crypto, store: &Store, now: i64) -> Result<PollReport, CryptoError> {
    if let Some(why) = crypto.cfg.missing(Backend::Monero) {
        return Err(PayError::NotConnected(why.into()).into());
    }
    let open: Vec<CryptoInvoice> = store
        .open_crypto_invoices(Backend::Monero.id())?
        .into_iter()
        .filter(|i| i.expires_at + CRYPTO_SETTLE_WINDOW > now)
        .collect();
    let mut rep = PollReport {
        open: open.len(),
        ..PollReport::default()
    };
    if open.is_empty() {
        return Ok(rep);
    }
    let got = crypto.monero_received(&open)?;
    for inv in &open {
        let received = got.get(&inv.invoice_id).copied().unwrap_or(0);
        if received <= 0 {
            continue;
        }
        let Some(plan) = store.peek_pay_ref(&inv.pay_ref, now)?.map(|p| p.plan) else {
            continue;
        };
        if received < inv.due_atomic && now < inv.expires_at + MONERO_PARTIAL_GRACE {
            continue;
        }
        let cents = pro_rata_cents(received, inv.due_atomic, plan.eur_cents());
        if cents <= 0 {
            continue;
        }
        if let HookOutcome::Credited(c) = credit(store, inv, cents, now)? {
            rep.credited += 1;
            rep.credited_cents += c;
        }
    }
    Ok(rep)
}

impl PaymentAdapter for Crypto {
    fn method(&self) -> Method {
        Method::Crypto
    }

    fn status(&self) -> Status {
        if !self.cfg.ready().is_empty() {
            return Status::Connected;
        }
        if self.cfg.backends.is_empty() {
            return Status::NotConnected(
                "Crypto is off: set ACCOUNTS_CRYPTO_BACKENDS=btcpay and/or monero".into(),
            );
        }
        let why: Vec<String> = self
            .cfg
            .backends
            .iter()
            .map(|b| format!("{}: {}", b.id(), self.cfg.missing(*b).unwrap_or("ok")))
            .collect();
        Status::NotConnected(why.join("; "))
    }

    /// Crypto needs the store (see [`start`]); the generic path is unused.
    fn start(&self, _req: &StartRequest) -> Result<StartOutcome, PayError> {
        match self.status() {
            Status::NotConnected(r) => Err(PayError::NotConnected(r)),
            Status::Connected => Err(PayError::Unsupported),
        }
    }

    /// Signature check + parse only, amount 0 (see [`btcpay_hook`] for the
    /// full flow the server uses).
    fn webhook(
        &self,
        headers: &[(String, String)],
        body: &[u8],
        _now: i64,
    ) -> Result<Webhook, PayError> {
        self.btcpay_webhook(headers, body)?;
        let v: Value =
            serde_json::from_slice(body).map_err(|_| PayError::BadPayload("not JSON".into()))?;
        parse_btcpay(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| {
            pairs
                .iter()
                .find(|(a, _)| *a == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn btcpay_sig() {
        let body = br#"{"deliveryId":"d1","type":"InvoiceSettled","metadata":{"orderId":"gp_X"}}"#;
        let sig = format!("sha256={}", crate::hmac::hmac_sha256_hex(b"whs", body));
        assert!(verify_btcpay(Some(&sig), body, "whs"));
        assert!(verify_btcpay(
            Some(&sig.to_ascii_uppercase().replace("SHA256=", "sha256=")),
            body,
            "whs"
        ));
        assert!(!verify_btcpay(Some(&sig), b"{}", "whs"));
        assert!(!verify_btcpay(Some(&sig), body, "other"));
        assert!(!verify_btcpay(None, body, "whs"));
        assert!(!verify_btcpay(Some("sha1=00"), body, "whs"));
        let w = parse_btcpay(&serde_json::from_slice(body).unwrap()).unwrap();
        assert_eq!(w.id.as_deref(), Some("btcpay:d1"));
        assert_eq!(w.events.len(), 1);
    }

    #[test]
    fn off_by_default() {
        let c = Crypto::disabled();
        assert!(!c.status().is_connected());
        assert!(c.cfg.ready().is_empty());
        let c = CryptoConfig::from_vars(vars(&[
            ("BTCPAY_URL", "https://x"),
            ("MONERO_WALLET_RPC_URL", "http://127.0.0.1:1"),
        ]));
        assert!(c.ready().is_empty(), "configured but not enabled stays off");
    }

    #[test]
    fn config_needs_every_piece() {
        let c = CryptoConfig::from_vars(vars(&[(
            "ACCOUNTS_CRYPTO_BACKENDS",
            "btcpay, monero-wallet-rpc",
        )]));
        assert_eq!(c.backends, vec![Backend::BtcPay, Backend::Monero]);
        assert!(c.ready().is_empty());
        let c = CryptoConfig::from_vars(vars(&[
            ("ACCOUNTS_CRYPTO_BACKEND", "monero"),
            ("MONERO_WALLET_RPC_URL", "http://127.0.0.1:18083/json_rpc"),
            ("MONERO_EUR_RATE", "150.5"),
        ]));
        assert_eq!(c.ready(), vec![Backend::Monero]);
        assert_eq!(c.monero.rate, Some(XmrRate::Fixed(15_050_000_000)));
        let c = CryptoConfig::from_vars(vars(&[
            ("ACCOUNTS_CRYPTO_BACKENDS", "monero"),
            ("MONERO_WALLET_RPC_URL", "http://127.0.0.1:18083/json_rpc"),
            ("MONERO_RATE_SOURCE", "btcpay"),
        ]));
        assert!(
            c.ready().is_empty(),
            "btcpay rate source needs the BTCPay API"
        );
        let dbg = format!(
            "{:?}",
            CryptoConfig::from_vars(vars(&[
                ("BTCPAY_API_KEY", "topsecret"),
                ("MONERO_WALLET_RPC_LOGIN", "u:pw-secret")
            ]))
        );
        assert!(!dbg.contains("topsecret") && !dbg.contains("pw-secret"));
    }

    #[test]
    fn money_math_is_integer() {
        assert_eq!(parse_decimal("150.5", 8), Some(15_050_000_000));
        assert_eq!(parse_decimal("20.00", 2), Some(2000));
        assert_eq!(parse_decimal("20.019", 2), Some(2001));
        assert_eq!(parse_decimal(".5", 2), Some(50));
        assert_eq!(parse_decimal("-1", 2), None);
        assert_eq!(parse_decimal("1e3", 2), None);
        assert_eq!(parse_decimal("", 2), None);
        // EUR 20 at EUR 200/XMR = 0.1 XMR.
        assert_eq!(piconero_due(2000, 200 * 100_000_000), Some(100_000_000_000));
        // Rounds up.
        assert_eq!(piconero_due(1, 3 * 100_000_000), Some(3_333_333_334));
        assert_eq!(fmt_xmr(100_000_000_000), "0.1");
        assert_eq!(fmt_xmr(1_500_000_000_000), "1.5");
        assert_eq!(fmt_xmr(2_000_000_000_000), "2");
        assert_eq!(fmt_xmr(1), "0.000000000001");
        assert_eq!(pro_rata_cents(50, 100, 2000), 1000);
        assert_eq!(pro_rata_cents(100, 100, 2000), 2000);
        assert_eq!(pro_rata_cents(150, 100, 2000), 3000);
        assert_eq!(pro_rata_cents(0, 100, 2000), 0);
    }
}
