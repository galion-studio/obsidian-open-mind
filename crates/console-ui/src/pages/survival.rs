// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Survival card on `/bank` (wave 4 slot 408, Console V2 prep).
//!
//! Renders the staged `GET /api/v1/survival` status of `console-static`
//! (slot 389, `--survival-file`): when `survival.json` was last written
//! (`asOf`, shown on the Zagreb clock), how old it is, whether it is stale,
//! and the state of the timer-free writer. The status route reports the file
//! mtime only, never its content (cash figures); this card goes one step
//! further and reads an allowlist of keys only, so even a misbehaving
//! upstream that put file content into the reply could not get it onto the
//! page. Strings from the reply are never echoed: `asOf` is parsed and
//! re-rendered, `mode` is matched against the two known names, numbers are
//! formatted here.
//!
//! States: fresh, stale (age over `staleAfterS`, default 3 × `refreshS`, or
//! the reader said `stale`), missing file, refreshing now, last run failed,
//! not served (404: the console runs without `--survival-file`), gated
//! (401/403), other HTTP errors, upstream down / not JSON.

use super::bank::span;
use crate::html::esc;
use crate::json::Value;

/// Why there is no status object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    /// HTTP status other than 200.
    Status(u16),
    /// No answer, or not JSON.
    Down,
}

/// The writer's old cron cadence (also the status route's default).
pub const REFRESH_DEFAULT_S: u64 = 900;
/// Stale after this many refresh intervals (same rule as the route).
pub const STALE_INTERVALS: u64 = 3;
/// `asOf` up to this far ahead of the console clock is jitter, not skew.
const SKEW_S: i64 = 60;
/// Largest number the card believes (≈ 31 years in seconds); anything
/// bigger is treated as absent.
const MAX_N: f64 = 1.0e9;

/// The status keys the card reads. Everything else in the reply is ignored.
pub const KEYS: &[&str] = &[
    "exists",
    "asOf",
    "ageS",
    "stale",
    "staleAfterS",
    "refreshS",
    "mode",
    "writer",
    "running",
    "lastStart",
    "lastExit",
    "runs",
    "failures",
];

fn num(v: &Value) -> Option<i64> {
    match v {
        Value::Num(n) if n.is_finite() && n.abs() <= MAX_N => Some(n.round() as i64),
        _ => None,
    }
}

fn clock(at: i64) -> String {
    super::p4::zagreb(at)
}

/// Parsed, allowlisted view of the status object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub exists: bool,
    /// `asOf` (file mtime) as Unix seconds.
    pub at: Option<i64>,
    /// Age in seconds at `now` (from `asOf`, else `ageS`).
    pub age: Option<i64>,
    /// `asOf` ahead of the console clock by this much (skew).
    pub ahead: Option<i64>,
    pub stale: bool,
    pub stale_after: u64,
    pub refresh: u64,
    /// `on-request` / `interval`; anything else is `None`.
    pub mode: Option<&'static str>,
    pub has_writer: bool,
    pub running: bool,
    pub last_start: Option<i64>,
    pub last_exit: Option<i64>,
    pub runs: Option<i64>,
    pub failures: Option<i64>,
}

