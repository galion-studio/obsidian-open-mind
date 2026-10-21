// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! How this crate maps onto Gaia's POSEIDON function.
//!
//! The Gaia design (ws/gaia `gaia/docs/gaia-design.md` section 4.2) gives
//! POSEIDON four systems: `vram-pool`, `quotas`, `queues`, `budget-caps`.
//! This crate does not depend on `gaia-core` (that would be a cycle:
//! `gaia-poseidon` depends on both). It publishes the wire contract instead,
//! as constants: [`FUNCTION`], [`SYSTEMS`], [`VERBS`], [`DISPATCH_VERBS`] and
//! [`OUTCOMES`], and returns a [`GaiaReport`] with the same fields as
//! `gaia_core::Report` (summary, integer metrics, dry-run flag). The real
//! consumer is `gaia_poseidon::contract`: its tests check every constant here
//! against the `gaia-core` enums at compile and test time and parse every
//! system x verb answer of [`dispatch`] through the same strict parser the
//! live adapter uses, so a drift on either side fails the build instead of
//! silently mis-mapping an outcome.
//!
//! Gaia dispatch never mutates the queue. Work is submitted through the
//! queue API; caps and quotas are set by the operator. Gaia gets status and
//! plans. Anything that smells of real money is refused.

use crate::engine::{Engine, MONEY_WORDS};
use serde::Serialize;

/// The Gaia function that owns this crate.
pub const FUNCTION: &str = "POSEIDON";

/// This crate's row for a Gaia ownership registry.
pub const REGISTRY_ROW: (&str, &str, &str) = (
    "openmind-queue",
    FUNCTION,
    "Durable job queue, quotas, hard-stop spend caps (numbers only), VRAM pool routing GPU -> LAN -> CPU. Never spends.",
);

/// The Gaia verbs (`gaia_core::Verb::name` spellings).
pub const VERBS: [&str; 4] = ["status", "run", "plan", "execute"];

/// The verbs [`dispatch`] can answer with `done` or `dry-run`. Every other
/// verb is refused here (Gaia submits work through `POST /jobs`).
pub const DISPATCH_VERBS: [&str; 2] = ["status", "plan"];

/// Outcomes a [`GaiaReport`] may carry (`gaia_core::Outcome::name`
/// spellings; `contained` is GAIA's own and never comes from here).
pub const OUTCOMES: [&str; 4] = ["done", "dry-run", "refused", "rejected"];

