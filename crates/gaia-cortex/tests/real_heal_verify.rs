// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Heal verify (the missing VERIFY step of the live heal loop), REAL: the
//! release `gaia-cortex` from this branch against the REAL release
//! event-mode nerve (`GALION_NERVE_BIN`, the 18a5a72 build live on the VPS).
//! Box only, loopback only, ports 23200-23209 (inside GAIA_TEST_PORTS
//! 19000-23999). Nothing touches systemd: the nerve's actions are live but
//! its `systemctl` is a fake script first on `PATH` (it logs the call and
//! answers `show` from a per-unit state file), its journal is a FIFO.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! `GALION_REAL=1 GALION_NERVE_BIN=... cargo test --release -p gaia-cortex --test real_heal_verify -- --nocapture --test-threads 1`
//!
//! Cases (one nerve + cortex, `CORTEX_ACTIONS=live`, `CORTEX_HEAL_VERIFY_S=30`):
//!   verified  tuner.service killed -> R3 restart executed by the nerve (fake systemctl ran)
//!             -> journal `Started` -> nerve holds 3 s -> `unit.state recovered` -> cortex
//!             `/heals` verified by nerve-recovery, well before the 30 s deadline
//!   failed    galion-desk.service killed, its systemd state stays `failed` -> restart executed
//!             -> no recovery -> at the deadline the cortex reads nerve `/state` (unit failed)
//!             -> R12-heal-failed, circuit open; the next crash is NOT restarted again
//!   bounds    CORTEX_HEAL_VERIFY_S=5 refuses to start

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 23200;
const CORTEX_PORT: u16 = 23201;

const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";
const ID_STARTED: &str = "39f53479d3a045ac8e11786248231fbf";

const TUNER: &str = "tuner.service";
const DESK: &str = "galion-desk.service";

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[heal-verify {}] {}",
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
            "MESSAGE_ID": id, "UNIT": unit, "MESSAGE": format!("{unit}: heal-verify real test"), "_PID": "1",
            "__REALTIME_TIMESTAMP": (now_unix() * 1_000_000).to_string(),
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        writeln!(self.0, "{v}").unwrap();
        self.0.flush().unwrap();
    }
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

fn heal_for(unit: &str) -> Value {
    get(CORTEX_PORT, "/heals")["recent"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|h| h["unit"] == unit)
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
            .expect("GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve 18a5a72>"),
    );
    assert!(bin.is_file(), "GALION_NERVE_BIN {} missing", bin.display());
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (heal-verify range 23200-23209)"
        );
    }
    Some(bin)
}

