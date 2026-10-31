// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Owner 2026-10-31 (swarm/watchdog-notify-merge-20261031):
//! * one problem counts as one: a `.path`/`.timer`/`.socket` folds into its
//!   service, never causes a restart; R4 escalates once per circuit; one
//!   handoff per root unit per hour. Friday replay (2026-10-30 16:09:40 CET,
//!   the self-modify gate .path + .service: live = 2 incidents, 2 paid
//!   handoffs) must give 1 incident and 1 handoff.
//! * `CORTEX_ACTION_MODE`: notify (default) never acts and never hands off,
//!   pages >= error once per incident; approve_each parks restarts/handoffs
//!   until the owner approves over the loopback API (bearer token).
//!
//! Mock nerve, mock relay, no network beyond 127.0.0.1, no model.

use gaia_cortex::config::{ActionMode, Config};
use gaia_cortex::engine::Cortex;
use gaia_cortex::guard::Guard;
use gaia_cortex::nerve::{NerveLink, Outcome};
use gaia_cortex::phone::{Phone, PhoneCfg, Sent};
use nerve_cortex_contract::{parse_events, ActionRequest, ActionStatus, Event};
use openmind_nerve::protected_hosts::ProtectedHosts;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FRIDAY: &str = include_str!("fixtures/friday-20261030-self-modify-gate.json");
/// 2026-10-30 16:09:40 CET
const T: u64 = 1_793_372_980;

#[derive(Clone, Default)]
struct MockNerve {
    reqs: Arc<Mutex<Vec<ActionRequest>>>,
    events: Arc<Mutex<Vec<Event>>>,
}

impl NerveLink for MockNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        self.reqs.lock().unwrap().push(req.clone());
        Outcome {
            status: if req.dry_run {
                ActionStatus::Simulated
            } else {
                ActionStatus::Executed
            },
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

static N: AtomicU64 = AtomicU64::new(0);

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "gaia-cortex-nm-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg(mode: ActionMode, actions: &str, hermes: bool) -> Config {
    let mut c = Config {
        data_dir: tmpdir(),
        ..Config::default()
    };
    c.actions = actions.into();
    c.action_mode = mode;
    c.hermes.enabled = hermes;
    c.validate().unwrap();
    c
}

fn cortex(c: Config) -> (Cortex, MockNerve) {
    let nerve = MockNerve::default();
    let cx = Cortex::new(
        c.clone(),
        Guard::with_hosts(ProtectedHosts::builtin(), &c.protected_units),
        Box::new(nerve.clone()),
    );
    (cx, nerve)
}

fn friday() -> Vec<Event> {
    parse_events(FRIDAY)
        .into_iter()
        .map(Result::unwrap)
        .collect()
}

fn inbox_files(c: &Config) -> usize {
    std::fs::read_dir(c.data_dir.join("hermes-inbox"))
        .map(|r| {
            r.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
                .count()
        })
        .unwrap_or(0)
}

fn wire(seq: u64, kind: &str, sev: &str, unit: &str, data: Value) -> Event {
    let l = json!({"v": 1, "seq": seq, "ts": "2026-10-30T16:09:41+01:00", "t": T + 1, "source": "journal",
                   "kind": kind, "severity": sev, "unit": unit, "group": "hermes",
                   "key": format!("{kind}:{unit}"), "msg": "m", "data": data, "count": 1});
    parse_events(&l.to_string()).pop().unwrap().unwrap()
}

#[test]
fn friday_replay_path_and_service_is_one_incident_one_handoff() {
    let c = cfg(ActionMode::Auto, "live", true);
    let (mut cx, nerve) = cortex(c.clone());
    let evs = friday();
    assert_eq!(
        evs.len(),
        7,
        "fixture: the 7 events of hoff-1793372981-34 + -35"
    );
    let d = cx.process(evs, T + 1);
    let esc: Vec<&Value> = d.iter().filter(|d| d["escalate"] == true).collect();
    assert_eq!(esc.len(), 1, "one escalation: {d:#?}");
    assert_eq!(esc[0]["subject"], "unit:hermes-self-modify-gate.service");
    assert_eq!(
        cx.correlator.open.len(),
        1,
        "one incident: {:?}",
        cx.correlator.open.keys()
    );
    assert_eq!(inbox_files(&c), 1, "one handoff file");
    assert_eq!(cx.stats.handoffs, 1);
    assert!(
        nerve.reqs.lock().unwrap().is_empty(),
        "no restart (not allowlisted, trigger folded)"
    );
    // The same .path failure 1 s later in its own batch: no new escalation, no new file.
    let again = wire(
        40,
        "unit.failed",
        "high",
        "hermes-self-modify-gate.path",
        json!({"unit_result": "unit-start-limit-hit"}),
    );
    let d = cx.process(vec![again], T + 2);
    assert!(d.iter().all(|d| d["escalate"] == false), "{d:#?}");
    assert_eq!(inbox_files(&c), 1);
    assert_eq!(cx.stats.escalations, 1);
    assert_eq!(cx.correlator.open.len(), 1);
}

