// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! One conversation = one Hermes thread (owner t443u/t444u, 2026-10-30 CET).
//!
//! * Every chat still sends `X-Hermes-Session-Key: galion-one`, so the
//!   gateway's memory key (and the memory pyramid L0 file `galion-one.md`)
//!   stays the same for every thread: memory is shared across conversations.
//! * `X-Hermes-Session-Id` is now per conversation:
//!   `one-<yyyymmdd>-<hhmm>-<6 hex>` (Europe/Zagreb wall clock).
//! * Choosing the thread for a turn ([`choose`]):
//!   1. `fresh` (the user tapped New chat) mints a new id;
//!   2. a thread the client names (the active or a reopened thread) is
//!      used as is, never split by the idle rule;
//!   3. otherwise the server's current thread continues if its last turn
//!      is under [`IDLE_SECS`] (30 min) old, else a new id is minted.
//!      Voice (/reply, /hear) has no thread of its own: it joins rule 3.
//! * Older threads (`one-admin`, `one-voice`) are left as they are and can
//!   be reopened like any other.

use crate::json::{self, Json};

/// Idle gap after which an un-named turn starts a new conversation.
pub const IDLE_SECS: u64 = 30 * 60;
/// Session key every Galion One thread shares (memory namespace).
pub const KEY: &str = "galion-one";

