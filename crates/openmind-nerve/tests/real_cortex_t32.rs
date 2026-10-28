// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of cortex T3.2 cross-sensor correlation (gated:
//! `GALION_REAL=1`; run with `--release` for the release binary).
//! Slot 179: ports 20490-20499 only.
//!
//! 1. `openmind-nerve replay` (the real binary) of the golden fixture:
//!    16/F, every incident has a `correlation` section, `edge` holds the
//!    dead-origin and cloudflared error-rate findings. Then every capture
//!    dir in `NERVE_T32_CAPTURES` (comma list; live VPS captures stay on the
//!    box, never committed): the `edge` incident lists its member findings,
//!    each covered finding is charged once.
//! 2. `openmind-nerve serve` on 127.0.0.1:20490, live: an `edge`-group
//!    component on :20491 (nothing listening -> hard down, an `edge`
//!    signal), the `surface` sensor on :20492 (nothing listening ->
//!    `surface-http-down`) and the unresolvable name `t32..invalid`
//!    (`surface-dns`; the box resolver is a fake-IP proxy that answers any
//!    well-formed name, so an empty label is the only offline DNS failure), the
//!    `sshd` sensor on a scratch auth log grown 1.1 MiB:
//!    - one `edge` incident, sensors `components` + `surface`, both
//!      surface findings its members and covered (charged once, by it);
//!    - the auth-log bleed opens no incident (no flood signal) and is one
//!      8-point deduction;
//!    - :20491 comes up: the `edge` incident resolves, the surface findings
//!      are then one uncovered `edge` deduction ("scored once").

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): PORT +0, ORIGIN +1, SURFACE +2; `CORTEX_T32_REAL_PORT_BASE`
/// moves the block, unset keeps the old
/// 20490-20499.
static PORTS: Block = Block::new("CORTEX_T32_REAL_PORT_BASE", 20490, 10);

#[allow(non_snake_case)]
fn PORT() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn ORIGIN() -> u16 {
    PORTS.port(1)
}
#[allow(non_snake_case)]
fn SURFACE() -> u16 {
    PORTS.port(2)
}

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

fn wait_state(secs: u64, ok: impl Fn(&Value) -> bool) -> Value {
    let t = Instant::now();
    loop {
        let st = get(PORT(), "/api/v1/state").unwrap_or_default();
        if ok(&st) || t.elapsed() > Duration::from_secs(secs) {
            return st;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn open<'a>(st: &'a Value, cause: &str) -> Option<&'a Value> {
    st["incidents"]["open"]
        .as_array()?
        .iter()
        .find(|i| i["cause"] == cause)
}

fn member_keys(i: &Value) -> Vec<String> {
    i["correlation"]["findings"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f["finding"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn covered_by(st: &Value, finding: &str) -> Option<String> {
    st["self_eval"]["covered"]
        .as_array()?
        .iter()
        .find(|c| c["finding"] == finding)
        .and_then(|c| c["by"].as_str().map(str::to_string))
}

fn replay(dir: &Path, data: &Path, env: &[(&str, &str)]) -> (Option<i32>, Value, u128) {
    let t = Instant::now();
    let mut c = Command::new(BIN);
    c.env_clear().env("NERVE_DATA_DIR", data);
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c
        .arg("replay")
        .arg(dir)
        .stderr(Stdio::null())
        .output()
        .expect("run replay");
    let ms = t.elapsed().as_millis();
    (
        o.status.code(),
        serde_json::from_slice(&o.stdout).unwrap_or_default(),
        ms,
    )
}

/// A one-route HTTP origin answering 200 on every request until `stop`.
fn origin(port: u16, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind origin");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut s, _)) => {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut b = [0u8; 1024];
                    let _ = s.read(&mut b);
                    let _ = s.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
                    );
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    })
}

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        .unwrap_or(0)
}

