// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/bank`: port of console-ide `src/routes/_console/bank.tsx` (rendering
//! and the `money` / `count` / `days` / `clean` helpers, word for word).
//!
//! Where the numbers come from is the open part. The TSX called a TanStack
//! server function that runs `readBank()` (`src/lib/bank.ts`) inside the
//! Node console: it reads provider keys from the secrets env files, calls
//! OpenRouter and River with them, and reads `hermes/state.db`. This Rust
//! UI reads no key file and calls no provider, so it renders the
//! `BankSnap` JSON from `GET /api/v1/bank` on the console API. That route
//! does not exist yet (live 2026-10-21 05:00 CEST: the TanStack console
//! answers 500 "Only HTML requests are supported here" to a JSON GET of a
//! path it has no route for, counted as a 404), so the page shows the
//! TSX's empty state plus why. Who serves `/api/v1/bank` is an owner
//! decision (docs/console-port-1.md). Since wave 4 slot 299 the page is
//! Rust-first even with an SPA fallback (`console_ui::SPA_FIRST` is empty):
//! the live Rust console (`--local-api`, 2026-10-27 20:41 CET +01:00) still
//! answers 404 for `/api/v1/bank`, and the Node console that read the keys
//! is retired, so the honest empty state is the whole page until a Rust
//! bank reader exists.

use super::jsfmt::{drop_word, finite, round, squeeze, to_fixed};
use crate::html::esc;
use crate::icons::icon;
use crate::json::Value;

/// `money(value)`.
pub fn money(v: &Value) -> String {
    match finite(v) {
        None => "—".into(),
        Some(n) => format!(
            "{}${}",
            if n < 0.0 { "-" } else { "" },
            to_fixed(n.abs(), 2)
        ),
    }
}

/// `count(value)`.
pub fn count(v: &Value) -> String {
    match finite(v) {
        None => "—".into(),
        Some(n) => super::jsfmt::num(round(n)),
    }
}

