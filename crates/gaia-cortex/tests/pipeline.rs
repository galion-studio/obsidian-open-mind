// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Simulated event streams through the whole cortex pipeline (mock nerve,
//! mock LLM transport; no network, no model).

use gaia_cortex::config::Config;
use gaia_cortex::engine::{run_decider, Cortex};
use gaia_cortex::guard::Guard;
use gaia_cortex::llm::{Ledger, Transport};
use gaia_cortex::nerve::{NerveLink, Outcome};
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
    reply: Arc<Mutex<Option<ActionStatus>>>,
}

impl NerveLink for MockNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        self.reqs.lock().unwrap().push(req.clone());
        let st = self.reply.lock().unwrap().unwrap_or(if req.dry_run {
            ActionStatus::Simulated
        } else {
            ActionStatus::Executed
        });
        Outcome {
            status: st,
            by: "nerve",
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

#[derive(Clone)]
struct MockLlm {
    calls: Arc<AtomicU64>,
    answer: String,
}

impl Transport for MockLlm {
    fn post(&self, url: &str, key: &str, body: &str, _t: u64) -> Result<String, String> {
        assert!(!url.contains("openrouter"));
        assert_eq!(key, "test-key-not-real");
        let v: Value = serde_json::from_str(body).unwrap();
        assert!(v["max_tokens"].as_u64().unwrap() <= 400);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"choices": [{"message": {"content": self.answer}}],
                  "usage": {"prompt_tokens": 900, "completion_tokens": 60}})
        .to_string())
    }
}

static N: AtomicU64 = AtomicU64::new(0);

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gaia-cortex-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cortex(cfg: Config) -> (Cortex, MockNerve, MockLlm) {
    let nerve = MockNerve::default();
    let llm = MockLlm {
        calls: Arc::new(AtomicU64::new(0)),
        answer: r#"{"choice":"escalate","reason":"unsure"}"#.into(),
    };
    let mut cx = Cortex::new(
        cfg.clone(),
        Guard::with_hosts(ProtectedHosts::builtin(), &cfg.protected_units),
        Box::new(nerve.clone()),
    );
    cx.advisor.transport = Box::new(llm.clone());
    cx.advisor.get_env =
        Box::new(|k| (k == "RIVER_API_KEY").then(|| "test-key-not-real".to_string()));
    (cx, nerve, llm)
}

fn cfg() -> Config {
    Config {
        data_dir: tmpdir(),
        ..Config::default()
    }
}

fn llm_cfg() -> Config {
    let mut c = cfg();
    c.llm.enabled = true;
    c
}

fn ev(kind: &str, sev: &str, data: Value) -> Event {
    let mut e = Event::new("nerve", kind, sev, data);
    e.ts = "2026-10-21T18:40:00+02:00".into();
    e
}

fn seq() -> u64 {
    N.fetch_add(1, Ordering::SeqCst) + 1000
}

fn crash(unit: &str) -> Event {
    ev(
        "unit.crash",
        "error",
        json!({"seq": seq(), "unit": unit, "exit_code": 1}),
    )
}

const T0: u64 = 1_792_600_000;

