// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The typed contract between `gaia-core` and `openmind-queue`.
//!
//! `openmind-queue` cannot depend on `gaia-core` (cycle), so it publishes its
//! Gaia vocabulary as constants (`openmind_queue::gaia::{FUNCTION, SYSTEMS,
//! VERBS, DISPATCH_VERBS, OUTCOMES}`). Before this module the adapter matched
//! those strings by hand, so a renamed system or a new outcome on either
//! side would have been mapped to `rejected` without anyone noticing
//! ("vocabulary-only coupling", docs/poseidon.md). Now:
//!
//! * every `gaia_core` value that crosses the wire goes through one
//!   exhaustive function here ([`wire_system`], [`wire_verb`],
//!   [`outcome_from_wire`]) whose tests pin it to the queue's constants;
//! * every answer from the queue is parsed by [`parse_dispatch`] /
//!   [`check_identity`], which refuse anything off-contract (wrong function,
//!   echoed system or verb that differs from what was sent, unknown outcome,
//!   `dry_run` disagreeing with the outcome, non-integer metrics) as a
//!   [`Violation`] instead of guessing.
//!
//! The in-process test at the bottom drives the queue's real
//! `gaia::dispatch` for every system x verb and parses each answer through
//! [`parse_dispatch`]: the same code path the live adapter uses over HTTP.

use gaia_core::{Directive, FunctionId, Outcome, Report, System, Verb};
use openmind_queue::gaia as wire;
use serde_json::Value;
use std::fmt;

/// `service` that `GET /healthz` must name.
pub const SERVICE: &str = "openmind-queue";

/// An answer that does not follow the published contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation(pub String);

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "contract violation: {}", self.0)
    }
}

fn bad(s: impl Into<String>) -> Violation {
    Violation(s.into())
}

/// The queue's spelling of a compute system, or `None` for a system the
/// queue does not serve. The string returned is the queue's own constant.
pub fn wire_system(s: System) -> Option<&'static str> {
    if !s.is_compute() {
        return None;
    }
    wire::SYSTEMS
        .iter()
        .map(|(n, _)| *n)
        .find(|n| *n == s.name())
}

/// The queue's spelling of a verb (exhaustive: a new `Verb` fails to build
/// here until it is mapped).
pub fn wire_verb(v: Verb) -> &'static str {
    let i = match v {
        Verb::Status => 0,
        Verb::Run => 1,
        Verb::Plan => 2,
        Verb::Execute => 3,
    };
    wire::VERBS[i]
}

/// Whether the queue's Gaia dispatch answers this verb (anything else must
/// go through `POST /jobs` or stays an operator action).
pub fn dispatchable(v: Verb) -> bool {
    wire::DISPATCH_VERBS.contains(&wire_verb(v))
}

/// A queue outcome string to a `gaia_core::Outcome`. `contained` is GAIA's
/// own verdict and is never accepted from the queue.
pub fn outcome_from_wire(s: &str) -> Option<Outcome> {
    if !wire::OUTCOMES.contains(&s) {
        return None;
    }
    [
        Outcome::Done,
        Outcome::DryRun,
        Outcome::Refused,
        Outcome::Rejected,
    ]
    .into_iter()
    .find(|o| o.name() == s)
}

fn int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().map(|u| i64::try_from(u).unwrap_or(i64::MAX)))
}

/// Check `GET /healthz`: the answer must come from `openmind-queue` serving
/// POSEIDON. Returns the queue's version.
pub fn check_identity(code: u16, v: &Value) -> Result<String, Violation> {
    if code != 200 {
        return Err(bad(format!("GET /healthz answered {code}")));
    }
    let service = v.get("service").and_then(Value::as_str).unwrap_or("");
    if service != SERVICE {
        return Err(bad(format!(
            "GET /healthz names service {service:?}, expected {SERVICE:?} (another program on this port?)"
        )));
    }
    let function = v.get("function").and_then(Value::as_str).unwrap_or("");
    if function != wire::FUNCTION || FunctionId::parse(function) != Some(FunctionId::COMPUTE) {
        return Err(bad(format!(
            "openmind-queue serves function {function:?}; Gaia routes compute to {}",
            FunctionId::COMPUTE
        )));
    }
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(bad("GET /healthz did not say ok"));
    }
    Ok(v.get("version")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string())
}

