// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the merged cortex (slot 158, `swarm/cortex-merge-all`:
//! cortex-t20-merge + state-titles + event-ingest + t212-edgerate + score-v2),
//! gated by `GALION_REAL=1`; run with `--release`. Ports 20280-20289
//! (`CORTEX_MERGE_ALL_REAL_PORT_BASE` moves them).
//!
//! Checks the cross-branch paths that no single branch could test:
//!
//! 1. One real `openmind-nerve serve` on 127.0.0.1:20280 registers the
//!    event-feed (T2.10) and edge-rate (T2.12, on by default) sensors next to
//!    the built-ins.
//! 2. A pushed B33 `ddos` event at `warn` (no signal) becomes a ddos finding
//!    that score v2 (T3.3) charges on its own (4 points, `ddos (warn)`), and
//!    `/api/v1/state` `cortex_events` carries it without `title`/`fix`
//!    (state-titles).
//! 3. The same event at `error` opens incident `gaia:<source>`; the finding is
//!    listed in `self_eval.covered` and no deduction of its own is left
//!    (scored once, as the incident).
//! 4. The event at `info` clears the finding, the incident's deduction goes
//!    after it resolves, and the score returns to its start value.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
/// Slot 383 (real-ports-batch-3) / 435: NERVE +0; `CORTEX_MERGE_ALL_REAL_PORT_BASE`
/// moves the 10-port block, unset keeps the old 20280-20289. (This lineage
/// has no `galion_realtest::ports::Block`; `base_from_env` is the same rule.)
fn base() -> u16 {
    galion_realtest::ports::base_from_env("CORTEX_MERGE_ALL_REAL_PORT_BASE", 20280, 10)
}

#[allow(non_snake_case)]
fn NERVE() -> u16 {
    base()
}

fn range() -> String {
    format!("{}-{}", base(), base() + 9)
}

/// Slot 435 (D-422-1): `NERVE_HUB_EVENT_FOLD=1` in the test env is passed to
/// the nerve and tightens step 3 to exactly one incident.
fn fold() -> bool {
    std::env::var("NERVE_HUB_EVENT_FOLD").as_deref() == Ok("1")
}
const SRC: &str = "t158";

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

fn post(sev: &str) {
    let data = json!({"id": "edge-flood", "component": "tunnel",
                      "title": "edge flood on 10.0.0.5:443", "fix": "ban 10.0.0.5"});
    let b = json!({"source": SRC, "kind": "ddos", "severity": sev, "data": data}).to_string();
    let (code, body) = http("POST", "/event", &b).expect("POST /event");
    assert_eq!(code, 200, "{body}");
}

