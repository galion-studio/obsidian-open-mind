// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Hermetic checks of `gaia heal-tick`'s refusal paths against an in-process
//! fake nerve on an ephemeral loopback port. Nothing is started or restarted
//! here: every case must stop before the apply step. The full apply path is
//! in `real_gaia_live.rs` (GALION_REAL=1) and scripts/real/gaia-live.sh.

use gaia::live_heal::{
    exit, heal_plan, heal_plan_cli, heal_plan_sha256, heal_tick, parse_cli, HealCfg,
};
use gaia_live::heal::{Ledger, TestTarget};
use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::approval::unix_now;
use hades_gate::token::{plan_hash, Claims};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Log = Arc<Mutex<Vec<Value>>>;

/// A minimal nerve: GET /health, POST /event, GET /events.
fn fake_nerve() -> (SocketAddr, Log) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let log: Log = Arc::default();
    let log2 = log.clone();
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
            let resp = if first.starts_with("GET /health") {
                r#"{"status":"ok"}"#.to_string()
            } else if first.starts_with("GET /state") {
                // D-L2: the nerve watches no box test service (count 0).
                r#"{"cycle":1,"components":[],"reflex":{"budgets":{}}}"#.to_string()
            } else if first.starts_with("POST /event") {
                let mut v: Value = serde_json::from_slice(&body).unwrap();
                v["ts"] = Value::from("2026-10-20T19:00:00+02:00");
                log2.lock().unwrap().push(v);
                r#"{"ok":true}"#.to_string()
            } else {
                Value::Array(log2.lock().unwrap().clone()).to_string()
            };
            let _ = write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                resp.len()
            );
        }
    });
    (addr, log)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gaia-heal-tick-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn prog() -> String {
    ["/bin/true", "/usr/bin/true"]
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .unwrap()
        .to_string()
}

/// A throwaway hades-gate: admin key pair (seed stays in the test dir, mode
/// 0600, never printed), replay ledger and disable file.
struct Gate {
    dir: PathBuf,
    config: PathBuf,
    seed: PathBuf,
}