#[test]
fn crash_decides_restart_via_nerve_dry_run() {
    let (mut cx, nerve, llm) = cortex(cfg());
    let d = cx.process(vec![crash("tuner.service")], T0);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["rule"], "R3-known-crash");
    assert_eq!(d[0]["outcome"], "restart:tuner.service");
    assert_eq!(d[0]["action"]["status"], "simulated");
    assert_eq!(d[0]["dry_run"], true);
    let reqs = nerve.reqs.lock().unwrap();
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].dry_run);
    assert_eq!(reqs[0].unit, "tuner.service");
    let s = cx.stats_json();
    assert_eq!(s["actions_simulated"], 1);
    assert_eq!(s["actions_executed"], 0);
    assert_eq!(s["llm_calls"], 0);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    // The alert went to the nerve's /event as a cortex.alert.
    assert!(nerve
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.kind == "cortex.alert"));
    // decisions.jsonl written
    let log = std::fs::read_to_string(cx.cfg.data_dir.join("decisions.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
}

#[test]
fn burst_coalesces_to_one_decision_and_dupes_drop() {
    let (mut cx, nerve, _) = cortex(cfg());
    let c = crash("hermes-rs.service");
    let burst = vec![
        ev(
            "unit.state",
            "warn",
            json!({"seq": seq(), "unit": "hermes-rs.service", "from": "active", "to": "failed"}),
        ),
        c.clone(),
        ev(
            "process.exit",
            "warn",
            json!({"seq": seq(), "unit": "hermes-rs.service", "pid": 77, "exit_code": 1}),
        ),
        c.clone(),
        c,
    ];
    let d = cx.process(burst, T0);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(nerve.reqs.lock().unwrap().len(), 1);
    let s = cx.stats_json();
    assert_eq!(s["events_in"], 5);
    assert_eq!(s["events_duplicate"], 2);
    assert_eq!(s["incidents_open"], 1);
}

#[test]
fn flapping_opens_circuit_then_escalates_once() {
    let (mut cx, nerve, _) = cortex(cfg());
    let mut rules = vec![];
    for i in 0..5u64 {
        let d = cx.process(vec![crash("tuner.service")], T0 + i * 60);
        rules.push(
            d.first()
                .map(|d| d["rule"].as_str().unwrap().to_string())
                .unwrap_or_else(|| "suppressed".into()),
        );
    }
    assert_eq!(
        rules,
        [
            "R3-known-crash",
            "R3-known-crash",
            "R2-circuit-open",
            "suppressed",
            "R2-circuit-open"
        ],
        "{rules:?}"
    );
    // 4th crash is within the 120 s alert cooldown of the 3rd; the 5th re-alerts (no new escalation).
    assert_eq!(
        nerve.reqs.lock().unwrap().len(),
        2,
        "no restart once the circuit is open"
    );
    assert_eq!(
        cx.stats.escalations, 1,
        "one escalation per circuit opening"
    );
    let esc = std::fs::read_to_string(cx.cfg.data_dir.join("escalations.jsonl")).unwrap();
    assert!(esc.contains("flapping"));
    // At the scenario's own clock (T0-based), not the wall clock: the
    // incidents_json view uses now_secs(), which made this assertion expire
    // one hour after T0 (time bomb found during T5.1, 2026-10-21 23:14 +02:00).
    assert!(cx.rules.circuit_open("tuner.service", T0 + 4 * 60));
    assert!(cx.incidents_json()["units"]["tuner.service"].is_object());
}

#[test]
fn hourly_cap_opens_circuit() {
    let mut c = cfg();
    c.flap_failures = 100;
    c.restarts_per_unit_hour = 2;
    let (mut cx, nerve, _) = cortex(c);
    let r: Vec<String> = (0..3u64)
        .map(|i| {
            cx.process(vec![crash("tuner.service")], T0 + i * 300)[0]["rule"]
                .as_str()
                .unwrap()
                .into()
        })
        .collect();
    assert_eq!(r, ["R3-known-crash", "R3-known-crash", "R3-cap"]);
    assert_eq!(nerve.reqs.lock().unwrap().len(), 2);
}

#[test]
fn protected_items_alert_only() {
    let (mut cx, nerve, _) = cortex(cfg());
    let evs = vec![
        crash("cloudflared.service"),
        crash("ssh.service"),
        crash("systemd-networkd.service"),
        crash("postfix.service"),
        crash("galion-backup.service"),
    ];
    for e in evs {
        let d = cx.process(vec![e], T0);
        assert_eq!(d[0]["rule"], "R1-protected", "{d:?}");
        assert_eq!(d[0]["outcome"], "none");
        assert_eq!(d[0]["alert"], true);
    }
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "dns_rtt", "host": "elaine.ns.cloudflare.com"}),
        )],
        T0,
    );
    assert_eq!(d[0]["rule"], "R1-protected");
    let d = cx.process(
        vec![ev(
            "unclassified",
            "critical",
            json!({"seq": seq(), "comm": "x", "exe": "/opt/galion/openmind/bin/x"}),
        )],
        T0,
    );
    assert_eq!(d[0]["rule"], "R1-protected");
    assert!(
        nerve.reqs.lock().unwrap().is_empty(),
        "never an action on protected items"
    );
    // Cloudflared is on the nerve reflex ALLOW, but never in the cortex restart list.
    assert!(!cx.rules.restart_allowed("cloudflared.service"));
}

