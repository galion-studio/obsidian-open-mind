// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 175: `gaia heal-tick` under Mate's accepted decisions (2026-10-21),
//! hermetic, against an in-process fake nerve whose `GET /state` reports
//! restarts for the target:
//! * D-L2: the nerve's restarts count, so Gaia + nerve stays at 3 per hour;
//!   an unreadable `/state` fails closed before any token is consumed;
//! * D-L4: a service that holds its port and never answers is reported as
//!   `gaia.heal.hung` (with the listener pid), never restarted or killed,
//!   and no token is consumed.
//!
//! The only process started is `/bin/true` (exits at once). The real
//! binaries and dummies are in real_gaia_decisions.rs (GALION_REAL=1).

use gaia::live_heal::{exit, heal_plan, heal_tick, HealCfg};
use gaia_live::heal::{Ledger, TestTarget};
use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::approval::unix_now;
use hades_gate::token::{plan_hash, Claims};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
struct Nerve {
    log: Arc<Mutex<Vec<Value>>>,
    /// (HTTP status, body) answered for GET /state.
    state: Arc<Mutex<(u16, String)>>,
}

impl Nerve {
    fn set_state(&self, code: u16, body: Value) {
        *self.state.lock().unwrap() = (code, body.to_string());
    }
    fn restarts(&self, svc: &str, n: u64) {
        self.set_state(
            200,
            json!({"cycle": 7, "components": [
                {"name": "console", "unit": "console.service", "restarts_1h": 0},
                {"name": svc, "unit": format!("{svc}.service"), "restarts_1h": n}],
                "reflex": {"budgets": {format!("{svc}.service"): {"dry_1h": 0, "live_1h": n}}}}),
        );
    }
    fn kinds(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|e| e["kind"].as_str().unwrap_or("").to_string())
            .collect()
    }
    fn last(&self) -> Value {
        self.log.lock().unwrap().last().cloned().unwrap()
    }
}

fn fake_nerve() -> (SocketAddr, Nerve) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let n = Nerve::default();
    n.set_state(200, json!({"cycle": 1, "components": []}));
    let n2 = n.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut first = String::new();
            r.read_line(&mut first).unwrap();
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let (code, resp) = if first.starts_with("GET /health") {
                (200, r#"{"status":"ok"}"#.to_string())
            } else if first.starts_with("GET /state") {
                n2.state.lock().unwrap().clone()
            } else if first.starts_with("POST /event") {
                let mut v: Value = serde_json::from_slice(&body).unwrap();
                v["ts"] = Value::from("2026-10-21T08:30:00+02:00");
                n2.log.lock().unwrap().push(v);
                (200, r#"{"ok":true}"#.to_string())
            } else {
                (
                    200,
                    Value::Array(n2.log.lock().unwrap().clone()).to_string(),
                )
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                resp.len()
            );
        }
    });
    (addr, n)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gaia-decisions-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn prog() -> String {
    ["/bin/true", "/usr/bin/true"]
        .iter()
        .find(|p| Path::new(p).is_file())
        .unwrap()
        .to_string()
}

/// A port in slot 175's band 20450-20459 with nothing listening. Tests in this binary run
/// in parallel: the hung test takes 20458-20459, every other one 20450-20457.
fn free_test_port(mut r: std::ops::RangeInclusive<u16>) -> u16 {
    r.find(|p| {
        TcpListener::bind(("127.0.0.1", *p)).is_ok()
            && std::net::TcpStream::connect(("127.0.0.1", *p)).is_err()
    })
    .expect("a free test port")
}

struct Gate {
    dir: PathBuf,
    config: PathBuf,
    seed: PathBuf,
}

impl Gate {
    fn new(dir: &Path) -> Gate {
        let g = dir.join("gate");
        std::fs::create_dir_all(&g).unwrap();
        let (seed, public) = (dir.join("admin.seed"), g.join("admin.pub"));
        hades_gate::keys::generate(&seed, &public).unwrap();
        let config = g.join("hades.toml");
        std::fs::write(
            &config,
            format!(
                "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
                public.display(),
                g.join("ledger.jsonl").display(),
                g.join("DISABLED").display()
            ),
        )
        .unwrap();
        Gate {
            dir: g,
            config,
            seed,
        }
    }
    fn token_file(&self, t: &TestTarget, name: &str) -> PathBuf {
        use std::os::unix::fs::OpenOptionsExt;
        let key = hades_gate::keys::load_seed(&self.seed).unwrap();
        let now = unix_now();
        let tok = Claims {
            action: "heal-apply".into(),
            target: t.name.clone(),
            plan_hash: plan_hash(&gaia_plan_bytes(&heal_plan(t))),
            nonce: hades_gate::keys::os_random::<16>().unwrap(),
            issued_at: now,
            expires_at: now + 300,
        }
        .sign(&key);
        let p = self.dir.parent().unwrap().join(name);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&p)
            .unwrap()
            .write_all(tok.as_bytes())
            .unwrap();
        p
    }
    fn consumed(&self) -> usize {
        std::fs::read_to_string(self.dir.join("ledger.jsonl"))
            .unwrap_or_default()
            .matches("\"event\":\"consumed\"")
            .count()
    }
}

