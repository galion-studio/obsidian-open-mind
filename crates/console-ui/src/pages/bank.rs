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
//!
//! Freshness (wave 4 slot 371, F-351-5 + F-351-3): the page honours the
//! reader's optional `asOf` (ISO with offset, shown on the Zagreb clock),
//! `ageS`, `stale` and `refreshS`. A snapshot older than 3 refresh intervals
//! (default 300 s) gets a stale warning but stays visible (D-351-3). Purses
//! may carry their own `asOf`; `"source":"survival"` purses are judged on the
//! 15-min clock of the retired `survival-cash` cron, so the one-shot
//! `survival.json` shows as stale instead of current. The page reloads itself
//! every `refreshS` (clamped 60..=3600 s) with `<meta http-equiv=refresh>`;
//! no JavaScript.

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

/// Reader refresh interval the page assumes when the snapshot has no
/// `refreshS` (D-351-3 proposal: 300 s), and the bounds it is clamped to.
pub const REFRESH_DEFAULT_S: u64 = 300;
pub const REFRESH_MIN_S: u64 = 60;
pub const REFRESH_MAX_S: u64 = 3_600;
/// `survival.json` was rewritten every 15 min by the retired `survival-cash`
/// cron (F-351-3); a purse with `"source":"survival"` is judged on that.
pub const SURVIVAL_REFRESH_S: u64 = 900;
/// Older than this many refresh intervals = stale (D-351-3: shown with a
/// warning, never hidden).
pub const STALE_INTERVALS: u64 = 3;
/// `asOf` up to this far in the future is clock jitter, not skew.
const SKEW_S: i64 = 60;

fn interval(v: &Value, default: u64) -> u64 {
    match finite(v.get("refreshS")) {
        Some(n) if n > 0.0 => (n.round() as u64).clamp(REFRESH_MIN_S, REFRESH_MAX_S),
        _ => default,
    }
}

/// How often the `/bank` page reloads itself (`<meta http-equiv=refresh>`):
/// the reader's `refreshS`, clamped to 60..=3600 s, else 300 s. With no
/// snapshot (404/502/down) it still reloads every 300 s, so the page picks
/// up a reader as soon as one answers.
pub fn refresh_s(snap: Option<&Value>) -> u64 {
    snap.map_or(REFRESH_DEFAULT_S, |v| interval(v, REFRESH_DEFAULT_S))
}

/// Freshness of a snapshot or a purse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fresh {
    /// `asOf` as Unix seconds (any ISO offset; shown on the Zagreb clock).
    pub at: Option<i64>,
    /// Age in seconds: `now - asOf`, else the reader's `ageS`.
    pub age: Option<i64>,
    /// `asOf` more than 60 s ahead of the console clock.
    pub ahead: Option<i64>,
    /// The reader said `stale:true`, or the age is over 3 intervals.
    pub stale: bool,
    /// Interval the age was judged on.
    pub every: u64,
}

/// Reads `asOf` / `ageS` / `stale` (the F-351-5 keys). `None` when the
/// object carries none of them (old readers, the Node shape).
pub fn freshness(v: &Value, now: i64, every: u64) -> Option<Fresh> {
    let as_of = v.get("asOf").as_str();
    let age_s = finite(v.get("ageS")).filter(|n| *n >= 0.0);
    let flag = v.get("stale").as_bool();
    if as_of.is_none() && age_s.is_none() && flag.is_none() {
        return None;
    }
    let at = as_of.and_then(super::security::parse_iso);
    let (mut age, mut ahead) = (None, None);
    if let Some(t) = at {
        let d = now - t;
        if d < -SKEW_S {
            ahead = Some(-d);
        } else {
            age = Some(d.max(0));
        }
    }
    if age.is_none() && ahead.is_none() {
        age = age_s.map(|n| n.round() as i64);
    }
    let old = age.is_some_and(|a| a as u64 > STALE_INTERVALS * every);
    Some(Fresh {
        at,
        age,
        ahead,
        stale: flag == Some(true) || old,
        every,
    })
}

