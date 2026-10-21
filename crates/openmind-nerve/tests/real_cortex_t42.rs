// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of cortex T4.2 HADES plan templates (gated:
//! `GALION_REAL=1`; run with `--release` for the release binary).
//! Slot 192: ports 20620-20629 only. Box only: nothing here touches the VPS.
//!
//! 1. `openmind-nerve replay DIR --plans` (the real binary) of the golden
//!    fixture and of every capture dir in `NERVE_T42_CAPTURES` (comma list;
//!    live VPS captures stay on the box, never committed): every plan passes
//!    the real hades-gate admin-safety review; the replay is repeated and
//!    must give byte-identical plans.
//! 2. `openmind-nerve serve` on 127.0.0.1:20620, live, with real sockets:
//!    - `t42-voice` (unit t42-voice.service) answers 503 `loading` on :20621
//!      -> `unit-restart` plan for t42-voice.service after the grace;
//!    - a loopback listener on :20622 no component owns -> `stray-stop`;
//!    - :20623 bound to 0.0.0.0 (no component) and `t42-api`
//!      (unit t42-api.service) bound to 0.0.0.0:20624 -> two `rebind-loopback`
//!      plans (listener-20623, t42-api.service);
//!    - 800 requests to the nerve API -> `ddos:nerve-api` -> `edge-rate-limit`.
//!    `GET /api/v1/cortex/plans` serves them; two cycles give the same plan
//!    text (same approval hash); the nerve ran nothing (the stray listener
//!    still answers, no reflex action, no audit entry); `openmind-nerve
//!    plans STATE.json` on the saved state gives the same plans. Then the
//!    stray closes and the voice answers 200: those plans disappear.

use hades_gate::safety;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT: u16 = 20620;
const VOICE: u16 = 20621;
const STRAY: u16 = 20622;
const PUBLIC: u16 = 20623;
const API: u16 = 20624;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http(port: u16, path: &str) -> Option<String> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    Some(b)
}

