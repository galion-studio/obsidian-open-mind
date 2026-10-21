// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The bench coordinator with a stand-in executor (no processes), plus the
//! real process executor's timeout and environment scrubbing.

use hermes_core::bench::{self, Exec, ExecResult, ProcessExec, RunCfg, Spec};
use hermes_core::engine::{Hermes, TaskState};
use hermes_core::gate::Gate;
use hermes_core::ledger::{self, Ledger, NewEvent};
use hermes_core::registry::Registry;
use hermes_core::retry::RetryPolicy;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const REG: &str = r#"
[hermes]
boss_agent = "hermes"
boss_model = "m-opus"
sub_model = "m-sonnet"
provider = "mock"

[retry]
max_attempts = 8
backoff_base_secs = 30
backoff_factor = 2
backoff_max_secs = 600
max_error_attempts = 2

[[sector]]
id = "bench"
name = "Benchmarks"
plan = "bench.md"
plan_builtin = true
lead = "bench.lead"
max_concurrent = 3
gaia_functions = ["ARTEMIS"]
gaia_systems = ["ecosystem-census", "hermes-agents"]
"#;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hermes-bench-coord-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn hermes(ledger: Option<&Path>) -> Hermes {
    let reg = Registry::parse(REG).unwrap();
    let mut h = Hermes::load(reg, Path::new("/nonexistent-plans")).unwrap();
    if let Some(p) = ledger {
        h.apply(&ledger::read_all(p).unwrap().events);
    }
    h
}

fn cfg(out: &Path) -> RunCfg {
    RunCfg {
        out: out.to_path_buf(),
        sha: "test".into(),
        rerun: false,
        only: Vec::new(),
        timeout: Duration::from_secs(30),
        time_scale: 0.001,
        cap: None,
    }
}

/// Stand-in for hermes-bench: writes the artefact the real runner writes.
#[derive(Default)]
struct Fake {
    /// argv substring -> number of failing attempts before success.
    fail: Mutex<BTreeMap<String, u32>>,
    /// argv substring -> pct written.
    pct: BTreeMap<String, u64>,
    calls: Mutex<Vec<(String, Instant, Instant)>>,
    active: Mutex<(usize, usize)>,
    work: Duration,
}

fn flag<'a>(argv: &'a [String], k: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == k)
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str)
}

impl Exec for Fake {
    fn exec(&self, argv: &[String], _stem: &Path, _t: Duration) -> ExecResult {
        let line = argv.join(" ");
        {
            let mut a = self.active.lock().unwrap();
            a.0 += 1;
            a.1 = a.1.max(a.0);
        }
        let t0 = Instant::now();
        std::thread::sleep(self.work);
        let mut res = ExecResult {
            exit: Some(0),
            ..ExecResult::default()
        };
        let failing = {
            let mut f = self.fail.lock().unwrap();
            let hit = f
                .iter_mut()
                .find(|(k, n)| line.contains(k.as_str()) && **n > 0);
            match hit {
                Some((_, n)) => {
                    *n -= 1;
                    true
                }
                None => false,
            }
        };
        if failing {
            res.exit = Some(1);
            res.error = Some("simulated crash".into());
        } else {
            let out = PathBuf::from(flag(argv, "--out").or(flag(argv, "--dir")).unwrap());
            let agent = flag(argv, "--agent").unwrap();
            if argv[0] == "run" {
                let b = flag(argv, "--bench").unwrap();
                let pct = self
                    .pct
                    .iter()
                    .find(|(k, _)| line.contains(k.as_str()))
                    .map_or(100, |(_, p)| *p);
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(
                    out.join(format!("hermes-{b}.{agent}.score.json")),
                    format!("{{\"pct\": {pct}, \"axes\": []}}"),
                )
                .unwrap();
                res.tail = format!("{b} [{agent}] via Hermes: {pct}%");
            } else {
                std::fs::write(flag(argv, "--out").unwrap(), "| B1 | 100% |\n").unwrap();
            }
        }
        self.active.lock().unwrap().0 -= 1;
        self.calls.lock().unwrap().push((line, t0, Instant::now()));
        res
    }
}

fn row<'a>(b: &'a bench::BenchRun, local: &str) -> &'a bench::Row {
    b.rows
        .iter()
        .find(|r| r.task == format!("bench:{local}"))
        .unwrap_or_else(|| panic!("no row {local}"))
}