#[test]
fn heal_verify_against_the_real_nerve() {
    let Some(bin) = gate() else { return };
    // unique dir per run; left in /tmp for inspection (no deletes)
    let dir = std::env::temp_dir().join(format!(
        "cortex-heal-verify-{}-{}",
        std::process::id(),
        now_unix()
    ));
    std::fs::create_dir_all(dir.join("nerve-data")).unwrap();
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
        r#"[{"name":"tuner","unit":"tuner.service","gaia":"DEMETER"},
            {"name":"galion-desk","unit":"galion-desk.service","gaia":"DEMETER"}]"#,
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
            // reflex layer on (as on the VPS) so POST /actions may run; every
            // component has restarts_per_hour 0, so the nerve never restarts on its own
            .env("NERVE_REFLEX", "1")
            .env("NERVE_SYSTEMD", "1")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_EVT_PIDFD", "0")
            .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
            .env("NERVE_EVT_RECOVER_HOLD", "3")
            // short dedupe so the 2nd desk crash reaches the cortex as a fresh event
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
            .env("CORTEX_HEAL_VERIFY_S", "30")
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
    assert_eq!(h["heal_verify"], json!(true), "{h}");
    std::thread::sleep(Duration::from_secs(3));

    // ------------------------------------------------------------- verified
    j.kill(TUNER);
    wait_until("nerve ran the tuner restart", 20, || {
        calls(&dir).iter().any(|c| c.contains(TUNER))
    });
    log(format!("fake systemctl: {:?}", calls(&dir)));
    wait_until("tuner heal check armed", 10, || {
        get(CORTEX_PORT, "/heals")["pending_checks"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c["unit"] == TUNER))
    });
    j.started(TUNER);
    let took = wait_until("tuner verified", 25, || !heal_for(TUNER).is_null());
    let ht = heal_for(TUNER);
    log(format!("tuner heal ({took:?}): {ht}"));
    assert_eq!(ht["outcome"], "verified", "{ht}");
    assert_eq!(ht["by"], "nerve-recovery", "{ht}");
    assert!(ht["heal_s"].as_u64().unwrap_or(99) < 30, "{ht}");
    let rt = rules_for(TUNER);
    log(format!("tuner rules: {rt:?}"));
    assert_eq!(rt.first().map(String::as_str), Some("R3-known-crash"));
    assert!(!rt.iter().any(|r| r.starts_with("R12")), "{rt:?}");
    assert_eq!(
        calls(&dir).iter().filter(|c| c.contains(TUNER)).count(),
        1,
        "one restart"
    );

    // --------------------------------------------------------------- failed
    std::fs::write(dir.join("units").join(DESK), "failed").unwrap();
    j.kill(DESK);
    wait_until("nerve ran the desk restart", 20, || {
        calls(&dir).iter().any(|c| c.contains(DESK))
    });
    let armed = Instant::now();
    log("desk restart executed; its unit stays failed; waiting for the 30 s deadline");
    let st = get(NERVE_PORT, "/state");
    let desk_state = st["components"]
        .as_array()
        .and_then(|a| a.iter().find(|c| c["unit"] == DESK).cloned())
        .unwrap_or(Value::Null);
    log(format!("nerve /state desk: {desk_state}"));
    wait_until("desk judged", 60, || !heal_for(DESK).is_null());
    let hd = heal_for(DESK);
    log(format!("desk heal after {:?}: {hd}", armed.elapsed()));
    assert_eq!(hd["outcome"], "failed", "{hd}");
    assert_eq!(hd["by"], "nerve-state", "{hd}");
    assert!(
        armed.elapsed() >= Duration::from_secs(20),
        "deadline, not early"
    );
    let r = rules_for(DESK);
    log(format!("desk rules: {r:?}"));
    assert!(r.contains(&"R12-heal-failed".to_string()), "{r:?}");
    // the circuit is open: the next crash is not restarted again
    let before = calls(&dir).iter().filter(|c| c.contains(DESK)).count();
    std::thread::sleep(Duration::from_secs(2));
    j.kill(DESK);
    wait_until("decision on the 2nd desk crash", 15, || {
        rules_for(DESK).len() > r.len()
    });
    std::thread::sleep(Duration::from_secs(2));
    let r2 = rules_for(DESK);
    log(format!("desk rules after the 2nd crash: {r2:?}"));
    assert_eq!(
        calls(&dir).iter().filter(|c| c.contains(DESK)).count(),
        before,
        "circuit open: no second restart"
    );
    assert!(
        r2[r.len()..].iter().any(|x| x.starts_with("R2")),
        "circuit-open decision expected: {r2:?}"
    );
    let hs = get(CORTEX_PORT, "/heals");
    log(format!(
        "/heals summary: armed {} verified {} failed {} success_rate {}",
        hs["armed"], hs["verified"], hs["failed"], hs["success_rate"]
    ));
    assert_eq!(
        (hs["verified"].as_u64(), hs["failed"].as_u64()),
        (Some(1), Some(1))
    );
    let nerve_saw: Vec<String> = get(NERVE_PORT, "/evt?since=0&n=1000")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| e["kind"].as_str().map(str::to_string))
        .filter(|k| k.starts_with("gaia.heal"))
        .collect();
    log(format!("nerve ring heal events: {nerve_saw:?}"));
    assert!(
        nerve_saw.iter().any(|k| k == "gaia.heal.verified"),
        "{nerve_saw:?}"
    );
    assert!(
        nerve_saw.iter().any(|k| k == "gaia.heal.failed"),
        "{nerve_saw:?}"
    );
    log(format!("logs kept in {}", dir.display()));
}

#[test]
fn heal_window_out_of_range_refuses_to_start() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "cortex-heal-bounds-{}-{}",
        std::process::id(),
        now_unix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cortex.json"), "{}").unwrap();
    for bad in ["5", "4000", "soon"] {
        let out = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"))
            .env_clear()
            .env("CORTEX_CONFIG", dir.join("cortex.json"))
            .env("CORTEX_LISTEN", "127.0.0.1:23209")
            .env("CORTEX_DATA_DIR", dir.join("data"))
            .env("CORTEX_HEAL_VERIFY_S", bad)
            .output()
            .unwrap();
        log(format!(
            "CORTEX_HEAL_VERIFY_S={bad}: exit {:?} {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
        assert!(!out.status.success(), "{bad} must refuse");
    }
}