/// Parse one `POST /gaia/dispatch` answer (HTTP 200) for directive `d`,
/// which was sent with `sent_verb`. Strict: see the module docs.
pub fn parse_dispatch(d: &Directive, sent_verb: Verb, v: &Value) -> Result<Report, Violation> {
    let o = v
        .as_object()
        .ok_or_else(|| bad("dispatch answer is not a JSON object"))?;
    let s = |k: &str| o.get(k).and_then(Value::as_str);
    if s("function") != Some(wire::FUNCTION) {
        return Err(bad(format!(
            "dispatch answer from function {:?}, expected {:?}",
            s("function"),
            wire::FUNCTION
        )));
    }
    let want_system = wire_system(d.target)
        .ok_or_else(|| bad(format!("{} is not served by openmind-queue", d.target)))?;
    if s("system") != Some(want_system) {
        return Err(bad(format!(
            "dispatch answered for system {:?}, asked {want_system:?}",
            s("system")
        )));
    }
    let want_verb = wire_verb(sent_verb);
    if s("verb") != Some(want_verb) {
        return Err(bad(format!(
            "dispatch answered verb {:?}, sent {want_verb:?}",
            s("verb")
        )));
    }
    let raw = s("outcome").unwrap_or("<missing>");
    let outcome =
        outcome_from_wire(raw).ok_or_else(|| bad(format!("unknown outcome {raw:?}")))?;
    let dry_run = match o.get("dry_run") {
        Some(Value::Bool(b)) => *b,
        None => outcome == Outcome::DryRun,
        Some(other) => return Err(bad(format!("dry_run is {other}, not a bool"))),
    };
    if dry_run != (outcome == Outcome::DryRun) {
        return Err(bad(format!("dry_run={dry_run} with outcome {raw:?}")));
    }
    if !dispatchable(sent_verb) && matches!(outcome, Outcome::Done | Outcome::DryRun) {
        return Err(bad(format!(
            "queue claims {raw:?} for read-only-forbidden verb {want_verb:?}"
        )));
    }
    let summary = match o.get("summary") {
        Some(Value::String(t)) => t.clone(),
        None | Some(Value::Null) => String::new(),
        Some(other) => return Err(bad(format!("summary is {other}, not a string"))),
    };
    let mut r = Report::for_directive(d, outcome).summary(summary);
    r.dry_run = dry_run;
    match o.get("metrics") {
        None | Some(Value::Null) => {}
        Some(Value::Array(ms)) => {
            for m in ms {
                match (m.get(0).and_then(Value::as_str), m.get(1).and_then(int)) {
                    (Some(k), Some(n)) if m.as_array().map(Vec::len) == Some(2) => {
                        r = r.metric(k, n)
                    }
                    _ => return Err(bad(format!("metric {m} is not [name, integer]"))),
                }
            }
        }
        Some(other) => return Err(bad(format!("metrics is {other}, not a list"))),
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmind_queue::engine::{Config, Engine, JobSpec, Record};
    use openmind_queue::vram::{Device, DeviceKind, DeviceState};
    use serde_json::json;

    const VERBS: [Verb; 4] = [Verb::Status, Verb::Run, Verb::Plan, Verb::Execute];

    #[test]
    fn systems_match_both_ways() {
        for s in System::COMPUTE {
            assert_eq!(wire_system(s), Some(s.name()), "{s}");
            assert_eq!(s.function(), FunctionId::COMPUTE);
        }
        assert_eq!(wire::SYSTEMS.len(), System::COMPUTE.len());
        for (n, _) in wire::SYSTEMS {
            let s = System::parse(n).unwrap_or_else(|| panic!("queue system {n} unknown to gaia-core"));
            assert!(s.is_compute(), "{n}");
        }
        assert_eq!(wire_system(System::parse("terminal").unwrap()), None);
        assert_eq!(wire::FUNCTION, FunctionId::COMPUTE.name());
    }

    #[test]
    fn verbs_and_outcomes_match() {
        assert_eq!(wire::VERBS.len(), VERBS.len());
        for v in VERBS {
            assert_eq!(wire_verb(v), v.name());
            assert_eq!(Verb::parse(wire_verb(v)), Some(v));
        }
        assert_eq!(
            VERBS.iter().filter(|v| dispatchable(**v)).count(),
            wire::DISPATCH_VERBS.len()
        );
        assert!(dispatchable(Verb::Status) && dispatchable(Verb::Plan));
        assert!(!dispatchable(Verb::Run) && !dispatchable(Verb::Execute));
        for o in wire::OUTCOMES {
            assert_eq!(outcome_from_wire(o).map(Outcome::name), Some(o));
        }
        assert_eq!(outcome_from_wire("contained"), None);
        assert_eq!(outcome_from_wire("ok"), None);
    }

    fn engine() -> Engine {
        let mut e = Engine::new(Config::default());
        e.apply(&Record::Device(Device {
            id: "gpu0".into(),
            kind: DeviceKind::Gpu,
            vram_gb: 24,
            provider: String::new(),
            cents_per_hour: 0,
            state: DeviceState::Approved,
            healthy: true,
        }));
        let r = e.enqueue(JobSpec::default(), 0).unwrap();
        e.apply(&r);
        e
    }

    /// The queue's real dispatch, serialised exactly like the server does,
    /// parsed by the adapter's parser: no violation for any system x verb.
    #[test]
    fn real_queue_dispatch_round_trips_for_every_system_and_verb() {
        let e = engine();
        let mut n = 0;
        for s in System::COMPUTE {
            for v in VERBS {
                for p in ["", "16", "48", "x", "revolut transfer"] {
                    let rep = wire::dispatch(&e, wire_system(s).unwrap(), wire_verb(v), p);
                    let js = serde_json::to_value(&rep).unwrap();
                    let d = Directive::new(1, s, v).payload(p);
                    let r = parse_dispatch(&d, v, &js)
                        .unwrap_or_else(|e| panic!("{s} {v:?} {p:?}: {e}: {js}"));
                    assert_eq!(r.outcome.name(), rep.outcome);
                    assert_eq!(r.dry_run, rep.dry_run);
                    assert_eq!(r.system, Some(s));
                    assert_eq!(r.metrics, rep.metrics);
                    n += 1;
                }
            }
        }
        assert_eq!(n, 4 * 4 * 5);
    }

    fn ok() -> Value {
        json!({"function":"POSEIDON","system":"queues","verb":"status","outcome":"done",
               "dry_run":false,"summary":"1 ready","metrics":[["ready",1]]})
    }

    #[test]
    fn off_contract_answers_are_violations() {
        let d = Directive::new(1, System::Queues, Verb::Status);
        assert!(parse_dispatch(&d, Verb::Status, &ok()).is_ok());
        let cases: Vec<(&str, Value)> = vec![
            ("function", json!("HEPHAESTUS")),
            ("system", json!("vram-pool")),
            ("verb", json!("plan")),
            ("outcome", json!("contained")),
            ("outcome", json!("ok")),
            ("dry_run", json!(true)),
            ("dry_run", json!("no")),
            ("summary", json!(5)),
            ("metrics", json!([["ready", "1"]])),
            ("metrics", json!([["ready", 1, 2]])),
            ("metrics", json!({"ready": 1})),
        ];
        for (k, val) in cases {
            let mut v = ok();
            v[k] = val.clone();
            let e = parse_dispatch(&d, Verb::Status, &v).expect_err(&format!("{k}={val}"));
            assert!(e.to_string().starts_with("contract violation"), "{e}");
        }
        assert!(parse_dispatch(&d, Verb::Status, &json!([1])).is_err());
        // A verb the queue may not answer positively.
        let mut v = ok();
        v["verb"] = json!("run");
        let run = Directive::new(1, System::Queues, Verb::Run);
        assert!(parse_dispatch(&run, Verb::Run, &v).is_err());
        v["outcome"] = json!("refused");
        assert!(parse_dispatch(&run, Verb::Run, &v).is_ok());
    }

    #[test]
    fn identity_must_be_openmind_queue_poseidon() {
        let good = json!({"ok":true,"function":"POSEIDON","service":"openmind-queue","version":"0.1.0"});
        assert_eq!(check_identity(200, &good).unwrap(), "0.1.0");
        assert!(check_identity(404, &good).is_err());
        for (k, val) in [
            ("service", json!("galion-one")),
            ("function", json!("HEPHAESTUS")),
            ("ok", json!(false)),
        ] {
            let mut v = good.clone();
            v[k] = val;
            assert!(check_identity(200, &v).is_err(), "{k}");
        }
        assert!(check_identity(200, &Value::Null).is_err());
    }
}