fn cfg(tag: &str, nerve: SocketAddr, port: u16) -> (HealCfg, Gate) {
    let d = tmp(tag);
    let gate = Gate::new(&d);
    let c = HealCfg {
        target: TestTarget {
            name: "dummy-dl".into(),
            port,
            health_path: "/health".into(),
            argv: vec![prog(), "--port".into(), port.to_string()],
        },
        allowed: 20450..=20459,
        nerve,
        state_dir: d.join("state"),
        hades_config: gate.config.clone(),
        token_file: None,
        wait: Duration::from_millis(200),
    };
    (c, gate)
}

#[test]
fn nerve_two_restarts_leave_exactly_one_for_gaia() {
    let (addr, n) = fake_nerve();
    n.restarts("dummy-dl", 2);
    let (mut c, g) = cfg("two", addr, free_test_port(20450..=20457));
    c.token_file = Some(g.token_file(&c.target, "a.tok"));
    let r = heal_tick(&c).unwrap();
    // /bin/true is spawned (the one allowed restart) and never comes up.
    assert_eq!(r.exit, exit::APPLY_FAILED, "{}", r.report.pretty());
    assert!(r.spawned);
    assert_eq!(g.consumed(), 1);
    let failed = n.last();
    assert_eq!(failed["kind"], "gaia.heal.failed");
    assert_eq!(failed["data"]["restarts_last_hour"], 3);
    assert_eq!(failed["data"]["gaia_restarts_last_hour"], 1);
    assert_eq!(failed["data"]["nerve_restarts_last_hour"], 2);
    let text = r.report.compact();
    assert!(text.contains("\"nerve_restarts_last_hour\":2"), "{text}");

    // Second tick, fresh token: 1 Gaia + 2 nerve = 3 -> capped, token kept.
    c.token_file = Some(g.token_file(&c.target, "b.tok"));
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::CAPPED, "{}", r.report.pretty());
    assert_eq!(g.consumed(), 1, "capped before the token is consumed");
    let capped = n.last();
    assert_eq!(capped["kind"], "gaia.heal.capped");
    assert_eq!(capped["data"]["gaia_restarts_last_hour"], 1);
    assert_eq!(capped["data"]["nerve_restarts_last_hour"], 2);
    assert_eq!(capped["data"]["combined"], true);
    assert_eq!(capped["data"]["token_consumed"], false);
    assert_eq!(capped["data"]["nerve"]["component"], "dummy-dl");
    assert!(r
        .report
        .get("detail")
        .unwrap()
        .as_str()
        .unwrap()
        .contains("1 Gaia + 2 nerve"));

    // The nerve's count drops to 0: the cap lifts at once (1 + 0 < 3).
    n.restarts("dummy-dl", 0);
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::APPLY_FAILED, "{}", r.report.pretty());
    assert_eq!(g.consumed(), 2);
    assert_eq!(
        Ledger::new(&c.state_dir).history("dummy-dl").unwrap().len(),
        2
    );
}

#[test]
fn nerve_three_restarts_cap_before_any_token_or_spawn() {
    let (addr, n) = fake_nerve();
    n.restarts("dummy-dl", 3);
    let (mut c, g) = cfg("three", addr, free_test_port(20450..=20457));
    c.token_file = Some(g.token_file(&c.target, "a.tok"));
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::CAPPED, "{}", r.report.pretty());
    assert!(!r.spawned);
    assert_eq!(r.approval_code, None, "token untouched, loop keeps it");
    assert_eq!(g.consumed(), 0);
    assert_eq!(n.kinds(), vec!["service.probe", "gaia.heal.capped"]);
    assert_eq!(n.last()["data"]["retry_after_s"], 3600);
    assert_eq!(n.last()["data"]["gaia_restarts_last_hour"], 0);
    assert!(!c.state_dir.join("restarts.jsonl").exists());
}

#[test]
fn reflex_budget_counts_even_when_the_component_row_says_zero() {
    let (addr, n) = fake_nerve();
    n.set_state(
        200,
        json!({"components": [{"name": "dummy-dl", "unit": "dummy-dl.service", "restarts_1h": 0}],
               "reflex": {"budgets": {"dummy-dl.service": {"live_1h": 3}}}}),
    );
    let (mut c, g) = cfg("budget", addr, free_test_port(20450..=20457));
    c.token_file = Some(g.token_file(&c.target, "a.tok"));
    assert_eq!(heal_tick(&c).unwrap().exit, exit::CAPPED);
    assert_eq!(g.consumed(), 0);
}