/// `days(value)`.
pub fn days(v: &Value) -> String {
    match finite(v) {
        None => "—".into(),
        Some(n) if (n - round(n)).abs() < 0.05 => super::jsfmt::num(round(n)),
        Some(n) => to_fixed(n, 1),
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `/\b(?:sk|rk|pk|or)-[A-Za-z0-9_-]{8,}\b/g` -> " ".
fn drop_tokens(s: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < s.len() {
        let boundary = i == 0 || !is_word(s[i - 1]);
        if boundary && i + 3 <= s.len() && s[i + 2] == '-' {
            let p: String = s[i..i + 2].iter().collect();
            if ["sk", "rk", "pk", "or"].contains(&p.as_str()) {
                let tok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
                let mut e = i + 3;
                while e < s.len() && tok(s[e]) {
                    e += 1;
                }
                // greedy then backtrack to a word boundary (JS regex semantics)
                while e >= i + 3 + 8 {
                    let end_ok = e == s.len() || !is_word(s[e]);
                    let last_word = is_word(s[e - 1]);
                    if last_word && end_ok {
                        out.push(' ');
                        i = e;
                        continue 'outer;
                    }
                    e -= 1;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn eq_fold(s: &[char], at: usize, w: &str) -> bool {
    let w: Vec<char> = w.chars().collect();
    at + w.len() <= s.len()
        && s[at..at + w.len()]
            .iter()
            .zip(&w)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// `/\bBearer\s+\S+/gi` -> " ".
fn drop_bearer(s: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if (i == 0 || !is_word(s[i - 1])) && eq_fold(s, i, "bearer") {
            let mut e = i + 6;
            let ws = e;
            while e < s.len() && is_ws(s[e]) {
                e += 1;
            }
            let tok = e;
            while e < s.len() && !is_ws(s[e]) {
                e += 1;
            }
            if e > ws && tok > ws && e > tok {
                out.push(' ');
                i = e;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `/(?:api[_-]?key|authorization|secret|password)\s*[:=]\s*\S+/gi` -> " ".
fn drop_assignments(s: &[char]) -> Vec<char> {
    let names = [
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "secret",
        "password",
    ];
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < s.len() {
        for n in names {
            if eq_fold(s, i, n) {
                let mut e = i + n.chars().count();
                while e < s.len() && is_ws(s[e]) {
                    e += 1;
                }
                if e < s.len() && (s[e] == ':' || s[e] == '=') {
                    e += 1;
                    while e < s.len() && is_ws(s[e]) {
                        e += 1;
                    }
                    let tok = e;
                    while e < s.len() && !is_ws(s[e]) {
                        e += 1;
                    }
                    if e > tok {
                        out.push(' ');
                        i = e;
                        continue 'outer;
                    }
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// `clean(value)`: strings only; drops `undefined`, key-looking tokens,
/// bearer headers and `secret=...` style assignments, squeezes, trims.
pub fn clean(v: &Value) -> String {
    let Some(s) = v.as_str() else {
        return String::new();
    };
    let s: Vec<char> = drop_word(s, "undefined", true).chars().collect();
    let s = drop_assignments(&drop_bearer(&drop_tokens(&s)));
    squeeze(&s.into_iter().collect::<String>())
}

fn purse_row(usd: &str, name: &str, note: &str) -> String {
    format!(
        "<li class=\"cash\"><span class=\"amt mono\">{}</span><span>{}</span><span class=\"muted\">{}</span></li>",
        esc(usd),
        esc(name),
        esc(note)
    )
}

fn cash_row(label: &str, value: &str) -> String {
    format!(
        "<li class=\"cash\"><span class=\"amt mono\">{}</span><span>{}</span></li>",
        esc(value),
        esc(label)
    )
}

/// Why there is no snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    Status(u16),
    Down,
}

/// Page body. `snap` is the `BankSnap` JSON from `/api/v1/bank`.
pub fn body(snap: Result<&Value, Miss>) -> String {
    let null = Value::Null;
    let s = snap.unwrap_or(&null);
    let mut out = String::from("<div class=\"page w46\">");
    out.push_str(&format!(
        "<header><p class=\"k\"><a href=\"/console\">{}Desk</a></p><h1><span class=\"ell\">Bank</span></h1>\
<p class=\"lede\">The cash the stack can measure</p></header>",
        icon("desk", "i i-sm")
    ));
    out.push_str("<section aria-label=\"Purses\"><h2>Purses</h2><ul class=\"rows\">");
    for p in s.get("purses").as_array() {
        let name = clean(p.get("name"));
        let note = clean(p.get("note"));
        out.push_str(&purse_row(
            &money(p.get("usd")),
            if name.is_empty() { "—" } else { &name },
            if note.is_empty() { "—" } else { &note },
        ));
    }
    out.push_str("</ul></section>");
    out.push_str("<section aria-label=\"Cash\"><h2>Cash</h2><ul class=\"rows\">");
    out.push_str(&cash_row("total", &money(s.get("totalUsd"))));
    out.push_str(&cash_row("7-day tokens in", &count(s.get("weekTokensIn"))));
    out.push_str(&cash_row(
        "7-day tokens out",
        &count(s.get("weekTokensOut")),
    ));
    out.push_str(&cash_row("recorded spend", &money(s.get("weekCostUsd"))));
    out.push_str(&cash_row("per day", &money(s.get("dailyUsd"))));
    out.push_str(&cash_row("days left", &days(s.get("daysLeft"))));
    out.push_str("</ul>");
    out.push_str(&format!(
        "<p class=\"note\">if this traffic moved to River {}</p>",
        esc(&money(s.get("riverIfMovedDailyUsd")))
    ));
    let line = clean(s.get("note"));
    if !line.is_empty() {
        out.push_str(&format!("<p class=\"note\">{}</p>", esc(&line)));
    }
    out.push_str("</section>");
    if let Err(m) = snap {
        let why = match m {
            Miss::Status(404) => "The console API has no /api/v1/bank route yet.".to_string(),
            Miss::Status(c) => format!("/api/v1/bank answered HTTP {c}."),
            Miss::Down => "The console API did not answer.".to_string(),
        };
        out.push_str(&format!(
            "<p class=\"note\">{} The console reads no key files and calls no provider, so balances show once a Rust bank reader serves /api/v1/bank (owner decision).</p>",
            esc(&why)
        ));
    }
    out.push_str("</div>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    fn sv(s: &str) -> Value {
        Value::Str(s.into())
    }

    #[test]
    fn helpers_match_the_tsx() {
        assert_eq!(money(&Value::Num(-3.456)), "-$3.46");
        assert_eq!(money(&Value::Num(0.125)), "$0.13");
        assert_eq!(money(&Value::Null), "—");
        assert_eq!(money(&sv("5")), "—");
        assert_eq!(count(&Value::Num(1234.5)), "1235");
        assert_eq!(count(&Value::Num(-2.5)), "-2");
        assert_eq!(days(&Value::Num(41.97)), "42");
        assert_eq!(days(&Value::Num(41.9)), "41.9");
        assert_eq!(days(&Value::Num(0.04)), "0");
    }

    #[test]
    fn clean_strips_secrets_like_the_regexes() {
        assert_eq!(clean(&sv("key sk-test_dummy_token left")), "key left");
        assert_eq!(clean(&sv("token or-12345678")), "token");
        assert_eq!(
            clean(&sv("short sk-1234567 stays")),
            "short sk-1234567 stays"
        );
        assert_eq!(clean(&sv("xsk-12345678 stays")), "xsk-12345678 stays");
        // bearer goes first, then "Authorization:" eats the next token, like JS
        assert_eq!(clean(&sv("Authorization: Bearer abc.def ok")), "");
        assert_eq!(clean(&sv("use Bearer abc.def ok")), "use ok");
        assert_eq!(clean(&sv("header bearer   xyz")), "header");
        assert_eq!(
            clean(&sv("API_KEY = hunter2 password=x secret:y end")),
            "end"
        );
        assert_eq!(
            clean(&sv("apikey=1 api-key:2 Undefined UNDEFINED done")),
            "done"
        );
        assert_eq!(
            clean(&sv("key limit left · $1.00 this week")),
            "key limit left · $1.00 this week"
        );
        assert_eq!(clean(&Value::Num(3.0)), "");
        // trailing '-' is not a word char: backtrack to the last word char
        assert_eq!(clean(&sv("pk-abcdefgh- tail")), "- tail");
    }

    #[test]
    fn full_snapshot() {
        let v = parse(br#"{"purses":[{"name":"OpenRouter","usd":12.5,"measured":true,"note":"key limit left \u00b7 $1.40 this week"},
            {"name":"River","usd":null,"measured":false,"note":"missing key"},
            {"name":"","usd":null,"measured":false,"note":"sk-abcdefghijkl"}],
          "totalUsd":12.5,"weekTokensIn":19391995,"weekTokensOut":1830315.4,"weekCostUsd":1.4,"dailyUsd":0.2,"daysLeft":62.5,
          "riverIfMovedDailyUsd":0.4579,"note":"OpenRouter recorded $1.40 this week, about $0.20 a day."}"#).unwrap();
        let b = body(Ok(&v));
        assert!(b.contains("<span class=\"amt mono\">$12.50</span><span>OpenRouter</span>"));
        assert!(b.contains("<span class=\"amt mono\">—</span><span>River</span><span class=\"muted\">missing key</span>"));
        assert!(b.contains("<span>—</span><span class=\"muted\">—</span>"));
        assert!(b.contains("19391995</span><span>7-day tokens in"));
        assert!(b.contains("1830315</span><span>7-day tokens out"));
        assert!(b.contains("$1.40</span><span>recorded spend"));
        assert!(b.contains("62.5</span><span>days left"));
        assert!(b.contains("if this traffic moved to River $0.46"));
        assert!(b.contains("about $0.20 a day."));
        assert!(!b.contains("route yet") && !b.contains("<script"));
    }

    #[test]
    fn missing_api_is_explained() {
        let b = body(Err(Miss::Status(404)));
        assert!(b.contains("—</span><span>total"));
        assert!(b.contains("if this traffic moved to River —"));
        assert!(b.contains("no /api/v1/bank route yet"));
        assert!(body(Err(Miss::Down)).contains("did not answer"));
        assert!(body(Err(Miss::Status(401))).contains("HTTP 401"));
        assert!(!body(Err(Miss::Status(404))).contains("TypeScript"));
        assert!(body(Err(Miss::Status(404))).contains("Rust bank reader"));
    }
}
