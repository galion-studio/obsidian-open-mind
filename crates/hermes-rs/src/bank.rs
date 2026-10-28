// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Bank report: the `BankSnap` JSON the console `/bank` page renders,
//! computed from provider replies and local spend files.
//!
//! Port of console-ide `src/lib/bank.ts` (`readBank`, sha256 d08f2248...)
//! with the two fixes from wave 4 slot 351:
//!
//! * **F-351-4:** OpenRouter `GET /api/v1/key` now carries `budgets` and
//!   `effective_budget` (guardrail / workspace / key-limit budgets, the one
//!   that blocks first). `limit_remaining` alone describes only the key's
//!   own limit, so it can overstate what is spendable. The purse is now the
//!   smallest of key limit left, credits left and the binding budget left,
//!   floored at 0 (a spent budget blocks requests with 403).
//! * **F-351-2:** the Node reader summed week spend from the legacy Python
//!   `state.db`, frozen since 2026-10-18 21:12 CET. Week spend now reads
//!   every given session DB in order (hermes-rs `rs-state.db` first, legacy
//!   second), de-duplicated by session id, plus today's figure from the
//!   hermes-rs spend ledger (`rs-spend.json`).
//!
//! This module does no network IO and reads no key file: provider replies
//! come in as [`Reply`] values. Who fetches them (and whether the reader
//! may call OpenRouter at all) is owner decision D-351-2.

use crate::time::{parse_rfc3339, zagreb_day, zagreb_offset, zagreb_rfc3339};
use serde_json::{json, Map, Value};

const WEEK_S: f64 = 7.0 * 24.0 * 3600.0;

/// One provider HTTP reply as the bank sees it.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// No key configured for this provider.
    MissingKey,
    /// Transport error or timeout (`fetch` threw in bank.ts).
    Failed,
    /// Not asked (e.g. providers off); treated like a failed request for
    /// OpenRouter and as "not checked" for River.
    NotAsked,
    /// HTTP status and the parsed JSON body (None when not JSON).
    Http(u16, Option<Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Purse {
    pub name: String,
    pub usd: Option<f64>,
    pub measured: bool,
    pub note: String,
}

impl Purse {
    fn new(name: &str, usd: Option<f64>, measured: bool, note: impl Into<String>) -> Self {
        Purse {
            name: name.into(),
            usd,
            measured,
            note: note.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({"name": self.name, "usd": opt_num(self.usd), "measured": self.measured, "note": self.note})
    }
}

/// One OpenRouter budget entry (`budgets[]` / `effective_budget`).
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    pub scope: String,
    pub limit_usd: f64,
    pub spend_usd: f64,
    pub remaining_usd: f64,
    pub reset_interval: Option<String>,
    /// Unix seconds of `resets_at`.
    pub resets_at: Option<i64>,
}

impl Budget {
    fn from_json(v: &Value) -> Option<Budget> {
        let o = v.as_object()?;
        let num = |k: &str| o.get(k).and_then(finite);
        let remaining = num("remaining_usd")?;
        let limit = num("limit_usd").unwrap_or(f64::NAN);
        let spend = num("spend_usd").unwrap_or(f64::NAN);
        Some(Budget {
            scope: o
                .get("scope")
                .and_then(Value::as_str)
                .unwrap_or("budget")
                .to_string(),
            limit_usd: limit,
            spend_usd: spend,
            remaining_usd: remaining,
            reset_interval: o
                .get("reset_interval")
                .and_then(Value::as_str)
                .map(str::to_string),
            resets_at: o
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339),
        })
    }

    /// Short human label, e.g. "member guardrail weekly budget".
    pub fn label(&self) -> String {
        let scope = match self.scope.as_str() {
            "workspace_default_member" => "workspace default guardrail (member)",
            "workspace_default_key" => "workspace default guardrail (key)",
            "member" => "member guardrail",
            "api_key" => "key guardrail",
            "workspace" => "workspace",
            "key_limit" => "key limit",
            _ => "OpenRouter",
        };
        let every = match self.reset_interval.as_deref() {
            Some("daily") => "daily ",
            Some("weekly") => "weekly ",
            Some("monthly") => "monthly ",
            None => "lifetime ",
            Some(_) => "",
        };
        format!("{scope} {every}budget")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "scope": self.scope,
            "limitUsd": finite_or_null(self.limit_usd),
            "spendUsd": finite_or_null(self.spend_usd),
            "remainingUsd": num(self.remaining_usd),
            "resetInterval": self.reset_interval,
            "resetsAt": self.resets_at.map(zagreb_rfc3339),
        })
    }
}