#[test]
fn unreadable_nerve_state_fails_closed_without_consuming_the_token() {
    let (addr, n) = fake_nerve();
    let (mut c, g) = cfg("closed", addr, free_test_port(20450..=20457));
    c.token_file = Some(g.token_file(&c.target, "a.tok"));
    for (code, body) in [
        (500, json!({"error": "boom"})),
        (200, json!([1, 2])),
        (200, json!({"cycle": 1})),
        (
            200,
            json!({"components": [{"name": "dummy-dl", "restarts_1h": "many"}]}),
        ),
    ] {
        n.set_state(code, body.clone());
        let e = heal_tick(&c).unwrap_err();
        assert!(e.contains("D-L2"), "{body}: {e}");
        assert_eq!(g.consumed(), 0, "{body}");
    }
    assert!(!c.state_dir.join("restarts.jsonl").exists());
    // A disabled gate still refuses without needing the nerve count.
    std::fs::write(g.dir.join("DISABLED"), "x").unwrap();
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::REFUSED);
    assert_eq!(n.last()["data"]["code"], "disabled");
}

#[test]
fn hung_service_is_reported_with_its_pid_and_never_restarted() {
    let (addr, n) = fake_nerve();
    // A "service" that accepts and never answers, held by this process.
    let port = free_test_port(20458..=20459);
    let l = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let held = Arc::new(Mutex::new(Vec::new()));
    let held2 = held.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            held2.lock().unwrap().push(s);
        }
    });
    let (mut c, g) = cfg("hung", addr, port);
    let tok = g.token_file(&c.target, "a.tok");
    c.token_file = Some(tok.clone());
    let t0 = std::time::Instant::now();
    let r = heal_tick(&c).unwrap();
    let took = t0.elapsed();
    assert_eq!(r.exit, exit::HUNG, "{}", r.report.pretty());
    assert!(!r.spawned);
    assert_eq!(r.approval_code, None);
    assert_eq!(r.report.get("outcome").unwrap().as_str(), Some("hung"));
    assert_eq!(n.kinds(), vec!["service.probe", "gaia.heal.hung"]);
    let ev = n.last();
    assert_eq!(ev["severity"], "error");
    assert_eq!(ev["source"], "gaia-live");
    let d = &ev["data"];
    assert_eq!(d["service"], "dummy-dl");
    assert_eq!(d["port"], port);
    assert_eq!(d["listener_pids"], json!([std::process::id()]));
    assert_eq!(d["gaia_started_pids"], json!([]));
    assert_eq!(d["kill"], false);
    assert_eq!(d["restart"], false);
    assert_eq!(d["token_consumed"], false);
    assert_eq!(d["action"], "report-only");
    assert_eq!(
        d["probes"].as_array().unwrap().len(),
        2,
        "confirmed by a second probe"
    );
    assert!(gaia_live::heal::hung_detail(
        d["probes"][1].as_str().unwrap()
    ));
    assert_eq!(g.consumed(), 0);
    assert!(tok.exists());
    assert!(!c.state_dir.join("restarts.jsonl").exists());
    // Two probes of ~1-2 s each plus the 300 ms pause; no restart wait.
    assert!(took < Duration::from_secs(6), "{took:?}");
    // Still held open by its owner: Gaia closed nothing of it.
    assert!(held.lock().unwrap().len() >= 2);
    let raw = serde_json::to_string(&*n.log.lock().unwrap()).unwrap();
    assert!(!raw.contains("hades1"), "no token text on the nerve");
}

#[test]
fn a_down_port_is_not_hung_and_heals_normally() {
    let (addr, n) = fake_nerve();
    let (mut c, g) = cfg("down", addr, free_test_port(20450..=20457));
    c.token_file = Some(g.token_file(&c.target, "a.tok"));
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::APPLY_FAILED, "{}", r.report.pretty());
    assert!(!n.kinds().contains(&"gaia.heal.hung".to_string()));
}

#[test]
fn nerve_restarts_cli_reads_a_saved_vps_state_and_a_live_nerve() {
    let f = format!(
        "{}/../gaia-live/tests/fixtures/vps-nerve-state-20261021.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let j = gaia::live_heal::nerve_restarts_cli(&[
        "--state-file",
        &f,
        "--service",
        "hermes-rs",
        "--service",
        "dummy-x",
    ])
    .unwrap();
    let v: Value = serde_json::from_str(&j.compact()).unwrap();
    assert_eq!(v["nerve_cycle"], 6941);
    assert_eq!(v["services"][0]["component"], "hermes-rs");
    assert_eq!(v["services"][0]["left_for_gaia"], 3);
    assert_eq!(v["services"][1]["component"], Value::Null);
    let (addr, n) = fake_nerve();
    n.restarts("galion-one", 1);
    let a = addr.to_string();
    let j =
        gaia::live_heal::nerve_restarts_cli(&["--nerve", &a, "--service", "galion-one"]).unwrap();
    let v: Value = serde_json::from_str(&j.compact()).unwrap();
    assert_eq!(v["services"][0]["count"], 1);
    assert_eq!(v["services"][0]["left_for_gaia"], 2);
    assert!(gaia::live_heal::nerve_restarts_cli(&["--service", "x"]).is_err());
    assert!(
        gaia::live_heal::nerve_restarts_cli(&["--nerve", "8.8.8.8:80", "--service", "x"]).is_err(),
        "loopback only"
    );
}