#[test]
fn full_matrix_runs_in_dependency_order_under_the_sector_cap() {
    let dir = tmp("full");
    let lp = dir.join("ledger.jsonl");
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let h = hermes(None);
    let fake = Arc::new(Fake {
        work: Duration::from_millis(40),
        ..Fake::default()
    });
    let mut b = bench::run(
        &h,
        &Gate::closed("test"),
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake.clone(),
        &cfg(&dir.join("out")),
    )
    .unwrap();
    assert_eq!(
        (b.done, b.failed, b.blocked, b.refused),
        (8, 0, 0, 0),
        "{b:#?}"
    );
    assert_eq!(b.cap, 3);
    assert!(b.peak <= 3 && b.peak >= 2, "peak {}", b.peak);
    assert!(fake.active.lock().unwrap().1 <= 3);
    assert!(b.all_100("scripted") && b.all_100("mock-llm"));
    assert!(
        hermes_core::report::spoken_ok(&b.spoken).is_ok(),
        "{}",
        b.spoken
    );
    // Order: each mock-llm run starts after its scripted run ended; the
    // scoreboards start after all their runs ended.
    let calls = fake.calls.lock().unwrap().clone();
    let span = |needle: &str| {
        calls
            .iter()
            .find(|c| c.0.contains(needle))
            .map(|c| (c.1, c.2))
            .unwrap_or_else(|| panic!("no call {needle}"))
    };
    for bn in ["banking", "wallstreet", "vending"] {
        let s = span(&format!("--bench {bn} --agent scripted"));
        let m = span(&format!("--bench {bn} --agent mock-llm"));
        assert!(m.0 >= s.1, "{bn}: mock-llm started before scripted ended");
        let sb = span("scoreboard --dir");
        assert!(sb.0 >= s.1);
    }
    assert_eq!(calls.len(), 8);
    // The ledger shows every state change and replays into Done.
    let rep = ledger::read_all(&lp).unwrap();
    assert!(rep.sound());
    let kinds = |k: &str| rep.events.iter().filter(|e| e.kind == k).count();
    assert_eq!((kinds("dispatched"), kinds("completed")), (8, 8));
    drop(l);
    let h2 = hermes(Some(&lp));
    for rt in h2.tasks.values() {
        assert_eq!(rt.state, TaskState::Done, "{}", rt.task.key);
    }
    // Readiness G16 passes on this record.
    let (ok, ev) = bench::readiness(&h2, &Gate::closed("test"), Some(&b));
    assert!(ok, "{ev}");
    // A second run without rerun has nothing to do.
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let mut again = bench::run(
        &h2,
        &Gate::closed("test"),
        &mut l,
        &RetryPolicy(h2.reg.retry.clone()),
        fake.clone(),
        &cfg(&dir.join("out")),
    )
    .unwrap();
    assert_eq!(again.done, 0);
    assert!(again.rows.iter().all(|r| r.outcome == "already-done"));
    assert_eq!(fake.calls.lock().unwrap().len(), 8, "nothing re-executed");
    // Save / load round trip; the no-op run does not replace last.json.
    let saved = bench::save(&dir, &mut b).unwrap();
    assert_eq!(saved, bench::last_path(&dir));
    let noop = bench::save(&dir, &mut again).unwrap();
    assert_ne!(noop, bench::last_path(&dir));
    assert!(noop.is_file());
    assert_eq!(bench::load_last(&dir).unwrap().unwrap(), b);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_crash_is_retried_then_completes() {
    let dir = tmp("retry");
    let lp = dir.join("ledger.jsonl");
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let h = hermes(None);
    let fake = Arc::new(Fake::default());
    fake.fail
        .lock()
        .unwrap()
        .insert("--bench wallstreet --agent scripted".into(), 1);
    let b = bench::run(
        &h,
        &Gate::closed("test"),
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake,
        &cfg(&dir.join("out")),
    )
    .unwrap();
    assert_eq!((b.done, b.failed, b.retries), (8, 0, 1), "{b:#?}");
    assert_eq!(row(&b, "S1.2").attempts, 2);
    let rep = ledger::read_all(&lp).unwrap();
    let kinds: Vec<&str> = rep
        .events
        .iter()
        .filter(|e| e.task.as_deref() == Some("bench:S1.2"))
        .map(|e| e.kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "dispatched",
            "stopped",
            "retry-scheduled",
            "dispatched",
            "completed"
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_persistent_failure_fails_and_blocks_its_dependents() {
    let dir = tmp("fail");
    let lp = dir.join("ledger.jsonl");
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let h = hermes(None);
    let fake = Arc::new(Fake::default());
    fake.fail
        .lock()
        .unwrap()
        .insert("--bench banking --agent scripted".into(), 99);
    let b = bench::run(
        &h,
        &Gate::closed("test"),
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake,
        &cfg(&dir.join("out")),
    )
    .unwrap();
    assert_eq!(row(&b, "S1.1").outcome, "failed");
    assert_eq!(row(&b, "S1.1").attempts, 2, "max_error_attempts = 2");
    for k in ["S2.1", "S3.1", "S3.2"] {
        assert_eq!(row(&b, k).outcome, "blocked", "{k}");
        assert_eq!(row(&b, k).attempts, 0, "{k} never ran");
    }
    for k in ["S1.2", "S1.3", "S2.2", "S2.3"] {
        assert_eq!(row(&b, k).outcome, "done", "{k}");
    }
    assert_eq!((b.done, b.failed, b.blocked), (4, 1, 3));
    let (ok, ev) = bench::readiness(&h, &Gate::closed("test"), Some(&b));
    assert!(!ok);
    assert!(
        ev.contains("left undone") && ev.contains("not done at 100%"),
        "{ev}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_scripted_score_below_100_fails_the_gate_entry() {
    let dir = tmp("pct");
    let (mut l, _) = Ledger::open(&dir.join("ledger.jsonl")).unwrap();
    let h = hermes(None);
    let mut fake = Fake::default();
    fake.pct
        .insert("--bench vending --agent scripted".into(), 97);
    let b = bench::run(
        &h,
        &Gate::closed("test"),
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        Arc::new(fake),
        &cfg(&dir.join("out")),
    )
    .unwrap();
    assert_eq!(row(&b, "S1.3").pct, Some(97));
    assert!(!b.all_100("scripted"));
    assert!(b.spoken.contains("fell short"), "{}", b.spoken);
    let (ok, ev) = bench::readiness(&h, &Gate::closed("test"), Some(&b));
    assert!(!ok && ev.contains("vending/scripted 97%"), "{ev}");
    let (ok, ev) = bench::readiness(&h, &Gate::closed("test"), None);
    assert!(!ok && ev.contains("no bench run recorded"), "{ev}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn operator_intake_of_a_live_run_is_refused_and_never_executed() {
    let dir = tmp("intake");
    let lp = dir.join("ledger.jsonl");
    let (mut l, _) = Ledger::open(&lp).unwrap();
    for title in [
        "`hermes-bench run --bench banking --agent live` (operator asks for a live run)",
        "`hermes-bench run --bench vending --agent scripted --seed 7 --days 10` (operator seed)",
        "`hermes-bench run --bench banking --agent scripted --bank-url http://198.51.100.7` (remote bank)",
    ] {
        let h = hermes(Some(&lp));
        let t = h.intake_task("bench", title, None, Vec::new()).unwrap();
        l.append(
            NewEvent::new("intake")
                .task(&t.key, &t.sector)
                .state("queued")
                .detail(serde_json::to_string(&t).unwrap()),
        )
        .unwrap();
    }
    drop(l);
    let h = hermes(Some(&lp));
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let fake = Arc::new(Fake::default());
    let mut c = cfg(&dir.join("out"));
    let keys: Vec<String> = bench::matrix(&h).into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys.len(), 11, "{keys:?}");
    let intake: Vec<&String> = keys.iter().filter(|k| !k.contains(":S")).collect();
    c.only = intake.iter().map(|k| (*k).clone()).collect();
    let b = bench::run(
        &h,
        &Gate::closed("test"),
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake.clone(),
        &c,
    )
    .unwrap();
    assert_eq!((b.done, b.refused), (1, 2), "{b:#?}");
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].0.contains("--seed 7 --days 10"), "{}", calls[0].0);
    assert!(calls
        .iter()
        .all(|c| !c.0.contains("live") && !c.0.contains("url")));
    let refused: Vec<&bench::Row> = b.rows.iter().filter(|r| r.outcome == "refused").collect();
    assert!(refused.iter().any(|r| r.detail.contains("paid model")));
    assert!(refused.iter().any(|r| r.detail.contains("--bank-url")));
    // The gate entry names the refused intake rows.
    let (ok, ev) = bench::readiness(&h, &Gate::closed("test"), Some(&b));
    assert!(!ok && ev.contains("refused rows"), "{ev}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_exec_kills_on_timeout() {
    let dir = tmp("timeout");
    let e = ProcessExec {
        bin: PathBuf::from("/bin/sleep"),
    };
    let t0 = Instant::now();
    let r = e.exec(
        &["5".into()],
        &dir.join("sleep"),
        Duration::from_millis(300),
    );
    assert!(r.timed_out, "{r:?}");
    assert!(t0.elapsed() < Duration::from_secs(3));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn process_exec_passes_only_the_allowed_environment() {
    let dir = tmp("env");
    // A harmless marker standing in for a secret; never a real value.
    std::env::set_var("HERMES_BENCH_TEST_MARKER", "must-not-reach-child");
    let e = ProcessExec {
        bin: PathBuf::from("/usr/bin/env"),
    };
    let r = e.exec(&[], &dir.join("env"), Duration::from_secs(5));
    assert_eq!(r.exit, Some(0), "{r:?}");
    let text = std::fs::read_to_string(dir.join("env.out")).unwrap();
    let keys: Vec<&str> = text.lines().filter_map(|l| l.split('=').next()).collect();
    assert!(!keys.is_empty());
    for k in &keys {
        assert!(bench::CHILD_ENV.contains(k), "child saw {k}");
    }
    assert!(!text.contains("must-not-reach-child"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn spec_round_trips_through_json() {
    let s = Spec::parse_command("hermes-bench run --bench banking --agent mock-llm").unwrap();
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["kind"], "run");
    assert_eq!(serde_json::from_value::<Spec>(v).unwrap(), s);
}

#[test]
fn only_full_runs_become_last_and_carry_earlier_scores_forward() {
    let dir = tmp("carry");
    let state = dir.join("state");
    let lp = dir.join("ledger.jsonl");
    let gate = Gate::closed("test");
    let fake = Arc::new(Fake::default());
    // Run 1: the mock-LLM vending run crashes on both attempts.
    fake.fail
        .lock()
        .unwrap()
        .insert("--bench vending --agent mock-llm".into(), 2);
    let h = hermes(None);
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let mut first = bench::run(
        &h,
        &gate,
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake.clone(),
        &cfg(&dir.join("out")),
    )
    .unwrap();
    drop(l);
    assert_eq!((first.done, first.failed, first.blocked), (6, 1, 1));
    assert_eq!(
        bench::save(&state, &mut first).unwrap(),
        bench::last_path(&state)
    );
    let (ok, ev) = bench::readiness(&h, &gate, bench::load_last(&state).unwrap().as_ref());
    assert!(
        !ok && ev.contains("left undone: bench:S2.3, bench:S3.2"),
        "{ev}"
    );

    // Run 2 (full, no rerun): only the failed and blocked rows execute; the
    // six done rows come back already-done and keep run 1's scores.
    let h = hermes(Some(&lp));
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let calls_before = fake.calls.lock().unwrap().len();
    let mut second = bench::run(
        &h,
        &gate,
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake.clone(),
        &cfg(&dir.join("out")),
    )
    .unwrap();
    drop(l);
    assert_eq!(fake.calls.lock().unwrap().len() - calls_before, 2);
    assert_eq!((second.done, second.failed, second.blocked), (2, 0, 0));
    assert!(!second.all_100("scripted"), "no pct before the carry");
    assert_eq!(
        bench::save(&state, &mut second).unwrap(),
        bench::last_path(&state)
    );
    let last = bench::load_last(&state).unwrap().unwrap();
    assert_eq!(last.run, second.run);
    let s11 = row(&last, "S1.1");
    assert_eq!((s11.outcome.as_str(), s11.pct), ("already-done", Some(100)));
    assert!(s11.detail.contains(&first.run), "{}", s11.detail);
    assert!(last.all_100("scripted") && last.all_100("mock-llm"));
    assert!(
        last.spoken.contains("every scripted run scored"),
        "{}",
        last.spoken
    );
    let (ok, ev) = bench::readiness(&h, &gate, Some(&last));
    assert!(ok, "{ev}");

    // Run 3: a partial re-run (--only) is kept as its own record only.
    let h = hermes(Some(&lp));
    let (mut l, _) = Ledger::open(&lp).unwrap();
    let mut c = cfg(&dir.join("out"));
    c.only = vec!["S1.1".into()];
    c.rerun = true;
    let mut third = bench::run(
        &h,
        &gate,
        &mut l,
        &RetryPolicy(h.reg.retry.clone()),
        fake.clone(),
        &c,
    )
    .unwrap();
    assert!(!third.full);
    assert_eq!(third.done, 1);
    let kept = bench::save(&state, &mut third).unwrap();
    assert_ne!(kept, bench::last_path(&state));
    assert!(kept.is_file());
    assert_eq!(bench::load_last(&state).unwrap().unwrap().run, second.run);
    let _ = std::fs::remove_dir_all(&dir);
}