/// Which figure set the OpenRouter purse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    KeyLimit,
    Credits,
    Budget,
    Survival,
    None,
}

impl Binding {
    pub fn as_str(self) -> &'static str {
        match self {
            Binding::KeyLimit => "key_limit",
            Binding::Credits => "credits",
            Binding::Budget => "budget",
            Binding::Survival => "survival",
            Binding::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouterPurse {
    pub purse: Purse,
    pub weekly: Option<f64>,
    pub monthly: Option<f64>,
    pub binding: Binding,
    pub budget: Option<Budget>,
}

fn finite(v: &Value) -> Option<f64> {
    v.as_f64().filter(|f| f.is_finite())
}

/// A JSON number the way `JSON.stringify` writes it (12 not 12.0).
pub fn num(f: f64) -> Value {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 {
        json!(f as i64)
    } else if f.is_finite() {
        json!(f)
    } else {
        Value::Null
    }
}

fn opt_num(f: Option<f64>) -> Value {
    f.map_or(Value::Null, num)
}

fn finite_or_null(f: f64) -> Value {
    if f.is_finite() {
        num(f)
    } else {
        Value::Null
    }
}

/// `dataOf(json)`: `json.data` when it is an object, else the object itself.
fn data_of(v: Option<&Value>) -> Option<&Map<String, Value>> {
    let o = v?.as_object()?;
    o.get("data").and_then(Value::as_object).or(Some(o))
}

/// JS `Number.prototype.toFixed` for the small non-negative-or-negative
/// money figures used here (ties round away from zero like JS).
pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    let p = 10f64.powi(digits as i32);
    let scaled = x.abs() * p;
    if scaled < 9.0e15 && scaled.fract() == 0.5 {
        let n = scaled.ceil() / p;
        let s = format!("{n:.digits$}");
        return if x < 0.0 { format!("-{s}") } else { s };
    }
    format!("{x:.digits$}")
}

/// Budget that blocks first. `effective_budget` when the key reply has the
/// field (null = no budget applies); older replies without it but with
/// `budgets[]` fall back to the entry with the least `remaining_usd`, which
/// is what OpenRouter documents for the not-yet-spent case.
pub fn binding_budget(data: &Map<String, Value>) -> Option<Budget> {
    if let Some(eff) = data.get("effective_budget") {
        return Budget::from_json(eff);
    }
    data.get("budgets")?
        .as_array()?
        .iter()
        .filter_map(Budget::from_json)
        .min_by(|a, b| a.remaining_usd.total_cmp(&b.remaining_usd))
}

fn short_zagreb(secs: i64) -> String {
    let s = zagreb_rfc3339(secs);
    let zone = if zagreb_offset(secs) == 7_200 {
        "CEST"
    } else {
        "CET"
    };
    format!("{} {} {zone}", &s[..10], &s[11..16])
}

/// OpenRouter purse from the `/api/v1/key` reply and, when the key has no
/// own limit, the `/api/v1/credits` reply (`None` = not fetched).
pub fn openrouter_purse(key: &Reply, credits: Option<&Reply>) -> RouterPurse {
    let fail = |note: String| RouterPurse {
        purse: Purse::new("OpenRouter", None, false, note),
        weekly: None,
        monthly: None,
        binding: Binding::None,
        budget: None,
    };
    let body = match key {
        Reply::MissingKey => return fail("missing key".into()),
        Reply::Failed | Reply::NotAsked => return fail("request failed".into()),
        Reply::Http(200, body) => body.as_ref(),
        Reply::Http(s, _) => return fail(format!("HTTP {s}")),
    };
    let empty = Map::new();
    let data = data_of(body).unwrap_or(&empty);
    let weekly = data.get("usage_weekly").and_then(finite);
    let monthly = data.get("usage_monthly").and_then(finite);
    let budget = binding_budget(data);
    let mut out = RouterPurse {
        purse: Purse::new("OpenRouter", None, false, ""),
        weekly,
        monthly,
        binding: Binding::None,
        budget: budget.clone(),
    };
    let week_bit = weekly.map_or(String::new(), |w| {
        format!(" · ${} this week", to_fixed(w, 2))
    });
    let month_bit = monthly.map_or(String::new(), |m| {
        format!(" · ${} this month", to_fixed(m, 2))
    });

    // (usd, binding, base note) candidates; the smallest wins.
    let mut cands: Vec<(f64, Binding, String)> = Vec::new();
    match data.get("limit_remaining") {
        Some(v) if finite(v).is_some() => {
            cands.push((
                finite(v).unwrap_or(0.0),
                Binding::KeyLimit,
                "key limit left".into(),
            ));
        }
        Some(v) if !v.is_null() => {
            out.purse.note = "balance not a number".into();
            return out;
        }
        _ => {
            // No own key limit: account credits bound it (bank.ts fallback).
            let credit: Result<f64, String> = match credits {
                Some(Reply::Http(200, cb)) => {
                    let cd = data_of(cb.as_ref());
                    let total = cd.and_then(|d| d.get("total_credits")).and_then(finite);
                    let usage = cd.and_then(|d| d.get("total_usage")).and_then(finite);
                    match (total, usage) {
                        (Some(t), Some(u)) => Ok(t - u),
                        _ => Err("balance not published".into()),
                    }
                }
                Some(Reply::Http(s, _)) => Err(format!("HTTP {s}")),
                Some(_) => Err("request failed".into()),
                None => Err("credits not read".into()),
            };
            match credit {
                Ok(c) => cands.push((c, Binding::Credits, "credits minus usage".into())),
                Err(note) if budget.is_none() => {
                    out.purse.note = note;
                    return out;
                }
                Err(_) => {}
            }
        }
    }
    if let Some(b) = &budget {
        cands.push((b.remaining_usd, Binding::Budget, String::new()));
    }
    // Strictly smaller wins, so a tie keeps the key/credits wording (parity).
    let mut best = cands[0].clone();
    for c in &cands[1..] {
        if c.0 < best.0 {
            best = c.clone();
        }
    }
    let (usd, binding, base) = best;
    let usd = usd.max(0.0);
    let note = if binding == Binding::Budget {
        let b = budget.as_ref().expect("budget candidate");
        let mut n = if b.remaining_usd <= 0.0 {
            format!(
                "blocked: {} spent (${} of ${})",
                b.label(),
                to_fixed(b.spend_usd, 2),
                to_fixed(b.limit_usd, 2)
            )
        } else {
            format!(
                "{} left (${} of ${})",
                b.label(),
                to_fixed(b.spend_usd, 2),
                to_fixed(b.limit_usd, 2)
            )
        };
        if let Some(at) = b.resets_at {
            n.push_str(&format!(", resets {}", short_zagreb(at)));
        }
        // say what the budget is tighter than
        let others: Vec<String> = cands
            .iter()
            .filter(|c| c.1 != Binding::Budget)
            .map(|c| match c.1 {
                Binding::KeyLimit => format!("key limit ${}", to_fixed(c.0, 2)),
                _ => format!("credits ${}", to_fixed(c.0, 2)),
            })
            .collect();
        if !others.is_empty() {
            n.push_str(&format!("; {} left", others.join(", ")));
        }
        n
    } else {
        base
    };
    out.purse = Purse::new(
        "OpenRouter",
        Some(usd),
        true,
        if binding == Binding::Credits {
            note // bank.ts: no week/month bits on the credits path
        } else {
            format!("{note}{week_bit}{month_bit}")
        },
    );
    out.binding = binding;
    out
}

/// River purse (`/v1/models` reply status only; no balance API).
pub fn river_purse(r: &Reply) -> Purse {
    match r {
        Reply::MissingKey => Purse::new("River", None, false, "missing key"),
        Reply::Failed => Purse::new("River", None, false, "request failed"),
        Reply::NotAsked => Purse::new("River", None, false, "not checked"),
        Reply::Http(200, _) => Purse::new(
            "River",
            None,
            false,
            "key live, balance not published by the API",
        ),
        Reply::Http(s, _) => Purse::new("River", None, false, format!("HTTP {s}")),
    }
}

fn text_field(o: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        o.get(*k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

/// `moneyUnder(value)`.
fn money_under(v: &Value, depth: usize) -> Option<f64> {
    let o = v.as_object()?;
    if depth > 4 {
        return None;
    }
    for name in ["usd", "cash", "credit", "credits"] {
        for (k, val) in o {
            if k.to_lowercase() == name {
                if let Some(n) = finite(val) {
                    return Some(n);
                }
            }
        }
    }
    o.values().find_map(|x| money_under(x, depth + 1))
}

/// `runpodPurse()` over a parsed survival.json.
pub fn runpod_purse(survival: Option<&Value>) -> Option<Purse> {
    let rp = survival?.get("runpod")?.as_object()?;
    if rp.get("measured") == Some(&Value::Bool(false)) {
        let note = text_field(rp, &["note", "reason", "message"]).unwrap_or("not measured".into());
        return Some(Purse::new("Runpod", None, false, note));
    }
    let amount = money_under(&Value::Object(rp.clone()), 0)?;
    let note = text_field(rp, &["note"]).unwrap_or("survival.json".into());
    Some(Purse::new("Runpod", Some(amount), true, note))
}

/// Last-known OpenRouter figure from survival.json when the live read
/// failed (design step 4 of docs/plans/console-bank-reader.md).
pub fn survival_router(survival: Option<&Value>, now: i64) -> Option<(f64, String)> {
    let s = survival?;
    let or = s.get("openrouter")?.as_object()?;
    if or.get("measured") != Some(&Value::Bool(true)) {
        return None;
    }
    let left = or.get("key_left").and_then(finite)?;
    let when = s
        .get("when")
        .and_then(Value::as_str)
        .and_then(parse_rfc3339);
    let note = match when {
        Some(w) => {
            let age_h = ((now - w).max(0) as f64) / 3600.0;
            format!(
                "from survival probe {} ({} h old)",
                short_zagreb(w),
                to_fixed(age_h, 1)
            )
        }
        None => "from survival probe (time unknown)".into(),
    };
    Some((left, note))
}

/// One session row's spend fields.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendRow {
    pub id: String,
    pub last_activity: Option<f64>,
    pub tokens_in: f64,
    pub tokens_out: f64,
    pub cost_usd: f64,
}

/// `activityMs(value)` in seconds: ms epoch (> 1e12), s epoch (> 1e9),
/// numeric strings, or RFC 3339 text.
pub fn activity_secs(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => {
            let f = n.as_f64()?;
            if f > 1e12 {
                Some(f / 1000.0)
            } else if f > 1e9 {
                Some(f)
            } else {
                None
            }
        }
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                return None;
            }
            if let Ok(f) = t.parse::<f64>() {
                return activity_secs(&json!(f));
            }
            parse_rfc3339(&t.replace(' ', "T")).map(|x| x as f64)
        }
        _ => None,
    }
}

