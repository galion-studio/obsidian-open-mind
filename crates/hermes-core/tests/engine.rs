// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Engine end to end on a small two-sector registry: intake, governor,
//! request-limit retries to completion, HADES holds, ledger replay and crash
//! recovery. Real ledger files, real threads; only the model is the mock.

use hermes_core::engine::{retry_table, Hermes, RunOpts, TaskState};
use hermes_core::gate::Gate;
use hermes_core::governor::Governor;
use hermes_core::ledger::{self, Ledger, NewEvent};
use hermes_core::plan;
use hermes_core::provider::MockProvider;
use hermes_core::registry::Registry;
use hermes_core::retry::RetryPolicy;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const REG: &str = r#"
[hermes]
boss_agent = "hermes"
boss_model = "opus"
sub_model = "sonnet"
provider = "mock"
concurrency_cap = 8
reserved_slots = 1

[retry]
max_attempts = 20
backoff_base_secs = 30
backoff_factor = 2
backoff_max_secs = 600
max_error_attempts = 2

[[sector]]
id = "alpha"
name = "Alpha"
plan = "alpha.md"
lead = "alpha.lead"
max_concurrent = 4
gaia_functions = ["MINERVA"]
gaia_systems = ["repo-check"]

[[sector]]
id = "beta"
name = "Beta"
plan = "beta.md"
lead = "beta.lead"
max_concurrent = 3
gaia_functions = ["APOLLO"]
gaia_systems = ["teacher-docs"]
"#;

const ALPHA: &str = "\
### S1: Build
| Task | Target | Useful | Units | Inputs / deps | Success |
|---|---|---|---|---|---|
| S1.1 crate A | 2 | 2 | a | none | ok |
| S1.2 crate B | 2 | 2 | b | none | ok |
| S1.3 crate C after A | 1 | 1 | c | S1.1 | ok |
| S1.4 crate D | 3 | 3 | d | none | ok |
### S2: Land
| Task | Target | Useful | Units | Inputs / deps | Success |
|---|---|---|---|---|---|
| S2.1 Land alpha on main after the owner merge | 1 | 1 | - | S1 | ok |
| S2.2 docs after landing | 1 | 1 | - | S2.1 | ok |
";

const BETA: &str = "\
### S1. Write
| task | target | useful | units | inputs / deps | success |
|---|---|---|---|---|---|
| T1.1 chapter one | 2 | 2 | - | - | ok |
| T1.2 chapter two | 2 | 1 | - | T1.1 | ok |
| T1.3 chapter three | 4 | 3 | - | - | ok |
";

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-core-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn hermes() -> Hermes {
    let reg = Registry::parse(REG).unwrap();
    Hermes::from_plans(
        reg,
        vec![plan::parse("alpha", ALPHA), plan::parse("beta", BETA)],
    )
}

fn gov(h: &Hermes, cap: usize) -> Governor {
    let caps = h
        .reg
        .sectors
        .iter()
        .map(|s| (s.id.clone(), s.max_concurrent))
        .collect();
    Governor::new(cap, caps)
}

fn opts() -> RunOpts {
    RunOpts {
        first_sessions: None,
        time_scale: 0.0005,
        max_wall: Duration::from_secs(60),
    }
}

#[test]
fn request_limit_stops_are_retried_until_done_and_merge_is_refused() {
    let dir = tmp("retry");
    let path = dir.join("ledger.jsonl");
    let mut h = hermes();
    let (mut l, _) = Ledger::open(&path).unwrap();
    let g = gov(&h, 7);
    // The platform only takes 3 slots: the governor (7) overshoots on purpose.
    let mock = Arc::new(MockProvider::new(3, Duration::from_millis(20)));
    let policy = RetryPolicy(h.reg.retry.clone());
    let s = h
        .run(
            &mut l,
            mock.clone(),
            &g,
            &policy,
            &Gate::closed("test"),
            &BTreeMap::new(),
            &opts(),
        )
        .unwrap();
    assert!(s.request_limit_stops > 0, "{s:?}");
    assert_eq!(s.retries, s.stops);
    assert_eq!(s.failed, 0);
    assert_eq!(s.completed, 7, "{s:?}");
    assert_eq!(s.refused, 1, "the merge task is refused without HADES");
    assert_eq!(
        s.blocked, 1,
        "docs after landing waits on the refused merge"
    );
    assert!(s.peak_slots <= 7);
    assert_eq!(h.tasks["alpha:S2.1"].state, TaskState::Refused);
    assert_eq!(h.tasks["alpha:S2.2"].state, TaskState::Blocked);
    // Dependency order held: C started after A completed.
    let rep = ledger::read_all(&path).unwrap();
    assert!(rep.sound());
    let pos = |kind: &str, task: &str| {
        rep.events
            .iter()
            .position(|e| e.kind == kind && e.task.as_deref() == Some(task))
            .unwrap()
    };
    assert!(pos("completed", "alpha:S1.1") < pos("dispatched", "alpha:S1.3"));
    let rows = retry_table(&rep.events);
    assert!(!rows.is_empty());
    assert!(rows
        .iter()
        .all(|r| r.outcome == "done" && r.retries == r.stops));
    // Replay from disk gives the same states.
    let mut again = hermes();
    again.apply(&rep.events);
    for (k, rt) in &h.tasks {
        assert_eq!(again.tasks[k].state, rt.state, "{k}");
    }
}