fn state() -> Value {
    http("GET", "/api/v1/state", "")
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn wait(what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    wait_for(what, 40, ok)
}

/// [`wait`] with its own limit in seconds.
fn wait_for(what: &str, secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = state();
        if ok(&st) {
            return st;
        }
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}: {}",
            st["self_eval"]
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn ddos_ids(st: &Value) -> Vec<String> {
    st["cortex"]["ddos_findings"]
        .as_array()
        .or_else(|| st["cortex"]["ddos"]["findings"].as_array())
        .into_iter()
        .flatten()
        .filter(|f| f["source"] == format!("event:{SRC}"))
        .filter_map(|f| f["id"].as_str().map(String::from))
        .collect()
}

fn deductions(st: &Value) -> Vec<(u64, String)> {
    st["self_eval"]["deductions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            (
                d["points"].as_u64().unwrap_or(0),
                d["what"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn mine(st: &Value) -> Vec<(u64, String)> {
    deductions(st)
        .into_iter()
        .filter(|(_, w)| w.contains(SRC))
        .collect()
}

fn score(st: &Value) -> u64 {
    st["self_eval"]["score"].as_u64().unwrap_or(0)
}

#[test]
fn real_cortex_merge_all_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir = std::env::temp_dir().join(format!("nerve-real-merge-all-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("events.jsonl");
    let comps = dir.join("components.json");
    std::fs::write(&comps, "[]").unwrap();
    let nerve = Kill(
        Command::new(BIN)
            .arg("serve")
            .arg("--log")
            .arg(&log)
            .env("NERVE_ADDR", format!("127.0.0.1:{}", NERVE()))
            .env("NERVE_INTERVAL", "3")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", range())
            .env("NERVE_HUB_EVENT_FOLD", if fold() { "1" } else { "0" })
            .env("NERVE_EVENT_FEED_FILE", &log)
            .env_remove("NERVE_SENSORS_OFF")
            .env_remove("NERVE_EDGE_HOSTS")
            .env_remove("NERVE_EVENT_FEED_SOURCES")
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );

    // 1. Sensors of all five branches in one process.
    let st = wait("first cycle", |s| s["cycle"].as_u64().unwrap_or(0) >= 1);
    let names: Vec<&str> = st["cortex_sensors"]
        .as_array()
        .expect("cortex_sensors")
        .iter()
        .filter_map(|s| s["name"].as_str())
        .collect();
    for n in ["listeners", "ddos", "event-feed", "edge-rate"] {
        assert!(names.contains(&n), "{n} registered: {names:?}");
    }
    let base = score(&st);
    // Slot 422 (F-383-2): host-level incidents the box itself causes (e.g.
    // `lxc-host` disk failing-soon while the box disk was 95% full on
    // 2026-10-28) are not this test's business; the score-back check
    // compares like with like.
    let others = |s: &Value| -> Vec<(u64, String)> {
        deductions(s)
            .into_iter()
            .filter(|(_, w)| !w.contains(SRC))
            .collect()
    };
    let base_others = others(&st);
    assert!(mine(&st).is_empty());
    eprintln!(
        "1. serve :{nerve} cycle {}: sensors {names:?}; start score {base} {}",
        st["cycle"],
        st["self_eval"]["grade"],
        nerve = NERVE()
    );

    // 2. warn ddos: own deduction (score v2), B33 event without title/fix.
    post("warn");
    let st = wait("warn ddos deduction", |s| !mine(s).is_empty());
    assert_eq!(
        ddos_ids(&st),
        [format!("ev-{SRC}-edge-flood")],
        "{}",
        st["cortex"]
    );
    let m = mine(&st);
    assert_eq!(m.len(), 1, "{m:?}");
    assert_eq!(m[0].0, 4, "{m:?}");
    assert!(m[0].1.starts_with("ddos (warn)"), "{m:?}");
    assert_eq!(score(&st), base.saturating_sub(4));
    let ev = st["cortex_events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == format!("event:{SRC}"))
        .cloned()
        .expect("B33 cortex event for the pushed ddos");
    assert_eq!(ev["kind"], "ddos");
    let top = &ev["data"]["top"];
    assert!(
        top.get("title").is_none() && top.get("fix").is_none(),
        "{ev}"
    );
    assert!(
        !ev.to_string().contains("10.0.0.5"),
        "no address leak: {ev}"
    );
    eprintln!(
        "2. cycle {}: deduction {m:?}; score {base} -> {}; cortex_events top keys {:?}",
        st["cycle"],
        score(&st),
        top.as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
    );

    // 3. error ddos: incident gaia:<src>, finding covered, scored once.
    post("error");
    let cause = format!("gaia:{SRC}");
    let st = wait("covered by incident", |s| {
        s["self_eval"]["covered"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c["by"] == cause.as_str()))
    });
    let cov: Vec<&Value> = st["self_eval"]["covered"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["by"] == cause.as_str())
        .collect();
    assert_eq!(cov.len(), 1, "{cov:?}");
    assert_eq!(cov[0]["finding"], format!("ddos:ev-{SRC}-edge-flood"));
    let all = deductions(&st);
    let inc: Vec<&(u64, String)> = all
        .iter()
        .filter(|(_, w)| w.starts_with("error incident: ") && w.contains(SRC))
        .collect();
    // Slot 422 (F-383-2, D-422-1): on the main-staging lineage the same
    // POST /event also becomes a hub push signal (server.rs event_signal ->
    // hub.incident_signals, nerve-wiring 8d7a9240) with cause `t158`, next to
    // the event-feed incident `gaia:t158`; slot 158 was written on a lineage
    // without that path. Exactly one `gaia:` incident plus at most that one
    // hub incident, until the owner decides whether to fold the two causes.
    let gaia_inc = format!("error incident: {cause}: ");
    let hub_inc = format!("error incident: {SRC}: pushed 1 error(s) in the last 2 min; ");
    assert_eq!(
        inc.iter().filter(|(_, w)| w.starts_with(&gaia_inc)).count(),
        1,
        "{all:?}"
    );
    assert!(
        inc.iter()
            .all(|(_, w)| w.starts_with(&gaia_inc) || w.starts_with(&hub_inc)),
        "{all:?}"
    );
    assert!(inc.len() <= 2, "{all:?}");
    // Slot 435 (D-422-1 staged): with the fold on, the hub signal joins
    // `gaia:<src>`: exactly one incident, one score.
    if fold() {
        assert_eq!(inc.len(), 1, "fold on: one cause scored once: {all:?}");
        assert_eq!(inc[0].0, 15, "{all:?}");
    }
    // Slot 435 (D-422-2): the raw address never reaches /state.
    let text = st.to_string();
    assert!(!text.contains("10.0.0.5"), "address in /state: {all:?}");
    eprintln!(
        "3. hub incident alongside gaia: {} (D-422-1)",
        inc.iter().any(|(_, w)| w.starts_with(&hub_inc))
    );
    assert!(
        !all.iter().any(|(_, w)| w.starts_with("ddos (")),
        "no own ddos deduction while covered: {all:?}"
    );
    assert!(!st["self_eval"]["pass"].as_bool().unwrap_or(true));
    eprintln!(
        "3. cycle {}: covered {} by {cause}; deductions {all:?}; score {} pass {}",
        st["cycle"],
        cov[0]["finding"],
        score(&st),
        st["self_eval"]["pass"]
    );

    // 4. info clears: finding gone, then the incident resolves.
    post("info");
    let st = wait("finding cleared", |s| ddos_ids(s).is_empty());
    eprintln!(
        "4. cycle {}: finding cleared; score {} deductions {:?}",
        st["cycle"],
        score(&st),
        deductions(&st)
    );
    // The hub signal stays in its 120 s push window (hub.rs PUSH_WINDOW), so
    // its incident can outlive the event-feed one: allow 180 s.
    let t2 = Instant::now();
    let st = wait_for("score back to start", 180, |s| {
        mine(s).is_empty()
            && !deductions(s)
                .iter()
                .any(|(_, w)| w.starts_with("error incident: ") && w.contains(SRC))
    });
    let sum: u64 = deductions(&st).iter().map(|d| d.0).sum();
    assert_eq!(
        score(&st),
        100u64.saturating_sub(sum),
        "{:?}",
        deductions(&st)
    );
    if others(&st) == base_others {
        assert_eq!(score(&st), base, "score back to start");
    } else {
        eprintln!(
            "   unrelated host deductions changed meanwhile: {base_others:?} -> {:?}",
            others(&st)
        );
    }
    eprintln!(
        "   cycle {}: incidents resolved after {:.1} s, score back to {} {}; cycle_ms {}",
        st["cycle"],
        t2.elapsed().as_secs_f64(),
        score(&st),
        st["self_eval"]["grade"],
        st["cycle_ms"]
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
