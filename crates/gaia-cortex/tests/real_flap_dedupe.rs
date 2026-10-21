// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 235 (T5.1 finding F1): the REAL release `gaia-cortex` against the
//! REAL release event-mode nerve (`openmind-nerve serve --mode event`,
//! `swarm/nerve-event-driven` bb287cc, the build live on the VPS), default
//! nerve dedupe 600 s as on the VPS. Box only, loopback only, slot 235
//! ports 21050-21059. Nothing touches systemd: the nerve is dry-run
//! (`NERVE_DRY_RUN=1`, actions not live) and reads its journal from a FIFO.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve>`.
//! `cargo test --release -p gaia-cortex --test real_flap_dedupe -- --nocapture --test-threads 1`
//!
//! Phases (a fresh nerve + cortex each):
//!   before   CORTEX_REPEAT_POLL_S=0 (the e982210 transport): tuner.service killed, then 5 more
//!            failures; the nerve folds them (its ring count 6), the cortex keeps ONE decision,
//!            no circuit (F1 reproduced)
//!   after    CORTEX_REPEAT_POLL_S=2: the same journal; the cortex reads the count rise from
//!            GET /evt and decides R2-circuit-open (flapping), escalated, no 2nd restart
//!   caps-dry dry-run cortex, flap rule out of the way (flap_failures 100): one folded repeat
//!            at a time -> restart, restart, R3-cap (2/unit/h although the dry-run nerve never
//!            rate-limits); then 6 more units -> total 6 restarts, then R3-cap-total
//!   caps-live CORTEX_ACTIONS=live against the dry-run nerve: same per-unit cap (2, then R3-cap)

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 21050;
const CORTEX_PORT: u16 = 21051;
const DEAD_PORT: u16 = 21059;

const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[235 {}] {}",
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

fn n(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout ({secs} s) waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
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
            "MESSAGE_ID": id, "UNIT": unit, "MESSAGE": format!("{unit}: slot 235 flap"), "_PID": "1",
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
    fn failed(&mut self, unit: &str) {
        self.entry(ID_FAILURE_RESULT, unit, json!({"UNIT_RESULT": "exit-code"}));
    }
}

/// One phase: a fresh nerve (dedupe 600 s) + cortex in their own dir.
struct Stack {
    _nerve: Proc,
    _cortex: Proc,
    j: Journal,
    dir: PathBuf,
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(bin: &Path, tag: &str, cortex_env: &[(&str, &str)], cortex_cfg: Value) -> Stack {
    let dir = std::env::temp_dir().join(format!("cortex-235-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("nerve-data")).unwrap();
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"f235-dead","port":{DEAD_PORT},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
    )
    .unwrap();
    let nerve = Command::new(bin)
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
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_DATA_DIR", dir.join("nerve-data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
        .env("NERVE_EVT_DEDUPE", "600")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("nerve.log")).unwrap())
        .spawn()
        .expect("spawn nerve");
    let nerve = Proc(nerve);
    wait_until("nerve /health", 30, || {
        http(NERVE_PORT, "GET", "/health", "").0 == 200
    });
    let j = Journal(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    let cfg = dir.join("cortex.json");
    std::fs::write(&cfg, cortex_cfg.to_string()).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_gaia-cortex"));
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("CORTEX_CONFIG", &cfg)
        .env("CORTEX_LISTEN", format!("127.0.0.1:{CORTEX_PORT}"))
        .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
        .env("CORTEX_DATA_DIR", dir.join("cortex-data"))
        .env("CORTEX_COALESCE_MS", "500");
    for (k, v) in cortex_env {
        c.env(k, v);
    }
    let cortex = Proc(
        c.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(dir.join("cortex.log")).unwrap())
            .spawn()
            .expect("spawn cortex"),
    );
    wait_until("cortex /health", 20, || {
        http(CORTEX_PORT, "GET", "/health", "").0 == 200
    });
    wait_until("cortex subscribed", 20, || {
        n(&get(NERVE_PORT, "/stats"), "streams") == 1
    });
    // let the warm-up (dead component) settle
    std::thread::sleep(Duration::from_secs(3));
    Stack {
        _nerve: nerve,
        _cortex: cortex,
        j,
        dir,
    }
}