#[test]
fn stack_wide_and_unknown_units_escalate_without_action() {
    let (mut cx, nerve, _) = cortex(cfg());
    let d = cx.process(
        vec![
            crash("tuner.service"),
            crash("hermes-rs.service"),
            crash("console.service"),
        ],
        T0,
    );
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["rule"], "R0-stack-wide");
    assert_eq!(d[0]["escalate"], true);
    let d = cx.process(vec![crash("mystery.service")], T0 + 10);
    assert_eq!(d[0]["rule"], "R4-unknown-unit");
    assert_eq!(d[0]["escalate"], true);
    assert!(nerve.reqs.lock().unwrap().is_empty());
}

#[test]
fn nerve_rate_limit_escalates() {
    let (mut cx, nerve, _) = cortex(cfg());
    *nerve.reply.lock().unwrap() = Some(ActionStatus::RateLimited);
    let d = cx.process(vec![crash("tuner.service")], T0);
    assert_eq!(d[0]["action"]["status"], "rate-limited");
    assert_eq!(d[0]["escalate"], true);
    // And a nerve circuit.open event opens the cortex circuit.
    let d = cx.process(
        vec![ev(
            "circuit.open",
            "error",
            json!({"seq": seq(), "unit": "galion-desk.service"}),
        )],
        T0,
    );
    assert_eq!(d[0]["rule"], "R10-remediation");
    assert!(cx.rules.circuit_open("galion-desk.service", T0 + 1));
}

#[test]
fn recovery_and_tasks_and_info_do_not_alert() {
    let (mut cx, nerve, llm) = cortex(llm_cfg());
    let d = cx.process(
        vec![ev(
            "unit.state",
            "info",
            json!({"seq": seq(), "unit": "tuner.service", "from": "activating", "to": "active"}),
        )],
        T0,
    );
    assert_eq!(d[0]["rule"], "R5-recovered");
    assert_eq!(d[0]["alert"], false);
    let d = cx.process(
        vec![ev(
            "task.finished",
            "info",
            json!({"seq": seq(), "id": "t1", "ok": true}),
        )],
        T0,
    );
    assert_eq!(d[0]["alert"], false);
    let d = cx.process(
        vec![ev(
            "slot.finished",
            "warn",
            json!({"seq": seq(), "id": "s9", "ok": false}),
        )],
        T0,
    );
    assert_eq!(d[0]["rule"], "R6-task-failed");
    assert_eq!(d[0]["alert"], true);
    assert!(nerve.reqs.lock().unwrap().is_empty());
    assert_eq!(
        llm.calls.load(Ordering::SeqCst),
        0,
        "no model call for deterministic cases even when enabled"
    );
}

#[test]
fn llm_disabled_by_default_makes_no_call() {
    let (mut cx, _, llm) = cortex(cfg());
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "load1", "value": 40}),
        )],
        T0,
    );
    assert_eq!(d[0]["rule"], "R7-threshold");
    assert_eq!(d[0]["llm"]["refused"], "disabled");
    assert_eq!(d[0]["escalate"], true);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    assert_eq!(cx.stats.llm_skipped_disabled, 1);
    assert!(
        !cx.cfg.data_dir.join("llm-spend.json").exists(),
        "disabled advisor never touches the ledger"
    );
}

