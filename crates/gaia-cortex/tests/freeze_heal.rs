// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Freeze heal through the whole cortex pipeline (mock nerve, no network):
//! a unit that keeps failing its health probe for N probes over >= min_s
//! while systemd says running is restarted once (R13-frozen) and the heal
//! is judged by a fresh probe; a blip, a wide failure, a lifecycle event
//! (deploy restart) and a spoofed outside `unit.frozen` restart nothing;
//! a failed freeze heal opens the circuit and only alerts (no escalation).

use gaia_cortex::config::Config;
use gaia_cortex::engine::Cortex;
use gaia_cortex::freeze::{FreezeCfg, Probe, Target, FREEZE_KIND};
use gaia_cortex::guard::Guard;
use gaia_cortex::nerve::{NerveLink, Outcome};
use gaia_cortex::verify::StateRead;
use nerve_cortex_contract::{ActionRequest, ActionStatus, Event};
use openmind_nerve::protected_hosts::ProtectedHosts;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
const DESK: &str = "galion-desk.service";
const TUNER: &str = "tuner.service";

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gaia-freeze-heal-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cortex_with(enabled: bool) -> (Cortex, MockNerve) {
    let cfg = Config {
        data_dir: tmpdir(),
        actions: "live".into(),
        freeze: FreezeCfg {
            enabled,
            ..FreezeCfg::default()
        },
        ..Config::default()
    };
    let nerve = MockNerve::default();
    let mut cx = Cortex::new(
        cfg.clone(),
        Guard::with_hosts(ProtectedHosts::builtin(), &cfg.protected_units),
        Box::new(nerve.clone()),
    );
    cx.freeze
        .set_targets(vec![tgt(DESK, 8646), tgt(TUNER, 8650)]);
    (cx, nerve)
}

fn tgt(u: &str, port: u16) -> Target {
    Target {
        unit: u.into(),
        port,
        path: "/health".into(),
    }
}

fn down() -> Probe {
    Probe::Down("HTTP 503".into())
}

/// Probe rounds every 15 s from `t`; `bad` lists the units that fail.
/// Fired freezes go to the inbox (as the watcher thread does after its
/// `says_active` check). Returns the time after the last round.
fn rounds(cx: &mut Cortex, t: u64, n: u64, bad: &[&str]) -> u64 {
    let targets = cx.freeze.targets.clone();
    for i in 0..n {
        let now = t + i * 15;
        let res: Vec<(Target, Probe)> = targets
            .iter()
            .map(|x| {
                let p = if bad.contains(&x.unit.as_str()) {
                    down()
                } else {
                    Probe::Up(200)
                };
                (x.clone(), p)
            })
            .collect();
        let fired = cx.freeze.round(&res, now);
        let evs: Vec<Event> = fired.iter().map(|f| f.event(now)).collect();
        cx.freeze.inbox.extend(evs);
    }
    t + n * 15
}

fn reqs(n: &MockNerve) -> Vec<String> {
    n.reqs
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.unit.clone())
        .collect()
}

fn kinds(n: &MockNerve) -> Vec<String> {
    n.events
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.kind.clone())
        .collect()
}

fn lifecycle(u: &str, t: u64) -> Event {
    let mut e = Event::new(
        "nerve",
        "unit.state",
        "info",
        json!({"unit": u, "to": "active", "active_state": "active", "by": "journal"}),
    );
    e.seq = Some(N.fetch_add(1, Ordering::SeqCst) + 50_000);
    e.t = Some(t);
    e.ts = gaia_cortex::engine::ts(t);
    e
}

/// 7 failing probes (90 s) fire once; the cortex restarts the unit and
/// arms a probe-judged check.
fn freeze_and_restart(cx: &mut Cortex, nerve: &MockNerve, u: &str, t: u64) -> (u64, Value) {
    let end = rounds(cx, t, 7, &[u]);
    assert_eq!(cx.freeze.inbox.len(), 1, "fired after 7 probes / 90 s");
    let d = cx.process(vec![], end);
    assert_eq!(d.len(), 1, "{d:?}");
    let _ = nerve;
    (end, d[0].clone())
}