/// Read the allowlisted keys of a `/api/v1/survival` reply at `now`.
pub fn read(v: &Value, now: i64) -> Status {
    let refresh = num(v.get("refreshS"))
        .filter(|n| *n > 0)
        .map_or(REFRESH_DEFAULT_S, |n| n as u64);
    let stale_after = num(v.get("staleAfterS"))
        .filter(|n| *n > 0)
        .map_or(refresh * STALE_INTERVALS, |n| n as u64);
    let at = v.get("asOf").as_str().and_then(super::security::parse_iso);
    let exists = v.get("exists").as_bool().unwrap_or(at.is_some());
    let (mut age, mut ahead) = (None, None);
    if let Some(t) = at {
        let d = now - t;
        if d < -SKEW_S {
            ahead = Some(-d);
        } else {
            age = Some(d.max(0));
        }
    }
    if at.is_none() {
        age = num(v.get("ageS")).filter(|n| *n >= 0);
    }
    let old = age.is_some_and(|a| a as u64 > stale_after);
    let stale = !exists || old || v.get("stale").as_bool() == Some(true);
    let mode = match v.get("mode").as_str() {
        Some("on-request") => Some("on-request"),
        Some("interval") => Some("interval"),
        _ => None,
    };
    Status {
        exists,
        at,
        age,
        ahead,
        stale,
        stale_after,
        refresh,
        mode,
        has_writer: matches!(v.get("writer"), Value::Str(s) if !s.is_empty()),
        running: v.get("running").as_bool() == Some(true),
        last_start: v
            .get("lastStart")
            .as_str()
            .and_then(super::security::parse_iso),
        last_exit: num(v.get("lastExit")),
        runs: num(v.get("runs")).filter(|n| *n >= 0),
        failures: num(v.get("failures")).filter(|n| *n >= 0),
    }
}

fn exit_text(code: i64) -> String {
    match code {
        0 => "ok".into(),
        -1 => "could not start".into(),
        -2 => "killed after 90 s".into(),
        -3 => "killed by a signal".into(),
        c => format!("exit {c}"),
    }
}

/// Short state word, also the `data-state` of the card (tests, CSS).
pub fn state(s: &Status) -> &'static str {
    if !s.exists {
        "missing"
    } else if s.ahead.is_some() {
        "skew"
    } else if s.stale {
        "stale"
    } else if s.age.is_none() {
        "unknown"
    } else {
        "fresh"
    }
}

fn row(label: &str, value: &str) -> String {
    format!(
        "<li class=\"cash\"><span class=\"amt mono\">{}</span><span>{}</span></li>",
        esc(value),
        esc(label)
    )
}

fn open(state: &str) -> String {
    format!("<section aria-label=\"Survival\" data-state=\"{state}\"><h2>Survival snapshot</h2>")
}

