// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of T2.12 per-hostname edge rate (gated: `GALION_REAL=1`,
//! run with `--release` for the release binary). Slot 107: ports 19770-19779.
//!
//! 1. `openmind-nerve replay` (the real binary) of the cortex fixture plus
//!    30 min of cloudflared `Request failed ... dest=` lines in the live VPS
//!    shape: `quiet.example.test` stays normal, `hot.example.test` (40 req/s
//!    for the last 60 s) floods; one `edge-host-rate` ddos finding and one
//!    `ddos:edge-cloudflared` incident. `NERVE_EDGE_HOSTS=0` removes all of it.
//! 2. The live VPS capture `/workspace/swarm/nerve-capture/replay1` (or
//!    `$CORTEX_REPLAY_DIR`): its real cloudflared lines become per-host
//!    streams, all normal, verdict still 61/D.
//! 3. `openmind-nerve serve` on 127.0.0.1:19770: `edge-rate` is registered
//!    and `/api/v1/state` shows `edge_hosts`; with `NERVE_SENSORS_OFF=edge-rate`
//!    on :19771 the feeder is off too (`edge_hosts` null).

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
/// Last fixture state: 2026-10-20T19:06:05+02:00.
const T_END: u64 = 1_792_515_965;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<Value> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (_, body) = b.split_once("\r\n\r\n")?;
    serde_json::from_str(body).ok()
}