/// `finiteOrZero(value)`.
pub fn finite_or_zero(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).unwrap_or(0.0),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeekSpend {
    pub tokens_in: f64,
    pub tokens_out: f64,
    pub cost_usd: f64,
    /// Sessions counted (after de-dup and the 7-day cutoff).
    pub sessions: u64,
    /// Rows skipped because an earlier source had the same session id.
    pub duplicates: u64,
}

/// Week totals over several sources in priority order. A session id seen
/// in an earlier source wins (hermes-rs `Layered` store order: own DB
/// first, legacy second).
pub fn week_spend(sources: &[Vec<SpendRow>], now: i64) -> WeekSpend {
    let cutoff = now as f64 - WEEK_S;
    let mut seen = std::collections::HashSet::new();
    let mut w = WeekSpend::default();
    for rows in sources {
        let mut here = std::collections::HashSet::new();
        for r in rows {
            if !r.id.is_empty() && seen.contains(&r.id) {
                w.duplicates += 1;
                continue;
            }
            if !r.id.is_empty() {
                here.insert(r.id.clone());
            }
            let Some(at) = r.last_activity else { continue };
            if at < cutoff {
                continue;
            }
            w.tokens_in += r.tokens_in;
            w.tokens_out += r.tokens_out;
            w.cost_usd += r.cost_usd;
            w.sessions += 1;
        }
        seen.extend(here);
    }
    w
}

