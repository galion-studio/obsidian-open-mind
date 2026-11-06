// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Galion public pricing + country → currency map (Mate / Slavik, 2026-11-06 CET).
//!
//! Config file: `home/pricing.json` (owner-editable). Same nominal numbers in
//! USD and EUR — not FX-converted. Currency is chosen server-side from the
//! hosting platform's country header (`CF-IPCountry` on Cloudflare,
//! `x-vercel-ip-country` on Vercel), with `?currency=usd|eur` as a test
//! override. No third-party geo API, no cookies, no tracking.
//!
//! **GB and IE** sit in `eur_countries` by default. Change that list in
//! `pricing.json` to move them.

use serde_json::Value;

const RAW: &str = include_str!("../home/pricing.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Currency {
    Usd,
    Eur,
}

impl Currency {
    pub fn as_str(self) -> &'static str {
        match self {
            Currency::Usd => "usd",
            Currency::Eur => "eur",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "usd" | "dollar" | "dollars" | "$" => Some(Currency::Usd),
            "eur" | "euro" | "euros" => Some(Currency::Eur),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Prices {
    pub one: u32,
    pub research: u32,
    pub one_period: &'static str,
    pub research_period: &'static str,
}

pub fn config() -> &'static Value {
    use std::sync::OnceLock;
    static CFG: OnceLock<Value> = OnceLock::new();
    CFG.get_or_init(|| serde_json::from_str(RAW).expect("home/pricing.json must be valid JSON"))
}

/// ISO country (from the edge header) + optional `?currency=` override → currency.
/// Unknown / empty / `XX` / `T1` (Cloudflare placeholders) → default (USD).
pub fn resolve(country_header: &str, query: &str) -> Currency {
    if let Some(c) = override_from_query(query) {
        return c;
    }
    let code = country_code(country_header);
    if code.is_empty() {
        return default_currency();
    }
    let cfg = config();
    if in_list(cfg, "usd_countries", &code) {
        return Currency::Usd;
    }
    if in_list(cfg, "eur_countries", &code) {
        return Currency::Eur;
    }
    default_currency()
}

pub fn prices(currency: Currency) -> Prices {
    let cfg = config();
    let key = currency.as_str();
    Prices {
        one: cfg["galion_one"][key].as_u64().unwrap_or(20) as u32,
        research: cfg["galion_research"][key].as_u64().unwrap_or(220) as u32,
        one_period: "month",
        research_period: "one-time",
    }
}

/// HTML amount fragment. Uses `&#36;` for USD so the page stays free of a raw `$`
/// (home self-contained test). EUR uses `&euro;`.
pub fn amount_html(currency: Currency, value: u32) -> String {
    match currency {
        Currency::Usd => format!("&#36;{value}"),
        Currency::Eur => format!("&euro;{value}"),
    }
}

pub fn note_html(currency: Currency) -> &'static str {
    match currency {
        Currency::Usd => "Prices in US dollars. No email needed: your account is a number. Pay by card (Stripe), crypto or cash by post.",
        Currency::Eur => "Prices in euros. No email needed: your account is a number. Pay by card (Stripe), crypto or cash by post.",
    }
}

fn default_currency() -> Currency {
    match config()["default_currency"].as_str().unwrap_or("usd") {
        "eur" => Currency::Eur,
        _ => Currency::Usd,
    }
}

fn override_from_query(query: &str) -> Option<Currency> {
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let k = it.next().unwrap_or("").trim();
        let v = it.next().unwrap_or("").trim();
        if k.eq_ignore_ascii_case("currency") {
            return Currency::parse(v);
        }
    }
    None
}

fn country_code(header: &str) -> String {
    header
        .trim()
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_uppercase()
}

fn in_list(cfg: &Value, key: &str, code: &str) -> bool {
    cfg[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .any(|c| c.eq_ignore_ascii_case(code))
        })
        .unwrap_or(false)
}