/// The four POSEIDON systems and what this crate implements for each.
pub const SYSTEMS: [(&str, &str); 4] = [
    (
        "vram-pool",
        "device registry, GPU -> LAN -> remote -> CPU placement, goal >= 1500 GB (model only)",
    ),
    ("quotas", "per-agent max_pending and max_inflight"),
    (
        "queues",
        "durable priority queues, leases, retries with backoff, dead letters, backpressure",
    ),
    (
        "budget-caps",
        "per-agent and global spend caps that hard-stop; numbers only, no money access",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GaiaReport {
    pub function: &'static str,
    pub system: String,
    pub verb: String,
    /// `done`, `dry-run`, `refused` or `rejected` (gaia-core spellings).
    pub outcome: &'static str,
    pub dry_run: bool,
    pub summary: String,
    pub metrics: Vec<(String, i64)>,
}

/// Load 0..=100 as Gaia's `Health::ok(load)` expects: the worse of queue
/// fill and VRAM fill.
pub fn load(e: &Engine) -> u8 {
    let (r, d, l, _) = e.depth();
    let live = (r + d + l) as u64;
    let q = (live * 100 / (e.cfg.max_depth.max(1) as u64)).min(100);
    let s = e.vram.summary();
    let v = (s.used_gb * 100)
        .checked_div(s.approved_gb)
        .map_or(0, |x| x.min(100));
    q.max(v) as u8
}

fn i(x: impl TryInto<i64>) -> i64 {
    x.try_into().unwrap_or(i64::MAX)
}

/// Answer one Gaia directive. Read-only.
pub fn dispatch(e: &Engine, system: &str, verb: &str, payload: &str) -> GaiaReport {
    let mut rep = GaiaReport {
        function: FUNCTION,
        system: system.to_string(),
        verb: verb.to_string(),
        outcome: "done",
        dry_run: false,
        summary: String::new(),
        metrics: Vec::new(),
    };
    let p = payload.to_ascii_lowercase();
    if MONEY_WORDS.iter().any(|w| p.contains(w)) {
        rep.outcome = "refused";
        rep.summary = "real-money access is not approved; POSEIDON tracks caps only".into();
        return rep;
    }
    if !SYSTEMS.iter().any(|(s, _)| *s == system) {
        rep.outcome = "rejected";
        rep.summary = format!("{system} is not a POSEIDON system");
        return rep;
    }
    if !VERBS.contains(&verb) {
        rep.outcome = "rejected";
        rep.summary = format!("unknown verb {verb}");
        return rep;
    }
    let (ready, delayed, leased, dead) = e.depth();
    match (system, verb) {
        ("queues", "status") => {
            rep.summary = format!(
                "{ready} ready, {delayed} delayed, {leased} leased, {dead} dead (max_depth {})",
                e.cfg.max_depth
            );
            rep.metrics = vec![
                ("ready".into(), i(ready)),
                ("delayed".into(), i(delayed)),
                ("leased".into(), i(leased)),
                ("dead".into(), i(dead)),
                ("enqueued".into(), i(e.stats.enqueued)),
                ("acked".into(), i(e.stats.acked)),
                ("backpressure".into(), i(e.stats.backpressure_refusals)),
            ];
        }
        ("quotas", "status") => {
            let (p, f) = e.quota_of("*");
            rep.summary = format!(
                "{} agent(s); default max_pending {p}, max_inflight {f}",
                e.agents().len()
            );
            rep.metrics = vec![
                ("agents".into(), i(e.agents().len())),
                ("quota_refusals".into(), i(e.stats.quota_refusals)),
            ];
        }
        ("budget-caps", "status") => {
            let g = e.global();
            rep.summary = format!(
                "global spent {} reserved {} cap {}; caps only, no money moves",
                g.spent,
                g.reserved,
                g.cap.map_or("none".to_string(), |c| c.to_string())
            );
            rep.metrics = vec![
                ("global_spent".into(), i(g.spent)),
                ("global_reserved".into(), i(g.reserved)),
                ("budget_refusals".into(), i(e.stats.budget_refusals)),
            ];
            for (name, a) in e.agents() {
                if let Some(c) = a.budget.cap {
                    rep.metrics.push((format!("cap:{name}"), i(c)));
                    rep.metrics
                        .push((format!("spent:{name}"), i(a.budget.spent)));
                }
            }
        }
        ("vram-pool", "status") => {
            let s = e.vram.summary();
            rep.summary = format!(
                "{} GB approved ({} routable devices), {} GB used, {} GB planned; goal {} GB, gap {} GB",
                s.approved_gb, s.routable_devices, s.used_gb, s.planned_gb, s.goal_gb, s.gap_gb
            );
            rep.metrics = vec![
                ("approved_gb".into(), i(s.approved_gb)),
                ("used_gb".into(), i(s.used_gb)),
                ("free_gb".into(), i(s.free_gb)),
                ("planned_gb".into(), i(s.planned_gb)),
                ("goal_gb".into(), i(s.goal_gb)),
                ("gap_gb".into(), i(s.gap_gb)),
                ("planned_cents_per_hour".into(), i(s.planned_cents_per_hour)),
            ];
        }
        ("vram-pool", "plan") => {
            rep.dry_run = true;
            rep.outcome = "dry-run";
            let gb: u32 = payload.trim().parse().unwrap_or(0);
            if gb == 0 {
                // Nothing was planned: not a dry run (kept consistent with
                // `outcome` so a strict reader never sees rejected + dry_run).
                rep.dry_run = false;
                rep.outcome = "rejected";
                rep.summary = "payload must be the GB needed, e.g. 40".into();
            } else {
                match e.vram.place(gb) {
                    Some(d) => {
                        rep.summary = format!("{gb} GB would go to device {d} (no spend)");
                        rep.metrics = vec![("free_gb_on_device".into(), i(e.vram.free(&d)))];
                    }
                    None => {
                        rep.summary = format!(
                            "{gb} GB fits no approved device now: offload to CPU or wait; more VRAM needs owner spend approval"
                        );
                    }
                }
            }
        }
        (_, "plan") => {
            rep.dry_run = true;
            rep.outcome = "dry-run";
            rep.summary = format!("{system}: no plan needed; use status");
        }
        _ => {
            rep.outcome = "refused";
            rep.summary = format!(
                "{system} {verb}: Gaia dispatch is read-only here; submit work via POST /jobs, caps and quotas are set by the operator"
            );
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Config, JobSpec, Record};
    use crate::vram::{Device, DeviceKind, DeviceState};

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

    #[test]
    fn four_systems_status_done() {
        let e = engine();
        for (s, _) in SYSTEMS {
            let r = dispatch(&e, s, "status", "");
            assert_eq!(r.outcome, "done", "{s}: {}", r.summary);
            assert_eq!(r.function, "POSEIDON");
        }
        let q = dispatch(&e, "queues", "status", "");
        assert!(q.metrics.contains(&("ready".into(), 1)));
    }

    #[test]
    fn plan_is_dry_run_and_never_mutates() {
        let e = engine();
        let r = dispatch(&e, "vram-pool", "plan", "16");
        assert_eq!(r.outcome, "dry-run");
        assert!(r.summary.contains("gpu0"));
        let r = dispatch(&e, "vram-pool", "plan", "48");
        assert!(r.summary.contains("fits no approved device"));
        assert_eq!(e.vram.used("gpu0"), 0);
    }

    #[test]
    fn every_answer_stays_inside_the_published_contract() {
        let e = engine();
        for (s, _) in SYSTEMS {
            for v in VERBS {
                for p in ["", "16", "48", "x"] {
                    let r = dispatch(&e, s, v, p);
                    assert!(OUTCOMES.contains(&r.outcome), "{s} {v}: {}", r.outcome);
                    assert_eq!(r.dry_run, r.outcome == "dry-run", "{s} {v} {p}");
                    assert_eq!((r.function, r.system.as_str(), r.verb.as_str()), (FUNCTION, s, v));
                    if !DISPATCH_VERBS.contains(&v) {
                        assert_eq!(r.outcome, "refused", "{s} {v} must stay read-only");
                    }
                }
            }
        }
    }

    #[test]
    fn money_unknown_and_writes_refused() {
        let e = engine();
        assert_eq!(
            dispatch(&e, "budget-caps", "run", "revolut top-up").outcome,
            "refused"
        );
        assert_eq!(dispatch(&e, "terminal", "status", "").outcome, "rejected");
        assert_eq!(dispatch(&e, "queues", "frob", "").outcome, "rejected");
        assert_eq!(
            dispatch(&e, "budget-caps", "execute", "raise").outcome,
            "refused"
        );
        assert_eq!(load(&e), 0);
    }
}
