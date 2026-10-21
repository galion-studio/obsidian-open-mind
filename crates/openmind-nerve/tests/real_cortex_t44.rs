// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Cortex T4.4: box-only end-to-end test of the heal path (gated:
//! `GALION_REAL=1`; run with `--release`, after
//! `cargo build --release -p hades-gate`, so the real `hades-gate` binary sits
//! next to the real `openmind-nerve` binary).
//! Slot 222: ports 20920-20929 only. Nothing here touches the VPS, ssh, DNS
//! or a packet filter; every process is started by the test and killed by it.
//!
//! The chain, all real processes and real sockets:
//!
//! 1. a dummy service (`t44-dummy.service`, this test binary re-executed as a
//!    child process) on 127.0.0.1:20922 hangs in `503 loading`;
//! 2. the real nerve (`openmind-nerve serve` on :20920) raises the cortex
//!    finding `stuck-loading-t44-dummy`, opens an incident and writes the
//!    `unit-restart` HADES plan template (`GET /api/v1/cortex/plans`);
//! 3. a throwaway admin key is made on the box with `hades-gate keygen`, the
//!    approval is minted with `hades-gate mint` over the exact plan text, and
//!    the real gate (`hades-gate serve` on :20921) decides it;
//! 4. the test's heal-apply executor (below; the nerve and the cortex never
//!    run plans) applies only a `WouldApply` for a `unit-restart` plan whose
//!    approval names the same target and plan hash: it restarts the dummy
//!    process once and verifies `200`;
//! 5. every attempt (applied, dry-run or refused) is recorded in the nerve
//!    (`POST /api/v1/event`, read back from `GET /api/v1/events` and the
//!    signal hub), and the nerve then shows the incident resolved and the
//!    plan gone.
//!
//! Refusal paths must fail closed (no restart, dummy pid unchanged): no
//! token (dry-run), wrong family (stray-stop plan; token for another plan;
//! token for another action), protected targets (the nerve makes no plan for
//! a stuck `t44-ssh-relay.service`; a hand-made plan for it or for
//! `hades-gate.service` is refused by the executor and by the gate), a forged
//! token, a disabled gate, a replayed token, an unreachable gate, and an
//! approval after the service is already healthy (precondition stop).
//! Token strings are never printed.

use hades_gate::safety;
use hades_gate::token::{plan_hash, to_hex};
use openmind_nerve::plans::is_protected;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const NERVE: u16 = 20920;
const GATE: u16 = 20921;
const DUMMY: u16 = 20922;
const RELAY: u16 = 20923;
const STRAY: u16 = 20924;
/// Nothing listens here (unreachable-gate case).
const NO_GATE: u16 = 20929;
const DUMMY_UNIT: &str = "t44-dummy.service";
const RELAY_UNIT: &str = "t44-ssh-relay.service";
const SOURCE: &str = "hades-heal";

// ---------------------------------------------------------------- helpers

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One HTTP/1.0 request; `(status, body)`.
fn req(port: u16, method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(
        s,
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let code = b.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, b.split_once("\r\n\r\n")?.1.to_string()))
}

fn get(port: u16, path: &str) -> Option<Value> {
    serde_json::from_str(&req(port, "GET", path, "")?.1).ok()
}

fn wait<T>(secs: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let t = Instant::now();
    loop {
        if let Some(x) = f() {
            return Some(x);
        }
        if t.elapsed() > Duration::from_secs(secs) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// A tiny threaded HTTP server: `503 loading` while `loading`, else 200.
fn server(addr: &str, stop: Arc<AtomicBool>, loading: bool) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut s, _)) => answer(&mut s, loading, 0),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    })
}