/// Cortex decisions on a subject, oldest first, without R10 echoes.
fn decisions(subject: &str) -> Vec<Value> {
    let mut v: Vec<Value> = get(CORTEX_PORT, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == subject && d["rule"] != "R10-remediation")
        .collect();
    v.sort_by_key(|d| {
        d["id"]
            .as_str()
            .and_then(|s| s.strip_prefix("dec-"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    v
}

fn rules_of(d: &[Value]) -> Vec<String> {
    d.iter()
        .map(|x| x["rule"].as_str().unwrap_or("").to_string())
        .collect()
}

fn nerve_restarts_for(unit: &str) -> Vec<Value> {
    get(NERVE_PORT, "/actions")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a["unit"] == unit && a["action"] == "restart")
        .collect()
}

/// The ring's unit.failed event for `unit` (the one the nerve folds into).
fn ring_failed(unit: &str) -> Value {
    get(NERVE_PORT, "/evt?since=0&n=1000")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .find(|e| e["kind"] == "unit.failed" && e["unit"] == unit)
        .unwrap_or(Value::Null)
}

fn gate() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and GALION_NERVE_BIN)");
        return None;
    }
    let bin = PathBuf::from(std::env::var("GALION_NERVE_BIN").expect(
        "GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve from swarm/nerve-event-driven bb287cc>",
    ));
    assert!(bin.is_file(), "GALION_NERVE_BIN {} missing", bin.display());
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (slot 235 range 21050-21059)"
        );
    }
    Some(bin)
}

const TUNER: &str = "tuner.service";
const SUBJ: &str = "unit:tuner.service";

/// kill + 5 more failures 700 ms apart (all inside the nerve's 600 s window).
fn crash_loop(j: &mut Journal) {
    j.kill(TUNER);
    wait_until("first decision on tuner", 15, || {
        !decisions(SUBJ).is_empty()
    });
    for _ in 0..5 {
        j.failed(TUNER);
        std::thread::sleep(Duration::from_millis(700));
    }
}