/// Today's hermes-rs ledger figure (`rs-spend.json`): only when its `day`
/// is today's Zagreb day, otherwise the ledger has not rolled yet and
/// today's spend is 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Today {
    pub day: String,
    pub usd: f64,
    pub calls: u64,
    pub ledger_day: Option<String>,
}

pub fn today_spend(ledger: Option<&Value>, now: i64) -> Option<Today> {
    let l = ledger?.as_object()?;
    let day = zagreb_day(now);
    let ledger_day = l.get("day").and_then(Value::as_str).map(str::to_string);
    let current = ledger_day.as_deref() == Some(day.as_str());
    Some(Today {
        usd: if current {
            l.get("day_usd").map(finite_or_zero).unwrap_or(0.0)
        } else {
            0.0
        },
        calls: if current {
            l.get("day_calls").and_then(Value::as_u64).unwrap_or(0)
        } else {
            0
        },
        day,
        ledger_day,
    })
}

/// Everything the snapshot is built from.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub openrouter_key: Reply,
    pub openrouter_credits: Option<Reply>,
    pub river: Reply,
    pub survival: Option<Value>,
    /// Session rows per source, priority order. Empty = no spend DB given.
    pub spend_sources: Vec<Vec<SpendRow>>,
    pub ledger: Option<Value>,
    pub now: i64,
}

