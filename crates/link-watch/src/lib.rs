// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Watch decision for link-watch.
//!
//! [`watch_decision`] turns probe facts the caller already holds into a report.
//! It does not spawn a process, open a socket, or talk to a service manager.
//! The binary probe loop dials. This function only describes what was seen.

#![forbid(unsafe_code)]

pub mod http;
pub mod urls;

use serde_json::{Map, Value};

/// Facts already collected by the caller. Nothing here is fetched.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchFacts {
    /// Legacy UTC stamp for the `at` key.
    pub at: String,
    /// Europe/Zagreb stamp for the `at_local` key.
    pub at_local: String,
    /// UTC clock `HH:MM:SS` used in an event line.
    pub stamp: String,
    /// Ollama `/api/ps` answered in the success range.
    pub laptop_port: bool,
    /// Model label, or empty when none was loaded.
    pub model: String,
    /// Milliseconds spent on the model probe.
    pub model_ms: i64,
    /// Hermes `/health` answered in the success range.
    pub hermes: bool,
    /// Milliseconds spent on the Hermes probe.
    pub hermes_ms: i64,
    /// France probe value: a bool, or the string `unconfigured`.
    pub france: Value,
    /// Laptop card object. An empty object when the file was absent.
    pub laptop: Value,
}

/// Identity of one sample. A change appends one event line. Equal samples do not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchSignature {
    /// Same as [`WatchFacts::laptop_port`].
    pub laptop_port: bool,
    /// Same as [`WatchFacts::model`].
    pub model: String,
    /// Same as [`WatchFacts::hermes`].
    pub hermes: bool,
    /// `true` only when `france` is the JSON bool `true`.
    pub france: bool,
    /// Text of `laptop.name`, using the legacy JSON text rules.
    pub card_name: String,
}

/// A report and the event list that belongs with it. Nothing here is executed.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchDecision {
    /// `link.json` object. Keys are the historical set, in that order.
    pub report: Value,
    /// Event lines after this sample. At most 12. Oldest dropped first.
    pub events: Vec<Value>,
    /// Pass this back as `previous` on the next call.
    pub signature: WatchSignature,
    /// `true` when `events` grew because the signature changed.
    pub events_changed: bool,
}

/// Build the report for one sample.
///
/// `previous` is the signature from the last call, or `None` on the first
/// sample. The first sample never appends an event. A later sample appends one
/// line only when its signature differs, then keeps the last 12 lines.
///
/// The returned value is a report. It does not spawn a process or open a socket.
#[must_use]
pub fn watch_decision(
    facts: &WatchFacts,
    previous: Option<&WatchSignature>,
    events: &[Value],
) -> WatchDecision {
    let signature = WatchSignature {
        laptop_port: facts.laptop_port,
        model: facts.model.clone(),
        hermes: facts.hermes,
        france: facts.france.as_bool().unwrap_or(false),
        card_name: match facts.laptop.get("name") {
            Some(value) => json_text(value),
            None => String::new(),
        },
    };
    let mut events = events.to_vec();
    let events_changed = match previous {
        Some(prior) if prior != &signature => {
            let bit = if facts.laptop_port { "up" } else { "down" };
            let model_display = if facts.model.is_empty() {
                "none"
            } else {
                facts.model.as_str()
            };
            let hermes_word = if facts.hermes { "up" } else { "down" };
            let stamp = facts.stamp.as_str();
            events.push(Value::String(format!(
                "{stamp}  laptop {bit}  model {model_display}  hermes {hermes_word}"
            )));
            let len = events.len();
            if len > 12 {
                events.drain(0..len - 12);
            }
            true
        }
        _ => false,
    };
    let mut row = Map::new();
    row.insert("at".to_owned(), Value::String(facts.at.clone()));
    row.insert("at_local".to_owned(), Value::String(facts.at_local.clone()));
    row.insert("laptop_port".to_owned(), Value::Bool(facts.laptop_port));
    row.insert("model".to_owned(), Value::String(facts.model.clone()));
    row.insert("model_ms".to_owned(), Value::Number(facts.model_ms.into()));
    row.insert("hermes".to_owned(), Value::Bool(facts.hermes));
    row.insert(
        "hermes_ms".to_owned(),
        Value::Number(facts.hermes_ms.into()),
    );
    row.insert("france".to_owned(), facts.france.clone());
    row.insert("laptop".to_owned(), facts.laptop.clone());
    row.insert("events".to_owned(), Value::Array(events.clone()));
    WatchDecision {
        report: Value::Object(row),
        events,
        signature,
        events_changed,
    }
}

/// CPython `str` of a JSON value, matching `jsonout::json_str`.
fn json_text(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(_) => "[]".to_owned(),
        Value::Object(_) => "{}".to_owned(),
    }
}
