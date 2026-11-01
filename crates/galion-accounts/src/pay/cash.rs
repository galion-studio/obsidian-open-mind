// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cash by post, Mullvad-style. The account page makes a one-time cash
//! reference (`GC-XXXX-XXXX-XXXX`, Crockford base32, 60 bits). The person
//! writes it on a slip of paper, puts it in an envelope with euro notes and
//! posts it. The operator opens the envelope, credits the reference in the
//! admin console (or `galion-accounts cash-credit`), and shreds the envelope
//! and slip. The account number never goes on paper; after crediting the
//! reference no longer points at the account.

use super::{Method, PayError, PaymentAdapter, StartOutcome, StartRequest, Status, Webhook};

/// Cash configuration.
#[derive(Debug, Clone, Default)]
pub struct CashConfig {
    /// Where envelopes go. `|` separates lines in the env var.
    pub address: Option<String>,
}

impl CashConfig {
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> CashConfig {
        CashConfig {
            address: get("ACCOUNTS_CASH_ADDRESS")
                .map(|a| a.split('|').map(str::trim).collect::<Vec<_>>().join("\n"))
                .filter(|a| !a.trim().is_empty()),
        }
    }
}

pub struct Cash {
    pub cfg: CashConfig,
}

/// A new cash reference: `GC-` + three groups of four Crockford characters.
pub fn new_reference() -> String {
    let r = crate::rng::crockford(12);
    format!("GC-{}-{}-{}", &r[0..4], &r[4..8], &r[8..12])
}

/// Normalise what an operator typed: upper case, Crockford look-alikes
/// (O->0, I/L->1), dashes re-inserted.
pub fn normalise_reference(s: &str) -> Option<String> {
    let mut chars: Vec<char> = Vec::new();
    let t = s.trim().to_ascii_uppercase();
    let t = t.strip_prefix("GC").unwrap_or(&t);
    for c in t.chars() {
        let c = match c {
            'O' => '0',
            'I' | 'L' => '1',
            '-' | ' ' => continue,
            c => c,
        };
        if !crate::rng::CROCKFORD.contains(&(c as u8)) || !c.is_ascii() {
            return None;
        }
        chars.push(c);
    }
    if chars.len() != 12 {
        return None;
    }
    let s: String = chars.into_iter().collect();
    Some(format!("GC-{}-{}-{}", &s[0..4], &s[4..8], &s[8..12]))
}

impl PaymentAdapter for Cash {
    fn method(&self) -> Method {
        Method::Cash
    }

    fn status(&self) -> Status {
        match &self.cfg.address {
            Some(_) => Status::Connected,
            None => Status::NotConnected(
                "Cash is not set up: no mailing address (ACCOUNTS_CASH_ADDRESS)".into(),
            ),
        }
    }

    fn start(&self, req: &StartRequest) -> Result<StartOutcome, PayError> {
        let Some(addr) = &self.cfg.address else {
            return Err(PayError::NotConnected("no mailing address".into()));
        };
        Ok(StartOutcome::Reference {
            reference: req.pay_ref.clone(),
            instructions: format!(
                "Write only this reference on a slip of paper: {}\n\
                 Put it in an envelope with euro notes ({} per month of {}).\n\
                 Do not write your account number, name or return address.\n\
                 Send it to:\n{}\n\
                 Time is added when the envelope is opened. The reference is valid for 120 days.",
                req.pay_ref,
                fmt_eur(req.plan.eur_cents()),
                req.plan.name(),
                addr
            ),
        })
    }

    fn webhook(&self, _: &[(String, String)], _: &[u8], _: i64) -> Result<Webhook, PayError> {
        Err(PayError::Unsupported)
    }
}

/// Escape text for HTML.
pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// Content-Security-Policy of the printable slip: no script at all.
pub const SLIP_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// A printable slip for one cash payment: the part that goes in the
/// envelope carries only the reference and the plan; the address is for
/// the outside of the envelope. No account number anywhere.
pub fn slip_html(
    reference: &str,
    plan: crate::plans::Plan,
    address: &str,
    valid_until: &str,
) -> String {
    let addr: String = address.lines().map(esc).collect::<Vec<_>>().join("<br>");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"robots\" content=\"noindex\"><title>Galion cash slip</title><style>\
body{{font:16px/1.45 system-ui,sans-serif;max-width:40rem;margin:2rem auto;padding:0 1rem;color:#111}}\
.slip{{border:2px dashed #111;padding:1.5rem;margin:1.5rem 0;text-align:center}}\
.ref{{font:700 2rem/1.2 ui-monospace,monospace;letter-spacing:.08em}}\
.addr{{border:1px solid #999;padding:1rem;font-size:1.1rem}}\
@media print{{.noprint{{display:none}}body{{margin:0}}}}\
</style></head><body>\
<h1>Pay Galion {plan} in cash</h1>\
<ol><li>Cut out the slip below and put it in an envelope with <b>{price}</b> in euro notes per month of {plan}.</li>\
<li>Do not write your account number, name or a return address anywhere.</li>\
<li>Post it to the address below. Time is added when the envelope is opened.</li></ol>\
<div class=\"slip\"><div>Galion {plan} &middot; {price} per month</div>\
<div class=\"ref\">{reference}</div><div>valid until {valid_until}</div></div>\
<p>Address for the envelope:</p><div class=\"addr\">{addr}</div>\
<p class=\"noprint\">Print this page (Ctrl+P) or copy the reference by hand. \
More notes = more months, pro rata. Coins and other currencies cannot be counted.</p>\
</body></html>",
        plan = esc(plan.name()),
        price = esc(&fmt_eur(plan.eur_cents())),
        reference = esc(reference),
        valid_until = esc(valid_until),
    )
}

pub fn fmt_eur(cents: i64) -> String {
    if cents % 100 == 0 {
        format!("€{}", cents / 100)
    } else {
        format!("€{}.{:02}", cents / 100, cents % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_round_trip() {
        for _ in 0..200 {
            let r = new_reference();
            assert_eq!(r.len(), 17);
            assert_eq!(normalise_reference(&r).as_deref(), Some(r.as_str()));
            assert_eq!(
                normalise_reference(&r.to_lowercase().replace('-', " ")).as_deref(),
                Some(r.as_str())
            );
        }
        assert_eq!(
            normalise_reference("gc-oil0-0000-0000").as_deref(),
            Some("GC-0110-0000-0000")
        );
        assert!(normalise_reference("GC-UUUU-0000-0000").is_none());
        assert!(normalise_reference("GC-0000-0000").is_none());
    }
}
