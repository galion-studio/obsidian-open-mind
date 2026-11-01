// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cryptocurrency, as a stub until Slavik picks a processor. Both options
//! are self-hosted (no custodian holds the coins or sees customers):
//!
//! * BTCPay Server (MIT): Bitcoin (+ Lightning; Monero via its plugin).
//!   Greenfield API: create an invoice with `metadata.orderId = <gp_ ref>`,
//!   redirect to its checkout, get `InvoiceSettled` webhooks signed with
//!   `BTCPay-Sig: sha256=<hex HMAC-SHA256(secret, raw body)>`. The webhook
//!   check and event parsing are implemented and tested here; invoice
//!   creation is not wired (no processor yet).
//! * monero-wallet-rpc (BSD-3-Clause): one subaddress per payment, credit
//!   on `get_transfers` with N confirmations, store tx_hash to stop double
//!   credit. Not implemented.
//!
//! Crypto tops up time like cash (no subscription): the amount is converted
//! to days of the chosen plan.

use serde_json::Value;

use super::{
    Method, PayError, PayEvent, PaymentAdapter, StartOutcome, StartRequest, Status, Webhook,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    None,
    BtcPay,
    MoneroRpc,
}

#[derive(Clone, Default)]
pub struct CryptoConfig {
    pub backend: Option<String>,
    pub btcpay_webhook_secret: Option<String>,
}

impl std::fmt::Debug for CryptoConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptoConfig")
            .field("backend", &self.backend)
            .field(
                "btcpay_webhook_secret",
                &self.btcpay_webhook_secret.as_ref().map(|_| "<set>"),
            )
            .finish()
    }
}

impl CryptoConfig {
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> CryptoConfig {
        CryptoConfig {
            backend: get("ACCOUNTS_CRYPTO_BACKEND").filter(|s| !s.is_empty()),
            btcpay_webhook_secret: get("BTCPAY_WEBHOOK_SECRET").filter(|s| !s.is_empty()),
        }
    }

    pub fn backend(&self) -> Backend {
        match self.backend.as_deref() {
            Some("btcpay") => Backend::BtcPay,
            Some("monero-wallet-rpc") => Backend::MoneroRpc,
            _ => Backend::None,
        }
    }
}

pub struct Crypto {
    pub cfg: CryptoConfig,
}

/// Check `BTCPay-Sig: sha256=<hex>` over the raw body.
pub fn verify_btcpay(header: Option<&str>, body: &[u8], secret: &str) -> bool {
    let Some(h) = header.and_then(|h| h.trim().strip_prefix("sha256=")) else {
        return false;
    };
    let want = crate::hmac::hmac_sha256_hex(secret.as_bytes(), body);
    crate::ct_eq(h.to_ascii_lowercase().as_bytes(), want.as_bytes())
}

/// `InvoiceSettled` -> a [`PayEvent::Credit`] for the invoice's orderId.
/// The amount must come from the invoice itself (GET), not the webhook; the
/// caller fills it in. Here it is left at 0 so nothing is credited from an
/// unverified amount.
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

impl PaymentAdapter for Crypto {
    fn method(&self) -> Method {
        Method::Crypto
    }

    fn status(&self) -> Status {
        Status::NotConnected(match self.cfg.backend() {
            Backend::None => "Crypto is not set up yet: choose BTCPay Server or monero-wallet-rpc (ACCOUNTS_CRYPTO_BACKEND)".into(),
            Backend::BtcPay => "BTCPay Server adapter is a stub in this prototype".into(),
            Backend::MoneroRpc => "monero-wallet-rpc adapter is a stub in this prototype".into(),
        })
    }

    fn start(&self, _req: &StartRequest) -> Result<StartOutcome, PayError> {
        match self.status() {
            Status::NotConnected(r) => Err(PayError::NotConnected(r)),
            Status::Connected => Err(PayError::Unsupported),
        }
    }

    fn webhook(
        &self,
        headers: &[(String, String)],
        body: &[u8],
        _now: i64,
    ) -> Result<Webhook, PayError> {
        match (self.cfg.backend(), &self.cfg.btcpay_webhook_secret) {
            (Backend::BtcPay, Some(secret)) => {
                let h = galion_common::http::header(headers, "btcpay-sig");
                if !verify_btcpay(h, body, secret) {
                    return Err(PayError::BadSignature("BTCPay-Sig mismatch"));
                }
                let v: Value = serde_json::from_slice(body)
                    .map_err(|_| PayError::BadPayload("not JSON".into()))?;
                parse_btcpay(&v)
            }
            _ => Err(PayError::NotConnected("crypto is not connected".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btcpay_sig() {
        let body = br#"{"deliveryId":"d1","type":"InvoiceSettled","metadata":{"orderId":"gp_X"}}"#;
        let sig = format!("sha256={}", crate::hmac::hmac_sha256_hex(b"whs", body));
        assert!(verify_btcpay(Some(&sig), body, "whs"));
        assert!(!verify_btcpay(Some(&sig), b"{}", "whs"));
        assert!(!verify_btcpay(None, body, "whs"));
        let w = parse_btcpay(&serde_json::from_slice(body).unwrap()).unwrap();
        assert_eq!(w.id.as_deref(), Some("btcpay:d1"));
        assert_eq!(w.events.len(), 1);
    }

    #[test]
    fn stub_is_not_connected() {
        let c = Crypto {
            cfg: CryptoConfig::default(),
        };
        assert!(!c.status().is_connected());
    }
}
