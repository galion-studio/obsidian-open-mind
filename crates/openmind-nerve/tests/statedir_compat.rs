// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T4.1(a) state-dir compatibility rehearsal (slot 223), against the real
//! `openmind-nerve` binary on the box, in both polling and event mode.
//!
//! The old `/var/lib/galion-nerve` layout is rebuilt from read-only GETs of
//! the live VPS nerve (127.0.0.1:8670 `/state`, `/alerts`, `/self-eval`,
//! captured 2026-10-21 22:55 +02:00 into `tests/fixtures/statedir-live/`;
//! no file was copied from the VPS, nothing secret is in them):
//!
//! - `reflex.json`: the live budget units, with restarts and rollbacks in
//!   every timestamp form an older layout may hold (int, float, numeric
//!   string, RFC 3339) and unknown top-level keys;
//! - `mode`: the live mode with a trailing newline;
//! - `alerts.json`: the live alert list (oldest first, as on disk), one
//!   entry with an unknown field;
//! - `self-eval/`: `latest.json`/`latest.md`, timestamped history in both
//!   the offset and the legacy offset-less name form, an unknown file;
//! - `evt-llm-spend.json`: an older spend record (no `refused`, an unknown
//!   field) with 1.25 USD already spent;
//! - `state.json`: the live state (derived; rewritten every cycle).
//!
//! Checks: no budget reset (restart counts, last rollbacks, spend total),
//! no lost history (alerts, self-eval files), unknown fields tolerated and
//! kept, mode honoured, and a damaged layout fails closed with copies kept.
//! Ports 20930-20939 (slot 223). No root, no sandbox, no network.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/statedir-live")
}

fn fixture(name: &str) -> Value {
    let p = fixtures().join(name);
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn http(port: u16, method: &str, path: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let _ = write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, body)
}

