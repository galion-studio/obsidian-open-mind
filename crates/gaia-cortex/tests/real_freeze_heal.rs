// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Freeze heal, REAL: the release `gaia-cortex` from this branch with the
//! freeze watch on, against the REAL release event-mode nerve
//! (`GALION_NERVE_BIN`) and four mock HTTP services whose health endpoint
//! can be switched between ok / 503 / hang. Box only, loopback only, ports
//! 23220-23229 (inside GAIA_TEST_PORTS 19000-23999). Nothing touches
//! systemd: the nerve's `systemctl` is a fake script first on `PATH` (logs
//! the call, `show` says `active`), its journal is a FIFO.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! `GALION_REAL=1 GALION_NERVE_BIN=... cargo test --release -p gaia-cortex --test real_freeze_heal -- --nocapture --test-threads 1`
//!
//! Cortex: `CORTEX_ACTIONS=live CORTEX_HEAL_VERIFY_S=30 CORTEX_FREEZE=1
//! CORTEX_FREEZE_PROBE_S=5 CORTEX_FREEZE_FAILS=6 CORTEX_FREEZE_MIN_S=60
//! CORTEX_FREEZE_COOLDOWN_S=60 CORTEX_FREEZE_TIMEOUT_S=2`.
//!   blip      galion-one answers 503 for ~10 s, then ok -> no restart
//!   deploy    openmind-memory: journal `Started` (a deliberate restart),
//!             then 503 for 45 s (< cooldown) -> no restart
//!   hung      galion-desk accepts and never answers -> R13-frozen restart
//!             executed by the nerve (fake systemctl); the mock is fixed
//!             when the restart lands -> verified by probe at the deadline
//!   breaker   tuner returns 503 and stays so -> R13-frozen restart ->
//!             still 503 at the deadline -> R12-heal-failed (alert only, no
//!             escalation), circuit open -> after the cooldown it fires again
//!             -> R2-circuit-open, NO second restart
//!   bounds    bad CORTEX_FREEZE_* values refuse to start (exit 2)

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 23220;
const CORTEX_PORT: u16 = 23221;
const DESK_PORT: u16 = 23222;
const TUNER_PORT: u16 = 23223;
const ONE_PORT: u16 = 23224;
const MEM_PORT: u16 = 23225;
const BOUNDS_PORT: u16 = 23229;

const ID_STARTED: &str = "39f53479d3a045ac8e11786248231fbf";
#[allow(dead_code)]
const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
#[allow(dead_code)]
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";

const DESK: &str = "galion-desk.service";
const TUNER: &str = "tuner.service";
const ONE: &str = "galion-one.service";
const MEM: &str = "openmind-memory.service";

const OK: u8 = 0;
const E503: u8 = 1;
const HANG: u8 = 2;

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[freeze-heal {}] {}",
        zagreb_time::from_unix(now_unix() as i64),
        msg.as_ref()
    );
}

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let t = Duration::from_secs(15);
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), t) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(t));
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() || s.write_all(body.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let out = String::from_utf8_lossy(&out).to_string();
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let b = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, b)
}