#[test]
fn trigger_failure_never_restarts_its_allowlisted_service() {
    let c = cfg(ActionMode::Auto, "live", false);
    let (mut cx, nerve) = cortex(c);
    let d = cx.process(
        vec![wire(
            1,
            "unit.failed",
            "high",
            "tuner.timer",
            json!({"unit_result": "resources"}),
        )],
        T,
    );
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["subject"], "unit:tuner.service");
    assert_eq!(d[0]["rule"], "R4-trigger-failed");
    assert_eq!(d[0]["outcome"], "none");
    assert_eq!(d[0]["escalate"], true);
    let d = cx.process(
        vec![wire(
            2,
            "unit.failed",
            "high",
            "tuner.timer",
            json!({"unit_result": "resources"}),
        )],
        T + 300,
    );
    assert_eq!(d[0]["rule"], "R4-trigger-failed");
    assert_eq!(d[0]["escalate"], false, "once per circuit");
    assert!(
        nerve.reqs.lock().unwrap().is_empty(),
        "a trigger never causes a restart"
    );
    // the service's own crash is still the service's (circuit now open: no restart either)
    let d = cx.process(
        vec![wire(
            3,
            "unit.failed",
            "high",
            "tuner.service",
            json!({"unit_result": "exit-code"}),
        )],
        T + 600,
    );
    assert_eq!(d[0]["rule"], "R2-circuit-open");
}

#[test]
fn r4_unknown_unit_escalates_once_per_circuit() {
    let c = cfg(ActionMode::Auto, "live", true);
    let (mut cx, _) = cortex(c.clone());
    for (i, t) in [0u64, 300, 700].iter().enumerate() {
        cx.process(
            vec![wire(
                10 + i as u64,
                "unit.failed",
                "high",
                "cortex-sbx-x.service",
                json!({"unit_result": "exit-code"}),
            )],
            T + t,
        );
    }
    assert_eq!(cx.stats.escalations, 1);
    assert_eq!(inbox_files(&c), 1);
}

#[test]
fn ntfy_relay_instances_are_one_handoff_per_hour() {
    let c = cfg(ActionMode::Auto, "live", true);
    let (mut cx, _) = cortex(c.clone());
    for i in 0..5u64 {
        let u = format!(
            "galion-ntfy-relay@{}-127.0.0.1:2586-127.0.0.1:{}.service",
            30 + i,
            50000 + i
        );
        cx.process(
            vec![wire(
                100 + i,
                "unit.failed",
                "high",
                &u,
                json!({"unit_result": "exit-code"}),
            )],
            T + i * 600,
        );
    }
    assert_eq!(cx.stats.escalations, 5, "each instance is its own unit");
    assert_eq!(inbox_files(&c), 1, "one handoff for the template");
    assert_eq!(cx.stats.handoffs_same_root, 4);
}

/// Loopback "relay" answering 200 to every request; returns the request heads.
fn relay() -> (std::net::SocketAddr, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    let got: Arc<Mutex<Vec<String>>> = Arc::default();
    let g = got.clone();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
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
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if all.len() >= i + 4 + len {
                        break;
                    }
                }
            }
            let _ =
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
            g.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&all).to_string());
        }
    });
    (a, got)
}