/// `42 s`, `17 min`, `10.0 h`, `3 d`.
pub fn span(s: i64) -> String {
    let s = s.max(0);
    if s < 90 {
        format!("{s} s")
    } else if s < 90 * 60 {
        format!("{} min", (s + 30) / 60)
    } else if s < 48 * 3_600 {
        to_fixed(s as f64 / 3_600.0, 1) + " h"
    } else {
        format!("{} d", (s + 43_200) / 86_400)
    }
}

fn clock(at: i64) -> String {
    super::p4::zagreb(at)
}

/// Short suffix for a purse note: `as of 2026-10-27 18:19 CET (+01:00), 10.0 h old`.
fn purse_suffix(f: &Fresh) -> String {
    let mut s = String::new();
    if f.stale {
        s.push_str("stale · ");
    }
    match (f.at, f.age, f.ahead) {
        (Some(t), _, Some(a)) => s.push_str(&format!("as of {} ({} ahead)", clock(t), span(a))),
        (Some(t), Some(a), _) => s.push_str(&format!("as of {}, {} old", clock(t), span(a))),
        (None, Some(a), _) => s.push_str(&format!("{} old", span(a))),
        _ => s.push_str("time unknown"),
    }
    s
}

/// The snapshot freshness lines under Cash (F-351-5).
fn snapshot_lines(f: Option<&Fresh>, every: u64) -> String {
    let mut out = String::new();
    let Some(f) = f else {
        out.push_str(&format!(
            "<p class=\"note\">The reader sent no snapshot time (asOf), so how fresh these figures are is unknown. This page reloads every {}.</p>",
            span(every as i64)
        ));
        return out;
    };
    let when = match (f.at, f.age, f.ahead) {
        (Some(t), _, Some(a)) => format!(
            "Snapshot as of {}, {} ahead of the console clock (clock skew), so its age is unknown.",
            clock(t),
            span(a)
        ),
        (Some(t), Some(a), _) => format!("Snapshot as of {} · {} ago.", clock(t), span(a)),
        (None, Some(a), _) => format!("Snapshot {} old.", span(a)),
        _ => "Snapshot time could not be read.".to_string(),
    };
    out.push_str(&format!(
        "<p class=\"note\">{} The reader refreshes every {}; this page reloads every {}.</p>",
        esc(&when),
        span(f.every as i64),
        span(every as i64)
    ));
    if f.stale {
        let why = match f.age {
            Some(a) if a as u64 > STALE_INTERVALS * f.every => format!(
                "Stale: the last snapshot is {} old, more than {} refreshes of {}.",
                span(a),
                STALE_INTERVALS,
                span(f.every as i64)
            ),
            _ => "Stale: the reader marks this snapshot as stale.".to_string(),
        };
        out.push_str(&format!(
            "<p class=\"note t-bad\" role=\"status\">{} The figures above may be out of date; check that the bank reader is running.</p>",
            esc(&why)
        ));
    }
    out
}

/// Page body. `snap` is the `BankSnap` JSON from `/api/v1/bank`, judged
/// against the console clock now.
pub fn body(snap: Result<&Value, Miss>) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    body_at(snap, now)
}

