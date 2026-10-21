// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 237 (cortex-incident-backfill): the REAL release `gaia-cortex`
//! against the REAL event-mode nerve (`openmind-nerve serve --mode event`,
//! release build of `swarm/nerve-event-driven` bb287cc), box only, loopback
//! only, slot 237 ports 21070-21079.
//!
//! Gated: `GALION_REAL=1` and `GALION_NERVE_BIN=<release openmind-nerve bb287cc>`.
//! Optional negative control: `GALION_CORTEX_BASE_BIN=<release gaia-cortex e982210>`.
//! Run: `cargo test --release -p gaia-cortex --test real_t237_backfill -- --nocapture`.
//!
//! Phases (one nerve + cortex, in order):
//!   P0 nerve A starts with 3 dead components (t237a/b/c): its warm-up cycle opens 3 incidents
//!   P1 base cortex (if given) never learns of them (the live VPS miss); the new cortex,
//!      started after the nerve, backfills all 3 from GET /incidents (R11 alerts, no action)
//!   P2 F3: tuner.service killed -> incident; 6 unrelated bursts keep it open; its own
//!      `Started` journal line resolves it in that burst
//!   P3 component a comes up: the nerve resolves its incident (incident.resolved carries only
//!      the id); the cortex maps the id to its cause and resolves its incident
//!   P4 component b comes up, then nerve A is SIGKILLed and nerve B starts on the same port:
//!      the cortex reconciles b (gone on B -> resolved) and follows c's new nerve id
//!      without a second alert
//!
//! Nothing here touches systemd, the VPS or the network beyond 127.0.0.1.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const NERVE_PORT: u16 = 21070;
const CORTEX_PORT: u16 = 21071;
const COMP_A: u16 = 21072;
const BASE_CORTEX_PORT: u16 = 21073;
const COMP_B: u16 = 21074;
const COMP_C: u16 = 21075;

const ID_FAILURE_RESULT: &str = "d9b373ed55a64feb8242e02dbe79a49c";
const ID_PROCESS_EXIT: &str = "98e322203f7a4ed290d09fe03c09fe15";
const ID_STARTED: &str = "39f53479d3a045ac8e11786248231fbf";