#[test]
fn notify_mode_is_default_never_acts_never_hands_off_and_pages_once() {
    assert_eq!(Config::default().action_mode, ActionMode::Notify);
    // even with actions = live (the VPS today) and the handoff on
    let c = cfg(ActionMode::Notify, "live", true);
    assert!(c.dry_run());
    let (mut cx, nerve) = cortex(c.clone());
    let d = cx.process(
        vec![wire(
            1,
            "unit.failed",
            "high",
            "tuner.service",
            json!({"unit_result": "exit-code"}),
        )],
        T,
    );
    assert_eq!(d[0]["rule"], "R3-known-crash");
    assert!(nerve.reqs.lock().unwrap()[0].dry_run, "simulated only");
    cx.process(friday(), T + 10);
    assert_eq!(cx.stats.escalations, 1);
    assert_eq!(inbox_files(&c), 0, "notify never hands off");
    assert_eq!(cx.stats.handoffs_notify_only, 1);
    assert_eq!(cx.health_json()["action_mode"], "notify");
    // pages: everything the cortex posted, through the phone
    let (addr, got) = relay();
    let p = Phone::new(PhoneCfg {
        addr,
        path: "/gaia-cortex".into(),
        per_hour: 6,
        page_all: true,
    });
    let posts = nerve.events.lock().unwrap().clone();
    let sent: Vec<Sent> = posts.iter().map(|e| p.offer(e, T + 10)).collect();
    let ok = sent.iter().filter(|s| **s == Sent::Ok).count();
    // two incidents (tuner crash, Friday gate) = two pages; Friday's
    // alert + escalation of the same incident = one page, the second is a duplicate
    assert_eq!(ok, 2, "{sent:?}");
    assert_eq!(
        sent.iter().filter(|s| **s == Sent::Duplicate).count(),
        posts.len() - 2
    );
    assert!(sent.contains(&Sent::Duplicate));
    let t0 = Instant::now();
    while got.lock().unwrap().is_empty() && t0.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(20));
    }
    while got.lock().unwrap().len() < 2 && t0.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let g = got.lock().unwrap();
    assert!(
        g.iter()
            .any(|r| r.contains("hermes-self-modify-gate.service")),
        "{g:?}"
    );
}

#[test]
fn approve_each_parks_then_runs_only_what_the_owner_approved() {
    let c = cfg(ActionMode::ApproveEach, "live", true);
    let (mut cx, nerve) = cortex(c.clone());
    let d = cx.process(
        vec![wire(
            1,
            "unit.failed",
            "high",
            "tuner.service",
            json!({"unit_result": "exit-code"}),
        )],
        T,
    );
    assert_eq!(d[0]["outcome"], "none", "parked, not run");
    assert!(nerve.reqs.lock().unwrap().is_empty());
    let pend = cx.approvals.to_json(T)["pending"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(pend.len(), 1);
    assert_eq!(
        (pend[0]["action"].as_str(), pend[0]["unit"].as_str()),
        (Some("restart"), Some("tuner.service"))
    );
    let id = pend[0]["id"].as_str().unwrap().to_string();
    // the page carries the approval id and is >= error
    let al = nerve
        .events
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.kind == "cortex.alert")
        .cloned()
        .unwrap();
    assert_eq!(al.data["approval"], id.as_str());
    assert_eq!(al.severity, "error");
    // persisted
    let log = std::fs::read_to_string(c.data_dir.join("pending-approvals.jsonl")).unwrap();
    assert!(log.contains(&id));
    // owner approves: the next batch (even empty) sends one live restart
    cx.approvals
        .decide(&id, gaia_cortex::approvals::Decision::Approve, T + 30)
        .unwrap();
    let d = cx.process(vec![], T + 31);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0]["rule"], "R14-owner-approved");
    let r = nerve.reqs.lock().unwrap().clone();
    assert_eq!(r.len(), 1);
    assert!(!r[0].dry_run, "approved + actions live = live");
    assert_eq!(r[0].unit, "tuner.service");
    // an escalation parks its handoff instead of writing the inbox
    cx.process(friday(), T + 100);
    assert_eq!(inbox_files(&c), 0);
    assert_eq!(cx.stats.handoffs_parked, 1);
    let pend = cx.approvals.to_json(T + 100)["pending"]
        .as_array()
        .unwrap()
        .clone();
    let h = pend.iter().find(|p| p["action"] == "handoff").unwrap();
    // rejected: nothing runs
    cx.approvals
        .decide(
            h["id"].as_str().unwrap(),
            gaia_cortex::approvals::Decision::Reject,
            T + 120,
        )
        .unwrap();
    cx.process(vec![], T + 121);
    assert_eq!(inbox_files(&c), 0);
    assert_eq!(nerve.reqs.lock().unwrap().len(), 1);
}