#[test]
fn real_cortex_t32_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!("nerve-real-t32-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Replays through the real binary.
    let (code, r, ms) = replay(&fx.join("cortex-replay"), &dir.join("d0"), &[]);
    assert_eq!(code, Some(1));
    assert_eq!(
        (r["score"].as_u64(), r["grade"].as_str()),
        (Some(16), Some("F"))
    );
    let incs = r["incidents"].as_array().unwrap();
    assert!(incs.iter().all(|i| i["correlation"].is_object()));
    let edge = incs.iter().find(|i| i["cause"] == "edge").unwrap();
    assert_eq!(
        member_keys(edge),
        [
            "bleeding:edge-dead-origins",
            "bleeding:error-rate-cloudflared"
        ]
    );
    eprintln!("fixture replay: {}/{} in {ms} ms", r["score"], r["grade"]);
    for cap in std::env::var("NERVE_T32_CAPTURES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        let cap = Path::new(cap);
        let (_, r, ms) = replay(
            cap,
            &dir.join("dc"),
            &[("NERVE_SSHD", "1"), ("NERVE_SSHD_AUTH_LOG", "0")],
        );
        assert!(r["score"].is_u64(), "{} gave no report", cap.display());
        eprintln!(
            "capture {}: score {} {} pass {} in {ms} ms",
            cap.display(),
            r["score"],
            r["grade"],
            r["pass"]
        );
        for d in r["deductions"].as_array().unwrap() {
            eprintln!(
                "  -{} {}",
                d["points"],
                d["what"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(110)
                    .collect::<String>()
            );
        }
        for i in r["incidents"].as_array().unwrap() {
            let c = &i["correlation"];
            eprintln!(
                "  incident {} [{}] rules {} sensors {} members {:?} folded {}",
                i["cause"],
                i["severity"],
                c["rules"],
                c["sensors"],
                member_keys(i),
                c["folded"]
            );
            // Members never inflate the component count.
            for m in member_keys(i) {
                assert!(!i["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|x| x == &Value::String(m.clone())));
            }
        }
        // Every member is charged at most once: no deduction names it
        // apart from its incident.
        let dtext: Vec<String> = r["deductions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["what"].as_str().unwrap_or("").to_string())
            .collect();
        for i in r["incidents"].as_array().unwrap() {
            for c in i["correlation"]["findings"].as_array().unwrap() {
                let (kind, id) = c["finding"].as_str().unwrap().split_once(':').unwrap();
                let n = r["cortex"]
                    .as_object()
                    .unwrap()
                    .values()
                    .filter_map(Value::as_array)
                    .flatten()
                    .filter(|f| f["id"] == id && f["kind"] == kind)
                    .map(|f| {
                        format!(
                            "{kind} ({}): {}",
                            f["severity"].as_str().unwrap_or(""),
                            f["title"].as_str().unwrap_or("")
                        )
                    })
                    .filter(|w| dtext.contains(w))
                    .count();
                assert_eq!(n, 0, "{id} charged twice");
            }
        }
    }

    // 2. Live serve.
    assert!(
        TcpListener::bind(("127.0.0.1", PORT())).is_ok(),
        "port {} busy",
        PORT()
    );
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"t32-origin","port":{},"path":"/health","group":"edge","gaia":"AETHER"}}]"#, ORIGIN()),
    )
    .unwrap();
    let auth = dir.join("auth.log");
    std::fs::write(&auth, b"").unwrap();
    let t_start = Instant::now();
    let child = Command::new(BIN)
        .arg("serve")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_ADDR", format!("127.0.0.1:{}", PORT()))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_REFLEX", "0")
        .env("NERVE_LISTEN_SCOPE", PORTS.range())
        .env("NERVE_SSHD", "1")
        .env("NERVE_SSHD_AUTH_LOG", &auth)
        .env(
            "NERVE_SURFACE_TARGETS",
            format!(
                "http://127.0.0.1:{}/health@200,http://t32..invalid/",
                SURFACE()
            ),
        )
        .env("NERVE_SURFACE_EVERY_S", "5")
        .env("NERVE_SURFACE_TIMEOUT_MS", "1500")
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn nerve");
    let pid = child.id();
    let _k = Kill(child);
    // Let the sshd sensor take its baseline, then grow the log 1.1 MiB.
    let st = wait_state(30, |s| s["cycle"].as_u64().unwrap_or(0) >= 1);
    assert!(
        st["cycle"].as_u64().unwrap_or(0) >= 1,
        "nerve did not start"
    );
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&auth)
            .unwrap();
        f.write_all(&vec![b'x'; 1_153_434]).unwrap();
    }
    let want = [
        "bleeding:surface-dns".to_string(),
        "bleeding:surface-http-down".to_string(),
    ];
    let st = wait_state(60, |s| {
        open(s, "edge").is_some_and(|i| want.iter().all(|w| member_keys(i).contains(w)))
            && s["cortex"]["bleeding"]
                .as_array()
                .is_some_and(|a| a.iter().any(|f| f["id"] == "sshd-auth-log-growth"))
    });
    let edge = open(&st, "edge").unwrap_or_else(|| panic!("no edge incident: {}", st["incidents"]));
    let keys = member_keys(edge);
    eprintln!(
        "live {:.1} s: edge [{}] rules {} sensors {} members {keys:?}; score {} {} pass {}",
        t_start.elapsed().as_secs_f64(),
        edge["severity"],
        edge["correlation"]["rules"],
        edge["correlation"]["sensors"],
        st["system"]["score"],
        st["system"]["grade"],
        st["system"]["pass"]
    );
    for w in &want {
        assert!(keys.contains(w), "{w} not in {keys:?}");
        assert_eq!(covered_by(&st, w).as_deref(), Some("edge"), "{w}");
    }
    let sensors = edge["correlation"]["sensors"].as_array().unwrap();
    assert!(sensors.iter().any(|s| s == "surface"), "{sensors:?}");
    assert_eq!(edge["components"], serde_json::json!(["t32-origin"]));
    assert_eq!(edge["cause"], "edge");
    // The auth-log bleed: no incident, one deduction.
    assert!(
        open(&st, "ssh-flood").is_none(),
        "a finding alone opens no incident"
    );
    let ded = st["self_eval"]["deductions"].as_array().unwrap();
    for d in ded {
        eprintln!("  -{} {}", d["points"], d["what"]);
    }
    let bleed: Vec<&Value> = ded
        .iter()
        .filter(|d| d["what"].as_str().unwrap_or("").contains("auth log grew"))
        .collect();
    assert_eq!(bleed.len(), 1);
    assert_eq!(bleed[0]["points"], 8);
    assert_eq!(
        ded.iter()
            .filter(|d| d["what"].as_str().unwrap_or("").contains("incident: edge"))
            .count(),
        1,
        "the edge incident is charged once"
    );
    assert!(
        !ded.iter().any(
            |d| d["what"].as_str().unwrap_or("").starts_with("bleeding (")
                && d["what"].as_str().unwrap_or("").contains("surface")
        ),
        "surface findings are not charged on top of the incident"
    );
    let rss = rss_kib(pid);
    eprintln!("cycle_ms {} rss {rss} KiB", st["cycle_ms"]);

    // 3. The origin comes up: edge incident resolves, surface findings are
    //    then one uncovered `edge` deduction.
    let stop = Arc::new(AtomicBool::new(false));
    let h = origin(ORIGIN(), stop.clone());
    let st = wait_state(60, |s| open(s, "edge").is_none());
    let resolved = st["incidents"]["resolved"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["cause"] == "edge")
        .cloned()
        .unwrap_or_default();
    eprintln!(
        "origin up: edge resolved {} after {} cycle(s); score {} {}",
        resolved["resolved"], resolved["cycles"], st["system"]["score"], st["system"]["grade"]
    );
    assert!(open(&st, "edge").is_none(), "{}", st["incidents"]);
    assert_eq!(resolved["state"], "resolved");
    let ded = st["self_eval"]["deductions"].as_array().unwrap();
    for d in ded {
        eprintln!(
            "  -{} {} | {}",
            d["points"],
            d["what"],
            d["why"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(160)
                .collect::<String>()
        );
    }
    let edge_d: Vec<&Value> = ded
        .iter()
        .filter(|d| {
            d["why"]
                .as_str()
                .unwrap_or("")
                .contains("same cause `edge`, scored once")
        })
        .collect();
    assert_eq!(
        edge_d.len(),
        1,
        "surface findings: one deduction for cause edge"
    );
    assert!(covered_by(&st, "bleeding:surface-dns").is_none());
    stop.store(true, Ordering::Relaxed);
    let _ = h.join();
    drop(_k);
    assert!(
        TcpListener::bind(("127.0.0.1", PORT())).is_ok(),
        "nerve gone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
