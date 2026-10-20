// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Page bodies. Each page is a pure function of the API data it was given,
//! so it can be tested without a network.

pub mod agents;
pub mod catalog;
pub mod gate;
pub mod not_ported;
pub mod services;
pub mod status;

use crate::json::Value;

/// One service row from `GET /api/v1/services` `.items[]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub heavy: bool,
    pub summary: String,
    pub repo: Option<String>,
    pub hosts: Vec<String>,
    pub port: Option<u64>,
    pub default_on: bool,
}

/// Parse the services catalog; rows without an id are skipped.
pub fn services_of(v: &Value) -> Vec<Service> {
    v.get("items")
        .as_array()
        .iter()
        .filter_map(|s| {
            let id = s.get("id").as_str().filter(|x| !x.is_empty())?.to_string();
            Some(Service {
                name: s.get("name").as_str().unwrap_or(&id).to_string(),
                kind: s.get("kind").as_str().unwrap_or("").to_string(),
                heavy: s.get("weight").as_str() == Some("heavy"),
                summary: s.get("summary").as_str().unwrap_or("").to_string(),
                repo: s.get("repo").as_str().map(str::to_string),
                hosts: s
                    .get("hosts")
                    .as_array()
                    .iter()
                    .filter_map(|h| h.as_str().map(str::to_string))
                    .collect(),
                port: s
                    .get("port")
                    .as_f64()
                    .filter(|p| *p >= 0.0)
                    .map(|p| p as u64),
                default_on: s.get("defaultOn").as_bool() == Some(true),
                id,
            })
        })
        .collect()
}

/// Catalog state, as `defaultServiceState()` in console-ide `lib/fleet.ts`:
/// heavy -> held, defaultOn -> online, else offline.
pub fn catalog_state(s: &Service) -> &'static str {
    if s.heavy {
        "held"
    } else if s.default_on {
        "online"
    } else {
        "offline"
    }
}

/// Desk pulse (`GET 127.0.0.1:8091/api/v1/awake` `.services[]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pulse {
    pub id: String,
    pub ok: bool,
}

pub fn pulses_of(v: &Value) -> Vec<Pulse> {
    v.get("services")
        .as_array()
        .iter()
        .filter_map(|r| {
            let id = r.get("id").as_str()?;
            if id.is_empty() || id == "undefined" {
                return None;
            }
            Some(Pulse {
                id: id.to_string(),
                ok: r.get("ok").as_bool() == Some(true),
            })
        })
        .collect()
}

/// `field()` / `hideUndefined()` from agents.tsx: drop the word `undefined`
/// (JS `\bundefined\b`), squeeze runs of spaces, trim.
pub fn tidy(s: &str) -> String {
    const W: &str = "undefined";
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(W) {
        let before = rest[..i]
            .chars()
            .next_back()
            .or_else(|| out.chars().next_back());
        let after = rest[i + W.len()..].chars().next();
        out.push_str(&rest[..i]);
        if word(before) || word(after) {
            out.push_str(W);
        }
        rest = &rest[i + W.len()..];
    }
    out.push_str(rest);
    let mut squeezed = String::with_capacity(out.len());
    for c in out.chars() {
        if c == ' ' && squeezed.ends_with(' ') {
            continue;
        }
        squeezed.push(c);
    }
    squeezed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidy_matches_js_word_boundary() {
        assert_eq!(tidy("undefined fine"), "fine");
        assert_eq!(tidy("{\"a\":\"undefined x\"}"), "{\"a\":\" x\"}");
        assert_eq!(
            tidy("undefinedness  and  _undefined"),
            "undefinedness and _undefined"
        );
        assert_eq!(tidy("ip50 undefined"), "ip50");
    }
}