#[test]
fn approve_each_with_dry_run_actions_only_simulates() {
    let c = cfg(ActionMode::ApproveEach, "dry-run", false);
    let (mut cx, nerve) = cortex(c);
    cx.process(
        vec![wire(
            1,
            "unit.failed",
            "high",
            "tuner.service",
            json!({"unit_result": "exit-code"}),
        )],
        T,
    );
    let id = cx.approvals.to_json(T)["pending"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    cx.approvals
        .decide(&id, gaia_cortex::approvals::Decision::Approve, T + 1)
        .unwrap();
    cx.process(vec![], T + 2);
    assert!(nerve.reqs.lock().unwrap()[0].dry_run);
}

// ------------------------------------------------------------ real binary: /approvals API

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http(port: u16, method: &str, path: &str, auth: Option<&str>, body: &str) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    // (name, value) like dispatch.rs builds its headers
    let a = auth
        .map(|t| ("Authorization", format!("Bearer {t}")))
        .map(|(n, v)| format!("{n}: {v}\r\n"))
        .unwrap_or_default();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\n{a}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let b = out.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    (code, serde_json::from_str(b).unwrap_or(Value::Null))
}

fn wait_for(f: &dyn Fn() -> bool, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn spawn(port: u16, dir: &std::path::Path, mode: &str, token: Option<&std::path::Path>) -> Kill {
    let mut c = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"));
    c.env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
        .env("CORTEX_DATA_DIR", dir)
        .env("CORTEX_COALESCE_MS", "100")
        .env("CORTEX_ACTION_MODE", mode)
        .env_remove("CORTEX_CONFIG")
        .env_remove("CORTEX_LLM_ENABLED")
        .env_remove("CORTEX_NERVE_URL")
        .env_remove("CORTEX_ACTIONS")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(t) = token {
        c.env("CORTEX_APPROVAL_TOKEN_FILE", t);
    }
    let k = Kill(c.spawn().unwrap());
    assert!(wait_for(
        &|| TcpStream::connect(("127.0.0.1", port)).is_ok(),
        10
    ));
    k
}

#[test]
fn binary_approvals_api_needs_mode_and_token() {
    let dir = tmpdir();
    let tok = dir.join("approval.token");
    std::fs::write(&tok, "test-token-0123456789abcdef\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    // default mode (notify): health says so; approvals cannot be decided
    let port = free_port();
    {
        let _k = spawn(port, &dir.join("n"), "notify", Some(&tok));
        let (_, h) = http(port, "GET", "/health", None, "");
        assert_eq!(h["action_mode"], "notify");
        assert_eq!(h["mode"], "dry-run");
        let (code, _) = http(
            port,
            "POST",
            "/approvals/apr-1/approve",
            Some("test-token-0123456789abcdef"),
            "",
        );
        assert_eq!(code, 409);
    }
    let port = free_port();
    let _k = spawn(port, &dir.join("a"), "approve_each", Some(&tok));
    let crash = r#"{"source":"journal","kind":"unit.failed","severity":"high","unit":"tuner.service","seq":7}"#;
    let (code, _) = http(port, "POST", "/v1/events", None, crash);
    assert_eq!(code, 202);
    assert!(wait_for(
        &|| http(port, "GET", "/approvals", None, "").1["pending"]
            .as_array()
            .is_some_and(|a| a.len() == 1),
        5
    ));
    let (_, a) = http(port, "GET", "/approvals", None, "");
    let id = a["pending"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(a["action_mode"], "approve_each");
    let path = format!("/approvals/{id}/approve");
    assert_eq!(http(port, "POST", &path, None, "").0, 401);
    assert_eq!(
        http(
            port,
            "POST",
            &path,
            Some("wrong-token-0123456789abcdef"),
            ""
        )
        .0,
        401
    );
    let (code, r) = http(port, "POST", &path, Some("test-token-0123456789abcdef"), "");
    assert_eq!(code, 200, "{r}");
    assert_eq!(r["decision"], "approved");
    assert_eq!(
        http(port, "POST", &path, Some("test-token-0123456789abcdef"), "").0,
        404,
        "decided once"
    );
    assert!(wait_for(
        &|| http(port, "GET", "/decisions", None, "").1[0]["rule"] == "R14-owner-approved",
        5
    ));
    let (_, d) = http(port, "GET", "/decisions", None, "");
    assert_eq!(
        d[0]["action"]["request"]["dry_run"], true,
        "actions dry-run: simulated"
    );
    // no token file configured: 403
    let port2 = free_port();
    let _k2 = spawn(port2, &dir.join("b"), "approve_each", None);
    assert_eq!(
        http(
            port2,
            "POST",
            "/approvals/x/approve",
            Some("test-token-0123456789abcdef"),
            ""
        )
        .0,
        403
    );
    let _ = std::fs::remove_dir_all(&dir);
}