#[test]
fn llm_advice_limited_to_allowlisted_choices() {
    let (mut cx, nerve, mut llm) = cortex(llm_cfg());
    llm.answer = r#"{"choice":"restart:tuner.service","reason":"looks wedged"}"#.into();
    cx.advisor.transport = Box::new(llm.clone());
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "unit": "tuner.service", "metric": "p99_ms", "value": 9000}),
        )],
        T0,
    );
    assert_eq!(d[0]["decided_by"], "llm-advisor");
    assert_eq!(d[0]["outcome"], "restart:tuner.service");
    assert!(nerve.reqs.lock().unwrap()[0].dry_run);
    let l = Ledger::load(&cx.cfg.data_dir.join("llm-spend.json")).unwrap();
    let want = (900.0 * 0.12 + 60.0 * 0.48) / 1e6;
    assert!(
        (l.total_usd - want).abs() < 1e-12,
        "{} vs {want}",
        l.total_usd
    );
    assert_eq!(l.calls, 1);
    // A free-form command is never accepted.
    llm.answer = r#"{"choice":"bash -c 'rm -rf /'","reason":"x"}"#.into();
    cx.advisor.transport = Box::new(llm.clone());
    let d = cx.process(
        vec![ev(
            "unclassified",
            "critical",
            json!({"seq": seq(), "comm": "kworkerz"}),
        )],
        T0 + 5,
    );
    assert_eq!(d[0]["outcome"], "none");
    assert_eq!(d[0]["escalate"], true);
    // Restart of a unit that was not offered (protected) is not taken either.
    llm.answer = r#"{"choice":"restart:ssh.service","reason":"x"}"#.into();
    cx.advisor.transport = Box::new(llm.clone());
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "mem"}),
        )],
        T0 + 6,
    );
    assert_eq!(d[0]["outcome"], "none");
    assert_eq!(nerve.reqs.lock().unwrap().len(), 1);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 3);
}

#[test]
fn budget_exhaustion_refuses_llm() {
    let c = llm_cfg();
    let ledger = c.data_dir.join("llm-spend.json");
    Ledger {
        total_usd: 14.9999,
        calls: 9999,
        ..Ledger::default()
    }
    .save(&ledger)
    .unwrap();
    let (mut cx, _, llm) = cortex(c);
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "load1"}),
        )],
        T0,
    );
    assert_eq!(d[0]["llm"]["refused"], "budget-cap");
    assert_eq!(d[0]["escalate"], true);
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
    // An unreadable ledger fails closed.
    std::fs::write(&ledger, "not json").unwrap();
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "load5"}),
        )],
        T0 + 1,
    );
    assert_eq!(d[0]["llm"]["refused"], "ledger-error");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn budget_runs_out_over_many_calls() {
    let mut c = llm_cfg();
    c.llm.budget_usd_total = 0.0005; // a few calls' worth
    c.llm.calls_per_hour = 1000;
    let (mut cx, _, llm) = cortex(c);
    let mut refused = 0;
    for i in 0..50u64 {
        let d = cx.process(
            vec![ev(
                "threshold.crossed",
                "critical",
                json!({"seq": seq(), "metric": format!("m{i}")}),
            )],
            T0 + i,
        );
        if d[0]["llm"]["refused"] == "budget-cap" {
            refused += 1;
        }
    }
    let l = Ledger::load(&cx.cfg.data_dir.join("llm-spend.json")).unwrap();
    assert!(
        l.total_usd <= 0.0005 + 1e-12,
        "spent {} over cap",
        l.total_usd
    );
    assert!(refused > 0);
    assert_eq!(llm.calls.load(Ordering::SeqCst) + refused, 50);
}