/// The card at `now` (Unix seconds).
pub fn card_at(got: Result<&Value, Miss>, now: i64) -> String {
    let v = match got {
        Ok(v @ Value::Obj(_)) => v,
        Ok(_) => return error_card("down", "The survival status was not a JSON object."),
        Err(Miss::Status(404)) => {
            let mut out = open("off");
            out.push_str(
                "<p class=\"note\">This console does not serve /api/v1/survival (started without --survival-file), so the age of survival.json is not shown here.</p></section>",
            );
            return out;
        }
        Err(Miss::Status(c @ (401 | 403))) => {
            return error_card(
                "gated",
                &format!(
                "/api/v1/survival answered HTTP {c}: pair this phone to see the survival status."
            ),
            )
        }
        Err(Miss::Status(c)) => {
            return error_card("error", &format!("/api/v1/survival answered HTTP {c}."))
        }
        Err(Miss::Down) => {
            return error_card(
                "down",
                "The console API did not answer /api/v1/survival with a JSON status.",
            )
        }
    };
    let s = read(v, now);
    let st = state(&s);
    let mut out = open(st);
    out.push_str("<ul class=\"rows\">");
    let when = match (s.exists, s.at) {
        (false, _) => "missing".to_string(),
        (true, Some(t)) => clock(t),
        (true, None) => "unknown".to_string(),
    };
    out.push_str(&row("survival.json as of", &when));
    let age = match (s.age, s.ahead) {
        (_, Some(a)) => format!("{} ahead (clock skew)", span(a)),
        (Some(a), _) => span(a),
        _ => "—".to_string(),
    };
    out.push_str(&row("age", &age));
    out.push_str(&row(
        "stale after",
        &format!(
            "{} ({} × {})",
            span(s.stale_after as i64),
            STALE_INTERVALS,
            span(s.refresh as i64)
        ),
    ));
    let writer = if !s.has_writer {
        "status only (no writer)".to_string()
    } else if s.running {
        "refreshing now".to_string()
    } else {
        match (s.last_start, s.last_exit) {
            (Some(t), Some(c)) => format!("last run {}: {}", clock(t), exit_text(c)),
            (Some(t), None) => format!("last run {}", clock(t)),
            _ => "no run since the console started".to_string(),
        }
    };
    out.push_str(&row("writer", &writer));
    if s.has_writer {
        let mut tally = format!(
            "{} runs, {} failed",
            s.runs.unwrap_or(0),
            s.failures.unwrap_or(0)
        );
        if let Some(m) = s.mode {
            tally.push_str(&format!(" · {m}"));
        }
        out.push_str(&row("refresh", &tally));
    }
    out.push_str("</ul>");
    match st {
        "missing" => out.push_str(
            "<p class=\"note t-bad\" role=\"status\">survival.json is missing: the writer has not written it yet.</p>",
        ),
        "unknown" => out.push_str(
            "<p class=\"note t-bad\" role=\"status\">The console sent no readable time for survival.json, so its age is unknown.</p>",
        ),
        "skew" => out.push_str(
            "<p class=\"note t-bad\" role=\"status\">survival.json is dated ahead of the console clock, so its age is unknown.</p>",
        ),
        "stale" => {
            let why = match s.age {
                Some(a) if a as u64 > s.stale_after => format!(
                    "Stale: survival.json is {} old, more than {} refreshes of {}.",
                    span(a),
                    STALE_INTERVALS,
                    span(s.refresh as i64)
                ),
                _ => "Stale: the console marks survival.json as stale.".to_string(),
            };
            out.push_str(&format!(
                "<p class=\"note t-bad\" role=\"status\">{} Survival figures may be out of date.</p>",
                esc(&why)
            ));
        }
        _ => {}
    }
    if s.has_writer && !s.running && s.last_exit.is_some_and(|c| c != 0) {
        out.push_str(&format!(
            "<p class=\"note t-bad\" role=\"status\">The last writer run failed ({}); the console waits {} before it tries again.</p>",
            esc(&exit_text(s.last_exit.unwrap_or(0))),
            span(s.refresh as i64)
        ));
    }
    out.push_str("</section>");
    out
}

fn error_card(state: &str, why: &str) -> String {
    let mut out = open(state);
    out.push_str(&format!(
        "<p class=\"note t-bad\" role=\"status\">{} The age of survival.json is unknown.</p></section>",
        esc(why)
    ));
    out
}