#[test]
fn flap_rule_sees_folded_repeats_before_after() {
    let Some(bin) = gate() else { return };
    let mut summary = serde_json::Map::new();

    // ---------------------------------------------------------------- before
    {
        let mut s = start(&bin, "before", &[("CORTEX_REPEAT_POLL_S", "0")], json!({}));
        let d0 = n(&get(NERVE_PORT, "/stats"), "deduped");
        crash_loop(&mut s.j);
        std::thread::sleep(Duration::from_secs(8));
        let ns = get(NERVE_PORT, "/stats");
        let ring = ring_failed(TUNER);
        let d = decisions(SUBJ);
        let cs = get(CORTEX_PORT, "/stats");
        log(format!(
            "BEFORE (repeat poll off): nerve deduped +{}, ring unit.failed seq {} count {}; cortex decisions {:?}; circuit_open {}",
            n(&ns, "deduped") - d0,
            ring["seq"],
            ring["count"],
            rules_of(&d),
            get(CORTEX_PORT, "/incidents")["units"][TUNER]["circuit_open"]
        ));
        assert!(n(&ns, "deduped") >= d0 + 5, "{ns}");
        assert_eq!(ring["count"], 6, "nerve folded 5 into the event: {ring}");
        assert_eq!(rules_of(&d), ["R3-known-crash"], "F1 reproduced: {d:?}");
        assert!(cs["repeat_watch"].is_null(), "{cs}");
        assert_eq!(nerve_restarts_for(TUNER).len(), 1);
        summary.insert(
            "before".into(),
            json!({"nerve_deduped": n(&ns, "deduped") - d0, "ring_count": ring["count"],
                   "cortex_rules": rules_of(&d), "nerve_restart_requests": 1}),
        );
    }

    // ---------------------------------------------------------------- after
    {
        let mut s = start(&bin, "after", &[("CORTEX_REPEAT_POLL_S", "2")], json!({}));
        let has_r2 = || {
            decisions(SUBJ)
                .iter()
                .any(|d| d["rule"] == "R2-circuit-open")
        };
        s.j.kill(TUNER);
        wait_until("first decision on tuner", 15, || {
            !decisions(SUBJ).is_empty()
        });
        // the same 5 folded failures, 700 ms apart; note when R2 shows up
        let t = Instant::now();
        let mut r2_after: Option<(usize, u128)> = None;
        for i in 1..=5 {
            s.j.failed(TUNER);
            let w = Instant::now();
            while w.elapsed() < Duration::from_millis(700) {
                if r2_after.is_none() && has_r2() {
                    r2_after = Some((i, t.elapsed().as_millis()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let el = wait_until("R2 after the folded repeats", 20, has_r2);
        let r2_after = r2_after.unwrap_or((5, t.elapsed().as_millis()));
        std::thread::sleep(Duration::from_secs(5));
        let d = decisions(SUBJ);
        let cs = get(CORTEX_PORT, "/stats");
        let ring = ring_failed(TUNER);
        let r2 = d.iter().find(|d| d["rule"] == "R2-circuit-open").unwrap();
        let inc = get(CORTEX_PORT, "/incidents");
        log(format!(
            "AFTER (repeat poll 2 s): ring count {}; cortex decisions {:?}; R2 seen after {} folded failure write(s), {} ms after the first ({} ms extra wait), reason {:?}, escalate {}, repeat_of {}; repeat_watch {}; nerve restart requests {}",
            ring["count"],
            rules_of(&d),
            r2_after.0,
            r2_after.1,
            el.as_millis(),
            r2["reason"],
            r2["escalate"],
            r2["repeat_of"],
            cs["repeat_watch"],
            nerve_restarts_for(TUNER).len()
        ));
        assert_eq!(ring["count"], 6);
        assert_eq!(d[0]["rule"], "R3-known-crash");
        assert_eq!(r2["escalate"], true, "{r2}");
        assert_eq!(r2["repeat_of"], json!([ring["seq"]]), "{r2}");
        assert_eq!(inc["units"][TUNER]["circuit_open"], true, "{inc}");
        assert_eq!(
            nerve_restarts_for(TUNER).len(),
            1,
            "no restart while flapping"
        );
        assert!(n(&cs["repeat_watch"], "repeats") >= 5, "{cs}");
        assert_eq!(n(&cs, "actions_executed"), 0);
        summary.insert(
            "after".into(),
            json!({"ring_count": ring["count"], "cortex_rules": rules_of(&d),
                   "r2_reason": r2["reason"], "r2_after_folded_writes": r2_after.0, "r2_ms_after_first_folded": r2_after.1 as u64,
                   "repeat_watch": cs["repeat_watch"], "nerve_restart_requests": 1}),
        );
    }

    // ---------------------------------------------------------------- caps (dry-run and live cortex)
    for mode in ["dry-run", "live"] {
        let mut s = start(
            &bin,
            &format!("caps-{mode}"),
            &[("CORTEX_REPEAT_POLL_S", "2"), ("CORTEX_ACTIONS", mode)],
            json!({"flap_failures": 100}),
        );
        assert_eq!(get(CORTEX_PORT, "/health")["mode"], mode);
        s.j.kill(TUNER);
        wait_until("cap phase: first decision", 15, || {
            decisions(SUBJ).len() == 1
        });
        for want in 2..=3 {
            std::thread::sleep(Duration::from_millis(600));
            s.j.failed(TUNER); // folded by the nerve; reaches the cortex via the ring
            wait_until(&format!("cap phase: decision {want}"), 20, || {
                decisions(SUBJ).len() >= want
            });
        }
        let d = decisions(SUBJ);
        let na = nerve_restarts_for(TUNER);
        let ring = ring_failed(TUNER);
        log(format!(
            "CAPS cortex {mode}: tuner ring count {} -> cortex {:?}; nerve saw {} restart request(s), results {:?}",
            ring["count"],
            rules_of(&d),
            na.len(),
            na.iter().map(|a| a["result"].clone()).collect::<Vec<_>>()
        ));
        assert_eq!(
            rules_of(&d),
            ["R3-known-crash", "R3-known-crash", "R3-cap"],
            "{d:?}"
        );
        assert_eq!(na.len(), 2, "cap 2/unit/h held by the cortex: {na:?}");
        assert!(na.iter().all(|a| a["result"] == "dry-run"), "{na:?}");
        let mut m = json!({"tuner_rules": rules_of(&d), "nerve_requests": na.len(),
                           "nerve_results": na.iter().map(|a| a["result"].clone()).collect::<Vec<_>>()});
        if mode == "dry-run" {
            // total cap: 2 used by tuner, 4 more units fit, the next ones do not
            let units = [
                "openmind-memory.service",
                "hermes-rs.service",
                "link-watch-rs.service",
                "galion-desk.service",
                "galion-one.service",
                "console.service",
            ];
            let mut seen = vec![];
            for u in units {
                s.j.kill(u);
                let subj = format!("unit:{u}");
                wait_until(&format!("total cap: {u}"), 15, || {
                    !decisions(&subj).is_empty()
                });
                seen.push(decisions(&subj)[0]["rule"].as_str().unwrap().to_string());
                std::thread::sleep(Duration::from_millis(1200));
            }
            let cs = get(CORTEX_PORT, "/stats");
            let total: usize = get(NERVE_PORT, "/actions")
                .as_array()
                .map(|a| a.iter().filter(|x| x["action"] == "restart").count())
                .unwrap_or(0);
            log(format!(
                "CAPS total (dry-run): {:?}; restarts_last_hour {}; nerve restart requests {total}",
                seen, cs["restarts_last_hour"]
            ));
            assert_eq!(
                seen,
                [
                    "R3-known-crash",
                    "R3-known-crash",
                    "R3-known-crash",
                    "R3-known-crash",
                    "R3-cap-total",
                    "R3-cap-total"
                ]
            );
            assert_eq!(total, 6);
            assert_eq!(cs["restarts_last_hour"], 6);
            m["total_cap_rules"] = json!(seen);
            m["nerve_restart_requests_total"] = json!(total);
        }
        let cs = get(CORTEX_PORT, "/stats");
        assert_eq!(n(&cs, "actions_executed"), 0, "{cs}");
        summary.insert(format!("caps_{mode}"), m);
    }
    log(format!(
        "SUMMARY {}",
        serde_json::to_string(&Value::Object(summary)).unwrap()
    ));
    for p in [NERVE_PORT, CORTEX_PORT] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} freed"
        );
    }
}