#[test]
fn calls_per_hour_cap() {
    let (mut cx, _, llm) = cortex(llm_cfg());
    let mut codes = vec![];
    for i in 0..8u64 {
        let d = cx.process(
            vec![ev(
                "threshold.crossed",
                "critical",
                json!({"seq": seq(), "metric": format!("x{i}")}),
            )],
            T0 + i,
        );
        codes.push(
            d[0]["llm"]["refused"]
                .as_str()
                .unwrap_or("called")
                .to_string(),
        );
    }
    assert_eq!(llm.calls.load(Ordering::SeqCst), 6);
    assert_eq!(&codes[6..], ["calls-per-hour-cap", "calls-per-hour-cap"]);
    // An hour later the window has room again.
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "later"}),
        )],
        T0 + 3700,
    );
    assert!(d[0]["llm"]["refused"].is_null());
}

#[test]
fn openrouter_and_hermes_endpoints_refused() {
    for ep in [
        "https://openrouter.ai/api/v1",
        "https://OpenRouter.ai/api/v1",
        "https://proxy.example.com/openrouter/v1",
        "http://127.0.0.1:8642/v1",
        "https://hermes.galion.app/v1",
        "http://api.river.ai/v1",
    ] {
        let mut c = llm_cfg();
        c.llm.endpoint = ep.into();
        let (mut cx, _, llm) = cortex(c);
        let d = cx.process(
            vec![ev(
                "threshold.crossed",
                "critical",
                json!({"seq": seq(), "metric": "load1"}),
            )],
            T0,
        );
        assert_eq!(d[0]["llm"]["refused"], "endpoint-guard", "{ep}");
        assert_eq!(llm.calls.load(Ordering::SeqCst), 0, "{ep}");
    }
    assert!(gaia_cortex::llm::endpoint_refusal("https://api.river.ai/v1").is_none());
}