impl Gate {
    fn new(dir: &std::path::Path) -> Gate {
        let g = dir.join("gate");
        std::fs::create_dir_all(&g).unwrap();
        let (seed, public) = (g.join("admin.seed"), g.join("admin.pub"));
        hades_gate::keys::generate(&seed, &public).unwrap();
        let config = g.join("hades.toml");
        std::fs::write(
            &config,
            format!(
                "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 900\n",
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

    /// Mint a token with `seed` for `action`/`target` over `plan`.
    fn mint_with(
        seed: &std::path::Path,
        action: &str,
        target: &str,
        plan: &[String],
        iat: u64,
        ttl: u64,
    ) -> String {
        let key = hades_gate::keys::load_seed(seed).unwrap();
        Claims {
            action: action.into(),
            target: target.into(),
            plan_hash: plan_hash(&gaia_plan_bytes(plan)),
            nonce: hades_gate::keys::os_random::<16>().unwrap(),
            issued_at: iat,
            expires_at: iat + ttl,
        }
        .sign(&key)
    }

    fn mint(&self, t: &TestTarget) -> String {
        Gate::mint_with(
            &self.seed,
            "heal-apply",
            &t.name,
            &heal_plan(t),
            unix_now(),
            300,
        )
    }

    fn consumed(&self) -> usize {
        std::fs::read_to_string(self.dir.join("ledger.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("\"event\":\"consumed\""))
            .count()
    }
}

/// A config whose target port is free (bound and released), inside a range
/// that is only that port, so nothing answers the probe.
fn cfg(tag: &str, nerve: SocketAddr, port: u16) -> (HealCfg, Gate) {
    let d = tmp(tag);
    let gate = Gate::new(&d);
    let c = HealCfg {
        target: TestTarget {
            name: "gaia-dummy".into(),
            port,
            health_path: "/health".into(),
            argv: vec![prog(), "--port".into(), port.to_string()],
        },
        allowed: 19230..=19239,
        nerve,
        state_dir: d.join("state"),
        hades_config: gate.config.clone(),
        token_file: None,
        wait: Duration::from_millis(200),
    };
    (c, gate)
}

fn with_token(c: &mut HealCfg, token: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    let p = c.state_dir.parent().unwrap().join(format!(
        "token-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&p)
        .unwrap();
    f.write_all(token.as_bytes()).unwrap();
    c.token_file = Some(p);
}

/// A port in slot 23's band 19230-19239 with nothing listening (19236-19239;
/// 19233/19234 belong to real_gaia_live).
fn free_test_port() -> u16 {
    (19236..=19239u16)
        .find(|p| {
            TcpListener::bind(("127.0.0.1", *p)).is_ok()
                && std::net::TcpStream::connect(("127.0.0.1", *p)).is_err()
        })
        .expect("a free test port")
}

fn kinds(log: &Log) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_string())
        .collect()
}

fn last(log: &Log) -> Value {
    log.lock().unwrap().last().cloned().unwrap()
}

#[test]
fn live_stack_port_is_refused_before_any_probe() {
    let (nerve, log) = fake_nerve();
    let (mut c, _g) = cfg("scope", nerve, 8420);
    c.target.argv = vec![prog(), "--port".into(), "8420".into()];
    let t = heal_tick(&c).unwrap();
    assert_eq!(t.exit, exit::REFUSED);
    assert_eq!(kinds(&log), vec!["gaia.heal.refused"]);
    let e = &log.lock().unwrap()[0];
    assert_eq!(e["data"]["stage"], "scope");
    assert!(!c.state_dir.exists(), "nothing written for a refused scope");
}

#[test]
fn down_service_without_token_is_planned_but_refused() {
    let (nerve, log) = fake_nerve();
    let (c, g) = cfg("notoken", nerve, free_test_port());
    let t = heal_tick(&c).unwrap();
    assert_eq!(t.exit, exit::REFUSED, "{}", t.report.pretty());
    assert_eq!(kinds(&log), vec!["service.probe", "gaia.heal.refused"]);
    assert_eq!(last(&log)["data"]["code"], "no-token");
    assert_eq!(
        last(&log)["data"]["plan_sha256"],
        heal_plan_sha256(&c.target).as_str()
    );
    let text = t.report.compact();
    assert!(text.contains("\"step\":\"nerve-in\""), "{text}");
    assert!(text.contains("\"step\":\"hades-plan\""), "{text}");
    assert!(text.contains("\"outcome\":\"refused\""), "{text}");
    assert!(!c.state_dir.join("restarts.jsonl").exists());
    assert_eq!(g.consumed(), 0);
}

#[test]
fn forged_tampered_wrong_scope_expired_are_refused_and_never_logged() {
    let port = free_test_port();
    let (nerve0, _) = fake_nerve();
    let (base, _g0) = cfg("bad-base", nerve0, port);
    let t = base.target.clone();
    let plan = heal_plan(&t);
    let now = unix_now();
    let mut other_port = t.clone();
    other_port.argv[2] = "19235".into();
    other_port.port = 19235;
    // A second, attacker-held key pair.
    let evil = tmp("bad-evil");
    hades_gate::keys::generate(&evil.join("evil.seed"), &evil.join("evil.pub")).unwrap();
    type Mk = Box<dyn Fn(&Gate) -> String>;
    let cases: Vec<(&str, &str, Mk)> = vec![
        (
            "forged",
            "forged",
            Box::new({
                let (e, p, n) = (evil.join("evil.seed"), plan.clone(), now);
                move |_| Gate::mint_with(&e, "heal-apply", "gaia-dummy", &p, n, 300)
            }),
        ),
        (
            "tampered",
            "forged",
            Box::new({
                let (p, n) = (plan.clone(), now);
                move |g: &Gate| {
                    let mut tok = Gate::mint_with(&g.seed, "heal-apply", "gaia-dummy", &p, n, 300);
                    // Flip the last signature nibble.
                    let c = tok.pop().unwrap();
                    tok.push(if c == '0' { '1' } else { '0' });
                    tok
                }
            }),
        ),
        (
            "garbage",
            "malformed",
            Box::new(|_| "approved-test".to_string()),
        ),
        (
            "wrong-target",
            "wrong-scope",
            Box::new({
                let (p, n) = (plan.clone(), now);
                move |g: &Gate| Gate::mint_with(&g.seed, "heal-apply", "gaia-other", &p, n, 300)
            }),
        ),
        (
            "wrong-action",
            "wrong-scope",
            Box::new({
                let (p, n) = (plan.clone(), now);
                move |g: &Gate| Gate::mint_with(&g.seed, "rollback", "gaia-dummy", &p, n, 300)
            }),
        ),
        (
            "wrong-plan",
            "wrong-scope",
            Box::new({
                let (p, n) = (heal_plan(&other_port), now);
                move |g: &Gate| Gate::mint_with(&g.seed, "heal-apply", "gaia-dummy", &p, n, 300)
            }),
        ),
        (
            "expired",
            "expired",
            Box::new({
                let (p, n) = (plan.clone(), now);
                move |g: &Gate| {
                    Gate::mint_with(&g.seed, "heal-apply", "gaia-dummy", &p, n - 400, 300)
                }
            }),
        ),
        (
            "too-long-ttl",
            "lifetime",
            Box::new({
                let (p, n) = (plan.clone(), now);
                move |g: &Gate| Gate::mint_with(&g.seed, "heal-apply", "gaia-dummy", &p, n, 3000)
            }),
        ),
    ];
    for (tag, code, mk) in cases {
        let (nerve, log) = fake_nerve();
        let (mut c, g) = cfg(&format!("bad-{tag}"), nerve, port);
        let tok = mk(&g);
        with_token(&mut c, &tok);
        let r = heal_tick(&c).unwrap();
        assert_eq!(r.exit, exit::REFUSED, "{tag}: {}", r.report.pretty());
        assert_eq!(
            kinds(&log),
            vec!["service.probe", "gaia.heal.refused"],
            "{tag}"
        );
        assert_eq!(last(&log)["data"]["code"], code, "{tag}");
        assert_eq!(last(&log)["data"]["stage"], "hades-approval", "{tag}");
        let all = format!("{} {:?}", r.report.compact(), log.lock().unwrap());
        assert!(!all.contains(&tok), "{tag}: token leaked");
        assert!(!c.state_dir.join("restarts.jsonl").exists(), "{tag}");
        assert_eq!(g.consumed(), 0, "{tag}");
    }
}

#[test]
fn valid_token_restarts_once_then_replay_is_refused() {
    let (nerve, log) = fake_nerve();
    let (mut c, g) = cfg("valid", nerve, free_test_port());
    let tok = g.mint(&c.target);
    with_token(&mut c, &tok);
    // /bin/true exits at once: approved and spawned, but never healthy.
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::APPLY_FAILED, "{}", r.report.pretty());
    assert_eq!(
        kinds(&log),
        vec!["service.probe", "gaia.heal.approved", "gaia.heal.failed"]
    );
    let approved = log.lock().unwrap()[1].clone();
    assert_eq!(approved["data"]["approved_by"], "owner");
    assert_eq!(approved["data"]["nonce"].as_str().unwrap().len(), 32);
    assert_eq!(g.consumed(), 1);
    let text = r.report.compact();
    assert!(text.contains("\"released\":true"), "{text}");
    assert!(!text.contains(&tok), "token leaked");
    assert_eq!(
        Ledger::new(&c.state_dir)
            .history("gaia-dummy")
            .unwrap()
            .len(),
        1
    );

    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::REFUSED, "{}", r.report.pretty());
    assert_eq!(last(&log)["data"]["code"], "replayed");
    assert_eq!(g.consumed(), 1);
    assert_eq!(
        Ledger::new(&c.state_dir)
            .history("gaia-dummy")
            .unwrap()
            .len(),
        1,
        "replay must not restart"
    );
}

#[test]
fn disable_needs_no_token_and_beats_a_valid_one() {
    let (nerve, log) = fake_nerve();
    let (mut c, g) = cfg("disabled", nerve, free_test_port());
    let tok = g.mint(&c.target);
    with_token(&mut c, &tok);
    std::fs::write(g.dir.join("DISABLED"), "disabled by admin\n").unwrap();
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::REFUSED);
    assert_eq!(last(&log)["data"]["code"], "disabled");
    assert_eq!(g.consumed(), 0, "a disabled gate consumes nothing");
    // Re-enabled, the same (unconsumed) token works once.
    std::fs::remove_file(g.dir.join("DISABLED")).unwrap();
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::APPLY_FAILED, "{}", r.report.pretty());
    assert_eq!(g.consumed(), 1);
}