fn wait_state(port: u16, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = get(port, "/api/v1/state").unwrap_or_default();
        if ok(&st) || t.elapsed() > Duration::from_secs(20) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn replay(dir: &Path, data: &Path, extra: &[(&str, &str)]) -> (Option<i32>, Value, Duration) {
    let mut c = Command::new(BIN);
    c.env_clear()
        .env("NERVE_DATA_DIR", data)
        .arg("replay")
        .arg(dir);
    for (k, v) in extra {
        c.env(k, v);
    }
    let t = Instant::now();
    let o = c.stderr(Stdio::null()).output().expect("run replay");
    let el = t.elapsed();
    (
        o.status.code(),
        serde_json::from_slice(&o.stdout).expect("replay JSON"),
        el,
    )
}

/// One journald JSON line in the shape of the live VPS cloudflared lines
/// (2026-10-21 01:10 CEST probe); addresses are documentation ranges.
fn cf_line(t: u64, host: &str) -> String {
    json!({
        "_SYSTEMD_UNIT": "cloudflared.service",
        "PRIORITY": "3",
        "__REALTIME_TIMESTAMP": format!("{}000000", t),
        "MESSAGE": format!("2026-10-20T17:05:00Z ERR Request failed error=\"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:4001: connect: connection refused\" connIndex=1 dest=https://{host}/wp-login.php event=0 ip=198.51.100.13 type=http"), // repo-check: allow (test fixture address, never dialled)
    })
    .to_string()
}

fn host_streams(r: &Value) -> Vec<(String, String, u64)> {
    r["cortex"]["ddos"]["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["stream"].as_str().unwrap_or("").starts_with("edge-host:"))
        .map(|s| {
            (
                s["stream"].as_str().unwrap().to_string(),
                s["level"].as_str().unwrap().to_string(),
                s["total"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

#[test]
fn real_edge_rate_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cortex-replay");
    let dir = std::env::temp_dir().join(format!("nerve-real-edgerate-{}", std::process::id()));
    let cap = dir.join("capture");
    std::fs::create_dir_all(&cap).unwrap();
    for e in std::fs::read_dir(&fx).unwrap().flatten() {
        std::fs::copy(e.path(), cap.join(e.file_name())).unwrap();
    }
    let mut lines = vec![];
    for k in 0..348u64 {
        let t = T_END - 1800 + k * 5;
        lines.push(cf_line(t, "quiet.example.test"));
        lines.push(cf_line(t + 2, "hot.example.test"));
    }
    for k in 0..2400u64 {
        lines.push(cf_line(T_END - 59 + k / 40, "hot.example.test"));
    }
    std::fs::write(cap.join("edge-flood.jsonl"), lines.join("\n") + "\n").unwrap();

    // 1a. Flood on one hostname.
    let (code, r, el) = replay(&cap, &dir.join("data"), &[]);
    assert_eq!(code, Some(1), "failing verdict exits 1");
    let hs = host_streams(&r);
    eprintln!(
        "replay flood capture ({} cf lines, {:?}): host streams {hs:?}",
        lines.len(),
        el
    );
    let lvl = |n: &str| hs.iter().find(|h| h.0 == n).map(|h| h.1.as_str());
    assert_eq!(lvl("edge-host:hot.example.test"), Some("flood"));
    assert_eq!(lvl("edge-host:quiet.example.test"), Some("normal"));
    // The fixture's own two dead-origin hosts are streams too.
    assert_eq!(hs.len(), 4, "{hs:?}");
    let f = r["cortex"]["ddos"]["findings"]
        .as_array()
        .expect("ddos findings")
        .iter()
        .find(|f| f["id"] == "edge-host-rate")
        .cloned()
        .expect("edge-host-rate finding");
    assert_eq!(
        (
            f["severity"].as_str(),
            f["source"].as_str(),
            f["kind"].as_str()
        ),
        (Some("critical"), Some("edge-rate"), Some("ddos")),
        "{f}"
    );
    let title = f["title"].as_str().unwrap();
    assert!(
        title.starts_with("1 public hostname(s) under a request flood"),
        "{title}"
    );
    assert!(!title.contains("example.test"));
    assert!(f["evidence"][0]
        .as_str()
        .unwrap()
        .contains("edge-host:hot.example.test"));
    let inc: Vec<&Value> = r["incidents"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["cause"] == "ddos:edge-cloudflared")
        .collect();
    assert_eq!(inc.len(), 1, "one incident for the edge flood");
    assert_eq!(inc[0]["severity"], "critical", "{}", inc[0]);
    assert!(
        inc[0]["title"]
            .as_str()
            .unwrap()
            .contains("per public hostname"),
        "{}",
        inc[0]
    );
    assert!(
        !r["incidents"].as_array().unwrap().iter().any(|i| i["cause"]
            .as_str()
            .unwrap_or("")
            .starts_with("ddos:edge-host:")),
        "no per-host double incident"
    );
    eprintln!("finding: [{}] {title}", f["severity"]);
    eprintln!(
        "incident: {} [{}] {}",
        inc[0]["id"], inc[0]["severity"], inc[0]["title"]
    );
    eprintln!("verdict: score {} grade {}", r["score"], r["grade"]);
    let score_on = r["score"].as_u64().unwrap();

    // 1b. Switched off: no host streams, no finding, the fixture verdict.
    let (_, off, _) = replay(&cap, &dir.join("data"), &[("NERVE_EDGE_HOSTS", "0")]);
    assert!(host_streams(&off).is_empty());
    assert!(off["cortex"]["ddos"]["findings"].is_null());
    assert_eq!(
        (off["score"].as_u64(), off["grade"].as_str()),
        (Some(21), Some("F"))
    );
    eprintln!(
        "NERVE_EDGE_HOSTS=0: no host streams, score {} (on: {score_on})",
        off["score"]
    );

    // 2. Real VPS capture.
    let live_cap = std::env::var("CORTEX_REPLAY_DIR")
        .unwrap_or_else(|_| "/workspace/swarm/nerve-capture/replay1".into());
    if Path::new(&live_cap).join("state-1.json").exists() {
        let (_, r, el) = replay(Path::new(&live_cap), &dir.join("data"), &[]);
        let hs = host_streams(&r);
        assert!(
            !hs.is_empty(),
            "the real capture has cloudflared dest= lines"
        );
        assert!(hs.iter().all(|h| h.1 == "normal"), "{hs:?}");
        assert!(r["cortex"]["ddos"]["findings"].is_null());
        assert_eq!(
            (r["score"].as_u64(), r["grade"].as_str()),
            (Some(61), Some("D")),
            "replay1 verdict changed"
        );
        let total: u64 = hs.iter().map(|h| h.2).sum();
        eprintln!("replay1 live capture ({el:?}): {} host streams, {total} failed requests, all normal, score 61 grade D (unchanged)", hs.len());
        for h in &hs {
            eprintln!("  {} {} total={}", h.0, h.1, h.2);
        }
    } else {
        eprintln!("replay1 live capture not on this box: skipped");
    }

    // 3. Serve: wiring visible on the live API.
    let comps = dir.join("components.json");
    std::fs::write(&comps, r#"[{"name":"dead-svc","port":19772}]"#).unwrap();
    let spawn = |port: u16, extra: &[(&str, &str)]| {
        let mut c = Command::new(BIN);
        c.arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", dir.join(format!("data-{port}")))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", "19770-19779")
            .env_remove("NERVE_SENSORS_OFF")
            .env_remove("NERVE_EDGE_HOSTS")
            .stderr(Stdio::null());
        for (k, v) in extra {
            c.env(k, v);
        }
        Kill(c.spawn().expect("spawn nerve"))
    };
    let names = |st: &Value| -> Vec<String> {
        st["cortex_sensors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["name"].as_str().map(str::to_string))
            .collect()
    };
    let nerve = spawn(19770, &[("NERVE_EDGE_HOSTS_MAX", "16")]);
    let st = wait_state(19770, |s| s["cycle"].as_u64().unwrap_or(0) >= 2);
    assert!(names(&st).contains(&"edge-rate".to_string()), "{st}");
    assert_eq!(
        st["edge_hosts"],
        json!({"hosts": 0, "max_hosts": 16, "lines_counted": 0, "folded_into_other": 0})
    );
    let us = st["cortex_sensors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "edge-rate")
        .and_then(|s| s["us"].as_u64());
    eprintln!(
        "serve :19770 cycle {}: sensors {:?}, edge_hosts {}, edge-rate us {us:?}, cycle_ms {}",
        st["cycle"],
        names(&st),
        st["edge_hosts"],
        st["cycle_ms"]
    );
    drop(nerve);
    let nerve = spawn(19771, &[("NERVE_SENSORS_OFF", "edge-rate")]);
    let st = wait_state(19771, |s| s["cycle"].as_u64().unwrap_or(0) >= 2);
    assert!(st["cycle"].as_u64().unwrap_or(0) >= 2, "{st}");
    assert!(!names(&st).contains(&"edge-rate".to_string()));
    assert!(st["edge_hosts"].is_null());
    eprintln!(
        "serve :19771 NERVE_SENSORS_OFF=edge-rate cycle {}: sensors {:?}, edge_hosts null",
        st["cycle"],
        names(&st)
    );
    drop(nerve);
    let _ = std::fs::remove_dir_all(&dir);
}