fn snap_note(r: &RouterPurse, total: Option<f64>, days_left: Option<f64>, now: i64) -> String {
    let week = r.weekly.unwrap_or(0.0);
    let month = r.monthly.unwrap_or(0.0);
    if !r.purse.measured || total.is_none() {
        return "No provider balance could be read, so there is no runway figure.".into();
    }
    if r.binding == Binding::Survival {
        // weekly/monthly usage is unknown here, not zero
        return "OpenRouter was not read live, so its balance is the last survival probe figure and its spend is unknown.".into();
    }
    let what = match (&r.binding, &r.budget) {
        (Binding::Budget, Some(b)) => format!("The {}", b.label()),
        _ => "The key limit".into(),
    };
    let what_lc = {
        let mut c = what.chars();
        match c.next() {
            Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    };
    let days = days_left.map_or("unknown".to_string(), |d| to_fixed(d, 0));
    let mut n = if week <= 0.0 && month <= 0.0 {
        "OpenRouter recorded no spend this week or this month, so no runway is estimated.".into()
    } else if week <= 0.0 {
        format!(
            "OpenRouter recorded $0 this week and ${} this month. At that monthly rate, about ${} a day, {what_lc} lasts about {days} days.",
            to_fixed(month, 2),
            to_fixed(month / 30.0, 2)
        )
    } else {
        format!(
            "OpenRouter recorded ${} this week, about ${} a day. {what} lasts about {days} days at that rate.",
            to_fixed(week, 2),
            to_fixed(week / 7.0, 2)
        )
    };
    // A resetting budget refills before the runway runs out: say so.
    if let (Binding::Budget, Some(b), Some(d)) = (&r.binding, &r.budget, days_left) {
        if let (Some(at), Some(_)) = (b.resets_at, &b.reset_interval) {
            if (at as f64) < now as f64 + d * 86_400.0 && at > now {
                n.push_str(&format!(" It resets {} before that.", short_zagreb(at)));
            }
        }
    }
    n
}

/// Build the `BankSnap` JSON. Key order and the first nine keys match
/// bank.ts; `asOf`, `binding`, `budget`, `localSpend` are additions the
/// current page ignores.
pub fn snapshot(inp: &Inputs) -> Value {
    let mut router = openrouter_purse(&inp.openrouter_key, inp.openrouter_credits.as_ref());
    if !router.purse.measured {
        if let Some((left, note)) = survival_router(inp.survival.as_ref(), inp.now) {
            router.purse = Purse::new("OpenRouter", Some(left.max(0.0)), true, note);
            router.binding = Binding::Survival;
        }
    }
    let mut purses = vec![
        router.purse.clone(),
        river_purse(&inp.river),
        Purse::new("Revolut", None, false, "not connected"),
    ];
    if let Some(rp) = runpod_purse(inp.survival.as_ref()) {
        purses.push(rp);
    }
    let measured: Vec<f64> = purses
        .iter()
        .filter(|p| p.measured)
        .filter_map(|p| p.usd.filter(|u| u.is_finite()))
        .collect();
    let total = if measured.is_empty() {
        None
    } else {
        Some(measured.iter().sum::<f64>())
    };
    let spend = week_spend(&inp.spend_sources, inp.now);
    let today = today_spend(inp.ledger.as_ref(), inp.now);
    let week = router.weekly.unwrap_or(0.0);
    let month = router.monthly.unwrap_or(0.0);
    let local_rate = !(week > 0.0 || month > 0.0) && spend.cost_usd > 0.0;
    let daily = if week > 0.0 {
        week / 7.0
    } else if month > 0.0 {
        month / 30.0
    } else if local_rate {
        spend.cost_usd / 7.0
    } else {
        0.0
    };
    let days_left = match total {
        Some(t) if daily >= 0.01 => Some(t / daily),
        _ => None,
    };
    let river_daily = (spend.tokens_in / 1e6 * 0.12 + spend.tokens_out / 1e6 * 0.48) / 7.0;
    let mut note = snap_note(&router, total, days_left, inp.now);
    let have_local = !inp.spend_sources.is_empty() || today.is_some();
    if have_local {
        let mut bit = format!(
            " hermes-rs sessions recorded ${} in the last 7 days",
            to_fixed(spend.cost_usd, 2)
        );
        if let Some(t) = &today {
            bit.push_str(&format!(
                " and ${} today ({} calls)",
                to_fixed(t.usd, 2),
                t.calls
            ));
        }
        bit.push('.');
        if local_rate && router.purse.measured {
            if let Some(d) = days_left {
                bit.push_str(&format!(
                    " At that local rate the balance lasts about {} days.",
                    to_fixed(d, 0)
                ));
            }
        }
        note.push_str(&bit);
    }
    let mut snap = Map::new();
    snap.insert(
        "purses".into(),
        Value::Array(purses.iter().map(Purse::to_json).collect()),
    );
    snap.insert("totalUsd".into(), opt_num(total));
    snap.insert("weekTokensIn".into(), num(spend.tokens_in));
    snap.insert("weekTokensOut".into(), num(spend.tokens_out));
    snap.insert("weekCostUsd".into(), num(spend.cost_usd));
    snap.insert("dailyUsd".into(), num(daily));
    snap.insert("daysLeft".into(), opt_num(days_left));
    snap.insert("riverIfMovedDailyUsd".into(), num(river_daily));
    snap.insert("note".into(), json!(note));
    snap.insert("asOf".into(), json!(zagreb_rfc3339(inp.now)));
    snap.insert("binding".into(), json!(router.binding.as_str()));
    snap.insert(
        "budget".into(),
        router.budget.as_ref().map_or(Value::Null, Budget::to_json),
    );
    snap.insert(
        "localSpend".into(),
        json!({
            "weekCostUsd": num(spend.cost_usd),
            "weekSessions": spend.sessions,
            "duplicateRows": spend.duplicates,
            "sources": inp.spend_sources.len(),
            "day": today.as_ref().map(|t| t.day.clone()),
            "todayUsd": opt_num(today.as_ref().map(|t| t.usd)),
            "todayCalls": today.as_ref().map(|t| t.calls),
            "ledgerDay": today.as_ref().and_then(|t| t.ledger_day.clone()),
            "rateSource": if week > 0.0 { "openrouter_week" } else if month > 0.0 { "openrouter_month" } else if local_rate { "local_week" } else { "none" },
        }),
    );
    Value::Object(snap)
}

/// Read session spend rows from a hermes `state.db` / `rs-state.db`,
/// read-only. Missing file = `Ok(empty)`; other errors are returned
/// without the OS text so a caller can log a short reason.
#[cfg(feature = "sqlite")]
pub fn read_spend_db(path: &std::path::Path) -> Result<Vec<SpendRow>, String> {
    use rusqlite::types::ValueRef;
    use rusqlite::{Connection, OpenFlags};
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "open failed".to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "busy timeout".to_string())?;
    let mut st = conn
        .prepare(
            "SELECT id, last_activity_at, input_tokens, output_tokens, estimated_cost_usd FROM sessions",
        )
        .map_err(|_| "no sessions table".to_string())?;
    let to_json = |v: ValueRef| -> Value {
        match v {
            ValueRef::Integer(i) => json!(i),
            ValueRef::Real(f) => json!(f),
            ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
            _ => Value::Null,
        }
    };
    let rows = st
        .query_map([], |r| {
            Ok(SpendRow {
                id: match r.get_ref(0)? {
                    ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                    ValueRef::Integer(i) => i.to_string(),
                    _ => String::new(),
                },
                last_activity: activity_secs(&to_json(r.get_ref(1)?)),
                tokens_in: finite_or_zero(&to_json(r.get_ref(2)?)),
                tokens_out: finite_or_zero(&to_json(r.get_ref(3)?)),
                cost_usd: finite_or_zero(&to_json(r.get_ref(4)?)),
            })
        })
        .map_err(|_| "query failed".to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "row read failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_matches_js() {
        assert_eq!(to_fixed(62.5, 0), "63"); // JS rounds ties up, Rust {:.0} gives 62
        assert_eq!(to_fixed(1.4, 2), "1.40");
        assert_eq!(to_fixed(0.4755, 2), "0.48");
        assert_eq!(to_fixed(-2.5, 0), "-3");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(13.75, 0), "14");
    }

    #[test]
    fn num_is_js_like() {
        assert_eq!(num(12.0).to_string(), "12");
        assert_eq!(num(12.5).to_string(), "12.5");
        assert_eq!(num(f64::NAN), Value::Null);
    }

    #[test]
    fn activity_forms() {
        let now = 1_761_620_400.0; // 2025-10-28 03:00 UTC-ish, any s epoch > 1e9
        assert_eq!(activity_secs(&json!(now)), Some(now));
        assert_eq!(activity_secs(&json!(now * 1000.0)), Some(now));
        assert_eq!(activity_secs(&json!("1761620400")), Some(now));
        assert_eq!(activity_secs(&json!(5)), None);
        assert_eq!(activity_secs(&json!("")), None);
        assert_eq!(
            activity_secs(&json!("2026-10-18 21:12:00+02:00")),
            parse_rfc3339("2026-10-18T21:12:00+02:00").map(|x| x as f64)
        );
        assert_eq!(activity_secs(&Value::Null), None);
        assert_eq!(finite_or_zero(&json!("3.5")), 3.5);
        assert_eq!(finite_or_zero(&json!("x")), 0.0);
        assert_eq!(finite_or_zero(&Value::Null), 0.0);
    }

    #[test]
    fn budget_labels() {
        let b = Budget::from_json(&json!({"scope":"member","limit_usd":10,"spend_usd":1,"remaining_usd":9,"reset_interval":"weekly","resets_at":null})).unwrap();
        assert_eq!(b.label(), "member guardrail weekly budget");
        let b = Budget::from_json(
            &json!({"scope":"workspace","remaining_usd":1,"reset_interval":null}),
        )
        .unwrap();
        assert_eq!(b.label(), "workspace lifetime budget");
        assert!(b.limit_usd.is_nan());
        assert_eq!(b.to_json()["limitUsd"], Value::Null);
        assert!(Budget::from_json(&json!({"scope":"member"})).is_none());
        assert!(Budget::from_json(&json!({"scope":"member","remaining_usd":"9"})).is_none());
    }

    #[test]
    fn dedupe_prefers_first_source() {
        let now = 2_000_000_000;
        let row = |id: &str, cost: f64| SpendRow {
            id: id.into(),
            last_activity: Some(now as f64 - 60.0),
            tokens_in: 1.0,
            tokens_out: 1.0,
            cost_usd: cost,
        };
        let w = week_spend(
            &[
                vec![row("a", 1.0), row("b", 2.0)],
                vec![row("a", 50.0), row("c", 4.0)],
            ],
            now,
        );
        assert_eq!(w.cost_usd, 7.0);
        assert_eq!(w.sessions, 3);
        assert_eq!(w.duplicates, 1);
        // empty ids are never de-duplicated
        let w = week_spend(&[vec![row("", 1.0)], vec![row("", 1.0)]], now);
        assert_eq!((w.sessions, w.duplicates), (2, 0));
    }

    #[test]
    fn non_200_and_missing() {
        let r = openrouter_purse(&Reply::Http(401, None), None);
        assert_eq!(r.purse.note, "HTTP 401");
        assert!(!r.purse.measured);
        assert_eq!(
            openrouter_purse(&Reply::MissingKey, None).purse.note,
            "missing key"
        );
        assert_eq!(
            openrouter_purse(&Reply::Failed, None).purse.note,
            "request failed"
        );
        let r = openrouter_purse(
            &Reply::Http(200, Some(json!({"data":{"limit_remaining":"5"}}))),
            None,
        );
        assert_eq!(r.purse.note, "balance not a number");
        assert_eq!(
            river_purse(&Reply::Http(200, None)).note,
            "key live, balance not published by the API"
        );
        assert_eq!(river_purse(&Reply::Http(403, None)).note, "HTTP 403");
        assert_eq!(river_purse(&Reply::NotAsked).note, "not checked");
    }
}