#[test]
fn admin_safety_refuses_even_a_valid_token_for_that_plan() {
    let (nerve, log) = fake_nerve();
    let (mut c, g) = cfg("safety", nerve, free_test_port());
    c.target.argv.push("--note=secret".into());
    let tok = g.mint(&c.target);
    with_token(&mut c, &tok);
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::REFUSED, "{}", r.report.pretty());
    assert_eq!(last(&log)["data"]["code"], "admin-safety");
    assert_eq!(g.consumed(), 0);
}

#[test]
fn cap_is_checked_before_the_token_is_consumed() {
    let (nerve, log) = fake_nerve();
    let (mut c, g) = cfg("capped", nerve, free_test_port());
    std::fs::create_dir_all(&c.state_dir).unwrap();
    let l = Ledger::new(&c.state_dir);
    let now = unix_now();
    for i in 0..3 {
        l.record("gaia-dummy", now - 60 + i, 1).unwrap();
    }
    let tok = g.mint(&c.target);
    with_token(&mut c, &tok);
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::CAPPED, "{}", r.report.pretty());
    assert_eq!(kinds(&log), vec!["service.probe", "gaia.heal.capped"]);
    assert_eq!(last(&log)["data"]["token_consumed"], false);
    assert_eq!(g.consumed(), 0);
}