#[test]
fn missing_key_refuses() {
    let (mut cx, _, llm) = cortex(llm_cfg());
    cx.advisor.get_env = Box::new(|_| None);
    let d = cx.process(
        vec![ev(
            "threshold.crossed",
            "critical",
            json!({"seq": seq(), "metric": "load1"}),
        )],
        T0,
    );
    assert_eq!(d[0]["llm"]["refused"], "no-key");
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn idle_means_zero_work_and_zero_calls() {
    let (cx, _, llm) = cortex(llm_cfg());
    let cx = Arc::new(Mutex::new(cx));
    let (tx, rx) = std::sync::mpsc::channel::<Vec<Event>>();
    let h = {
        let cx = cx.clone();
        std::thread::spawn(move || run_decider(rx, cx, Duration::from_millis(50)))
    };
    std::thread::sleep(Duration::from_millis(1500));
    {
        let g = cx.lock().unwrap();
        assert_eq!(g.stats.batches, 0);
        assert_eq!(g.stats.decisions, 0);
        assert_eq!(g.stats.llm_calls, 0);
    }
    // One event -> exactly one batch, then idle again.
    tx.send(vec![crash("tuner.service")]).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(cx.lock().unwrap().stats.batches, 1);
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(cx.lock().unwrap().stats.batches, 1);
    drop(tx);
    h.join().unwrap();
    assert_eq!(llm.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn own_alerts_are_not_re_ingested() {
    let (mut cx, nerve, _) = cortex(cfg());
    let d = cx.process(
        vec![Event::new(
            "gaia-cortex",
            "cortex.alert",
            "error",
            json!({"subject": "unit:tuner.service"}),
        )],
        T0,
    );
    assert!(d.is_empty());
    assert_eq!(cx.stats.events_ignored, 1);
    assert!(nerve.reqs.lock().unwrap().is_empty());
}

#[test]
fn live_mode_sends_live_request_only_when_configured() {
    let mut c = cfg();
    c.actions = "live".into();
    let (mut cx, nerve, _) = cortex(c);
    let d = cx.process(vec![crash("tuner.service")], T0);
    assert_eq!(d[0]["dry_run"], false);
    assert!(!nerve.reqs.lock().unwrap()[0].dry_run);
    // Without a nerve endpoint a live request is refused, never run locally.
    let mut c = cfg();
    c.actions = "live".into();
    let mut cx = Cortex::new(
        c,
        Guard::with_hosts(ProtectedHosts::builtin(), &[]),
        Box::new(gaia_cortex::nerve::NoNerve),
    );
    let d = cx.process(vec![crash("tuner.service")], T0);
    assert_eq!(d[0]["action"]["status"], "refused");
    assert_eq!(d[0]["action"]["by"], "local");
}

/// Real nerve-event-driven wire lines (evt::NerveEvent v1) as journald
/// produces them for one crash.
fn evt(seq: u64, kind: &str, sev: &str, unit: &str, data: Value) -> Event {
    let l = json!({"v": 1, "seq": seq, "ts": "2026-10-21T18:40:00+02:00", "t": T0, "source": "journal",
                   "kind": kind, "severity": sev, "unit": unit, "group": "other",
                   "key": format!("{kind}:{unit}"), "msg": "m", "data": data, "count": 1});
    nerve_cortex_contract::parse_events(&l.to_string())
        .pop()
        .unwrap()
        .unwrap()
}

#[test]
fn nerve_evt_crash_without_auto_restart_restarts() {
    let (mut cx, nerve, _) = cortex(cfg());
    let d = cx.process(
        vec![
            evt(
                10,
                "unit.exit",
                "high",
                "tuner.service",
                json!({"exit_code": "exited", "exit_status": "1"}),
            ),
            evt(
                11,
                "unit.failed",
                "high",
                "tuner.service",
                json!({"unit_result": "exit-code"}),
            ),
        ],
        T0,
    );
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["rule"], "R3-known-crash");
    assert_eq!(nerve.reqs.lock().unwrap()[0].source, "gaia-cortex");
}

#[test]
fn nerve_evt_systemd_restarting_and_start_limit() {
    let (mut cx, nerve, _) = cortex(cfg());
    let d = cx.process(
        vec![
            evt(
                20,
                "unit.exit",
                "high",
                "hermes-rs.service",
                json!({"exit_status": "1"}),
            ),
            evt(
                21,
                "unit.restart_scheduled",
                "high",
                "hermes-rs.service",
                json!({"n_restarts": "1"}),
            ),
        ],
        T0,
    );
    assert_eq!(d[0]["rule"], "R3b-systemd-restarting");
    assert_eq!(d[0]["outcome"], "none");
    let d = cx.process(
        vec![evt(
            22,
            "unit.start_limit",
            "critical",
            "galion-desk.service",
            json!({"unit_result": "start-limit-hit"}),
        )],
        T0 + 5,
    );
    assert_eq!(d[0]["rule"], "R2-start-limit");
    assert_eq!(d[0]["escalate"], true);
    assert!(nerve.reqs.lock().unwrap().is_empty());
    // Keepalives are housekeeping: no batch, no decision.
    let mut k = Event::new("nerve", "stream.keepalive", "info", Value::Null);
    k.msg = Some(String::new());
    assert!(cx.process(vec![k], T0 + 300).is_empty());
    assert_eq!(cx.stats.events_housekeeping, 1);
    // Our own action.result echo is not re-decided as a problem.
    let echo = evt(
        23,
        "action.result",
        "info",
        "tuner.service",
        json!({"result": "dry-run", "source": "gaia-cortex"}),
    );
    let d = cx.process(vec![echo], T0 + 301);
    assert_eq!(d[0]["alert"], false);
    // Disk crossing at critical -> threshold rule, escalated (advisor off).
    let mut disk = Event::new(
        "threshold",
        "disk.crossed",
        "critical",
        json!({"path": "/", "pct": 97}),
    );
    disk.seq = Some(24);
    let d = cx.process(vec![disk], T0 + 302);
    assert_eq!(d[0]["rule"], "R7-threshold");
    assert_eq!(d[0]["subject"], "threshold:/");
}

#[test]
fn transient_unit_not_on_allowlist_escalates() {
    let (mut cx, nerve, _) = cortex(cfg());
    let d = cx.process(
        vec![
            evt(
                30,
                "unit.exit",
                "high",
                "cortex-sbx-crash-1.service",
                json!({"exit_status": "3"}),
            ),
            evt(
                31,
                "unit.failed",
                "high",
                "cortex-sbx-crash-1.service",
                json!({"unit_result": "exit-code"}),
            ),
        ],
        T0,
    );
    assert_eq!(d[0]["rule"], "R4-unknown-unit");
    assert!(nerve.reqs.lock().unwrap().is_empty());
}

#[test]
fn nerve_batches_and_engine_incidents_do_not_loop() {
    let (mut cx, nerve, _) = cortex(cfg());
    let mut b = Event::new(
        "escalation",
        "escalation.batch",
        "high",
        json!({"batch": {"id": 1, "seqs": [4, 5]}}),
    );
    b.seq = Some(9);
    assert!(
        cx.process(vec![b], T0).is_empty(),
        "batch summaries are bookkeeping"
    );
    let mut i = Event::new(
        "engine",
        "incident.opened",
        "high",
        json!({"cause": "edge", "components": ["cloudflared"]}),
    );
    i.seq = Some(10);
    let d = cx.process(vec![i], T0);
    assert_eq!(d[0]["rule"], "R11-nerve-incident");
    assert_eq!(d[0]["subject"], "nerve-incident:edge");
    assert_eq!(d[0]["alert"], true);
    assert_eq!(d[0]["escalate"], false);
    assert!(nerve.reqs.lock().unwrap().is_empty());
}

/// Mate t217: incidents the rules cannot settle go to the Hermes boss inbox,
/// once per incident, never per event; protected units and restarts do not.
#[test]
fn unresolved_incident_handed_to_hermes_once() {
    let mut c = cfg();
    c.actions = "live".into();
    c.hermes.enabled = true;
    let inbox = c.data_dir.join("hermes-inbox");
    let (mut cx, nerve, _) = cortex(c);
    let files = || -> Vec<serde_json::Value> {
        let mut v: Vec<_> = std::fs::read_dir(&inbox)
            .map(|r| r.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.retain(|p: &std::path::PathBuf| p.extension().is_some_and(|x| x == "json"));
        v.iter()
            .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap())
            .collect()
    };
    // restarts (R3-known-crash) are the cortex's own job: no handoff
    cx.process(vec![crash("tuner.service")], T0);
    cx.process(vec![crash("tuner.service")], T0 + 60);
    assert!(files().is_empty());
    assert_eq!(nerve.reqs.lock().unwrap().len(), 2);
    // third crash opens the circuit (R2) -> escalation -> one handoff
    for i in 2..8u64 {
        cx.process(vec![crash("tuner.service")], T0 + i * 200);
    }
    let f = files();
    assert_eq!(f.len(), 1, "one handoff for the incident: {f:?}");
    assert_eq!(f[0]["kind"], "cortex.handoff");
    assert_eq!(f[0]["rule"], "R2-circuit-open");
    assert!(f[0]["handoff_id"].as_str().unwrap().starts_with("hoff-"));
    assert!(f[0]["recent_decisions"].as_array().unwrap().len() >= 2);
    assert_eq!(cx.stats.handoffs, 1);
    // protected unit: alert only, never handed off for action
    cx.process(vec![crash("cloudflared.service")], T0 + 2000);
    assert_eq!(files().len(), 1);
    // budget stop file from the dispatcher: no more handoffs
    std::fs::write(inbox.join(gaia_cortex::handoff::STOP_FILE), "spent").unwrap();
    cx.process(
        vec![ev(
            "mystery.thing",
            "critical",
            json!({"seq": seq(), "what": "unknown"}),
        )],
        T0 + 3000,
    );
    assert_eq!(files().len(), 1);
    assert_eq!(cx.stats.handoffs_budget_stopped, 1);
    assert_eq!(cx.stats_json()["hermes_handoff"]["budget_stopped"], true);
}