/// [`body`] at a given Unix time (tests).
pub fn body_at(snap: Result<&Value, Miss>, now: i64) -> String {
    let null = Value::Null;
    let s = snap.unwrap_or(&null);
    let every = refresh_s(snap.ok());
    let mut out = String::from("<div class=\"page w46\">");
    out.push_str(&format!(
        "<header><p class=\"k\"><a href=\"/console\">{}Desk</a></p><h1><span class=\"ell\">Bank</span></h1>\
<p class=\"lede\">The cash the stack can measure</p></header>",
        icon("desk", "i i-sm")
    ));
    out.push_str("<section aria-label=\"Purses\"><h2>Purses</h2><ul class=\"rows\">");
    for p in s.get("purses").as_array() {
        let name = clean(p.get("name"));
        let mut note = clean(p.get("note"));
        let p_every = if p.get("source").as_str() == Some("survival") {
            interval(p, SURVIVAL_REFRESH_S)
        } else {
            interval(p, every)
        };
        if let Some(f) = freshness(p, now, p_every) {
            if !note.is_empty() {
                note.push_str(" · ");
            }
            note.push_str(&purse_suffix(&f));
        }
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
    if snap.is_ok() {
        out.push_str(&snapshot_lines(freshness(s, now, every).as_ref(), every));
    }
    out.push_str("</section>");
    if let Err(m) = snap {
        let why = match m {
            Miss::Status(404) => "The console API has no /api/v1/bank route yet.".to_string(),
            Miss::Status(c) => format!("/api/v1/bank answered HTTP {c}."),
            Miss::Down => "The console API did not answer.".to_string(),
        };
        out.push_str(&format!(
            "<p class=\"note\">{} The console reads no key files and calls no provider, so balances show once a Rust bank reader serves /api/v1/bank (owner decision). This page reloads every {}.</p>",
            esc(&why),
            span(every as i64)
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

    // 2026-10-28 04:00:00 CET (+01:00) = 03:00:00 UTC.
    const T0: i64 = 1_793_156_400;

    #[test]
    fn clock_constant_is_what_the_tests_think() {
        assert_eq!(
            super::super::p4::zagreb(T0),
            "2026-10-28 04:00 CET (+01:00)"
        );
        assert_eq!(
            super::super::security::parse_iso("2026-10-28T04:00:00+01:00"),
            Some(T0)
        );
    }

    #[test]
    fn as_of_is_shown_on_the_zagreb_clock_with_its_age() {
        let v = parse(
            br#"{"purses":[],"totalUsd":1,"asOf":"2026-10-28T04:00:00+01:00","refreshS":300}"#,
        )
        .unwrap();
        let b = body_at(Ok(&v), T0 + 5 * 60);
        assert!(
            b.contains("Snapshot as of 2026-10-28 04:00 CET (+01:00) · 5 min ago."),
            "{b}"
        );
        assert!(b.contains("The reader refreshes every 5 min; this page reloads every 5 min."));
        assert!(!b.contains("Stale"));
        // A UTC stamp from the reader is still shown on the Zagreb clock.
        let z = parse(br#"{"asOf":"2026-10-28T03:00:00Z"}"#).unwrap(); // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
        let b = body_at(Ok(&z), T0 + 30);
        assert!(
            b.contains("as of 2026-10-28 04:00 CET (+01:00) · 30 s ago."),
            "{b}"
        );
        assert!(!b.contains("03:00"));
    }

    #[test]
    fn old_or_flagged_snapshots_warn_but_still_show() {
        let v = parse(br#"{"purses":[{"name":"OpenRouter","usd":2}],"totalUsd":2,"asOf":"2026-10-28T04:00:00+01:00"}"#).unwrap();
        // 3 x 300 s is the edge: not stale yet.
        assert!(!body_at(Ok(&v), T0 + 900).contains("Stale"));
        let b = body_at(Ok(&v), T0 + 901);
        assert!(b.contains("<p class=\"note t-bad\" role=\"status\">Stale: the last snapshot is 15 min old, more than 3 refreshes of 5 min."), "{b}");
        assert!(
            b.contains("$2.00</span><span>total"),
            "figures stay visible"
        );
        let b = body_at(Ok(&v), T0 + 10 * 3600);
        assert!(b.contains("10.0 h old"));
        let f = parse(br#"{"asOf":"2026-10-28T04:00:00+01:00","stale":true}"#).unwrap();
        assert!(body_at(Ok(&f), T0 + 1).contains("the reader marks this snapshot as stale"));
        // ageS alone (no asOf) is honoured.
        let a = parse(br#"{"ageS":7200,"refreshS":60}"#).unwrap();
        let b = body_at(Ok(&a), T0);
        assert!(
            b.contains("Snapshot 2.0 h old.") && b.contains("more than 3 refreshes of 60 s"),
            "{b}"
        );
    }

    #[test]
    fn missing_or_bad_times_are_said_plainly() {
        let v = parse(br#"{"purses":[],"note":"ok"}"#).unwrap();
        let b = body_at(Ok(&v), T0);
        assert!(b.contains("sent no snapshot time (asOf)"));
        assert!(b.contains("reloads every 5 min"));
        let bad = parse(br#"{"asOf":"yesterday"}"#).unwrap();
        assert!(body_at(Ok(&bad), T0).contains("Snapshot time could not be read."));
        let ahead = parse(br#"{"asOf":"2026-10-28T05:00:00+01:00","ageS":5}"#).unwrap();
        let b = body_at(Ok(&ahead), T0);
        assert!(
            b.contains("60 min ahead of the console clock (clock skew)"),
            "{b}"
        );
        assert!(!b.contains("Stale"));
        // Errors keep the old text, plus the reload hint, no freshness line.
        let e = body_at(Err(Miss::Status(502)), T0);
        assert!(e.contains("HTTP 502") && e.contains("reloads every 5 min"));
        assert!(!e.contains("Snapshot"));
        // Hostile asOf text never reaches the page.
        let h = parse(br#"{"asOf":"<script>alert(1)</script>"}"#).unwrap();
        assert!(!body_at(Ok(&h), T0).contains("<script"));
    }

    #[test]
    fn survival_purses_age_on_their_own_clock() {
        // F-351-3: survival.json was last written 2026-10-27 18:19 CET.
        let v = parse(br#"{"asOf":"2026-10-28T04:00:00+01:00","purses":[
            {"name":"Runpod","usd":3,"note":"from survival probe","source":"survival","asOf":"2026-10-27T18:19:00+01:00"},
            {"name":"Fresh","usd":1,"source":"survival","asOf":"2026-10-28T03:50:00+01:00"},
            {"name":"OpenRouter","usd":2,"note":"key limit left","asOf":"2026-10-28T03:59:00+01:00"}]}"#).unwrap();
        let b = body_at(Ok(&v), T0);
        assert!(b.contains("<span class=\"muted\">from survival probe · stale · as of 2026-10-27 18:19 CET (+01:00), 9.7 h old</span>"), "{b}");
        assert!(
            b.contains(
                "<span class=\"muted\">as of 2026-10-28 03:50 CET (+01:00), 10 min old</span>"
            ),
            "{b}"
        );
        assert!(
            b.contains("key limit left · as of 2026-10-28 03:59 CET (+01:00), 60 s old"),
            "{b}"
        );
        // purse without time keys: note untouched
        let n = parse(br#"{"purses":[{"name":"River","usd":null,"note":"missing key"}]}"#).unwrap();
        assert!(body_at(Ok(&n), T0).contains("<span class=\"muted\">missing key</span>"));
    }

    #[test]
    fn refresh_interval_is_clamped() {
        assert_eq!(refresh_s(None), 300);
        let r = |s: &str| refresh_s(Some(&parse(s.as_bytes()).unwrap()));
        assert_eq!(r(r#"{}"#), 300);
        assert_eq!(r(r#"{"refreshS":5}"#), 60);
        assert_eq!(r(r#"{"refreshS":120}"#), 120);
        assert_eq!(r(r#"{"refreshS":99999}"#), 3600);
        assert_eq!(r(r#"{"refreshS":-1}"#), 300);
        assert_eq!(r(r#"{"refreshS":"300"}"#), 300);
        assert_eq!(span(89), "89 s");
        assert_eq!(span(90), "2 min");
        assert_eq!(span(5399), "90 min");
        assert_eq!(span(5400), "1.5 h");
        assert_eq!(span(3 * 86_400), "3 d");
    }
}