fn get(port: u16, path: &str) -> Option<Value> {
    let b = http(port, path)?;
    serde_json::from_str(b.split_once("\r\n\r\n")?.1).ok()
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
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// A tiny HTTP server on `addr` answering `status`/`body` until `stop`;
/// `status` can be switched while it runs.
fn server(
    addr: &str,
    stop: Arc<AtomicBool>,
    loading: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut s, _)) => {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut b = [0u8; 1024];
                    let _ = s.read(&mut b);
                    let (code, body) = if loading.load(Ordering::Relaxed) {
                        ("503 Service Unavailable", r#"{"status":"loading"}"#)
                    } else {
                        ("200 OK", r#"{"status":"ok"}"#)
                    };
                    let _ = write!(
                        s,
                        "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    })
}

fn plan_ids(p: &Value) -> Vec<String> {
    p["plans"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn text(x: &Value) -> String {
    x["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every plan passes the real hades-gate review; returns (family, target) lines.
fn gate_check(p: &Value, label: &str) -> Vec<String> {
    assert_eq!(p["executed"], false, "{label}");
    let mut out = vec![];
    for x in p["plans"].as_array().unwrap() {
        let t = text(x);
        let target = x["target"].as_str().unwrap();
        assert_eq!(
            safety::review(target, t.as_bytes()),
            Ok(()),
            "{label}: {} refused by hades-gate:\n{t}",
            x["id"]
        );
        assert!(hades_gate::token::valid_target(target));
        out.push(format!(
            "{} -> {target} [{}] incident {} findings {} ({} steps, {} B)",
            x["family"].as_str().unwrap(),
            x["severity"].as_str().unwrap(),
            x["incident"],
            x["findings"],
            x["steps"].as_array().unwrap().len(),
            x["plan_bytes"]
        ));
    }
    out
}

fn replay_plans(dir: &Path, data: &Path, env: &[(&str, &str)]) -> (Value, u128) {
    let t = Instant::now();
    let mut c = Command::new(BIN);
    c.env_clear().env("NERVE_DATA_DIR", data);
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c
        .arg("replay")
        .arg(dir)
        .arg("--plans")
        .stderr(Stdio::null())
        .output()
        .expect("run replay");
    assert_eq!(o.status.code(), Some(0), "{}", dir.display());
    (
        serde_json::from_slice(&o.stdout).expect("plans json"),
        t.elapsed().as_millis(),
    )
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
fn real_cortex_t42_on_the_box() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let fx = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = std::env::temp_dir().join(format!("nerve-real-t42-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Replays through the real binary.
    let mut caps: Vec<(PathBuf, Vec<(&str, &str)>)> = vec![(fx.join("cortex-replay"), vec![])];
    for c in std::env::var("NERVE_T42_CAPTURES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
    {
        caps.push((
            PathBuf::from(c),
            vec![("NERVE_SSHD", "1"), ("NERVE_SSHD_AUTH_LOG", "0")],
        ));
    }
    for (cap, env) in &caps {
        let (p, ms) = replay_plans(cap, &dir.join("d1"), env);
        let (again, _) = replay_plans(cap, &dir.join("d2"), env);
        assert_eq!(
            p,
            again,
            "{}: plans differ between two replays",
            cap.display()
        );
        eprintln!(
            "replay {}: {} plan(s) {} in {ms} ms, {} skipped",
            cap.display(),
            p["count"],
            p["by_family"],
            p["skipped"].as_array().unwrap().len()
        );
        for l in gate_check(&p, &cap.display().to_string()) {
            eprintln!("  {l}");
        }
        for s in p["skipped"].as_array().unwrap() {
            eprintln!("  skipped {}: {}", s["finding"], s["reason"]);
        }
    }

    // 2. Live serve with real sockets.
    for p in [PORT, VOICE, STRAY, PUBLIC, API] {
        assert!(TcpListener::bind(("127.0.0.1", p)).is_ok(), "port {p} busy");
    }
    let stop_all = Arc::new(AtomicBool::new(false));
    let stop_stray = Arc::new(AtomicBool::new(false));
    let loading = Arc::new(AtomicBool::new(true));
    let never = Arc::new(AtomicBool::new(false));
    let h_voice = server(
        &format!("127.0.0.1:{VOICE}"),
        stop_all.clone(),
        loading.clone(),
    );
    let h_stray = server(
        &format!("127.0.0.1:{STRAY}"),
        stop_stray.clone(),
        never.clone(),
    );
    let h_pub = server(
        &format!("0.0.0.0:{PUBLIC}"),
        stop_all.clone(),
        never.clone(),
    );
    let h_api = server(&format!("0.0.0.0:{API}"), stop_all.clone(), never.clone());
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(
            r#"[{{"name":"t42-voice","unit":"t42-voice.service","port":{VOICE},"path":"/health","gaia":"AETHER","critical":false}},
                {{"name":"t42-api","unit":"t42-api.service","port":{API},"path":"/health","gaia":"APOLLO"}}]"#
        ),
    )
    .unwrap();
    let t0 = Instant::now();
    let child = Command::new(BIN)
        .arg("serve")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_ADDR", format!("127.0.0.1:{PORT}"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", &comps)
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_LISTEN_SCOPE", "20620-20629")
        .env("NERVE_LOADING_GRACE", "4")
        .env("NERVE_DDOS_MIN_RPS", "5")
        .env("NERVE_DDOS_MIN_REQUESTS", "200")
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn nerve");
    let pid = child.id();
    let _k = Kill(child);
    assert!(
        wait(15, || get(PORT, "/api/v1/state")
            .filter(|s| s["cycle"].as_u64() >= Some(1)))
        .is_some(),
        "nerve did not start"
    );
    // Flood the nerve API: 8 threads x 100 requests.
    let hs: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                (0..100)
                    .filter(|_| http(PORT, "/healthz").is_some())
                    .count()
            })
        })
        .collect();
    let sent: usize = hs.into_iter().map(|h| h.join().unwrap()).sum();
    let want = [
        "plan-edge-rate-limit-cloudflare-edge-nerve-api".to_string(),
        "plan-unit-restart-t42-voice.service".to_string(),
        format!("plan-rebind-loopback-listener-{PUBLIC}"),
        "plan-rebind-loopback-t42-api.service".to_string(),
        format!("plan-stray-stop-stray-listener-{STRAY}"),
    ];
    let p = wait(60, || {
        get(PORT, "/api/v1/cortex/plans").filter(|p| {
            let ids = plan_ids(p);
            want.iter().all(|w| ids.contains(w))
        })
    })
    .unwrap_or_else(|| {
        panic!(
            "plans never complete: {:#}",
            get(PORT, "/api/v1/cortex/plans").unwrap_or_default()
        )
    });
    let t_plans = t0.elapsed().as_secs_f64();
    eprintln!("live: {sent} flood requests; all 5 families' plans after {t_plans:.1} s:");
    for l in gate_check(&p, "live") {
        eprintln!("  {l}");
    }
    let by_id = |p: &Value, id: &str| {
        p["plans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == id)
            .cloned()
            .unwrap()
    };
    let voice = by_id(&p, "plan-unit-restart-t42-voice.service");
    assert!(voice["incident"].is_string(), "{voice}");
    eprintln!(
        "  unit-restart plan text:\n    {}",
        text(&voice).replace('\n', "\n    ")
    );
    // Stable across cycles: same text -> same hades-gate plan hash.
    let c1 = get(PORT, "/api/v1/state").unwrap()["cycle"]
        .as_u64()
        .unwrap();
    let p2 = wait(20, || {
        get(PORT, "/api/v1/state").filter(|s| s["cycle"].as_u64() > Some(c1))
    })
    .unwrap()["cortex_plans"]
        .clone();
    for id in &want[1..] {
        assert_eq!(
            text(&by_id(&p, id)),
            text(&by_id(&p2, id)),
            "{id} changed between cycles"
        );
        assert_eq!(
            hades_gate::token::plan_hash(text(&by_id(&p, id)).as_bytes()),
            hades_gate::token::plan_hash(text(&by_id(&p2, id)).as_bytes())
        );
    }
    // The nerve ran nothing: the stray still answers, no reflex/audit action.
    assert!(
        http(STRAY, "/").is_some_and(|b| b.contains("200 OK")),
        "stray was touched"
    );
    let st = get(PORT, "/api/v1/state").unwrap();
    assert!(
        st["reflex"]["recent"]
            .as_array()
            .is_none_or(|a| a.is_empty()),
        "{}",
        st["reflex"]["recent"]
    );
    let audit = get(PORT, "/api/v1/audit").unwrap_or_default();
    eprintln!("  reflex.recent {} audit {}", st["reflex"]["recent"], audit);
    // CLI on the saved state gives the same plans.
    let sf = dir.join("state.json");
    std::fs::write(&sf, st.to_string()).unwrap();
    let o = Command::new(BIN).arg("plans").arg(&sf).output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    let cli: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(cli, st["cortex_plans"], "CLI differs from the served plans");
    let bad = Command::new(BIN)
        .arg("plans")
        .arg(dir.join("missing.json"))
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    let rss = rss_kib(pid);
    eprintln!("  cycle_ms {} rss {rss} KiB", st["cycle_ms"]);

    // 3. Fix two problems: the stray closes, the voice is healthy.
    stop_stray.store(true, Ordering::Relaxed);
    h_stray.join().unwrap();
    loading.store(false, Ordering::Relaxed);
    let t_fix = Instant::now();
    let gone = [
        format!("plan-stray-stop-stray-listener-{STRAY}"),
        "plan-unit-restart-t42-voice.service".to_string(),
    ];
    let p3 = wait(40, || {
        get(PORT, "/api/v1/cortex/plans").filter(|p| {
            let ids = plan_ids(p);
            gone.iter().all(|g| !ids.contains(g))
        })
    })
    .unwrap_or_else(|| {
        panic!(
            "fixed problems keep their plans: {:?}",
            plan_ids(&get(PORT, "/api/v1/cortex/plans").unwrap_or_default())
        )
    });
    eprintln!(
        "fixed: stray + voice plans gone after {:.1} s; left: {:?}",
        t_fix.elapsed().as_secs_f64(),
        plan_ids(&p3)
    );
    assert!(plan_ids(&p3).contains(&"plan-rebind-loopback-t42-api.service".to_string()));

    stop_all.store(true, Ordering::Relaxed);
    for h in [h_voice, h_pub, h_api] {
        h.join().unwrap();
    }
    let _ = std::fs::remove_dir_all(&dir);
}