fn answer(s: &mut TcpStream, loading: bool, pid: u32) {
    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
    let mut b = [0u8; 2048];
    let _ = s.read(&mut b);
    let (code, body) = if loading {
        (
            "503 Service Unavailable",
            format!(r#"{{"status":"loading","pid":{pid}}}"#),
        )
    } else {
        ("200 OK", format!(r#"{{"status":"ok","pid":{pid}}}"#))
    };
    let _ = write!(
        s,
        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Not a test: the body of the dummy service when this test binary is
/// re-executed with `T44_DUMMY_PORT` set. On start it hangs in `loading`
/// if the marker file exists (and removes it, like a wedged model load),
/// else it is healthy. It exits by itself after 10 minutes at most.
#[test]
fn t44_dummy_service_child() {
    let Ok(port) = std::env::var("T44_DUMMY_PORT") else {
        return;
    };
    let marker = PathBuf::from(std::env::var("T44_DUMMY_MARKER").unwrap());
    let stuck = marker.exists();
    if stuck {
        std::fs::remove_file(&marker).unwrap();
    }
    let l = TcpListener::bind(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
    l.set_nonblocking(true).unwrap();
    let pid = std::process::id();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(600) {
        match l.accept() {
            Ok((mut s, _)) => answer(&mut s, stuck, pid),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The dummy "unit": a child process this test can restart.
struct Dummy {
    child: Option<Child>,
    marker: PathBuf,
    restarts: u32,
}

impl Dummy {
    fn start(marker: &Path, stuck: bool) -> Self {
        if stuck {
            std::fs::write(marker, b"wedged\n").unwrap();
        }
        let mut d = Self {
            child: None,
            marker: marker.to_path_buf(),
            restarts: 0,
        };
        d.spawn();
        d
    }

    fn spawn(&mut self) {
        let c = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "t44_dummy_service_child",
                "--nocapture",
                "--test-threads",
                "1",
            ])
            .env_remove("GALION_REAL")
            .env("T44_DUMMY_PORT", DUMMY.to_string())
            .env("T44_DUMMY_MARKER", &self.marker)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn dummy");
        self.child = Some(c);
        assert!(
            wait(10, || health(DUMMY)).is_some(),
            "dummy did not come up"
        );
    }

    fn pid(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    /// `systemctl restart` for the dummy: stop the process, start it again.
    fn restart(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        // The port must be free before the new process binds it.
        assert!(wait(10, || TcpListener::bind(("127.0.0.1", DUMMY)).ok()).is_some());
        self.restarts += 1;
        self.spawn();
    }
}

impl Drop for Dummy {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// `(status, pid)` of a health endpoint.
fn health(port: u16) -> Option<(u16, u64)> {
    let (code, body) = req(port, "GET", "/health", "")?;
    let v: Value = serde_json::from_str(&body).ok()?;
    Some((code, v["pid"].as_u64().unwrap_or(0)))
}

fn plan_text(p: &Value) -> String {
    p["steps"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn plans(port: u16) -> Vec<Value> {
    get(port, "/api/v1/cortex/plans")
        .and_then(|p| p["plans"].as_array().cloned())
        .unwrap_or_default()
}

fn plan_by_id(port: u16, id: &str) -> Option<Value> {
    plans(port).into_iter().find(|p| p["id"] == id)
}

// --------------------------------------------------------------- executor

#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    /// The plan ran: the unit was restarted once and verified.
    Applied { old_pid: u64, new_pid: u64 },
    /// The gate said dry-run (no token): nothing ran.
    DryRun,
    /// Refused by the executor or the gate (`code`): nothing ran.
    Refused(String),
}

/// The heal-apply executor of this test. The nerve and the cortex only
/// write plans; this is the separate executor the T4.2 plan text names. It
/// knows one family (`unit-restart`) and one unit (the dummy), and it fails
/// closed on everything else.
struct Executor<'a> {
    gate: u16,
    nerve: u16,
    dummy: &'a mut Dummy,
    records: u32,
}

impl Executor<'_> {
    fn apply(&mut self, plan: &Value, token: &str) -> Outcome {
        let out = self.decide_and_run(plan, token);
        self.record(plan, &out);
        out
    }

    fn decide_and_run(&mut self, plan: &Value, token: &str) -> Outcome {
        let refuse = |c: &str| Outcome::Refused(c.to_string());
        if plan["action"] != "heal-apply" {
            return refuse("executor:action");
        }
        if plan["family"] != "unit-restart" {
            return refuse("executor:family");
        }
        let target = plan["target"].as_str().unwrap_or("");
        if is_protected(target) || safety::review_target(target).is_err() {
            return refuse("executor:protected-target");
        }
        let text = plan_text(plan);
        let body = json!({"action": "heal-apply", "target": target, "plan": text, "token": token})
            .to_string();
        let Some((code, resp)) = req(self.gate, "POST", "/v1/approve", &body) else {
            return refuse("executor:gate-unreachable");
        };
        let Ok(v) = serde_json::from_str::<Value>(&resp) else {
            return refuse("executor:gate-bad-reply");
        };
        if v["executed"] != false || v["dry_run"] != true {
            return refuse("executor:gate-bad-reply");
        }
        match (code, v["decision"].as_str()) {
            (200, Some("DryRun")) => return Outcome::DryRun,
            (200, Some("WouldApply")) => {}
            _ => return Outcome::Refused(format!("gate:{}", v["code"].as_str().unwrap_or("?"))),
        }
        // The approval must name exactly what is about to run.
        let a = &v["approved"];
        if a["action"] != "heal-apply"
            || a["target"] != target
            || a["plan_sha256"] != to_hex(&plan_hash(text.as_bytes()))
        {
            return refuse("executor:approval-mismatch");
        }
        if target != DUMMY_UNIT {
            return refuse("executor:unknown-unit");
        }
        // Precondition from the plan: the component still fails its check.
        let old = match health(DUMMY) {
            Some((503, pid)) => pid,
            _ => return refuse("executor:precondition"),
        };
        self.dummy.restart();
        match wait(30, || health(DUMMY).filter(|h| h.0 == 200)) {
            Some((_, new_pid)) if new_pid != old => Outcome::Applied {
                old_pid: old,
                new_pid,
            },
            _ => refuse("executor:verify-failed"),
        }
    }

    /// Every attempt goes into the nerve's event log.
    fn record(&mut self, plan: &Value, out: &Outcome) {
        let (outcome, code, sev) = match out {
            Outcome::Applied { .. } => ("applied", String::new(), "info"),
            Outcome::DryRun => ("dry-run", String::new(), "info"),
            Outcome::Refused(c) => ("refused", c.clone(), "warn"),
        };
        let ev = json!({
            "source": SOURCE, "kind": "heal-apply", "severity": sev,
            "data": {"plan": plan["id"], "family": plan["family"], "target": plan["target"],
                     "incident": plan["incident"], "outcome": outcome, "code": code,
                     "plan_sha256": to_hex(&plan_hash(plan_text(plan).as_bytes()))},
        });
        let r = req(self.nerve, "POST", "/api/v1/event", &ev.to_string());
        assert_eq!(r.as_ref().map(|r| r.0), Some(200), "nerve record: {r:?}");
        self.records += 1;
    }
}

// ------------------------------------------------------------------ gate

fn gate_bin() -> PathBuf {
    let p = Path::new(BIN).with_file_name("hades-gate");
    assert!(
        p.exists(),
        "{} missing: run `cargo build --release -p hades-gate` first (same CARGO_TARGET_DIR)",
        p.display()
    );
    p
}

fn gate(args: &[&str]) -> std::process::Output {
    Command::new(gate_bin())
        .args(args)
        .output()
        .expect("run hades-gate")
}

/// Mint a token with the real CLI; returns the token text (never printed).
fn mint(dir: &Path, seed: &Path, action: &str, target: &str, text: &str, name: &str) -> String {
    let plan = dir.join(format!("{name}.plan"));
    std::fs::write(&plan, text).unwrap();
    let out = dir.join(format!("{name}.tok"));
    let o = gate(&[
        "mint",
        "--seed",
        seed.to_str().unwrap(),
        "--action",
        action,
        "--target",
        target,
        "--plan-file",
        plan.to_str().unwrap(),
        "--ttl",
        "300",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    std::fs::read_to_string(&out).unwrap().trim().to_string()
}

fn gate_direct(target: &str, text: &str, token: &str) -> (u16, Value) {
    let body =
        json!({"action": "heal-apply", "target": target, "plan": text, "token": token}).to_string();
    let (c, b) = req(GATE, "POST", "/v1/approve", &body).expect("gate reply");
    (c, serde_json::from_str(&b).unwrap())
}

// ------------------------------------------------------------------- test

#[test]
fn real_cortex_t44_end_to_end_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let t0 = Instant::now();
    let dir = std::env::temp_dir().join(format!("nerve-real-t44-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for p in [NERVE, GATE, DUMMY, RELAY, STRAY, NO_GATE] {
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok(), "port {p} busy");
    }

    // Test key on the box (throwaway, in the temp dir).
    let (seed, public) = (dir.join("admin.seed"), dir.join("admin.pub"));
    let o = gate(&[
        "keygen",
        "--seed",
        seed.to_str().unwrap(),
        "--pub",
        public.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let ledger = dir.join("ledger.jsonl");
    let cfg = dir.join("hades.toml");
    std::fs::write(
        &cfg,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\nmax_ttl_secs = 600\n",
            public.display(),
            ledger.display(),
            dir.join("DISABLED").display()
        ),
    )
    .unwrap();
    let cfg_s = cfg.to_str().unwrap().to_string();
    let gate_child = Command::new(gate_bin())
        .args(["serve", "--config", &cfg_s, "--port", &GATE.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn hades-gate");
    let _gate = Kill(gate_child);
    let st = wait(10, || get(GATE, "/health")).expect("gate did not start");
    assert_eq!(st["mode"], "dry-run");
    assert_eq!(st["disabled"], false);

    // The dummy unit (stuck), a protected stuck component, one stray.
    let mut dummy = Dummy::start(&dir.join("dummy.wedged"), true);
    let pid0 = u64::from(dummy.pid());
    assert_eq!(health(DUMMY), Some((503, pid0)));
    let stop = Arc::new(AtomicBool::new(false));
    let h_relay = server(&format!("127.0.0.1:{RELAY}"), stop.clone(), true);
    let h_stray = server(&format!("127.0.0.1:{STRAY}"), stop.clone(), false);

    // The real nerve.
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"t44-dummy","unit":"{DUMMY_UNIT}","port":{DUMMY},"path":"/health","gaia":"AETHER","critical":false}},
                {{"name":"t44-ssh-relay","unit":"{RELAY_UNIT}","port":{RELAY},"path":"/health","gaia":"APOLLO","critical":false}},
                {{"name":"t44-hades-gate","port":{GATE},"path":"/health","gaia":"HADES","critical":false}}]"#
        ),
    )
    .unwrap();
    let events = dir.join("events.jsonl");
    let nerve_child = Command::new(BIN)
        .arg("serve")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_ADDR", format!("127.0.0.1:{NERVE}"))
        .env("NERVE_INTERVAL", "3")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_EVENT_LOG", &events)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_LISTEN_SCOPE", "20920-20929")
        .env("NERVE_LOADING_GRACE", "4")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn nerve");
    let _nerve = Kill(nerve_child);
    assert!(
        wait(15, || get(NERVE, "/api/v1/state")
            .filter(|s| s["cycle"].as_u64() >= Some(1)))
        .is_some(),
        "nerve did not start"
    );

    // 1-2. finding -> incident -> plan.
    let dummy_plan_id = format!("plan-unit-restart-{DUMMY_UNIT}");
    let stray_plan_id = format!("plan-stray-stop-stray-listener-{STRAY}");
    let plan = wait(60, || {
        plan_by_id(NERVE, &dummy_plan_id)
            .filter(|p| p["incident"].is_string())
            .filter(|_| plan_by_id(NERVE, &stray_plan_id).is_some())
    })
    .unwrap_or_else(|| {
        panic!(
            "no plans: {:#}",
            get(NERVE, "/api/v1/cortex/plans").unwrap_or_default()
        )
    });
    let t_plan = t0.elapsed().as_secs_f64();
    let stray_plan = plan_by_id(NERVE, &stray_plan_id).unwrap();
    let state = get(NERVE, "/api/v1/state").unwrap();
    let inc_id = plan["incident"].as_str().unwrap().to_string();
    let inc = state["incidents"]["open"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"] == inc_id.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("incident {inc_id} not open: {:#}", state["incidents"]));
    assert!(
        plan["findings"]
            .to_string()
            .contains("stuck-loading-t44-dummy"),
        "{}",
        plan["findings"]
    );
    let text = plan_text(&plan);
    assert_eq!(safety::review(DUMMY_UNIT, text.as_bytes()), Ok(()));
    eprintln!(
        "chain: finding {} -> incident {inc_id} (cause {}, state {}) -> {} after {t_plan:.1} s ({} B, sha256 {}…)",
        plan["findings"],
        inc["cause"],
        inc["state"],
        plan["id"],
        text.len(),
        &to_hex(&plan_hash(text.as_bytes()))[..16]
    );
    // Protected: the stuck relay gets no plan, only a skip with the reason.
    let skipped = get(NERVE, "/api/v1/cortex/plans").unwrap()["skipped"].clone();
    let relay_skip = skipped
        .as_array()
        .unwrap()
        .iter()
        .find(|s| {
            s["finding"]
                .to_string()
                .contains("stuck-loading-t44-ssh-relay")
        })
        .cloned()
        .unwrap_or_else(|| panic!("relay not skipped: {skipped:#}"));
    assert!(
        relay_skip["reason"].to_string().contains("protected"),
        "{relay_skip}"
    );
    assert!(plans(NERVE).iter().all(|p| p["target"] != RELAY_UNIT));
    eprintln!(
        "protected: {} skipped: {}",
        relay_skip["finding"], relay_skip["reason"]
    );
    // The nerve itself ran nothing.
    assert!(state["reflex"]["recent"]
        .as_array()
        .is_none_or(|a| a.is_empty()));

    let mut ex = Executor {
        gate: GATE,
        nerve: NERVE,
        dummy: &mut dummy,
        records: 0,
    };
    let mut log: Vec<(String, Outcome)> = vec![];
    let refused = |c: &str| Outcome::Refused(c.to_string());

    // 3. Refusal paths: all fail closed, the dummy keeps its pid.
    eprintln!("refusal paths:");
    check(&mut ex, &mut log, "no token", &plan, "", Outcome::DryRun);
    let tok_stray = mint(
        &dir,
        &seed,
        "heal-apply",
        stray_plan["target"].as_str().unwrap(),
        &plan_text(&stray_plan),
        "stray",
    );
    check(
        &mut ex,
        &mut log,
        "wrong family (stray-stop, valid token)",
        &stray_plan,
        &tok_stray,
        refused("executor:family"),
    );
    check(
        &mut ex,
        &mut log,
        "token of the stray-stop plan",
        &plan,
        &tok_stray,
        refused("gate:wrong-scope"),
    );
    let tok_rollback = mint(&dir, &seed, "rollback", DUMMY_UNIT, &text, "rollback");
    check(
        &mut ex,
        &mut log,
        "token for action rollback",
        &plan,
        &tok_rollback,
        refused("gate:wrong-scope"),
    );
    let mut relabeled = plan.clone();
    relabeled["family"] = json!("stray-stop");
    check(
        &mut ex,
        &mut log,
        "family relabeled on a valid plan",
        &relabeled,
        "",
        refused("executor:family"),
    );
    for (name, unit) in [("relay", RELAY_UNIT), ("hades", "hades-gate.service")] {
        let mut p = plan.clone();
        p["target"] = json!(unit);
        p["steps"] = json!(plan["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().replace(DUMMY_UNIT, unit))
            .collect::<Vec<_>>());
        let tok = mint(&dir, &seed, "heal-apply", unit, &plan_text(&p), name);
        check(
            &mut ex,
            &mut log,
            &format!("protected target {unit}"),
            &p,
            &tok,
            refused("executor:protected-target"),
        );
        // The gate refuses it too, on its own (defence in depth).
        let (c, v) = gate_direct(unit, &plan_text(&p), &tok);
        assert_eq!((c, v["code"].as_str()), (403, Some("admin-safety")), "{v}");
        eprintln!("    gate on its own: {c} {} ({})", v["code"], v["reason"]);
    }
    let good = mint(&dir, &seed, "heal-apply", DUMMY_UNIT, &text, "good");
    // Forged: one hex digit of the signature flipped.
    let mut forged: Vec<char> = good.chars().collect();
    let last = forged.len() - 1;
    forged[last] = if forged[last] == '0' { '1' } else { '0' };
    let forged: String = forged.into_iter().collect();
    check(
        &mut ex,
        &mut log,
        "forged signature",
        &plan,
        &forged,
        refused("gate:forged"),
    );
    // Signed by another key (also made on the box, not the gate's admin key).
    let (seed2, pub2) = (dir.join("other.seed"), dir.join("other.pub"));
    assert!(gate(&[
        "keygen",
        "--seed",
        seed2.to_str().unwrap(),
        "--pub",
        pub2.to_str().unwrap()
    ])
    .status
    .success());
    let other = mint(&dir, &seed2, "heal-apply", DUMMY_UNIT, &text, "other");
    check(
        &mut ex,
        &mut log,
        "token signed by another key",
        &plan,
        &other,
        refused("gate:forged"),
    );
    check(
        &mut ex,
        &mut log,
        "garbage token",
        &plan,
        "not-a-token",
        refused("gate:malformed"),
    );
    let mut changed = plan.clone();
    changed["steps"][4] = json!(format!("run systemctl restart {DUMMY_UNIT} twice"));
    check(
        &mut ex,
        &mut log,
        "plan text changed after minting",
        &changed,
        &good,
        refused("gate:wrong-scope"),
    );
    let o = gate(&["disable", "--config", &cfg_s]);
    assert!(o.status.success());
    check(
        &mut ex,
        &mut log,
        "gate disabled, valid token",
        &plan,
        &good,
        refused("gate:disabled"),
    );
    assert!(gate(&["enable", "--config", &cfg_s]).status.success());
    ex.gate = NO_GATE;
    check(
        &mut ex,
        &mut log,
        "gate unreachable, valid token",
        &plan,
        &good,
        refused("executor:gate-unreachable"),
    );
    ex.gate = GATE;
    assert_eq!(ex.dummy.restarts, 0);
    assert_eq!(health(DUMMY), Some((503, pid0)));
    assert!(
        req(STRAY, "GET", "/health", "").is_some_and(|r| r.0 == 200),
        "stray was touched"
    );

    // Real VPS plans (optional, box copy of a live capture run through
    // `openmind-nerve plans`): with a valid token each, the executor still
    // runs none of them here (other families, or units it does not own).
    if let Ok(path) = std::env::var("NERVE_T44_LIVE_PLANS") {
        let live: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let ps = live["plans"].as_array().cloned().unwrap_or_default();
        eprintln!("live VPS plans from {path}: {}", ps.len());
        for (n, p) in ps.iter().enumerate() {
            let t = p["target"].as_str().unwrap();
            assert_eq!(
                safety::review(t, plan_text(p).as_bytes()),
                Ok(()),
                "{}",
                p["id"]
            );
            let tok = mint(
                &dir,
                &seed,
                "heal-apply",
                t,
                &plan_text(p),
                &format!("live{n}"),
            );
            let before = ex.dummy.pid();
            let out = ex.apply(p, &tok);
            eprintln!("  live {:<34} -> {out:?}", p["id"].as_str().unwrap());
            assert!(
                matches!(&out, Outcome::Refused(c) if c == "executor:family" || c == "executor:unknown-unit"),
                "{out:?}"
            );
            assert_eq!(ex.dummy.pid(), before);
            log.push((format!("live {}", p["id"]), out));
        }
    }

    // 4. The real approval: applied once.
    eprintln!("approved path:");
    let t_apply = Instant::now();
    let out = ex.apply(&plan, &good);
    let apply_s = t_apply.elapsed().as_secs_f64();
    eprintln!("  {:<38} -> {out:?} in {apply_s:.2} s", "valid token");
    let Outcome::Applied { old_pid, new_pid } = out.clone() else {
        panic!("not applied: {out:?}");
    };
    assert_eq!(old_pid, pid0);
    assert_eq!(ex.dummy.restarts, 1);
    log.push(("valid token".into(), out));
    check(
        &mut ex,
        &mut log,
        "replay of the used token",
        &plan,
        &good,
        refused("gate:replayed"),
    );
    let fresh = mint(&dir, &seed, "heal-apply", DUMMY_UNIT, &text, "fresh");
    check(
        &mut ex,
        &mut log,
        "fresh token, service already healthy",
        &plan,
        &fresh,
        refused("executor:precondition"),
    );
    assert_eq!(ex.dummy.restarts, 1, "a second restart ran");
    assert_eq!(health(DUMMY), Some((200, new_pid)));

    // 5. The nerve: records, then incident resolved and plan gone.
    let records = ex.records;
    let evs = wait(10, || {
        get(NERVE, "/api/v1/events?n=200").filter(|e| {
            e.as_array().is_some_and(|a| {
                a.iter().filter(|x| x["source"] == SOURCE).count() as u32 >= records
            })
        })
    })
    .expect("events");
    let mine: Vec<&Value> = evs
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["source"] == SOURCE)
        .collect();
    assert_eq!(mine.len() as u32, records);
    let applied: Vec<&&Value> = mine
        .iter()
        .filter(|x| x["data"]["outcome"] == "applied")
        .collect();
    assert_eq!(applied.len(), 1, "{mine:?}");
    assert_eq!(applied[0]["data"]["incident"], inc_id.as_str());
    assert_eq!(applied[0]["data"]["target"], DUMMY_UNIT);
    let n_refused = mine
        .iter()
        .filter(|x| x["data"]["outcome"] == "refused")
        .count();
    eprintln!(
        "nerve record: {} event(s) from {SOURCE}: 1 applied, 1 dry-run, {n_refused} refused; event log {} B",
        mine.len(),
        std::fs::metadata(&events).map(|m| m.len()).unwrap_or(0)
    );
    let sig = get(NERVE, "/api/v1/signals").unwrap();
    assert!(sig.to_string().contains(SOURCE), "{sig}");
    let t_fix = Instant::now();
    let resolved = wait(60, || {
        let s = get(NERVE, "/api/v1/state")?;
        let open = s["incidents"]["open"]
            .as_array()?
            .iter()
            .any(|i| i["id"] == inc_id.as_str());
        let gone = plan_by_id(NERVE, &dummy_plan_id).is_none();
        let r = s["incidents"]["resolved"]
            .as_array()?
            .iter()
            .find(|i| i["id"] == inc_id.as_str())
            .cloned();
        (!open && gone).then_some(r).flatten()
    })
    .unwrap_or_else(|| {
        panic!(
            "incident not resolved: {:#}",
            get(NERVE, "/api/v1/incidents").unwrap_or_default()
        )
    });
    let left: Vec<String> = plans(NERVE)
        .iter()
        .map(|p| p["id"].as_str().unwrap_or("").to_string())
        .collect();
    eprintln!(
        "nerve: incident {inc_id} {} (resolved {}) and plan gone {:.1} s after the restart; plans left {left:?}",
        resolved["state"],
        resolved["resolved"],
        t_fix.elapsed().as_secs_f64()
    );
    assert_eq!(resolved["state"], "resolved");
    assert!(left.contains(&stray_plan_id), "stray plan should stay");

    let evlog = std::fs::read_to_string(&events).unwrap();
    for t in [&good, &fresh, &tok_stray, &tok_rollback, &other] {
        assert!(
            !evlog.contains(t.as_str()),
            "token text in the nerve event log"
        );
    }
    // The ledger holds the one approval and the refusals, never a token.
    let led = std::fs::read_to_string(&ledger).unwrap();
    for t in [&good, &fresh, &tok_stray, &tok_rollback, &other] {
        assert!(!led.contains(t.as_str()), "token text in the ledger");
    }
    let lines = led.lines().count();
    eprintln!(
        "ledger: {lines} line(s), {} refused, {} dry-run; {} outcomes total; whole test {:.1} s",
        led.matches("\"refused\"").count(),
        led.matches("\"dry-run\"").count(),
        log.len(),
        t0.elapsed().as_secs_f64()
    );

    stop.store(true, Ordering::Relaxed);
    h_relay.join().unwrap();
    h_stray.join().unwrap();
    drop(dummy);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Apply `p` with `tok`, expect `want`; on anything but `Applied` the dummy
/// must keep its pid (nothing ran).
fn check(
    ex: &mut Executor<'_>,
    log: &mut Vec<(String, Outcome)>,
    label: &str,
    p: &Value,
    tok: &str,
    want: Outcome,
) {
    let before = ex.dummy.pid();
    let out = ex.apply(p, tok);
    eprintln!("  {label:<38} -> {out:?}");
    assert_eq!(out, want, "{label}");
    if !matches!(want, Outcome::Applied { .. }) {
        assert_eq!(ex.dummy.pid(), before, "{label}: dummy restarted");
        assert_eq!(health(DUMMY).map(|h| h.1), Some(u64::from(before)));
    }
    log.push((label.to_string(), out));
}