#[test]
fn heal_plan_is_deterministic_and_bound_to_port_and_argv() {
    let t = TestTarget {
        name: "gaia-dummy".into(),
        port: 19231,
        health_path: "/health".into(),
        argv: vec!["/x/gaia-dummy".into(), "--port".into(), "19231".into()],
    };
    assert_eq!(heal_plan(&t), heal_plan(&t.clone()));
    let mut u = t.clone();
    u.argv = vec!["/x/gaia-dummy".into(), "--port 19231".into()];
    assert_ne!(heal_plan_sha256(&t), heal_plan_sha256(&u));
    assert!(hades_gate::safety::review(&t.name, &gaia_plan_bytes(&heal_plan(&t))).is_ok());
}

#[test]
fn heal_plan_cli_writes_a_new_plan_file() {
    let d = tmp("plancli");
    let out = d.join("plan.txt");
    let p = prog();
    let out_s = out.display().to_string();
    let args = [
        "--service",
        "gaia-dummy",
        "--port",
        "19231",
        "--allow-ports",
        "19230-19239",
        "--out",
        &out_s,
        "--",
        &p,
        "--port",
        "19231",
    ];
    let j = heal_plan_cli(&args).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(
        j.get("plan_sha256").and_then(gaia_core::Json::as_str),
        Some(hades_gate::token::to_hex(&plan_hash(&bytes)).as_str())
    );
    assert!(heal_plan_cli(&args).is_err(), "never overwrites");
    let mut live = args;
    live[3] = "8420";
    live[11] = "8420";
    assert!(heal_plan_cli(&live).is_err(), "scope guard");
}