/// [`card_at`] against the console clock now.
pub fn card(got: Result<&Value, Miss>) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    card_at(got, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    // 2026-10-28 17:00:00 +01:00
    const NOW: i64 = 1_793_203_200;

    fn c(body: &str) -> String {
        card_at(Ok(&parse(body.as_bytes()).unwrap()), NOW)
    }

    #[test]
    fn fresh_shows_zagreb_as_of_and_age() {
        let h = c(
            r#"{"ok":true,"exists":true,"asOf":"2026-10-28T16:55:00+01:00","ageS":300,"stale":false,"staleAfterS":2700,"refreshS":900,"mode":"interval","writer":"/opt/galion/rust/bin/galion-survival","running":false,"lastStart":"2026-10-28T16:54:58+01:00","lastExit":0,"runs":3,"failures":0}"#,
        );
        assert!(h.contains("data-state=\"fresh\""), "{h}");
        assert!(h.contains(">2026-10-28 16:55 CET (+01:00)<"), "{h}");
        assert!(h.contains(">5 min<"), "{h}");
        assert!(h.contains(">45 min (3 × 15 min)<"), "{h}");
        assert!(
            h.contains("last run 2026-10-28 16:54 CET (+01:00): ok"),
            "{h}"
        );
        assert!(h.contains("3 runs, 0 failed · interval"), "{h}");
        assert!(!h.contains("t-bad"), "{h}");
        assert!(!h.contains("/opt/galion"), "writer path not shown: {h}");
    }

    #[test]
    fn utc_as_of_is_shown_on_the_zagreb_clock() {
        let h = c(r#"{"exists":true,"asOf":"2026-10-28T15:55:00Z"}"#); // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
        assert!(h.contains(">2026-10-28 16:55 CET (+01:00)<"), "{h}");
        // Summer time side.
        let s = card_at(
            Ok(&parse(br#"{"exists":true,"asOf":"2026-10-18T21:35:50+02:00"}"#).unwrap()),
            NOW,
        );
        assert!(s.contains(">2026-10-18 21:35 CEST (+02:00)<"), "{s}");
        assert!(s.contains("data-state=\"stale\""), "{s}");
    }

    #[test]
    fn stale_by_age_and_by_flag() {
        // The live one-shot: 2026-10-27 18:19:50 CET, ~22.7 h at NOW.
        let h = c(
            r#"{"exists":true,"asOf":"2026-10-27T18:19:50+01:00","stale":true,"staleAfterS":2700,"refreshS":900,"writer":null}"#,
        );
        assert!(h.contains("data-state=\"stale\""), "{h}");
        assert!(
            h.contains("Stale: survival.json is 22.7 h old, more than 3 refreshes of 15 min."),
            "{h}"
        );
        assert!(h.contains("status only (no writer)"), "{h}");
        // Age under the line but the console says stale.
        let f = c(r#"{"exists":true,"asOf":"2026-10-28T16:59:00+01:00","stale":true}"#);
        assert!(
            f.contains("Stale: the console marks survival.json as stale."),
            "{f}"
        );
        // Age over our own line even if the flag says false.
        let o =
            c(r#"{"exists":true,"asOf":"2026-10-28T15:00:00+01:00","stale":false,"refreshS":900}"#);
        assert!(o.contains("data-state=\"stale\""), "{o}");
        // Exactly on the line is not stale (route rule: age > staleAfterS).
        let e = c(r#"{"exists":true,"asOf":"2026-10-28T16:15:00+01:00","refreshS":900}"#);
        assert!(e.contains("data-state=\"fresh\""), "{e}");
    }

    #[test]
    fn missing_file_and_skew() {
        let m = c(
            r#"{"ok":false,"exists":false,"asOf":null,"ageS":null,"stale":true,"writer":"/w","runs":0,"failures":0}"#,
        );
        assert!(m.contains("data-state=\"missing\""), "{m}");
        assert!(m.contains(">missing<"), "{m}");
        assert!(m.contains("no run since the console started"), "{m}");
        let k = c(r#"{"exists":true,"asOf":"2026-10-28T18:00:00+01:00"}"#);
        assert!(k.contains("data-state=\"skew\""), "{k}");
        assert!(k.contains("60 min ahead (clock skew)"), "{k}");
    }

    #[test]
    fn writer_failure_running_and_codes() {
        let f = c(
            r#"{"exists":true,"asOf":"2026-10-28T16:59:00+01:00","writer":"/w","running":false,"lastStart":"2026-10-28T16:58:00+01:00","lastExit":1,"runs":1,"failures":1,"refreshS":900}"#,
        );
        assert!(
            f.contains("last run 2026-10-28 16:58 CET (+01:00): exit 1"),
            "{f}"
        );
        assert!(
            f.contains("The last writer run failed (exit 1); the console waits 15 min"),
            "{f}"
        );
        let r = c(
            r#"{"exists":true,"asOf":"2026-10-28T16:59:00+01:00","writer":"/w","running":true,"lastExit":1}"#,
        );
        assert!(r.contains("refreshing now"), "{r}");
        assert!(!r.contains("last writer run failed"), "{r}");
        for (code, text) in [
            (-1, "could not start"),
            (-2, "killed after 90 s"),
            (-3, "killed by a signal"),
        ] {
            let b = format!(
                r#"{{"exists":true,"asOf":"2026-10-28T16:59:00+01:00","writer":"/w","lastStart":"2026-10-28T16:58:00+01:00","lastExit":{code}}}"#
            );
            assert!(c(&b).contains(text), "{code}");
        }
    }

    #[test]
    fn error_states() {
        let off = card_at(Err(Miss::Status(404)), NOW);
        assert!(
            off.contains("data-state=\"off\"") && off.contains("--survival-file"),
            "{off}"
        );
        let g = card_at(Err(Miss::Status(401)), NOW);
        assert!(
            g.contains("data-state=\"gated\"") && g.contains("HTTP 401"),
            "{g}"
        );
        let e = card_at(Err(Miss::Status(502)), NOW);
        assert!(
            e.contains("data-state=\"error\"") && e.contains("HTTP 502"),
            "{e}"
        );
        let d = card_at(Err(Miss::Down), NOW);
        assert!(
            d.contains("data-state=\"down\"") && d.contains("did not answer"),
            "{d}"
        );
        let a = card_at(Ok(&parse(b"[1,2]").unwrap()), NOW);
        assert!(
            a.contains("data-state=\"down\"") && a.contains("not a JSON object"),
            "{a}"
        );
        for h in [off, g, e, d, a] {
            assert!(
                h.starts_with("<section") && h.ends_with("</section>"),
                "{h}"
            );
        }
    }

    #[test]
    fn never_file_content_and_never_raw_strings() {
        // A misbehaving upstream that leaks survival.json content and
        // hostile strings: none of it reaches the page.
        let h = c(
            r#"{"exists":true,"asOf":"<script>alert(1)</script>","mode":"<b>x</b>","writer":"<img src=x>",
            "lastStart":"\"><i>","file":"/opt/galion/galion-light/state/survival.json",
            "when":"2026-10-28T16:00:00+01:00","openrouter":{"usd":12.34,"key":"sk-or-v1-abcdef"},
            "revolut":{"usd":999.99},"runpod":{"usd":55.5},"cpu":{"load":18.02},"lane":"zai-org/GLM-5.3-Flash","ageS":120}"#,
        );
        for bad in [
            "<script",
            "<b>",
            "<img",
            "<i>",
            "12.34",
            "sk-or",
            "999.99",
            "55.5",
            "18.02",
            "GLM",
            "survival.json content",
            "/opt/galion",
            "openrouter",
            "revolut",
            "runpod",
        ] {
            assert!(!h.contains(bad), "{bad} leaked: {h}");
        }
        // Unparseable asOf falls back to ageS.
        assert!(h.contains(">2 min<"), "{h}");
        assert!(h.contains(">unknown<"), "{h}");
    }

    const UTC_INPUT: &str = r#"{"exists":true,"asOf":"2026-10-28T15:55:00Z","writer":"/w","lastStart":"2026-10-28T15:55:00Z","lastExit":0}"#; // repo-check: allow (upstream UTC input, shown on the Zagreb clock)

    #[test]
    fn absurd_numbers_are_ignored_and_never_z() {
        let h = c(
            r#"{"exists":true,"ageS":1e300,"refreshS":-5,"staleAfterS":"x","runs":-1,"failures":1e99,"writer":"/w"}"#,
        );
        assert!(h.contains(">—<"), "{h}");
        assert!(
            h.contains("data-state=\"unknown\"") && h.contains("no readable time"),
            "{h}"
        );
        assert!(h.contains("45 min (3 × 15 min)"), "{h}");
        assert!(h.contains("0 runs, 0 failed"), "{h}");
        for s in [h, c(UTC_INPUT)] {
            assert!(!s.contains("Z<") && !s.contains(":00Z"), "{s}");
        }
    }

    #[test]
    fn keys_allowlist_is_what_read_uses() {
        // Guard against someone adding a key to `read` without listing it.
        let src = include_str!("survival.rs");
        let body = &src[src.find("pub fn read(").unwrap()..src.find("fn exit_text").unwrap()];
        let mut used: Vec<&str> = body
            .split(".get(\"")
            .skip(1)
            .filter_map(|s| s.split('"').next())
            .collect();
        used.sort_unstable();
        used.dedup();
        let mut keys = KEYS.to_vec();
        keys.sort_unstable();
        assert_eq!(used, keys);
    }
}