fn get_json(port: u16, path: &str) -> Value {
    let (c, b) = http(port, "GET", path);
    assert_eq!(c, 200, "{path}: {b}");
    serde_json::from_str(&b).unwrap_or_else(|e| panic!("{path}: {e}: {b}"))
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Nerve(Child);
impl Drop for Nerve {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-t41-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

/// Start the real binary on `port`. `event`: `serve --mode event`.
fn start(port: u16, dir: &Path, event: bool, extra: &[(&str, String)]) -> Nerve {
    // One dead component on our own range: the cycle has something to say
    // without touching any real service.
    std::fs::write(
        dir.join("components.json"),
        r#"[{"name":"t41-dead","port":20939,"path":"/health","gaia":"DEMETER","system":"tuner"}]"#,
    )
    .unwrap();
    let mut c = Command::new(BIN);
    c.arg("serve");
    if event {
        c.args([
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--log",
            &dir.join("events.jsonl").display().to_string(),
        ]);
    }
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_LISTEN_SCOPE", "20930-20939")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in extra {
        c.env(k, v);
    }
    let n = Nerve(c.spawn().expect("spawn nerve"));
    wait_until("first cycle", 40, || {
        let (code, b) = http(port, "GET", "/state");
        code == 200
            && serde_json::from_str::<Value>(&b).is_ok_and(|v| v["cycle"].as_u64() >= Some(1))
    });
    n
}

const H: u64 = 3600;

/// Restarts (inside the last hour) the old reflex.json carries per unit.
const EXPECT_LIVE_1H: &[(&str, u64)] = &[
    ("hermes-rs.service", 2),
    ("galion-one.service", 2),
    ("cloudflared.service", 1),
];

fn rfc3339(secs: u64) -> String {
    // UTC "Z" form: an older writer may not have used Zagreb offsets.
    let days = secs / 86400;
    let rem = secs % 86400;
    // civil-from-days (Howard Hinnant), enough for test stamps.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Build the old layout in `<dir>/data` from the live captures.
/// Returns the unit -> rollback instant map it wrote.
fn build_old_layout(dir: &Path, t: u64) -> Vec<(String, u64)> {
    let data = dir.join("data");
    let state = fixture("api-state.json");
    let units: Vec<String> = state["reflex"]["budgets"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    for (u, _) in EXPECT_LIVE_1H {
        assert!(units.iter().any(|x| x == u), "live budgets lack {u}");
    }
    // Every live unit appears (empty history = the live live_1h of 0),
    // plus the injected history in all timestamp forms.
    let mut restarts = serde_json::Map::new();
    for u in &units {
        restarts.insert(u.clone(), json!([]));
    }
    restarts.insert(
        "hermes-rs.service".into(),
        json!([t - 2 * H, t - 1200, t - 600]),
    );
    restarts.insert(
        "galion-one.service".into(),
        json!([(t - 300) as f64 + 0.75, rfc3339(t - 900)]),
    );
    restarts.insert("cloudflared.service".into(), json!(format!("{}", t - 120)));
    let rollbacks = vec![
        ("openmind-memory.service".to_string(), t - 3 * H),
        ("console.service".to_string(), t - 5 * H),
    ];
    let reflex = json!({
        "restarts": restarts,
        "rollbacks": {
            "openmind-memory.service": t - 3 * H,
            "console.service": rfc3339(t - 5 * H),
        },
        "layout": 1,
        "written_by": "galion-nerve (older build)",
        "future": {"per_unit_caps": {"hermes-rs.service": 3}},
    });
    std::fs::write(data.join("reflex.json"), reflex.to_string()).unwrap();
    std::fs::write(
        data.join("mode"),
        format!("{}\n", state["mode"].as_str().unwrap()),
    )
    .unwrap();
    // /alerts is newest first; the file is oldest first.
    let mut alerts: Vec<Value> = fixture("api-alerts.json")
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .cloned()
        .collect();
    alerts[0]["legacy_ack"] = json!({"by": "mate", "at": "2026-10-21T17:30:00+02:00"});
    std::fs::write(
        data.join("alerts.json"),
        serde_json::to_string_pretty(&alerts).unwrap(),
    )
    .unwrap();
    let se = fixture("api-self-eval.json");
    let sd = data.join("self-eval");
    std::fs::create_dir_all(&sd).unwrap();
    std::fs::write(
        sd.join("latest.json"),
        serde_json::to_string_pretty(&se).unwrap(),
    )
    .unwrap();
    std::fs::write(sd.join("latest.md"), "# old self-eval\n").unwrap();
    for name in HISTORY {
        let mut r = se.clone();
        r["history_marker"] = json!(name);
        std::fs::write(sd.join(name), r.to_string()).unwrap();
    }
    std::fs::write(sd.join("notes.txt"), "kept by an operator\n").unwrap();
    std::fs::write(
        data.join("evt-llm-spend.json"),
        r#"{"total_usd":1.25,"calls":7,"model_seen":"older-build"}"#,
    )
    .unwrap();
    std::fs::write(data.join("state.json"), state.to_string()).unwrap();
    rollbacks
}

const HISTORY: &[&str] = &[
    "self-eval-20261021T065435.json",
    "self-eval-20261021T072435.json",
    "self-eval-20261021T201657+0200.json",
    "self-eval-20261021T214657+0200.json",
    "self-eval-20261021T224657+0200.json",
];

fn check_preserved(port: u16, dir: &Path, rollbacks: &[(String, u64)], mode: &str) {
    let data = dir.join("data");
    let st = get_json(port, "/state");
    let b = &st["reflex"]["budgets"];
    for (u, n) in EXPECT_LIVE_1H {
        assert_eq!(b[*u]["live_1h"].as_u64(), Some(*n), "{u}: {}", b[*u]);
    }
    for (u, at) in rollbacks {
        let shown = b[u.as_str()]["last_rollback"].as_str().unwrap_or("");
        let back = parse(shown);
        assert_eq!(back, Some(*at), "{u}: {shown}");
    }
    assert!(st["reflex"].get("load_note").is_none(), "{}", st["reflex"]);
    assert!(st["alerts"].get("load_note").is_none(), "{}", st["alerts"]);
    assert_eq!(st["mode"], mode, "mode file honoured");
    // Alerts: the whole old history is still on disk, unknown field kept.
    let on_disk: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(data.join("alerts.json")).unwrap()).unwrap();
    let old: Vec<Value> = fixture("api-alerts.json").as_array().unwrap().clone();
    for a in &old {
        assert!(
            on_disk
                .iter()
                .any(|d| d["key"] == a["key"] && d["ts"] == a["ts"]),
            "lost alert {}",
            a["key"]
        );
    }
    assert!(
        on_disk.iter().any(|d| d.get("legacy_ack").is_some()),
        "unknown alert field dropped"
    );
    let api_alerts = get_json(port, "/alerts");
    assert!(
        api_alerts.as_array().unwrap().len() >= 50.min(old.len()),
        "API serves the old history"
    );
    // Self-eval history: nothing old deleted, a new report written.
    let sd = data.join("self-eval");
    for h in HISTORY {
        assert!(sd.join(h).exists(), "lost {h}");
    }
    assert!(sd.join("notes.txt").exists());
    let latest: Value =
        serde_json::from_str(&std::fs::read_to_string(sd.join("latest.json")).unwrap()).unwrap();
    assert_eq!(latest["tool"], "openmind-nerve");
    assert_ne!(
        latest["ts"],
        fixture("api-self-eval.json")["ts"],
        "new report"
    );
    // reflex.json: no action ran, so the file is as the old build left it.
    let r: Value =
        serde_json::from_str(&std::fs::read_to_string(data.join("reflex.json")).unwrap()).unwrap();
    assert_eq!(r["layout"], 1);
    assert!(r["future"]["per_unit_caps"].is_object());
}

fn parse(s: &str) -> Option<u64> {
    // Minimal RFC 3339 with offset -> unix (Zagreb-rendered stamps).
    if s.len() < 25 {
        return None;
    }
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, m, d) = (n(0, 4)?, n(5, 7)?, n(8, 10)?);
    let (hh, mm, ss) = (n(11, 13)?, n(14, 16)?, n(17, 19)?);
    let sign = if &s[19..20] == "-" { -1 } else { 1 };
    let off = sign * (n(20, 22)? * 3600 + n(23, 25)? * 60);
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86400 + hh * 3600 + mm * 60 + ss - off) as u64)
}