fn get(port: u16, path: &str) -> Value {
    let (c, b) = http(port, "GET", path, "");
    if c != 200 {
        return Value::Null;
    }
    serde_json::from_str(&b).unwrap_or(Value::Null)
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout ({secs} s) waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
    t.elapsed()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Journal(std::fs::File);
impl Journal {
    fn entry(&mut self, id: &str, unit: &str, extra: Value) {
        let mut v = json!({
            "MESSAGE_ID": id, "UNIT": unit, "MESSAGE": format!("{unit}: freeze-heal real test"), "_PID": "1",
            "__REALTIME_TIMESTAMP": (now_unix() * 1_000_000).to_string(),
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        writeln!(self.0, "{v}").unwrap();
        self.0.flush().unwrap();
    }
    #[allow(dead_code)]
    fn kill(&mut self, unit: &str) {
        self.entry(
            ID_PROCESS_EXIT,
            unit,
            json!({"EXIT_CODE": "killed", "EXIT_STATUS": "9"}),
        );
        self.entry(ID_FAILURE_RESULT, unit, json!({"UNIT_RESULT": "signal"}));
    }
    fn started(&mut self, unit: &str) {
        self.entry(
            ID_STARTED,
            unit,
            json!({"JOB_RESULT": "done", "MESSAGE": format!("Started {unit}")}),
        );
    }
}

/// A fake `systemctl`: logs every call; `restart` succeeds; `show` answers
/// each unit from `units/<unit>` (default `active`); everything else exit 0.
fn fake_systemctl(dir: &Path) -> PathBuf {
    let bin = dir.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(dir.join("units")).unwrap();
    let d = dir.display();
    let script = format!(
        r#"#!/bin/sh
echo "$*" >> "{d}/systemctl.calls"
case "$1" in
  show)
    shift
    for a in "$@"; do
      case "$a" in
        *.service)
          st=$(cat "{d}/units/$a" 2>/dev/null || echo active)
          printf 'Id=%s\nLoadState=loaded\nActiveState=%s\nSubState=%s\nResult=success\nUnitFileState=enabled\nMainPID=4242\nNRestarts=0\nExecMainStatus=0\n\n' "$a" "$st" "$st"
          ;;
      esac
    done
    ;;
esac
exit 0
"#
    );
    let p = bin.join("systemctl");
    std::fs::write(&p, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("systemctl.calls"))
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("restart"))
        .map(str::to_string)
        .collect()
}

/// A mock service health endpoint: `mode` ok (200) / 503 / hang (read the
/// request, never answer for 60 s).
fn mock(port: u16) -> Arc<AtomicU8> {
    let mode = Arc::new(AtomicU8::new(OK));
    let l = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let m = mode.clone();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let m = m.load(Ordering::SeqCst);
            std::thread::spawn(move || {
                let mut s = s;
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                match m {
                    HANG => std::thread::sleep(Duration::from_secs(60)),
                    E503 => {
                        let _ = s.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbusy");
                    }
                    _ => {
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}");
                    }
                }
            });
        }
    });
    mode
}

fn restarts(dir: &Path, unit: &str) -> usize {
    calls(dir).iter().filter(|c| c.contains(unit)).count()
}

fn heals() -> Value {
    get(CORTEX_PORT, "/heals")
}

fn heal_for(unit: &str) -> Value {
    heals()["recent"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|h| h["unit"] == unit)
        .unwrap_or(Value::Null)
}

/// Open escalations from the heal / freeze path (R12 / R13 / R2 for a unit).
/// The nerve engine's own critical incidents (R11, unchanged) may escalate.
fn heal_escalations() -> Vec<Value> {
    get(CORTEX_PORT, "/escalations")["open"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| {
            let r = e["rule"].as_str().unwrap_or("");
            r.starts_with("R12") || r.starts_with("R13") || r.starts_with("R2")
        })
        .collect()
}

fn track(unit: &str) -> Value {
    heals()["freeze"]["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|t| t["unit"] == unit)
        .unwrap_or(Value::Null)
}

fn rules_for(unit: &str) -> Vec<String> {
    let subj = format!("unit:{unit}");
    let mut v: Vec<Value> = get(CORTEX_PORT, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == subj.as_str() && d["rule"] != "R10-remediation")
        .collect();
    v.sort_by_key(|d| {
        d["id"]
            .as_str()
            .and_then(|s| s.strip_prefix("dec-"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    v.iter()
        .map(|d| d["rule"].as_str().unwrap_or("").to_string())
        .collect()
}

fn gate() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and GALION_NERVE_BIN)");
        return None;
    }
    let bin = PathBuf::from(
        std::env::var("GALION_NERVE_BIN")
            .expect("GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve>"),
    );
    assert!(bin.is_file(), "GALION_NERVE_BIN {} missing", bin.display());
    for p in [
        NERVE_PORT,
        CORTEX_PORT,
        DESK_PORT,
        TUNER_PORT,
        ONE_PORT,
        MEM_PORT,
    ] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (freeze-heal range 23220-23229)"
        );
    }
    Some(bin)
}

