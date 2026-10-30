// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the `event-feed` sensor (cortex plan T2.10), gated by
//! `GALION_REAL=1`; run with `--release`. Slot 106: ports 19760-19769.
//!
//! The real `openmind-nerve serve` on 127.0.0.1:19760 writes its own
//! `POST /event` JSONL log (`--log`) and reads it back through
//! `NERVE_EVENT_FEED_FILE` (plus a survival-guardian JSONL file).
//!
//! 1. Gaia's real heal loop: when `GAIA_BIN` points at the `gaia` binary,
//!    `gaia heal-tick` probes a dead test service on :19761 (no approval
//!    file, so HADES refuses and nothing is restarted) and posts
//!    `service.probe` + `gaia.heal.refused`; without `GAIA_BIN` the test
//!    posts the same two records itself. The cortex shows
//!    `service-down-t106` and `heal-refused-t106`.
//! 2. `gaia.heal.capped` posted: critical bleeding finding, incident
//!    `gaia:t106` opens, B33 `cortex_events` carries `crit` without the
//!    probe detail's address.
//! 3. The service comes up on :19761 and a second `heal-tick` (or an `up`
//!    probe record) posts `service.probe` info: every finding clears.
//! 4. A survival line `ok:false` appended to the second file shows
//!    `probe-failed-disk`; `ok:true` clears it. Cycle time and RSS printed.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): NERVE +0, SVC +1; `CORTEX_EVENT_FEED_REAL_PORT_BASE`
/// moves the block, unset keeps the old
/// 19760-19769.
static PORTS: Block = Block::new("CORTEX_EVENT_FEED_REAL_PORT_BASE", 19760, 10);

#[allow(non_snake_case)]
fn NERVE() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn SVC() -> u16 {
    PORTS.port(1)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(method: &str, path: &str, body: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{}", NERVE()).parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(
        s,
        "{method} {path} HTTP/1.0\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (head, body) = b.split_once("\r\n\r\n")?;
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, body.to_string()))
}

fn post(source: &str, kind: &str, sev: &str, data: Value) {
    let b = json!({"source": source, "kind": kind, "severity": sev, "data": data}).to_string();
    let (code, body) = http("POST", "/event", &b).expect("POST /event");
    assert_eq!(code, 200, "{body}");
}