#[test]
fn cli_parse_keeps_start_argv_separate() {
    let a = [
        "--service",
        "gaia-dummy",
        "--port",
        "19232",
        "--allow-ports",
        "19230-19239",
        "--nerve",
        "127.0.0.1:19231",
        "--state",
        "/tmp/s",
        "--hades-config",
        "/tmp/h.toml",
        "--token-file",
        "/tmp/t",
        "--",
        "/x/gaia-dummy",
        "--port",
        "19232",
    ];
    let c = parse_cli(&a).unwrap();
    assert_eq!(c.target.port, 19232);
    assert_eq!(c.target.argv, vec!["/x/gaia-dummy", "--port", "19232"]);
    assert_eq!(c.allowed, 19230..=19239);
    assert_eq!(c.token_file, Some(PathBuf::from("/tmp/t")));
    assert!(parse_cli(&a[..14]).is_err(), "needs --");
    let mut no_gate = a.to_vec();
    no_gate.drain(10..12);
    assert!(parse_cli(&no_gate).is_err(), "needs --hades-config");
}

/// S3.1: the 3/h budget is global. Three restarts recorded in the global
/// ledger (next to hades-gate's replay ledger) cap a tick whose `--state`
/// dir is brand new, and the admin's token is not consumed.
#[test]
fn a_new_state_dir_cannot_reset_the_global_budget() {
    let (nerve, log) = fake_nerve();
    let (mut c, g) = cfg("global-cap", nerve, free_test_port());
    let global = gaia_live::heal::global_ledger_path(&g.dir.join("ledger.jsonl"));
    assert_eq!(global, g.dir.join("gaia-restarts.jsonl"));
    let gl = Ledger::at(global.clone());
    let now = unix_now();
    for i in 0..3 {
        gl.record_synced("gaia-dummy", now - 120 + i, 4242).unwrap();
    }
    let tok = g.mint(&c.target);
    with_token(&mut c, &tok);
    c.state_dir = c.state_dir.join("fresh-never-used");
    assert!(!c.state_dir.exists());
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::CAPPED, "{}", r.report.pretty());
    assert!(!r.spawned);
    assert_eq!(r.approval_code, None);
    assert_eq!(kinds(&log), vec!["service.probe", "gaia.heal.capped"]);
    assert_eq!(last(&log)["data"]["token_consumed"], false);
    assert_eq!(
        last(&log)["data"]["global_ledger"],
        global.display().to_string()
    );
    assert_eq!(g.consumed(), 0);
    assert!(
        !c.state_dir.join("restarts.jsonl").exists(),
        "the fresh state ledger is empty, the cap came from the global one"
    );
}

/// A corrupt global ledger fails closed: the tick errors before any token
/// is consumed or anything is started.
#[test]
fn a_corrupt_global_ledger_fails_closed() {
    let (nerve, _log) = fake_nerve();
    let (mut c, g) = cfg("global-corrupt", nerve, free_test_port());
    std::fs::write(g.dir.join("gaia-restarts.jsonl"), "{\"service\":\"gaia-du").unwrap();
    let tok = g.mint(&c.target);
    with_token(&mut c, &tok);
    let e = heal_tick(&c).unwrap_err();
    assert!(e.starts_with("ledger:"), "{e}");
    assert_eq!(g.consumed(), 0);
}

/// Refusals carry the hades-gate code for the loop's token bookkeeping.
#[test]
fn refusals_report_their_approval_code() {
    let (nerve, _log) = fake_nerve();
    let (c, _g) = cfg("code-no-token", nerve, free_test_port());
    let r = heal_tick(&c).unwrap();
    assert_eq!(r.exit, exit::REFUSED, "{}", r.report.pretty());
    assert_eq!(r.approval_code.as_deref(), Some("no-token"));
    assert!(!r.spawned);
}
