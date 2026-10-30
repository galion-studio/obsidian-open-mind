// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Heal verification through the whole cortex pipeline (mock nerve, no
//! network): an executed restart arms one check; a held nerve recovery
//! verifies it; a unit still down at the deadline fails it, which opens
//! the circuit and escalates; dry-run / refused restarts arm nothing.

use gaia_cortex::config::Config;
use gaia_cortex::engine::{decider_wait, run_heal_checks, Cortex};
use gaia_cortex::guard::Guard;
use gaia_cortex::nerve::{NerveLink, Outcome};
use gaia_cortex::verify::StateRead;
use nerve_cortex_contract::{ActionRequest, ActionStatus, Event};
use openmind_nerve::protected_hosts::ProtectedHosts;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
struct MockNerve {
    reqs: Arc<Mutex<Vec<ActionRequest>>>,
    events: Arc<Mutex<Vec<Event>>>,
    reply: Arc<Mutex<Option<(ActionStatus, &'static str)>>>,
}

impl NerveLink for MockNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        self.reqs.lock().unwrap().push(req.clone());
        let (status, by) = self.reply.lock().unwrap().unwrap_or(if req.dry_run {
            (ActionStatus::Simulated, "nerve")
        } else {
            (ActionStatus::Executed, "nerve")
        });
        Outcome {
            status,
            by,
            reason: "mock".into(),
        }
    }
    fn post_event(&mut self, ev: &Event) -> bool {
        self.events.lock().unwrap().push(ev.clone());
        true
    }
    fn pull_events(&mut self, _n: usize) -> Result<Vec<Event>, String> {
        Ok(vec![])
    }
}

static N: AtomicU64 = AtomicU64::new(0);
const T0: u64 = 1_793_350_000;
const U: &str = "tuner.service";

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gaia-heal-verify-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn live_cfg(dir: PathBuf) -> Config {
    Config {
        data_dir: dir,
        actions: "live".into(),
        ..Config::default()
    }
}

fn cortex(cfg: Config) -> (Cortex, MockNerve) {
    let nerve = MockNerve::default();
    let cx = Cortex::new(
        cfg.clone(),
        Guard::with_hosts(ProtectedHosts::builtin(), &cfg.protected_units),
        Box::new(nerve.clone()),
    );
    (cx, nerve)
}

fn ev(kind: &str, sev: &str, t: u64, data: Value) -> Event {
    let mut e = Event::new("nerve", kind, sev, data);
    e.seq = Some(N.fetch_add(1, Ordering::SeqCst) + 10_000);
    e.t = Some(t);
    e.ts = gaia_cortex::engine::ts(t);
    e
}

fn crash(t: u64) -> Event {
    ev(
        "unit.failed",
        "error",
        t,
        json!({"unit": U, "exit_code": 1}),
    )
}

fn recovered(t: u64) -> Event {
    ev(
        "unit.state",
        "info",
        t,
        json!({"unit": U, "to": "active", "active_state": "active", "recovered": true, "by": "journal"}),
    )
}

fn kinds(n: &MockNerve) -> Vec<String> {
    n.events
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.kind.clone())
        .collect()
}