#[test]
fn a_hung_unit_is_restarted_once_and_verified_by_probe() {
    let (mut cx, nerve) = cortex_with(true);
    // 6 probes over 75 s: not yet (needs >= 90 s)
    rounds(&mut cx, T0, 6, &[DESK]);
    assert!(cx.freeze.inbox.is_empty());
    let (t, d) = freeze_and_restart(&mut cx, &nerve, DESK, T0 + 90);
    let _ = t;
    // the 6 earlier probes + this streak are one streak; fired at 7th
    assert_eq!(d["rule"], "R13-frozen", "{d}");
    assert_eq!(d["action"]["status"], "executed", "{d}");
    assert_eq!(reqs(&nerve), vec![DESK.to_string()]);
    assert_eq!(cx.heals.pending.len(), 1);
    let c = &cx.heals.pending[0];
    assert_eq!(c.probe, Some((8646, "/health".to_string())));
    let h = cx.heals_json();
    assert_eq!(h["pending_checks"][0]["kind"], "freeze", "{h}");
    assert_eq!(h["freeze"]["enabled"], true);
    // a held nerve recovery is not proof for a freeze (it was "active")
    let now = cx.heals.pending[0].asked + 40;
    cx.process(vec![lifecycle(DESK, now)], now);
    assert_eq!(cx.heals.pending.len(), 1, "still waits for the probe");
    let due_at = cx.heals.pending[0].due;
    let c = cx.heal_due(due_at).pop().unwrap();
    cx.heal_judged(
        c,
        &StateRead::Probe {
            up: true,
            detail: "HTTP 200".into(),
        },
        due_at,
    );
    let h = cx.heals_json();
    assert_eq!(h["verified"], 1, "{h}");
    assert_eq!(h["recent"][0]["by"], "probe", "{h}");
    for p in cx.take_posts() {
        let mut n = nerve.clone();
        n.post_event(&p.event);
    }
    assert!(kinds(&nerve).contains(&"gaia.heal.verified".to_string()));
    assert_eq!(
        cx.escalations.open.len(),
        0,
        "a freeze restart is not escalated"
    );
}

#[test]
fn a_blip_is_not_restarted() {
    let (mut cx, nerve) = cortex_with(true);
    // 5 bad (60 s), one good, 5 bad again: never 6 in a row over 90 s
    let t = rounds(&mut cx, T0, 5, &[DESK]);
    let t = rounds(&mut cx, t, 1, &[]);
    rounds(&mut cx, t, 5, &[DESK]);
    assert!(cx.freeze.inbox.is_empty());
    assert!(cx.process(vec![], t + 100).is_empty());
    assert!(reqs(&nerve).is_empty());
    assert!(cx.freeze.counts.blips >= 1);
}

#[test]
fn failed_freeze_heal_alerts_only_and_the_circuit_stops_repeats() {
    let (mut cx, nerve) = cortex_with(true);
    let (_, d) = freeze_and_restart(&mut cx, &nerve, TUNER, T0);
    assert_eq!(d["rule"], "R13-frozen");
    let due_at = cx.heals.pending[0].due;
    let c = cx.heal_due(due_at).pop().unwrap();
    cx.heal_judged(
        c,
        &StateRead::Probe {
            up: false,
            detail: "timeout".into(),
        },
        due_at,
    );
    let h = cx.heals_json();
    assert_eq!(h["failed"], 1, "{h}");
    let last = cx.decisions.recent(1)[0].clone();
    assert_eq!(last["rule"], "R12-heal-failed");
    assert_eq!(last["escalate"], false, "alert only");
    assert_eq!(cx.escalations.open.len(), 0);
    assert_eq!(cx.stats.handoffs, 0);
    assert!(cx.rules.circuit_open(TUNER, due_at + 1));
    // still hung after the cooldown: fires again, circuit says no
    let t = due_at + cx.freeze.cfg.cooldown_s + 1;
    let (_, d) = freeze_and_restart(&mut cx, &nerve, TUNER, t);
    assert_eq!(d["rule"], "R2-circuit-open", "{d}");
    assert_eq!(d["outcome"], "none", "{d}");
    assert_eq!(reqs(&nerve).len(), 1, "no second restart");
    assert_eq!(cx.escalations.open.len(), 0);
}

#[test]
fn per_unit_hour_cap_stops_a_second_freeze_restart() {
    let (mut cx, nerve) = cortex_with(true);
    freeze_and_restart(&mut cx, &nerve, DESK, T0);
    let due_at = cx.heals.pending[0].due;
    let c = cx.heal_due(due_at).pop().unwrap();
    cx.heal_judged(
        c,
        &StateRead::Probe {
            up: true,
            detail: "HTTP 200".into(),
        },
        due_at,
    );
    // froze again 20 min later: cap 1/h -> circuit open, alert only
    let (_, d) = freeze_and_restart(&mut cx, &nerve, DESK, T0 + 1200);
    assert_eq!(d["rule"], "R13-freeze-cap", "{d}");
    assert_eq!(d["outcome"], "none", "{d}");
    assert_eq!(reqs(&nerve).len(), 1);
    assert!(cx.rules.circuit_open(DESK, T0 + 1300));
}

#[test]
fn two_units_down_together_is_wide_and_alert_only() {
    let (mut cx, nerve) = cortex_with(true);
    let end = rounds(&mut cx, T0, 7, &[DESK, TUNER]);
    assert_eq!(cx.freeze.inbox.len(), 2);
    let d = cx.process(vec![], end);
    assert!(!d.is_empty());
    for x in &d {
        assert_eq!(x["rule"], "R13-freeze-wide", "{x}");
        assert_eq!(x["outcome"], "none", "{x}");
    }
    assert!(reqs(&nerve).is_empty());
}