#[test]
fn freeze_heal_against_the_real_nerve() {
    let Some(bin) = gate() else { return };
    // unique dir per run; left in /tmp for inspection (no deletes)
    let dir = std::env::temp_dir().join(format!(
        "cortex-freeze-heal-{}-{}",
        std::process::id(),
        now_unix()
    ));
    std::fs::create_dir_all(dir.join("nerve-data")).unwrap();
    let (desk, tuner, one, mem) = (
        mock(DESK_PORT),
        mock(TUNER_PORT),
        mock(ONE_PORT),
        mock(MEM_PORT),
    );
    let fakebin = fake_systemctl(&dir);
    let path = format!(
        "{}:{}",
        fakebin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        dir.join("components.json"),
        format!(
            r#"[{{"name":"galion-desk","unit":"{DESK}","port":{DESK_PORT},"gaia":"DEMETER"}},
                {{"name":"tuner","unit":"{TUNER}","port":{TUNER_PORT},"gaia":"DEMETER"}},
                {{"name":"galion-one","unit":"{ONE}","port":{ONE_PORT},"gaia":"DEMETER"}},
                {{"name":"memory","unit":"{MEM}","port":{MEM_PORT},"gaia":"DEMETER"}}]"#
        ),
    )
    .unwrap();
    let _nerve = Proc(
        Command::new(&bin)
            .args([
                "serve",
                "--mode",
                "event",
                "--listen",
                &format!("127.0.0.1:{NERVE_PORT}"),
                "--log",
                &dir.join("nerve-events.jsonl").display().to_string(),
            ])
            .env_clear()
            .env("PATH", &path)
            .env("NERVE_DATA_DIR", dir.join("nerve-data"))
            .env("NERVE_COMPONENTS", dir.join("components.json"))
            .env("NERVE_DRY_RUN", "0")
            .env("NERVE_EVT_ACTIONS", "live")
            .env("NERVE_REFLEX", "1")
            .env("NERVE_SYSTEMD", "1")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_EVT_PIDFD", "0")
            .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
            .env("NERVE_EVT_RECOVER_HOLD", "3")
            .env("NERVE_EVT_DEDUPE", "1")
            .env("NERVE_EVT_DEBOUNCE", "1")
            .env("NERVE_EVT_MAX_WAIT", "4")
            .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
            .env("NERVE_EVT_CYCLE_MIN_GAP", "2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("nerve.log")).unwrap())
            .spawn()
            .expect("spawn nerve"),
    );
    wait_until("nerve /health", 30, || {
        http(NERVE_PORT, "GET", "/health", "").0 == 200
    });
    let mut j = Journal(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    wait_until("nerve /state lists the mocks active", 60, || {
        let st = get(NERVE_PORT, "/state");
        let cs = st["components"].as_array().cloned().unwrap_or_default();
        [DESK, TUNER, ONE, MEM].iter().all(|u| {
            cs.iter().any(|c| {
                c["unit"] == *u && c["port"].as_u64().is_some() && c["unit_state"] == "active"
            })
        })
    });
    let cfg = dir.join("cortex.json");
    std::fs::write(&cfg, "{}").unwrap();
    let _cortex = Proc(
        Command::new(env!("CARGO_BIN_EXE_gaia-cortex"))
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("CORTEX_CONFIG", &cfg)
            .env("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}"))
            .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
            .env("CORTEX_DATA_DIR", dir.join("cortex-data"))
            .env("CORTEX_COALESCE_MS", "500")
            .env("CORTEX_ACTIONS", "live")
            .env("CORTEX_ACTION_MODE", "auto")
            .env("CORTEX_HEAL_VERIFY_S", "30")
            .env("CORTEX_FREEZE", "1")
            .env("CORTEX_FREEZE_PROBE_S", "5")
            .env("CORTEX_FREEZE_FAILS", "6")
            .env("CORTEX_FREEZE_MIN_S", "60")
            .env("CORTEX_FREEZE_COOLDOWN_S", "60")
            .env("CORTEX_FREEZE_TIMEOUT_S", "2")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("cortex.log")).unwrap())
            .spawn()
            .expect("spawn cortex"),
    );
    wait_until("cortex /health", 20, || {
        http(CORTEX_PORT, "GET", "/health", "").0 == 200
    });
    wait_until("cortex subscribed", 20, || {
        get(NERVE_PORT, "/stats")["streams"].as_u64() == Some(1)
    });
    let h = get(CORTEX_PORT, "/health");
    log(format!("cortex health: {h}"));
    assert_eq!(h["freeze_watch"], json!(true), "{h}");
    wait_until("freeze targets from the nerve", 30, || {
        heals()["freeze"]["targets"]
            .as_array()
            .map_or(0, |a| a.len())
            == 4
    });
    log(format!("freeze targets: {}", heals()["freeze"]["targets"]));

    // ------------------------------------------------- blip + deploy cooldown
    j.started(MEM);
    wait_until("memory cooldown from the Started event", 15, || {
        !track(MEM)["cooldown_until"].is_null()
    });
    log(format!("memory after Started: {}", track(MEM)));
    mem.store(E503, Ordering::SeqCst);
    one.store(E503, Ordering::SeqCst);
    std::thread::sleep(Duration::from_secs(11));
    one.store(OK, Ordering::SeqCst);
    log(format!("galion-one blip over: {}", track(ONE)));
    std::thread::sleep(Duration::from_secs(34));
    mem.store(OK, Ordering::SeqCst);
    std::thread::sleep(Duration::from_secs(6));
    let f = heals()["freeze"].clone();
    log(format!("after blip/deploy: counts {}", f["counts"]));
    assert_eq!(restarts(&dir, ONE), 0, "a blip is not restarted");
    assert_eq!(restarts(&dir, MEM), 0, "a deploy restart is not a freeze");
    assert!(f["counts"]["blips"].as_u64().unwrap_or(0) >= 1, "{f}");
    assert!(!rules_for(ONE).iter().any(|r| r.starts_with("R13")));
    assert!(!rules_for(MEM).iter().any(|r| r.starts_with("R13")));

    // ------------------------------------------------------------------- hung
    desk.store(HANG, Ordering::SeqCst);
    let hung = Instant::now();
    log("galion-desk hangs (accepts, never answers)");
    wait_until("desk freeze restart executed", 120, || {
        restarts(&dir, DESK) == 1
    });
    let took = hung.elapsed();
    desk.store(OK, Ordering::SeqCst); // the restart "fixed" it
    log(format!("desk restarted after {took:?}: {:?}", calls(&dir)));
    assert!(
        took >= Duration::from_secs(55),
        "not before ~60 s: {took:?}"
    );
    let rd = rules_for(DESK);
    log(format!("desk rules: {rd:?}"));
    assert!(rd.contains(&"R13-frozen".to_string()), "{rd:?}");
    wait_until("desk judged", 60, || !heal_for(DESK).is_null());
    let hd = heal_for(DESK);
    log(format!("desk heal: {hd}"));
    assert_eq!(hd["outcome"], "verified", "{hd}");
    assert_eq!(hd["by"], "probe", "{hd}");
    assert_eq!(hd["kind"], "freeze", "{hd}");

    // ---------------------------------------------------------------- breaker
    tuner.store(E503, Ordering::SeqCst);
    log("tuner returns 503 and stays so");
    wait_until("tuner freeze restart executed", 120, || {
        restarts(&dir, TUNER) == 1
    });
    wait_until("tuner judged", 60, || !heal_for(TUNER).is_null());
    let ht = heal_for(TUNER);
    log(format!("tuner heal: {ht}"));
    assert_eq!(ht["outcome"], "failed", "{ht}");
    assert_eq!(ht["by"], "probe", "{ht}");
    let esc = heal_escalations();
    log(format!("heal escalations open: {esc:?}"));
    assert!(esc.is_empty(), "failed heal: alert only");
    let r1 = rules_for(TUNER);
    log(format!("tuner rules: {r1:?}"));
    assert!(r1.contains(&"R12-heal-failed".to_string()), "{r1:?}");
    // keeps failing: the next freeze (after the 60 s cooldown) hits the circuit
    wait_until("second tuner freeze decision", 180, || {
        rules_for(TUNER).iter().any(|r| r.starts_with("R2"))
    });
    std::thread::sleep(Duration::from_secs(3));
    let r2 = rules_for(TUNER);
    log(format!("tuner rules after the 2nd freeze: {r2:?}"));
    assert_eq!(restarts(&dir, TUNER), 1, "circuit open: no second restart");
    assert!(heal_escalations().is_empty(), "still alert only");
    // the nerve's echo of our gaia.heal.failed is not decided again
    let echoed: Vec<Value> = get(CORTEX_PORT, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| {
            d["kinds"].as_array().is_some_and(|k| {
                k.iter()
                    .any(|k| k.as_str().is_some_and(|k| k.starts_with("gaia.heal.")))
            })
        })
        .collect();
    assert!(echoed.is_empty(), "heal echo decided: {echoed:?}");
    tuner.store(OK, Ordering::SeqCst);

    let hs = heals();
    log(format!(
        "/heals summary: armed {} verified {} failed {}; freeze counts {}",
        hs["armed"], hs["verified"], hs["failed"], hs["freeze"]["counts"]
    ));
    let nerve_saw: Vec<String> = get(NERVE_PORT, "/evt?since=0&n=2000")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| e["kind"].as_str().map(str::to_string))
        .filter(|k| k.starts_with("gaia.heal") || k.starts_with("cortex"))
        .collect();
    log(format!("nerve ring cortex/heal events: {nerve_saw:?}"));
    assert!(
        nerve_saw.iter().any(|k| k == "gaia.heal.verified"),
        "{nerve_saw:?}"
    );
    assert!(
        nerve_saw.iter().any(|k| k == "gaia.heal.failed"),
        "{nerve_saw:?}"
    );
    assert_eq!(restarts(&dir, ONE) + restarts(&dir, MEM), 0);
    log(format!("logs kept in {}", dir.display()));
}

#[test]
fn freeze_env_out_of_range_refuses_to_start() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "cortex-freeze-bounds-{}-{}",
        std::process::id(),
        now_unix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cortex.json"), "{}").unwrap();
    for (k, bad) in [
        ("CORTEX_FREEZE", "yes"),
        ("CORTEX_FREEZE_PROBE_S", "1"),
        ("CORTEX_FREEZE_FAILS", "1"),
        ("CORTEX_FREEZE_MIN_S", "10"),
        ("CORTEX_FREEZE_COOLDOWN_S", "5"),
        ("CORTEX_FREEZE_PER_HOUR", "9"),
        ("CORTEX_FREEZE_TIMEOUT_S", "20"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"))
            .env_clear()
            .env("CORTEX_CONFIG", dir.join("cortex.json"))
            .env("CORTEX_LISTEN", format!("127.0.0.1:{BOUNDS_PORT}"))
            .env("CORTEX_DATA_DIR", dir.join("data"))
            .env("CORTEX_FREEZE", "1")
            .env(k, bad)
            .output()
            .unwrap();
        log(format!(
            "{k}={bad}: exit {:?} {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
        assert_eq!(out.status.code(), Some(2), "{k}={bad} must refuse");
    }
}