fn now_unix() -> u64 {
    std::time::SystemTime::now() // repo-check: allow (unix seconds for journal timestamps; printed times go through zagreb_time)
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn log(msg: impl AsRef<str>) {
    println!(
        "[t237 {}] {}",
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
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    if s.write_all(head.as_bytes()).is_err() {
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
        std::thread::sleep(Duration::from_millis(100));
    }
    t.elapsed()
}

fn n(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

struct Proc {
    child: Child,
}
impl Proc {
    fn pid(&self) -> u32 {
        self.child.id()
    }
    fn kill9(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Drop for Proc {
    fn drop(&mut self) {
        self.kill9();
    }
}

/// A component that answers every request with `status` until dropped
/// (200 `{"ok":true}` = healthy, 503 = degrading but not down).
struct Responder(Arc<AtomicBool>);
impl Responder {
    fn up(port: u16, status: u16) -> Responder {
        let l = TcpListener::bind(("127.0.0.1", port)).expect("component bind");
        l.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        std::thread::spawn(move || {
            while !s2.load(Ordering::SeqCst) {
                match l.accept() {
                    Ok((mut c, _)) => {
                        let _ = c.set_nonblocking(false);
                        let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut buf = [0u8; 2048];
                        let _ = c.read(&mut buf);
                        let body = if status == 200 {
                            r#"{"ok":true}"#
                        } else {
                            r#"{"ok":false,"status":"loading"}"#
                        };
                        let _ = c.write_all(format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(30)),
                }
            }
        });
        Responder(stop)
    }
}
impl Drop for Responder {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn work_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("cortex-t237-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start_nerve(bin: &Path, dir: &Path, fifo: &Path, tag: &str) -> Proc {
    let data = dir.join(format!("nerve-data-{tag}"));
    std::fs::create_dir_all(&data).unwrap();
    let comps: Vec<Value> = [
        ("t237-a", COMP_A, "t237a", true),
        ("t237-b", COMP_B, "t237b", false),
        ("t237-c", COMP_C, "t237c", false),
    ]
    .iter()
    .map(|(name, port, sys, crit)| json!({"name": name, "port": port, "path": "/health", "gaia": "DEMETER", "system": sys, "critical": crit}))
        .collect();
    std::fs::write(dir.join("components.json"), Value::Array(comps).to_string()).unwrap();
    let stderr = std::fs::File::create(dir.join(format!("nerve-{tag}.log"))).unwrap();
    let child = Command::new(bin)
        .args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{NERVE_PORT}"),
            "--log",
            &dir.join(format!("nerve-events-{tag}.jsonl"))
                .display()
                .to_string(),
        ])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_DATA_DIR", &data)
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_PIDFD", "0")
        .env("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display()))
        // 1 s dedupe so the cycle pokes in P3 are not folded (the VPS runs 600 s)
        .env("NERVE_EVT_DEDUPE", "1")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("spawn nerve");
    let p = Proc { child };
    wait_until("nerve /health", 30, || {
        http(NERVE_PORT, "GET", "/health", "").0 == 200
    });
    p
}

fn start_cortex(bin: &Path, dir: &Path, port: u16, tag: &str) -> Proc {
    let data = dir.join(format!("cortex-data-{tag}"));
    let stderr = std::fs::File::create(dir.join(format!("cortex-{tag}.log"))).unwrap();
    let child = Command::new(bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("CORTEX_LISTEN", format!("127.0.0.1:{port}"))
        .env("CORTEX_NERVE_URL", format!("http://127.0.0.1:{NERVE_PORT}"))
        .env("CORTEX_DATA_DIR", &data)
        .env("CORTEX_COALESCE_MS", "500")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .expect("spawn cortex");
    let p = Proc { child };
    wait_until("cortex /health", 20, || {
        http(port, "GET", "/health", "").0 == 200
    });
    p
}

struct Journal(std::fs::File);
impl Journal {
    fn entry(&mut self, id: &str, unit: &str, extra: Value) {
        let mut v = json!({
            "MESSAGE_ID": id, "UNIT": unit, "MESSAGE": format!("{unit}: t237"), "_PID": "1",
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
    fn started(&mut self, unit: &str) {
        self.entry(
            ID_STARTED,
            unit,
            json!({"JOB_RESULT": "done", "JOB_TYPE": "start"}),
        );
    }
}

/// Nerve open incidents: component -> (id, cause).
fn nerve_open() -> Vec<(String, String, String)> {
    get(NERVE_PORT, "/incidents")["open"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|i| {
            let comp = i["components"].as_array()?.first()?.as_str()?.to_string();
            Some((
                comp,
                i["id"].as_str()?.to_string(),
                i["cause"].as_str()?.to_string(),
            ))
        })
        .collect()
}

/// The nerve's raw event ring (`GET /events/stream?since=0`, read until
/// it goes quiet for 1 s): the exact lines the cortex receives.
fn nerve_ring() -> Vec<Value> {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", NERVE_PORT)) else {
        return vec![];
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    let _ = s.write_all(
        b"GET /events/stream?since=0 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
    );
    let mut out = Vec::new();
    let mut buf = [0u8; 65536];
    while let Ok(k) = s.read(&mut buf) {
        if k == 0 {
            break;
        }
        out.extend_from_slice(&buf[..k]);
    }
    String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .filter(|v| v.is_object())
        .collect()
}

fn cortex_open(port: u16, cause: &str) -> Option<Value> {
    get(port, "/incidents")["open"]
        .as_array()?
        .iter()
        .find(|i| i["cause"] == cause)
        .cloned()
}

fn cortex_resolved(port: u16, id: &Value) -> Option<Value> {
    get(port, "/incidents")["resolved"]
        .as_array()?
        .iter()
        .find(|i| i["id"] == *id)
        .cloned()
}

fn decisions_for(port: u16, subject: &str) -> Vec<Value> {
    get(port, "/decisions?n=500")
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d["subject"] == subject && d["rule"] != "R10-remediation")
        .collect()
}

fn gate() -> Option<PathBuf> {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and GALION_NERVE_BIN)");
        return None;
    }
    let bin = PathBuf::from(
        std::env::var("GALION_NERVE_BIN")
            .expect("GALION_REAL=1 needs GALION_NERVE_BIN=<release openmind-nerve bb287cc>"),
    );
    assert!(bin.is_file(), "GALION_NERVE_BIN {} missing", bin.display());
    Some(bin)
}

#[test]
fn real_t237_backfill_and_incident_lifetime() {
    let Some(nerve_bin) = gate() else { return };
    for p in [
        NERVE_PORT,
        CORTEX_PORT,
        COMP_A,
        BASE_CORTEX_PORT,
        COMP_B,
        COMP_C,
    ] {
        assert!(
            TcpListener::bind(("127.0.0.1", p)).is_ok(),
            "port {p} busy (slot 237 range 21070-21079)"
        );
    }
    let cortex_bin = PathBuf::from(env!("CARGO_BIN_EXE_gaia-cortex"));
    let dir = work_dir();
    let fifo = dir.join("journal.fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let mut summary = serde_json::Map::new();

    // ---- P0 nerve A: all three down (connection refused); a is critical
    // (hard), b and c are "critical": false (degrading, not hard: three
    // hard-down groups would fold into one stack-wide incident, seen in the
    // first run) -> three incidents in its warm-up cycle
    let mut nerve = start_nerve(&nerve_bin, &dir, &fifo, "a");
    let mut j = Journal(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    wait_until("nerve A warm-up incidents", 30, || nerve_open().len() >= 3);
    let before = nerve_open();
    let find = |v: &[(String, String, String)], c: &str| {
        v.iter()
            .find(|x| x.0 == c)
            .cloned()
            .unwrap_or_else(|| panic!("{c} not in {v:?}"))
    };
    let (_, a_id, a_cause) = find(&before, "t237-a");
    let (_, b_id, b_cause) = find(&before, "t237-b");
    let (_, c_id, c_cause) = find(&before, "t237-c");
    let nstats = get(NERVE_PORT, "/stats");
    log(format!(
        "P0 nerve A pid {} seq {}: open {a_id}={a_cause} {b_id}={b_cause} {c_id}={c_cause}",
        nerve.pid(),
        n(&nstats, "seq")
    ));
    summary.insert("P0".into(), json!({"nerve_seq": n(&nstats, "seq"), "open": before.iter().map(|x| json!({"component": x.0, "id": x.1, "cause": x.2})).collect::<Vec<_>>()}));
    let causes = [&a_cause, &b_cause, &c_cause].map(|c| format!("nerve-incident:{c}"));

    // ---- P1a negative control: the base cortex (e982210) misses them
    if let Ok(base) = std::env::var("GALION_CORTEX_BASE_BIN") {
        let mut b = start_cortex(Path::new(&base), &dir, BASE_CORTEX_PORT, "base");
        wait_until("base cortex subscribed", 20, || {
            n(&get(NERVE_PORT, "/stats"), "streams") == 1
        });
        std::thread::sleep(Duration::from_secs(4));
        let seen: Vec<&String> = causes
            .iter()
            .filter(|c| cortex_open(BASE_CORTEX_PORT, c).is_some())
            .collect();
        let open_n = get(BASE_CORTEX_PORT, "/incidents")["open"]
            .as_array()
            .map_or(0, |a| a.len());
        log(format!(
            "P1a base cortex e982210: nerve incidents seen {} of 3, open incidents {open_n}",
            seen.len()
        ));
        assert!(seen.is_empty(), "base cortex unexpectedly saw {seen:?}");
        summary.insert(
            "P1a_base".into(),
            json!({"seen": seen.len(), "open": open_n}),
        );
        b.kill9(); // the nerve notices the dead stream only at its next write (keepalive)
    }

    // ---- P1 new cortex started after the nerve: backfill
    let t = Instant::now();
    let mut cortex = start_cortex(&cortex_bin, &dir, CORTEX_PORT, "a");
    wait_until("cortex backfilled 3 nerve incidents", 20, || {
        causes.iter().all(|c| cortex_open(CORTEX_PORT, c).is_some())
    });
    let ms = t.elapsed().as_millis() as u64;
    let st = get(CORTEX_PORT, "/stats");
    assert_eq!(n(&st, "nerve_incidents_backfilled"), 3, "{st}");
    assert_eq!(n(&st, "actions_requested"), 0, "{st}");
    let mut alerts = 0;
    for c in &causes {
        let d = decisions_for(CORTEX_PORT, c);
        assert_eq!(d.len(), 1, "{c}: {d:?}");
        assert_eq!(d[0]["rule"], "R11-nerve-incident");
        assert!(
            d[0]["reason"].as_str().unwrap_or("").contains("backfill"),
            "{}",
            d[0]
        );
        assert!(d[0]["action"].is_null());
        alerts += d[0]["alert"].as_bool().unwrap_or(false) as u32;
    }
    let map = get(CORTEX_PORT, "/incidents")["nerve_incidents"].clone();
    assert_eq!(map[&a_cause], a_id.as_str(), "{map}");
    let c_inc = cortex_open(CORTEX_PORT, &causes[2]).unwrap()["id"].clone();
    log(format!("P1 cortex pid {} backfilled 3/3 in {ms} ms (R11 x3, alerts {alerts}, 0 actions); map {map}", cortex.pid()));
    summary.insert(
        "P1".into(),
        json!({"backfilled": 3, "ms": ms, "alerts": alerts, "actions": 0}),
    );

    // ---- P2 F3: an open unit incident survives unrelated bursts
    j.kill("tuner.service");
    wait_until("tuner decision", 15, || {
        !decisions_for(CORTEX_PORT, "unit:tuner.service").is_empty()
    });
    let tun = cortex_open(CORTEX_PORT, "unit:tuner.service").expect("tuner incident");
    let tun_id = tun["id"].clone();
    let b0 = n(&get(CORTEX_PORT, "/stats"), "batches");
    for u in [
        "ssh.service",
        "cloudflared.service",
        "postfix.service",
        "galion-backup.service",
        "hades-gate.service",
        "console.service",
    ] {
        j.kill(u);
        let s = format!("unit:{u}");
        wait_until(&format!("decision {u}"), 15, || {
            !decisions_for(CORTEX_PORT, &s).is_empty()
        });
        std::thread::sleep(Duration::from_millis(700));
    }
    let batches = n(&get(CORTEX_PORT, "/stats"), "batches") - b0;
    let still = cortex_open(CORTEX_PORT, "unit:tuner.service")
        .expect("F3: tuner incident resolved by unrelated bursts");
    assert_eq!(still["id"], tun_id);
    assert_eq!(still["last_seen"], tun["last_seen"], "{still}");
    log(format!(
        "P2 tuner incident {tun_id} still open after {batches} unrelated batches (last_seen {})",
        still["last_seen"]
    ));
    let t = Instant::now();
    j.started("tuner.service");
    wait_until(
        "tuner incident resolved by its own Started line",
        15,
        || cortex_open(CORTEX_PORT, "unit:tuner.service").is_none(),
    );
    let r = cortex_resolved(CORTEX_PORT, &tun_id).expect("resolved entry");
    assert_eq!(r["resolved_by"], "cleared", "{r}");
    let ms = t.elapsed().as_millis() as u64;
    log(format!(
        "P2 tuner Started -> incident resolved_by cleared in {ms} ms (duration {} s)",
        r["duration_s"]
    ));
    summary.insert("P2".into(), json!({"unrelated_batches": batches, "still_open": true, "resolve_ms": ms, "resolved_by": "cleared"}));

    // ---- P3 the nerve resolves a's incident; its incident.resolved has only the id
    let a_inc = cortex_open(CORTEX_PORT, &causes[0]).expect("a open in cortex")["id"].clone();
    let comp_a = Responder::up(COMP_A, 200);
    let t = Instant::now();
    let mut pokes = 0;
    while nerve_open().iter().any(|x| x.1 == a_id) {
        assert!(
            t.elapsed() < Duration::from_secs(120),
            "nerve never resolved {a_id}"
        );
        j.failed("postfix.service"); // a stack event -> one engine cycle (min gap 5 s)
        pokes += 1;
        std::thread::sleep(Duration::from_secs(6));
    }
    let nerve_ms = t.elapsed().as_millis() as u64;
    let resolved_ev: Vec<Value> = nerve_ring()
        .into_iter()
        .filter(|e| e["kind"] == "incident.resolved" && e["data"]["id"] == a_cause.as_str())
        .collect();
    wait_until("cortex resolved a", 20, || {
        cortex_open(CORTEX_PORT, &causes[0]).is_none()
    });
    let cortex_ms = t.elapsed().as_millis() as u64 - nerve_ms;
    let st = get(CORTEX_PORT, "/stats");
    let r = cortex_resolved(CORTEX_PORT, &a_inc).expect("a resolved entry");
    assert_eq!(r["resolved_by"], "cleared", "{r}");
    let a_dec = decisions_for(CORTEX_PORT, &causes[0]);
    assert!(
        a_dec[0]["reason"]
            .as_str()
            .unwrap_or("")
            .ends_with("resolved"),
        "{a_dec:?}"
    );
    assert_eq!(
        resolved_ev.len(),
        1,
        "the nerve published one incident.resolved for {a_cause}"
    );
    let data_keys: Vec<String> = resolved_ev
        .first()
        .and_then(|e| e["data"].as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    log(format!(
        "P3 nerve resolved {a_id} after {pokes} pokes / {nerve_ms} ms; its incident.resolved data keys {data_keys:?}; cortex resolved {a_inc} (resolved_by cleared) +{cortex_ms} ms; map now {}",
        get(CORTEX_PORT, "/incidents")["nerve_incidents"]
    ));
    assert!(
        get(CORTEX_PORT, "/incidents")["nerve_incidents"][&a_cause].is_null(),
        "{st}"
    );
    summary.insert("P3".into(), json!({"pokes": pokes, "nerve_ms": nerve_ms, "cortex_extra_ms": cortex_ms, "nerve_resolved_data_keys": data_keys}));

    // ---- P4 nerve restart: b resolved meanwhile (reconcile), c follows its new id
    let comp_b = Responder::up(COMP_B, 200);
    assert!(
        nerve_open().iter().any(|x| x.1 == b_id),
        "nerve A still holds b (no cycle yet)"
    );
    let dec_c = decisions_for(CORTEX_PORT, &causes[2]).len();
    let b_inc = cortex_open(CORTEX_PORT, &causes[1]).expect("b open in cortex")["id"].clone();
    drop(j);
    nerve.kill9();
    let _ = std::fs::remove_file(&fifo);
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let t = Instant::now();
    let nerve_b = start_nerve(&nerve_bin, &dir, &fifo, "b");
    let mut j = Journal(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    wait_until("nerve B warm-up c incident", 30, || {
        nerve_open().iter().any(|x| x.0 == "t237-c")
    });
    let after = nerve_open();
    let (_, c2_id, _) = find(&after, "t237-c");
    assert!(
        after.iter().all(|x| x.0 != "t237-a" && x.0 != "t237-b"),
        "{after:?}"
    );
    wait_until("cortex reconciled b after the nerve restart", 60, || {
        cortex_open(CORTEX_PORT, &causes[1]).is_none()
    });
    let ms = t.elapsed().as_millis() as u64;
    let st = get(CORTEX_PORT, "/stats");
    let inc = get(CORTEX_PORT, "/incidents");
    assert!(n(&st, "nerve_restarts") >= 1, "{st}");
    assert!(n(&st, "nerve_incidents_reconciled") >= 1, "{st}");
    assert!(n(&st, "nerve_incidents_remapped") >= 1, "{st}");
    assert_eq!(
        cortex_resolved(CORTEX_PORT, &b_inc).unwrap()["resolved_by"],
        "cleared"
    );
    assert_eq!(
        cortex_open(CORTEX_PORT, &causes[2]).unwrap()["id"],
        c_inc,
        "c keeps its cortex incident"
    );
    assert_eq!(
        inc["nerve_incidents"][&c_cause],
        c2_id.as_str(),
        "{}",
        inc["nerve_incidents"]
    );
    let rb: Vec<Value> = decisions_for(CORTEX_PORT, &causes[1]);
    assert!(
        rb[0]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("reconciled"),
        "{rb:?}"
    );
    // give the replayed ring a moment: no second alert for c
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        decisions_for(CORTEX_PORT, &causes[2]).len(),
        dec_c,
        "no second alert for c"
    );
    log(format!(
        "P4 nerve B pid {} (c {c_id} -> {c2_id}): b reconciled+resolved in {ms} ms, c remapped (same cortex incident {c_inc}, decisions {dec_c} unchanged)",
        nerve_b.pid()
    ));
    summary.insert("P4".into(), json!({"reconcile_ms": ms, "reconciled": n(&st, "nerve_incidents_reconciled"), "remapped": n(&st, "nerve_incidents_remapped"), "nerve_b_open": after.iter().map(|x| json!({"component": x.0, "id": x.1})).collect::<Vec<_>>(), "c_new_id": c2_id, "c_decisions_unchanged": dec_c}));

    // ---- end: dry-run only, nothing executed
    j.started("console.service");
    let st = get(CORTEX_PORT, "/stats");
    assert_eq!(n(&st, "actions_executed"), 0, "{st}");
    assert_eq!(n(&st, "llm_calls"), 0, "{st}");
    summary.insert("final_stats".into(), st.clone());
    log(format!("final cortex stats {st}"));
    if let Ok(p) = std::env::var("GALION_T237_OUT") {
        let _ = std::fs::write(
            p,
            serde_json::to_string_pretty(&Value::Object(summary)).unwrap(),
        );
    }
    drop(comp_a);
    drop(comp_b);
    cortex.kill9();
    drop(nerve_b);
    let _ = std::fs::remove_dir_all(&dir);
}