/// Loopback stub that records every request (must stay empty: the spend
/// cap has to refuse before any call goes out).
fn stub(port: u16) -> Arc<Mutex<u32>> {
    let l = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let hits = Arc::new(Mutex::new(0u32));
    let h2 = hits.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            *h2.lock().unwrap() += 1;
            drop(c);
        }
    });
    hits
}

fn paid_env(stub_port: u16, budget: &str, fifo: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display())),
        ("NERVE_EVT_ESCALATE", "openai".into()),
        (
            "NERVE_EVT_LLM_ENDPOINT",
            format!("http://127.0.0.1:{stub_port}/v1/chat/completions"),
        ),
        ("NERVE_EVT_LLM_MODEL", "stub-model".into()),
        ("NERVE_EVT_LLM_KEY_ENV", "NERVE_T41_STUB_KEY".into()),
        ("NERVE_T41_STUB_KEY", "stub-key-not-real".into()),
        ("NERVE_EVT_LLM_BUDGET_USD", budget.into()),
        ("NERVE_EVT_LLM_PRICE_IN_PER_MTOK", "0.5".into()),
        ("NERVE_EVT_LLM_PRICE_OUT_PER_MTOK", "1.5".into()),
        ("NERVE_EVT_LLM_CALLS_PER_HOUR", "4".into()),
        ("NERVE_EVT_DEDUPE", "1".into()),
    ]
}