#[test]
fn cap_under_the_platform_limit_means_no_stops() {
    let dir = tmp("nostop");
    let mut h = hermes();
    let (mut l, _) = Ledger::open(&dir.join("ledger.jsonl")).unwrap();
    let g = gov(&h, 4);
    let mock = Arc::new(MockProvider::new(4, Duration::from_millis(10)));
    let policy = RetryPolicy(h.reg.retry.clone());
    let s = h
        .run(
            &mut l,
            mock.clone(),
            &g,
            &policy,
            &Gate::closed("t"),
            &BTreeMap::new(),
            &opts(),
        )
        .unwrap();
    assert_eq!(s.stops, 0, "{s:?}");
    assert!(mock.peak() <= 4);
    assert_eq!(s.completed, 7);
}

#[test]
fn hermes_restart_mid_run_retries_the_running_task() {
    let dir = tmp("crash");
    let path = dir.join("ledger.jsonl");
    {
        let (mut l, _) = Ledger::open(&path).unwrap();
        l.append(NewEvent::new("queued").task("beta:T1.1", "beta"))
            .unwrap();
        l.append(
            NewEvent::new("dispatched")
                .task("beta:T1.1", "beta")
                .attempt(1)
                .state("running"),
        )
        .unwrap();
        // Crash: no completion line.
    }
    let mut h = hermes();
    let (mut l, rep) = Ledger::open(&path).unwrap();
    h.apply(&rep.events);
    assert_eq!(h.tasks["beta:T1.1"].state, TaskState::Running);
    let g = gov(&h, 7);
    let mock = Arc::new(MockProvider::new(10, Duration::from_millis(5)));
    let policy = RetryPolicy(h.reg.retry.clone());
    let s = h
        .run(
            &mut l,
            mock,
            &g,
            &policy,
            &Gate::closed("t"),
            &BTreeMap::new(),
            &opts(),
        )
        .unwrap();
    assert_eq!(h.tasks["beta:T1.1"].state, TaskState::Done);
    assert_eq!(
        h.tasks["beta:T1.1"].attempts, 2,
        "attempt 2 after the crash"
    );
    assert!(s.completed >= 3);
}

#[test]
fn dry_run_respects_caps_and_first_sessions_and_writes_nothing() {
    let h = hermes();
    let plan = h.dry_run(&Gate::closed("t"), Some(1), Some(6));
    assert_eq!(plan.worker_slots, 5);
    assert!(plan.slots_used <= 5);
    assert!(plan.rows.iter().all(|r| r.session == "S1"));
    assert_eq!(plan.count("wait-deps"), 2, "S1.3 and T1.2");
    assert!(plan.count("wait-cap") > 0);
    // Round-robin: both sectors get a dispatch.
    for s in ["alpha", "beta"] {
        assert!(plan
            .rows
            .iter()
            .any(|r| r.sector == s && r.decision == "dispatch"));
    }
}

#[test]
fn stalls_are_retried_too() {
    let dir = tmp("stall");
    let mut h = hermes();
    let (mut l, _) = Ledger::open(&dir.join("ledger.jsonl")).unwrap();
    let g = gov(&h, 7);
    let mock = Arc::new(MockProvider::new(50, Duration::from_millis(5)).with_stall_every(3));
    let policy = RetryPolicy(h.reg.retry.clone());
    let s = h
        .run(
            &mut l,
            mock,
            &g,
            &policy,
            &Gate::closed("t"),
            &BTreeMap::new(),
            &opts(),
        )
        .unwrap();
    assert!(s.stall_stops > 0, "{s:?}");
    assert_eq!(s.failed, 0);
    assert_eq!(s.completed, 7);
}

#[test]
fn operator_intake_cannot_downgrade_a_merge_and_survives_replay() {
    use hermes_core::action::ActionClass;
    let dir = tmp("intake");
    let path = dir.join("ledger.jsonl");
    let h = hermes();
    let t = h
        .intake_task(
            "alpha",
            "Merge swarm/integration-1 (8fd26f6) into main",
            Some(ActionClass::Code),
            vec![],
        )
        .unwrap();
    assert_eq!(
        t.class,
        ActionClass::Merge,
        "operator said code; classifier says merge"
    );
    assert_eq!(t.key, "alpha:H1");
    assert!(h.intake_task("nope", "x", None, vec![]).is_err());
    assert!(h
        .intake_task("alpha", "x", None, vec!["alpha:S9.9".into()])
        .is_err());
    {
        let (mut l, _) = Ledger::open(&path).unwrap();
        l.append(
            NewEvent::new("intake")
                .task(&t.key, "alpha")
                .state("queued")
                .detail(serde_json::to_string(&t).unwrap()),
        )
        .unwrap();
    }
    let mut again = hermes();
    again.apply(&ledger::read_all(&path).unwrap().events);
    assert_eq!(again.tasks["alpha:H1"].state, TaskState::Queued);
    let (mut l, _) = Ledger::open(&path).unwrap();
    let g = gov(&again, 7);
    let mock = Arc::new(MockProvider::new(10, Duration::from_millis(2)));
    let policy = RetryPolicy(again.reg.retry.clone());
    let only_h = RunOpts {
        first_sessions: Some(1),
        ..opts()
    };
    again
        .run(
            &mut l,
            mock,
            &g,
            &policy,
            &Gate::closed("t"),
            &BTreeMap::new(),
            &only_h,
        )
        .unwrap();
    assert_eq!(again.tasks["alpha:H1"].state, TaskState::Refused);
}