/// Prefer Cloudflare, then Vercel. Empty when neither is present.
pub fn country_from_headers<'a>(
    cf_ip_country: Option<&'a str>,
    vercel_ip_country: Option<&'a str>,
) -> &'a str {
    for raw in [cf_ip_country, vercel_ip_country].into_iter().flatten() {
        let t = raw.trim();
        if !t.is_empty() {
            return t;
        }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_json_loads_and_matches_owner_numbers() {
        let cfg = config();
        assert_eq!(cfg["galion_one"]["usd"], 20);
        assert_eq!(cfg["galion_one"]["eur"], 20);
        assert_eq!(cfg["galion_research"]["usd"], 220);
        assert_eq!(cfg["galion_research"]["eur"], 220);
        assert_eq!(cfg["default_currency"], "usd");
        assert!(in_list(cfg, "usd_countries", "US"));
        assert!(in_list(cfg, "usd_countries", "CA"));
        assert!(in_list(cfg, "usd_countries", "AU"));
        assert!(in_list(cfg, "usd_countries", "NZ"));
        // GB / IE editable, default EUR — report stays true while listed here.
        assert!(in_list(cfg, "eur_countries", "GB"));
        assert!(in_list(cfg, "eur_countries", "IE"));
        assert!(in_list(cfg, "eur_countries", "DE"));
        assert!(in_list(cfg, "eur_countries", "FR"));
        assert!(in_list(cfg, "eur_countries", "HR"));
    }

    #[test]
    fn country_to_currency_map() {
        assert_eq!(resolve("US", ""), Currency::Usd);
        assert_eq!(resolve("us", ""), Currency::Usd);
        assert_eq!(resolve("CA", ""), Currency::Usd);
        assert_eq!(resolve("AU", ""), Currency::Usd);
        assert_eq!(resolve("NZ", ""), Currency::Usd);
        assert_eq!(resolve("DE", ""), Currency::Eur);
        assert_eq!(resolve("FR", ""), Currency::Eur);
        assert_eq!(resolve("HR", ""), Currency::Eur);
        assert_eq!(resolve("GB", ""), Currency::Eur);
        assert_eq!(resolve("IE", ""), Currency::Eur);
        assert_eq!(resolve("CH", ""), Currency::Eur);
        assert_eq!(resolve("NO", ""), Currency::Eur);
        // Unknown / Cloudflare placeholders → default USD
        assert_eq!(resolve("", ""), Currency::Usd);
        assert_eq!(resolve("XX", ""), Currency::Usd);
        assert_eq!(resolve("T1", ""), Currency::Usd);
        assert_eq!(resolve("JP", ""), Currency::Usd);
        assert_eq!(resolve("BR", ""), Currency::Usd);
    }

    #[test]
    fn query_override_wins() {
        assert_eq!(resolve("DE", "currency=usd"), Currency::Usd);
        assert_eq!(resolve("US", "currency=eur"), Currency::Eur);
        assert_eq!(resolve("US", "foo=1&currency=EUR&bar=2"), Currency::Eur);
        assert_eq!(resolve("DE", "currency=nope"), Currency::Eur); // bad override ignored
    }

    #[test]
    fn header_preference() {
        assert_eq!(country_from_headers(Some("DE"), Some("US")), "DE");
        assert_eq!(country_from_headers(None, Some("US")), "US");
        assert_eq!(country_from_headers(Some("  "), Some("CA")), "CA");
        assert_eq!(country_from_headers(None, None), "");
    }

    #[test]
    fn amounts_have_no_raw_dollar() {
        let u = amount_html(Currency::Usd, 20);
        let e = amount_html(Currency::Eur, 20);
        assert!(u.contains("&#36;20"));
        assert!(!u.contains("$"));
        assert!(e.contains("&euro;20"));
        assert_eq!(prices(Currency::Usd).one, 20);
        assert_eq!(prices(Currency::Eur).research, 220);
    }
}