/// Push one high-severity batch through the journal FIFO and return the
/// escalation outcome and detail.
fn one_escalation(port: u16, fifo: &Path) -> (String, String) {
    let mut j = std::fs::OpenOptions::new().write(true).open(fifo).unwrap();
    let before = get_json(port, "/stats")["batches"].as_u64().unwrap();
    for _ in 0..5 {
        writeln!(
            j,
            r#"{{"MESSAGE_ID":"98e322203f7a4ed290d09fe03c09fe15","UNIT":"openmind-memory.service","MESSAGE":"openmind-memory.service: sim","_PID":"1","EXIT_STATUS":"1"}}"#
        )
        .unwrap();
    }
    j.flush().unwrap();
    wait_until("batch", 30, || {
        get_json(port, "/stats")["batches"].as_u64().unwrap() > before
    });
    let esc = get_json(port, "/escalations");
    let last = esc.as_array().unwrap().last().cloned().unwrap_or_default();
    (
        last["outcome"].as_str().unwrap_or("").to_string(),
        last["detail"].as_str().unwrap_or("").to_string(),
    )
}

fn mkfifo(p: &Path) {
    assert!(Command::new("mkfifo").arg(p).status().unwrap().success());
}

#[test]
fn old_layout_loads_in_polling_then_event_mode() {
    let dir = tmp("old");
    let t = now();
    let rollbacks = build_old_layout(&dir, t);
    let mode = fixture("api-state.json")["mode"]
        .as_str()
        .unwrap()
        .to_string();
    let spend0 = std::fs::read(dir.join("data/evt-llm-spend.json")).unwrap();

    // 1. polling mode (the live default before NERVE_MODE=event).
    {
        let _n = start(20930, &dir, false, &[]);
        check_preserved(20930, &dir, &rollbacks, &mode);
        // Polling mode never touches the event-mode spend record.
        assert_eq!(
            std::fs::read(dir.join("data/evt-llm-spend.json")).unwrap(),
            spend0
        );
    }

    // 2. event mode on the same dir (second boot): still nothing reset,
    // and the paid slot sees the old 1.25 USD: the worst
    // case (about 0.0021 USD) no longer fits a 1.251 cap, so it refuses without
    // calling the stub; a reset spend (0 USD) would have called it.
    let fifo = dir.join("journal.fifo");
    mkfifo(&fifo);
    let hits = stub(20932);
    let n = start(20931, &dir, true, &paid_env(20932, "1.251", &fifo));
    check_preserved(20931, &dir, &rollbacks, &mode);
    let (out, detail) = one_escalation(20931, &fifo);
    assert_eq!(out, "refused", "{detail}");
    assert!(detail.contains("spent 1.2500"), "{detail}");
    assert_eq!(*hits.lock().unwrap(), 0, "no paid call went out");
    drop(n);
    let spend: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("data/evt-llm-spend.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(spend["total_usd"], 1.25, "{spend}");
    assert_eq!(spend["calls"], 7);
    assert_eq!(spend["model_seen"], "older-build", "unknown field kept");
    assert!(spend["refused"].as_u64() >= Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn damaged_layout_fails_closed_and_keeps_copies() {
    let dir = tmp("bad");
    let t = now();
    build_old_layout(&dir, t);
    let data = dir.join("data");
    std::fs::write(
        data.join("reflex.json"),
        "{\"restarts\": {\"hermes-rs.service\": [1,",
    )
    .unwrap();
    std::fs::write(data.join("alerts.json"), "{\"not\": \"a list\"}").unwrap();
    std::fs::write(data.join("evt-llm-spend.json"), "{\"total_usd\": -3}").unwrap();
    let spend0 = std::fs::read(data.join("evt-llm-spend.json")).unwrap();
    let fifo = dir.join("journal.fifo");
    mkfifo(&fifo);
    let hits = stub(20934);
    let n = start(20933, &dir, true, &paid_env(20934, "50", &fifo));
    let st = get_json(20933, "/state");
    let rn = st["reflex"]["load_note"].as_str().unwrap_or("");
    assert!(
        rn.contains("bad JSON") && rn.contains("live restarts held until"),
        "{rn}"
    );
    let an = st["alerts"]["load_note"].as_str().unwrap_or("");
    assert!(an.contains("not an alert list"), "{an}");
    let (out, detail) = one_escalation(20933, &fifo);
    assert_eq!(out, "refused", "{detail}");
    assert!(detail.contains("not a usable spend record"), "{detail}");
    assert_eq!(*hits.lock().unwrap(), 0);
    drop(n);
    // The damaged spend record is never overwritten; copies of all three.
    assert_eq!(
        std::fs::read(data.join("evt-llm-spend.json")).unwrap(),
        spend0
    );
    let names: Vec<String> = std::fs::read_dir(&data)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    for f in ["reflex.json", "alerts.json", "evt-llm-spend.json"] {
        assert!(
            names
                .iter()
                .any(|n| n.starts_with(&format!("{f}.corrupt-"))),
            "no copy of {f}: {names:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Gated real-life run (`GALION_REAL=1`, best with `--release`): rebuild the
/// state dir from a *fresh* read-only capture of the live nerve (directory
/// in `NERVE_T41_LIVE_DIR` holding `api-state.json` and `api-alerts.json`
/// from GET :8670 `/state` and `/alerts`; default: the committed fixtures)
/// and check the new binary reports every live budget exactly as the live
/// nerve did (live_1h count per unit, last_rollback), keeps every live
/// top-level `/state` key, and serves the live alert history.
#[test]
fn real_live_capture_round_trips_through_new_binary() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let src = std::env::var("NERVE_T41_LIVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| fixtures());
    let read = |n: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(src.join(n)).unwrap()).unwrap()
    };
    let live = read("api-state.json");
    let live_alerts = read("api-alerts.json");
    let t = now();
    let dir = tmp("real");
    let data = dir.join("data");
    let budgets = live["reflex"]["budgets"].as_object().unwrap().clone();
    let mut restarts = serde_json::Map::new();
    let mut rollbacks = serde_json::Map::new();
    for (u, b) in &budgets {
        let n = b["live_1h"].as_u64().unwrap();
        restarts.insert(
            u.clone(),
            json!((0..n).map(|i| t - 60 - i * 60).collect::<Vec<_>>()),
        );
        if let Some(r) = b["last_rollback"].as_str() {
            rollbacks.insert(u.clone(), json!(r));
        }
    }
    std::fs::write(
        data.join("reflex.json"),
        json!({"restarts": restarts, "rollbacks": rollbacks}).to_string(),
    )
    .unwrap();
    std::fs::write(data.join("mode"), live["mode"].as_str().unwrap()).unwrap();
    let on_disk: Vec<Value> = live_alerts
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .cloned()
        .collect();
    std::fs::write(
        data.join("alerts.json"),
        serde_json::to_string(&on_disk).unwrap(),
    )
    .unwrap();
    for (event, port) in [(false, 20935u16), (true, 20936u16)] {
        let _n = start(port, &dir, event, &[]);
        let st = get_json(port, "/state");
        for (u, b) in &budgets {
            let got = &st["reflex"]["budgets"][u.as_str()];
            assert_eq!(got["live_1h"], b["live_1h"], "{u} event={event}");
            assert_eq!(
                got["last_rollback"], b["last_rollback"],
                "{u} event={event}"
            );
        }
        for k in live.as_object().unwrap().keys() {
            assert!(
                st.get(k).is_some(),
                "live /state key {k} missing (event={event})"
            );
        }
        assert_eq!(st["mode"], live["mode"]);
        let a = get_json(port, "/alerts");
        let served: Vec<&Value> = a.as_array().unwrap().iter().collect();
        for old in live_alerts.as_array().unwrap().iter().take(40) {
            assert!(
                served
                    .iter()
                    .any(|s| s["key"] == old["key"] && s["ts"] == old["ts"]),
                "alert {} not served (event={event})",
                old["key"]
            );
        }
        eprintln!(
            "event={event}: {} budgets equal, {} live keys present, {} alerts served",
            budgets.len(),
            live.as_object().unwrap().len(),
            served.len()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