/// A Galion One thread id this server will send or reopen.
pub fn valid(id: &str) -> bool {
    id.len() > 4
        && id.len() <= 64
        && id.starts_with("one-")
        && id != "one-warm"
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `one-20261030-1042-a1b2c3` from a unix second and 3 random bytes.
pub fn mint(unix: u64, rnd: [u8; 3]) -> String {
    let t = crate::zagreb::rfc3339(unix as i64); // 2026-10-30T10:42:00+01:00
    let d: String = t
        .get(0..10)
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let h: String = t
        .get(11..16)
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    format!("one-{d}-{h}-{:02x}{:02x}{:02x}", rnd[0], rnd[1], rnd[2])
}

/// Pick the thread for a turn (see the module doc). `current` is the
/// server's last thread and the unix second of its last turn.
pub fn choose(
    requested: &str,
    fresh: bool,
    current: Option<(&str, u64)>,
    now: u64,
    rnd: [u8; 3],
) -> String {
    if !fresh {
        if valid(requested) {
            return requested.to_string();
        }
        if let Some((id, last)) = current {
            if valid(id) && now.saturating_sub(last) < IDLE_SECS {
                return id.to_string();
            }
        }
    }
    mint(now, rnd)
}

/// `{"id":..,"last":..}` state file text -> (id, last).
pub fn parse_state(text: &str) -> Option<(String, u64)> {
    let v = json::parse(text.trim()).ok()?;
    let id = json::text_of(json::get(&v, "id"));
    let last = match json::get(&v, "last") {
        Some(Json::Num(n)) => n.parse::<f64>().ok()? as u64,
        _ => return None,
    };
    valid(&id).then_some((id, last))
}

pub fn state_text(id: &str, last: u64) -> String {
    format!("{{\"id\":{},\"last\":{last}}}\n", json::dump_str(id))
}

fn num(v: Option<&Json>) -> f64 {
    match v {
        Some(Json::Num(n)) => n.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Gateway `/api/sessions` body -> Galion One thread list JSON (newest
/// first as the gateway sends them). Only `galion-one` keyed rows with a
/// Galion One id; titles are the first user message (gateway title).
pub fn list_json(sessions: &[u8], active: &str) -> String {
    let text = String::from_utf8_lossy(sessions);
    let rows = match json::parse(text.trim()) {
        Ok(v) => match json::get(&v, "data") {
            Some(Json::Arr(a)) => a.clone(),
            _ => Vec::new(),
        },
        Err(_) => return r#"{"error":"hermes"}"#.to_string(),
    };
    let mut out = Vec::new();
    for r in rows.iter() {
        let id = json::text_of(json::get(r, "id"));
        if json::text_of(json::get(r, "session_key")) != KEY || !valid(&id) {
            continue;
        }
        let title = json::text_of(json::get(r, "title"));
        let title = if title.trim().is_empty() {
            id.clone()
        } else {
            title
        };
        let at = match num(json::get(r, "ended_at")) {
            x if x > 0.0 => x,
            _ => num(json::get(r, "started_at")),
        };
        out.push(format!(
            "{{\"id\":{},\"title\":{},\"count\":{},\"at\":{}}}",
            json::dump_str(&id),
            json::dump_str(&title.chars().take(80).collect::<String>()),
            num(json::get(r, "message_count")) as u64,
            at as u64
        ));
        if out.len() >= 40 {
            break;
        }
    }
    format!(
        "{{\"active\":{},\"threads\":[{}]}}",
        json::dump_str(active),
        out.join(",")
    )
}

fn content_text(c: Option<&Json>) -> String {
    match c {
        Some(Json::Str(s)) => s.clone(),
        Some(Json::Arr(parts)) => parts
            .iter()
            .map(|p| json::text_of(json::get(p, "text")))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Gateway `/api/sessions/{id}/messages` body -> `{"id","messages":[{role,text}]}`
/// (user and assistant text only; tool traffic is not sent to the phone).
pub fn messages_json(id: &str, msgs: &[u8]) -> String {
    let text = String::from_utf8_lossy(msgs);
    let rows = match json::parse(text.trim()) {
        Ok(v) => match json::get(&v, "data") {
            Some(Json::Arr(a)) => a.clone(),
            _ => Vec::new(),
        },
        Err(_) => return r#"{"error":"hermes"}"#.to_string(),
    };
    let mut out = Vec::new();
    for m in rows.iter() {
        let role = json::text_of(json::get(m, "role"));
        if role != "user" && role != "assistant" {
            continue;
        }
        let t = content_text(json::get(m, "content"));
        if t.trim().is_empty() {
            continue;
        }
        out.push(format!(
            "{{\"role\":{},\"text\":{}}}",
            json::dump_str(&role),
            json::dump_str(&t)
        ));
    }
    let keep = out.len().saturating_sub(60);
    format!(
        "{{\"id\":{},\"messages\":[{}]}}",
        json::dump_str(id),
        out[keep..].join(",")
    )
}

/// Gateway `/api/sessions/{id}` body belongs to Galion One.
pub fn is_one_row(meta: &[u8]) -> bool {
    let text = String::from_utf8_lossy(meta);
    json::parse(text.trim())
        .map(|v| json::text_of(json::get(&v, "session_key")) == KEY)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: [u8; 3] = [0xa1, 0xb2, 0xc3];

    #[test]
    fn ids() {
        // 2026-10-30 09:42:00 UTC = 10:42 CET.
        assert_eq!(mint(1_793_353_320, R), "one-20261030-1042-a1b2c3");
        assert!(valid("one-20261030-1042-a1b2c3"));
        assert!(valid("one-admin") && valid("one-voice"));
        assert!(!valid("one-warm") && !valid("api-x") && !valid("one-") && !valid("one-a b"));
        assert!(!valid(&format!("one-{}", "x".repeat(70))));
    }

    #[test]
    fn idle_rule_and_reopen() {
        let now = 1_793_353_320;
        let cur = Some(("one-20261030-0900-000000", now - 60));
        // continues the current thread inside 30 min
        assert_eq!(choose("", false, cur, now, R), "one-20261030-0900-000000");
        // a gap of 30 min or more starts a new one
        let old = Some(("one-20261030-0900-000000", now - IDLE_SECS));
        assert_eq!(choose("", false, old, now, R), "one-20261030-1042-a1b2c3");
        // a named (reopened) thread is kept however old
        assert_eq!(choose("one-admin", false, old, now, R), "one-admin");
        // New chat always mints
        assert_eq!(
            choose("one-admin", true, cur, now, R),
            "one-20261030-1042-a1b2c3"
        );
        // junk ids are ignored
        assert_eq!(
            choose("../x", false, None, now, R),
            "one-20261030-1042-a1b2c3"
        );
    }

    #[test]
    fn state_round_trip() {
        let s = state_text("one-admin", 42);
        assert_eq!(parse_state(&s), Some(("one-admin".into(), 42)));
        assert_eq!(parse_state("{}"), None);
        assert_eq!(parse_state("{\"id\":\"api-1\",\"last\":1}"), None);
    }

    #[test]
    fn list_and_messages() {
        let s = br#"{"data":[{"id":"one-20261030-1042-a1b2c3","session_key":"galion-one","title":"Hi there","message_count":4,"started_at":10,"ended_at":20},
            {"id":"api-1","session_key":"","title":"x"},{"id":"one-warm","session_key":"one-warm"},
            {"id":"one-admin","session_key":"galion-one","title":"","message_count":2,"started_at":5}]}"#;
        let l = list_json(s, "one-admin");
        assert_eq!(
            l,
            r#"{"active":"one-admin","threads":[{"id":"one-20261030-1042-a1b2c3","title":"Hi there","count":4,"at":20},{"id":"one-admin","title":"one-admin","count":2,"at":5}]}"#
        );
        let m = br#"{"data":[{"role":"user","content":"a"},{"role":"tool","content":"t"},{"role":"assistant","content":[{"type":"text","text":"b"}]},{"role":"assistant","content":""}]}"#;
        assert_eq!(
            messages_json("one-admin", m),
            r#"{"id":"one-admin","messages":[{"role":"user","text":"a"},{"role":"assistant","text":"b"}]}"#
        );
        assert!(is_one_row(
            br#"{"id":"one-admin","session_key":"galion-one"}"#
        ));
        assert!(!is_one_row(
            br#"{"id":"api-1","session_key":"console:hermes:x"}"#
        ));
    }
}