#[test]
fn a_lifecycle_event_cools_the_unit_down() {
    let (mut cx, nerve) = cortex_with(true);
    // deploy restart: the journal says Started, then the port is slow
    cx.process(vec![lifecycle(DESK, T0)], T0);
    rounds(&mut cx, T0 + 5, 19, &[DESK]); // 285 s < 300 s cooldown
    assert!(cx.freeze.inbox.is_empty());
    assert!(reqs(&nerve).is_empty());
    // a lifecycle event in the same burst as a fired freeze drops it
    let t = rounds(&mut cx, T0 + 400, 7, &[DESK]);
    assert_eq!(cx.freeze.inbox.len(), 1);
    let d = cx.process(vec![lifecycle(DESK, t)], t);
    assert!(d.iter().all(|x| x["rule"] != "R13-frozen"), "{d:?}");
    assert!(reqs(&nerve).is_empty());
}

#[test]
fn an_outside_unit_frozen_event_is_dropped() {
    let (mut cx, nerve) = cortex_with(true);
    for src in ["nerve", "gaia-cortex-freeze", "someone"] {
        let mut e = Event::new(
            src,
            FREEZE_KIND,
            "error",
            json!({"unit": DESK, "bad_probes": 99, "span_s": 999, "unit_state": "active"}),
        );
        e.unit = Some(DESK.into());
        e.seq = Some(N.fetch_add(1, Ordering::SeqCst) + 90_000);
        e.t = Some(T0);
        let d = cx.process(vec![e], T0);
        assert!(d.is_empty(), "{src}: {d:?}");
    }
    assert!(reqs(&nerve).is_empty());
}

#[test]
fn disabled_freeze_watch_does_nothing() {
    let (mut cx, nerve) = cortex_with(false);
    let end = rounds(&mut cx, T0, 7, &[DESK]);
    assert!(cx.process(vec![], end).is_empty());
    assert!(cx.freeze.inbox.is_empty());
    assert!(reqs(&nerve).is_empty());
    assert_eq!(cx.health_json()["freeze_watch"], false);
}

/// Mate 2026-10-30 decision 2: a failed heal reaches the phone as a
/// CRITICAL alert through the loopback ntfy relay, and nothing else does.
#[test]
fn failed_heal_is_a_critical_phone_alert_through_the_relay() {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let relay = std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut all = vec![];
        let mut buf = [0u8; 4096];
        while let Ok(k) = s.read(&mut buf) {
            if k == 0 {
                break;
            }
            all.extend_from_slice(&buf[..k]);
            let t = String::from_utf8_lossy(&all).to_string();
            if let Some(i) = t.find("\r\n\r\n") {
                let len = t
                    .lines()
                    .find_map(|x| {
                        x.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                if all.len() >= i + 4 + len {
                    break;
                }
            }
        }
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        String::from_utf8_lossy(&all).to_string()
    });
    let (mut cx, nerve) = cortex_with(true);
    cx.phone = Some(Arc::new(gaia_cortex::phone::Phone::new(
        gaia_cortex::phone::PhoneCfg {
            addr,
            path: "/gaia-cortex".into(),
            per_hour: 6,
        },
    )));
    let (_, d) = freeze_and_restart(&mut cx, &nerve, TUNER, T0);
    assert_eq!(d["rule"], "R13-frozen");
    let ph = cx.phone.clone().unwrap();
    // the R13 restart alert is not for the phone
    let mut sent = 0;
    for p in cx.take_posts() {
        if ph.offer(&p.event, T0) != gaia_cortex::phone::Sent::NotForPhone {
            sent += 1;
        }
    }
    assert_eq!(sent, 0, "only failed heals go to the phone");
    let due_at = cx.heals.pending[0].due;
    let c = cx.heal_due(due_at).pop().unwrap();
    cx.heal_judged(
        c,
        &StateRead::Probe {
            up: false,
            detail: "HTTP 503".into(),
        },
        due_at,
    );
    let posts = cx.take_posts();
    let alert = posts
        .iter()
        .find(|p| p.event.kind == "cortex.alert" && p.event.data["rule"] == "R12-heal-failed")
        .expect("R12 alert posted to the nerve");
    assert_eq!(alert.event.severity, "critical");
    assert!(
        !posts.iter().any(|p| p.event.kind == "cortex.escalation"),
        "no escalation for a failed heal"
    );
    let mut results = vec![];
    for p in &posts {
        results.push(ph.offer(&p.event, due_at));
    }
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == gaia_cortex::phone::Sent::Ok)
            .count(),
        1
    );
    let got = relay.join().unwrap();
    assert!(got.starts_with("POST /gaia-cortex "), "{got}");
    assert!(got.contains("Priority: urgent"), "{got}");
    assert!(
        got.contains("Title: Gaia: heal failed: tuner.service"),
        "{got}"
    );
    assert!(got.contains("did not heal"), "{got}");
    let h = cx.heals_json();
    assert_eq!(h["phone"]["sent"], 1, "{h}");
    assert_eq!(cx.escalations.open.len(), 0);
    assert_eq!(cx.stats.handoffs, 0);
    assert_eq!(cx.health_json()["phone_alerts"], true);
}