fn state() -> Value {
    http("GET", "/api/v1/state", "")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

/// Poll until `ok` holds (or 30 s pass); returns the last state.
fn wait(ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state();
        if ok(&st) || t.elapsed() > Duration::from_secs(30) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn ids(st: &Value) -> Vec<String> {
    let mut v: Vec<String> = ["vulnerabilities", "bleeding"]
        .iter()
        .flat_map(|k| st["cortex"][k].as_array().cloned().unwrap_or_default())
        .filter(|f| {
            f["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("event:"))
        })
        .filter_map(|f| f["id"].as_str().map(String::from))
        .collect();
    v.sort();
    v
}

fn find(st: &Value, id: &str) -> Value {
    st["cortex"]["bleeding"]
        .as_array()
        .and_then(|a| a.iter().find(|f| f["id"] == id).cloned())
        .unwrap_or(Value::Null)
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn feed_us(st: &Value) -> u64 {
    st["cortex_sensors"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["name"] == "event-feed"))
        .and_then(|s| s["us"].as_u64())
        .unwrap_or(0)
}

/// `gaia heal-tick` against the test service, or the same records by hand.
fn heal_tick(gaia: Option<&str>, dir: &Path, up: bool) -> String {
    if let Some(g) = gaia {
        let o = Command::new(g)
            .args([
                "heal-tick",
                "--compact",
                "--service",
                "t106",
                "--port",
                &SVC().to_string(),
                "--allow-ports",
                &PORTS.range(),
                "--nerve",
                &format!("127.0.0.1:{}", NERVE()),
                "--state",
                &dir.join("gaia-state").to_string_lossy(),
                "--hades-config",
                &dir.join("no-hades-config.toml").to_string_lossy(),
                "--wait-ms",
                "500",
                "--",
                "/bin/true",
                &SVC().to_string(),
            ])
            // Slot 423: the slot block may sit above gaia's default box test
            // range 19000-19999; GAIA_TEST_PORTS (honoured only with
            // GALION_REAL=1, inside 19000-23999) moves it to this block.
            .env("GAIA_TEST_PORTS", PORTS.range())
            .stderr(Stdio::null())
            .output()
            .expect("run gaia heal-tick");
        let out = String::from_utf8_lossy(&o.stdout);
        let v: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| {
            eprintln!(
                "   gaia stdout not JSON ({e}): {}",
                out.chars().take(300).collect::<String>()
            );
            Value::Null
        });
        format!(
            "gaia heal-tick exit {:?} outcome {} detail {}",
            o.status.code(),
            v["result"]["outcome"],
            v["result"]["detail"]
        )
    } else {
        let probe = format!("t106-manual-{}", std::process::id());
        if up {
            post(
                "gaia-live",
                "service.probe",
                "info",
                json!({"service": "t106", "port": SVC(), "state": "up", "detail": "200", "probe_id": probe}),
            );
        } else {
            post(
                "gaia-live",
                "service.probe",
                "warn",
                json!({"service": "t106", "port": SVC(), "state": "down", "detail": format!("connect 127.0.0.1:{}: refused", SVC()), "probe_id": probe}),
            );
            post(
                "gaia-live",
                "gaia.heal.refused",
                "warn",
                json!({"service": "t106", "port": SVC(), "stage": "approval", "reason": "no approval file given", "probe_id": probe}),
            );
        }
        "GAIA_BIN unset: posted the gaia heal-tick records by hand".into()
    }
}

#[test]
fn real_event_feed_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let gaia = std::env::var("GAIA_BIN")
        .ok()
        .filter(|p| Path::new(p).exists());
    let dir = std::env::temp_dir().join(format!("nerve-real-event-feed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("events.jsonl");
    let surv = dir.join("survival.jsonl");
    std::fs::write(&surv, "").unwrap();
    let comps = dir.join("components.json");
    std::fs::write(&comps, "[]").unwrap();
    let nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .arg("--log")
            .arg(&log)
            .env("NERVE_ADDR", format!("127.0.0.1:{}", NERVE()))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", PORTS.range())
            .env(
                "NERVE_EVENT_FEED_FILE",
                format!("{},{}", log.display(), surv.display()),
            )
            .env_remove("NERVE_SENSORS_OFF")
            .env_remove("NERVE_EVENT_FEED_SOURCES")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let st = wait(|s| s["cycle"].as_u64().unwrap_or(0) >= 1);
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .expect("cortex_sensors")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    assert!(names.contains(&"event-feed"), "{names:?}");
    eprintln!(
        "serve :{} cycle {}: sensors {names:?}",
        NERVE(),
        st["cycle"]
    );

    // 1. Gaia probes the dead service; HADES refuses (no approval file).
    let how = heal_tick(gaia.as_deref(), &dir, false);
    eprintln!("1. {how}");
    let st = wait(|s| ids(s).len() >= 2);
    assert_eq!(
        ids(&st),
        ["heal-refused-t106", "service-down-t106"],
        "{}",
        st["cortex"]
    );
    let down = find(&st, "service-down-t106");
    assert_eq!(
        (
            down["kind"].as_str(),
            down["severity"].as_str(),
            down["source"].as_str()
        ),
        (Some("bleeding"), Some("warn"), Some("event:gaia-live"))
    );
    assert!(!down["title"].as_str().unwrap().contains("127.0.0.1"));
    eprintln!(
        "   cycle {}: {} | {}",
        st["cycle"],
        down["title"],
        find(&st, "heal-refused-t106")["title"]
    );

    // 2. Restart cap reached: critical, incident, B33 event.
    post(
        "gaia-live",
        "gaia.heal.capped",
        "error",
        json!({"service": "t106", "port": SVC(), "restarts_last_hour": 3, "cap": 3, "retry_after_s": 900, "probe_id": "t106-cap"}),
    );
    let st = wait(|s| find(s, "heal-capped-t106").is_object());
    let cap = find(&st, "heal-capped-t106");
    assert_eq!(cap["severity"], "critical", "{}", st["cortex"]);
    assert_eq!(cap["incident_cause"], "gaia:t106");
    let inc = st["incidents"]["open"]
        .as_array()
        .or_else(|| st["incidents"].as_array())
        .and_then(|a| a.iter().find(|i| i["cause"] == "gaia:t106").cloned())
        .unwrap_or_else(|| panic!("incident gaia:t106 open: {}", st["incidents"]));
    let ev = st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == "event:gaia-live")
        .cloned()
        .expect("B33 event for event:gaia");
    assert_eq!(
        (ev["kind"].as_str(), ev["severity"].as_str()),
        (Some("bleeding"), Some("crit"))
    );
    assert!(!ev.to_string().contains("127.0.0.1"), "{ev}");
    let ts = ev["ts"].as_str().unwrap();
    assert!(ts.ends_with("+02:00") || ts.ends_with("+01:00"), "{ts}");
    eprintln!(
        "2. cycle {}: {} | incident {} {} | event {} | score {} | cycle_ms {} event-feed us {} rss {} KiB",
        st["cycle"], cap["title"], inc["id"], inc["severity"], ev, st["system"]["score"],
        st["cycle_ms"], feed_us(&st), rss_kib(nerve.0.id())
    );

    // 3. The service comes up: the next probe clears everything.
    let svc = TcpListener::bind(("127.0.0.1", SVC())).expect("slot port free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ =
                c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}");
        }
    });
    let how = heal_tick(gaia.as_deref(), &dir, true);
    eprintln!("3. {how}");
    let st = wait(|s| s["cycle"].as_u64().unwrap_or(0) > 0 && ids(s).is_empty());
    assert!(ids(&st).is_empty(), "cleared: {}", st["cortex"]);
    assert!(!st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["source"] == "event:gaia-live"));
    eprintln!(
        "   cycle {}: all event findings cleared, score {}",
        st["cycle"], st["system"]["score"]
    );

    // 4. Survival guardian JSONL (second file).
    let line = |ok: bool| {
        format!(
            "{{\"ts\":\"{}\",\"source\":\"guardian\",\"kind\":\"probe\",\"name\":\"disk\",\"ok\":{ok},\"detail\":\"96% used\"}}\n",
            zagreb_time::now_rfc3339()
        )
    };
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&surv)
        .unwrap();
    f.write_all(line(false).as_bytes()).unwrap();
    let st = wait(|s| ids(s) == ["probe-failed-disk"]);
    assert_eq!(ids(&st), ["probe-failed-disk"], "{}", st["cortex"]);
    eprintln!(
        "4. cycle {}: {}",
        st["cycle"],
        find(&st, "probe-failed-disk")["title"]
    );
    f.write_all(line(true).as_bytes()).unwrap();
    let st = wait(|s| ids(s).is_empty());
    assert!(ids(&st).is_empty());
    let (_, evs) = http("GET", "/api/v1/events?n=50", "").unwrap();
    let n = serde_json::from_str::<Value>(&evs)
        .unwrap()
        .as_array()
        .map(Vec::len)
        .unwrap_or(0);
    eprintln!(
        "   cycle {}: survival probe cleared; {n} records in the nerve log; cycle_ms {} event-feed us {} rss {} KiB",
        st["cycle"], st["cycle_ms"], feed_us(&st), rss_kib(nerve.0.id())
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