#[test]
fn executed_restart_is_verified_by_held_recovery() {
    let (mut cx, nerve) = cortex(live_cfg(tmpdir()));
    let d = cx.process(vec![crash(T0)], T0);
    assert_eq!(d[0]["action"]["status"], "executed");
    assert!(d[0]["action"]["verify"]["check"]
        .as_str()
        .unwrap()
        .starts_with("heal-"));
    assert_eq!(cx.heals.pending.len(), 1);
    assert_eq!(cx.health_json()["heals_pending"], 1);
    // the nerve's held recovery 40 s later
    cx.process(vec![recovered(T0 + 40)], T0 + 40);
    assert!(cx.heals.pending.is_empty());
    let h = cx.heals_json();
    assert_eq!(h["verified"], 1, "{h}");
    assert_eq!(h["success_rate"], 1.0);
    assert_eq!(h["recent"][0]["by"], "nerve-recovery");
    assert_eq!(h["recent"][0]["heal_s"], 40);
    assert!(kinds(&nerve).contains(&"gaia.heal.verified".to_string()));
    assert!(!cx.rules.circuit_open(U, T0 + 41));
    let log = std::fs::read_to_string(cx.cfg.data_dir.join("heals.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 2, "armed + finished");
}

#[test]
fn unit_still_down_at_deadline_fails_opens_circuit_and_escalates() {
    let (mut cx, nerve) = cortex(live_cfg(tmpdir()));
    cx.process(vec![crash(T0)], T0);
    let esc_before = cx.escalations.open.len();
    assert!(cx.heal_due(T0 + 179).is_empty(), "not due yet");
    let due = cx.heal_due(T0 + 180);
    assert_eq!(due.len(), 1);
    cx.heal_judged(
        due.into_iter().next().unwrap(),
        &StateRead::Unit {
            state: "failed".into(),
            status: "failing".into(),
        },
        T0 + 180,
    );
    let h = cx.heals_json();
    assert_eq!(h["failed"], 1, "{h}");
    assert!(cx.rules.circuit_open(U, T0 + 181), "no more blind restarts");
    assert_eq!(cx.escalations.open.len(), esc_before + 1);
    let k = kinds(&cx_posts(&mut cx, &nerve));
    assert!(k.contains(&"gaia.heal.failed".to_string()), "{k:?}");
    let last = cx.decisions.recent(1)[0].clone();
    assert_eq!(last["rule"], "R12-heal-failed");
    assert_eq!(last["decided_by"], "heal-verify");
    // A new crash now finds the circuit open: no restart asked.
    let reqs_before = nerve.reqs.lock().unwrap().len();
    let d = cx.process(vec![crash(T0 + 200)], T0 + 200);
    assert_eq!(d[0]["outcome"], "none", "{}", d[0]);
    assert_eq!(nerve.reqs.lock().unwrap().len(), reqs_before);
}

/// `heal_judged` queues posts; `process` would flush them on the next batch.
fn cx_posts(cx: &mut Cortex, nerve: &MockNerve) -> MockNerve {
    for p in cx.take_posts() {
        let mut n = nerve.clone();
        n.post_event(&p.event);
    }
    nerve.clone()
}

#[test]
fn deadline_with_unit_active_verifies_by_state_and_unreachable_is_unverified() {
    let (mut cx, _) = cortex(live_cfg(tmpdir()));
    cx.process(vec![crash(T0)], T0);
    let c = cx.heal_due(T0 + 180).pop().unwrap();
    cx.heal_judged(
        c,
        &StateRead::Unit {
            state: "active".into(),
            status: "healthy".into(),
        },
        T0 + 180,
    );
    assert_eq!(cx.heals_json()["verified"], 1);
    assert_eq!(cx.heals_json()["recent"][0]["by"], "nerve-state");
    // second unit: nerve gone at the deadline
    let mut e = crash(T0 + 300);
    e.data["unit"] = json!("galion-desk.service");
    cx.process(vec![e], T0 + 300);
    let c = cx.heal_due(T0 + 480).pop().unwrap();
    let esc = cx.escalations.open.len();
    cx.heal_judged(
        c,
        &StateRead::Unreachable("connect refused".into()),
        T0 + 480,
    );
    let h = cx.heals_json();
    assert_eq!(h["unverified"], 1, "{h}");
    assert_eq!(
        cx.escalations.open.len(),
        esc,
        "unverified alerts, never escalates"
    );
    assert!(!cx.rules.circuit_open("galion-desk.service", T0 + 481));
}

#[test]
fn crash_again_after_restart_is_failed_again_and_rules_still_decide() {
    let (mut cx, nerve) = cortex(live_cfg(tmpdir()));
    cx.process(vec![crash(T0)], T0);
    let d = cx.process(vec![crash(T0 + 20)], T0 + 20);
    assert_eq!(cx.heals_json()["failed_again"], 1);
    // the second crash went through the normal rules (restart #2, re-armed)
    assert_eq!(d[0]["rule"], "R3-known-crash", "{}", d[0]);
    assert_eq!(cx.heals.pending.len(), 1);
    assert_eq!(nerve.reqs.lock().unwrap().len(), 2);
}

#[test]
fn dry_run_refused_and_off_arm_nothing_lost_reply_arms() {
    // dry-run (the default)
    let (mut cx, _) = cortex(Config {
        data_dir: tmpdir(),
        ..Config::default()
    });
    let d = cx.process(vec![crash(T0)], T0);
    assert_eq!(d[0]["action"]["status"], "simulated");
    assert!(cx.heals.pending.is_empty());
    // refused by the nerve
    let (mut cx, nerve) = cortex(live_cfg(tmpdir()));
    *nerve.reply.lock().unwrap() = Some((ActionStatus::Refused, "nerve"));
    cx.process(vec![crash(T0)], T0);
    assert!(cx.heals.pending.is_empty());
    // verification off
    let mut c = live_cfg(tmpdir());
    c.heal_verify_s = 0;
    let (mut cx, _) = cortex(c);
    cx.process(vec![crash(T0)], T0);
    assert!(cx.heals.pending.is_empty());
    assert_eq!(cx.heals_json()["enabled"], false);
    // lost reply: the restart may have run, so it is verified
    let (mut cx, nerve) = cortex(live_cfg(tmpdir()));
    *nerve.reply.lock().unwrap() = Some((ActionStatus::Failed, "unknown"));
    cx.process(vec![crash(T0)], T0);
    assert_eq!(cx.heals.pending.len(), 1);
}

#[test]
fn pending_check_survives_a_cortex_restart() {
    let dir = tmpdir();
    {
        let (mut cx, _) = cortex(live_cfg(dir.clone()));
        cx.process(vec![crash(T0)], T0);
        assert_eq!(cx.heals.pending.len(), 1);
    }
    let (cx, _) = cortex(live_cfg(dir));
    assert_eq!(cx.heals.pending.len(), 1, "re-armed from heals.jsonl");
    assert_eq!(cx.heals_json()["rearmed"], 1);
}

#[test]
fn run_heal_checks_reads_state_outside_the_lock() {
    let (mut cx, _) = cortex(live_cfg(tmpdir()));
    cx.process(vec![crash(T0)], T0);
    let cx = Arc::new(Mutex::new(cx));
    let calls = Arc::new(AtomicU64::new(0));
    let (c2, cx2) = (calls.clone(), cx.clone());
    let read = move |unit: &str| {
        assert!(
            cx2.try_lock().is_ok(),
            "engine lock must be free during the read"
        );
        c2.fetch_add(1, Ordering::SeqCst);
        assert_eq!(unit, U);
        StateRead::Unit {
            state: "inactive".into(),
            status: "absent".into(),
        }
    };
    assert!(
        run_heal_checks(&cx, &read, T0 + 10).is_empty(),
        "nothing due: no read"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let posts = run_heal_checks(&cx, &read, T0 + 180);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(posts.iter().any(|p| p.event.kind == "gaia.heal.failed"));
    assert_eq!(cx.lock().unwrap().heals_json()["failed"], 1);
}

#[test]
fn decider_wait_takes_the_earliest() {
    let s = Some(Duration::from_secs(900));
    assert_eq!(decider_wait(None, None, T0), None, "idle = no wake-ups");
    assert_eq!(decider_wait(s, None, T0), s);
    assert_eq!(
        decider_wait(None, Some(T0 + 60), T0),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        decider_wait(s, Some(T0 + 60), T0),
        Some(Duration::from_secs(60))
    );
    assert_eq!(
        decider_wait(None, Some(T0 - 5), T0),
        Some(Duration::from_secs(1)),
        "overdue = soon"
    );
}

#[test]
fn config_window_bounds() {
    let env = |v: &'static str| move |k: &str| (k == "CORTEX_HEAL_VERIFY_S").then(|| v.to_string());
    assert_eq!(Config::load(None, &|_| None).unwrap().heal_verify_s, 180);
    assert_eq!(Config::load(None, &env("0")).unwrap().heal_verify_s, 0);
    assert_eq!(Config::load(None, &env("600")).unwrap().heal_verify_s, 600);
    assert!(Config::load(None, &env("10")).is_err());
    assert!(Config::load(None, &env("3601")).is_err());
    assert!(Config::load(None, &env("soon")).is_err());
}
