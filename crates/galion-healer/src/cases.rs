// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Port of galion-light `guardian/desk_heal.py` `cycle()`/`follow()`.
//! A clear check writes nothing to the nerve. A new fault signature
//! (`name:detail`) opens one case: written to desk.jsonl and inbox/<id>.json,
//! one `heal sent` nerve line. After `follow_s` (or as soon as the probe is
//! clear) a follow-up check writes `heal held|returned`.
//!
//! The OpenRouter Flash call is not ported: it needs an API key read from a
//! secrets file, which this port never does. `how` says so.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const NOT_SENT_GATEWAY: &str = "not sent. the gateway itself is down, so no model was called";
pub const NOT_SENT_PORT: &str = "not sent. model call not ported (no secrets read)";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Case {
    pub id: String,
    pub at: String,
    pub service: String,
    pub why: String,
    pub raw: String,
    pub sig: String,
    pub asked: bool,
    pub followed: bool,
    pub follow_at_unix: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check: Option<String>,
}

pub type Open = BTreeMap<String, Case>;

#[derive(Debug, Default, PartialEq)]
pub struct CycleOut {
    /// Rows appended to desk.jsonl (and written to inbox/<id>.json).
    pub desk: Vec<Case>,
    pub nerve: Vec<String>,
    pub down: Vec<String>,
    pub api_calls: u32,
}

/// Case id like desk_heal.py: compact stamp + first 4 chars of the service.
pub fn case_id(stamp: &str, service: &str) -> String {
    let c: String = stamp
        .split('+')
        .next()
        .unwrap_or(stamp)
        .chars()
        .filter(|c| *c != '-' && *c != ':')
        .collect();
    format!("{c}{}", service.chars().take(4).collect::<String>())
}

fn cut(s: &str, n: usize) -> &str {
    openmind_survival::truncate(s, n)
}

fn follow(case: &mut Case, ok: bool, detail: &str, stamp: &str) -> String {
    let held = if ok { "held" } else { "returned" };
    let how = case.how.clone().unwrap_or_default();
    let line = format!(
        "heal {held} service={} what={} when={stamp} why={} how={} check={detail}",
        case.service,
        case.why,
        cut(&case.raw, 120),
        cut(&how, 180)
    );
    case.follow = Some(held.into());
    case.follow_at = Some(stamp.into());
    case.check = Some(detail.into());
    line
}

/// One cycle. `results` is (name, ok, detail) for every probe in order;
/// `recheck(name)` re-probes one service for the follow-up.
pub fn cycle(
    open: &mut Open,
    results: &[(String, bool, String)],
    recheck: &mut dyn FnMut(&str) -> (bool, String),
    now_unix: f64,
    stamp: &str,
    follow_s: u64,
) -> CycleOut {
    let mut out = CycleOut::default();
    for (name, ok, detail) in results {
        if *ok {
            continue;
        }
        out.down.push(name.clone());
        let sig = format!("{name}:{detail}");
        if let Some(cur) = open.get_mut(name) {
            if cur.sig == sig && cur.asked {
                if now_unix >= cur.follow_at_unix && !cur.followed {
                    let (ok2, d2) = recheck(name);
                    out.nerve.push(follow(cur, ok2, &d2, stamp));
                    cur.followed = true;
                    out.desk.push(cur.clone());
                }
                continue;
            }
        }
        let mut case = Case {
            id: case_id(stamp, name),
            at: stamp.into(),
            service: name.clone(),
            why: detail.clone(),
            raw: format!("{name} probe failed: {detail}"),
            sig,
            asked: true,
            followed: false,
            follow_at_unix: now_unix + follow_s as f64,
            ..Case::default()
        };
        out.desk.push(case.clone());
        let how = if name == "hermes" {
            NOT_SENT_GATEWAY
        } else {
            NOT_SENT_PORT
        };
        case.how = Some(how.into());
        out.nerve.push(format!(
            "heal sent service={name} what={detail} when={} why={} how={}",
            case.at,
            case.raw,
            cut(how, 180)
        ));
        open.insert(name.clone(), case);
    }
    let names: Vec<String> = open.keys().cloned().collect();
    for name in names {
        if out.down.contains(&name) {
            continue;
        }
        let c = open.get_mut(&name).expect("key exists");
        if c.asked && !c.followed {
            let (ok2, d2) = recheck(&name);
            out.nerve.push(follow(c, ok2, &d2, stamp));
            c.followed = true;
            out.desk.push(c.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, ok: bool, d: &str) -> (String, bool, String) {
        (name.into(), ok, d.into())
    }

    #[test]
    fn lifecycle() {
        let mut open = Open::new();
        let mut ok_now = |_: &str| (true, "200".to_string());
        // clear: nothing
        let o = cycle(
            &mut open,
            &[r("one", true, "200")],
            &mut ok_now,
            0.0,
            "2026-10-18T21:00:00+02:00",
            60,
        );
        assert!(o.nerve.is_empty() && o.desk.is_empty() && o.down.is_empty());
        // fault: one case, one nerve line
        let o = cycle(
            &mut open,
            &[r("one", false, "http 500")],
            &mut ok_now,
            100.0,
            "2026-10-18T21:00:30+02:00",
            60,
        );
        assert_eq!(o.down, vec!["one"]);
        assert_eq!(o.desk.len(), 1);
        assert_eq!(o.desk[0].id, "20261018T210030one");
        assert!(o.nerve[0]
            .starts_with("heal sent service=one what=http 500 when=2026-10-18T21:00:30+02:00"));
        assert!(o.nerve[0].contains(NOT_SENT_PORT));
        // same fault before follow time: silent
        let o = cycle(
            &mut open,
            &[r("one", false, "http 500")],
            &mut ok_now,
            130.0,
            "s",
            60,
        );
        assert!(o.nerve.is_empty());
        // same fault after follow time: follow-up runs (recheck says still bad)
        let mut bad = |_: &str| (false, "http 500".to_string());
        let o = cycle(
            &mut open,
            &[r("one", false, "http 500")],
            &mut bad,
            161.0,
            "t",
            60,
        );
        assert!(o.nerve[0].starts_with("heal returned service=one"));
        assert!(open["one"].followed);
        // still down, already followed: silent
        let o = cycle(
            &mut open,
            &[r("one", false, "http 500")],
            &mut bad,
            300.0,
            "u",
            60,
        );
        assert!(o.nerve.is_empty());
        // new signature: new case
        let o = cycle(
            &mut open,
            &[r("one", false, "http 502")],
            &mut bad,
            301.0,
            "2026-10-18T21:10:00+02:00",
            60,
        );
        assert_eq!(o.desk.len(), 1);
        // clears before follow time: immediate held follow-up
        let o = cycle(
            &mut open,
            &[r("one", true, "200")],
            &mut ok_now,
            302.0,
            "v",
            60,
        );
        assert!(o.nerve[0].starts_with("heal held service=one what=http 502"));
        assert_eq!(o.desk[0].follow.as_deref(), Some("held"));
    }

    #[test]
    fn gateway_not_sent() {
        let mut open = Open::new();
        let mut f = |_: &str| (false, "x".to_string());
        let o = cycle(
            &mut open,
            &[r("hermes", false, "connect: refused")],
            &mut f,
            0.0,
            "s",
            60,
        );
        assert!(o.nerve[0].contains(NOT_SENT_GATEWAY));
        assert_eq!(o.api_calls, 0);
    }
}
