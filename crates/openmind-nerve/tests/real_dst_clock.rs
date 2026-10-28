// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for nerve T3.4 (slot 157, gated: `GALION_REAL=1`; run with
//! `--release` so the binary is the release build). Ports 20270-20279.
//!
//! 1. `serve`: the real binary on 127.0.0.1:20270 watching a live HTTP
//!    service (a thread here, :20271), self-eval every cycle, with 100
//!    legacy offset-less `self-eval-*.json` files pre-seeded. The new files
//!    carry the Zagreb offset, parse back to the report instant, and
//!    retention keeps exactly 96 with the new ones kept.
//! 2. `replay` with real VPS data on a fake clock: `/state` snapshots of the
//!    live galion-nerve (read-only GET over ssh when `NERVE_DST_SSH` is set,
//!    e.g. `root@host`, key in `NERVE_DST_SSH_KEY`; else the files in
//!    `NERVE_DST_CAPTURE`) are re-stamped every 30 s over 02:25+02:00 ..
//!    02:35+01:00 on 2026-10-25 (70 real minutes through the fold) and,
//!    as a control, over an ordinary night (2026-10-21 02:25 .. 03:35+02:00).
//!    Both runs of the release binary must give the same score, findings
//!    and incident causes; the fold run's report is stamped +01:00.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): PORT +0, SVC +1; `NERVE_DST_REAL_PORT_BASE`
/// moves the block, unset keeps the old
/// 20270-20279.
static PORTS: Block = Block::new("NERVE_DST_REAL_PORT_BASE", 20270, 10);

#[allow(non_snake_case)]
fn PORT() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn SVC() -> u16 {
    PORTS.port(1)
}
/// 2026-10-25T03:00:00+02:00 == 02:00:00+01:00.
const FOLD: u64 = 1_792_890_000;
/// 2026-10-21T02:25:00+02:00, the control night.
const CONTROL: u64 = 1_792_542_300;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(path: &str) -> Option<(u16, String)> {
    let addr = format!("127.0.0.1:{}", PORT()).parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    write!(s, "GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    let (h, body) = b.split_once("\r\n\r\n")?;
    Some((h.split_whitespace().nth(1)?.parse().ok()?, body.to_string()))
}

fn json_at(path: &str) -> Value {
    get(path)
        .and_then(|(_, b)| serde_json::from_str(&b).ok())
        .unwrap_or_default()
}

fn self_eval_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("self-eval-"))
                .collect()
        })
        .unwrap_or_default()
}

fn real() -> bool {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return false;
    }
    true
}

#[test]
fn real_serve_writes_offset_named_self_evals_and_prunes_legacy() {
    if !real() {
        return;
    }
    let svc = TcpListener::bind(("127.0.0.1", SVC())).expect("slot port free");
    std::thread::spawn(move || {
        for mut c in svc.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = c.read(&mut buf);
            let _ = c.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 3\r\n\r\nok\n");
        }
    });
    let dir = std::env::temp_dir().join(format!("nerve-real-dst-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let se = dir.join("data/self-eval");
    std::fs::create_dir_all(&se).unwrap();
    // 100 legacy files, one per 10 min ending a day ago, as the live VPS has.
    let day_ago = SystemTime::now() - Duration::from_secs(86_400); // repo-check: allow (feeds zagreb-time / file mtimes)
    for i in 0..100u64 {
        let t = day_ago - Duration::from_secs(600 * (100 - i));
        let secs = t.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        let n = format!(
            "self-eval-{}.json",
            openmind_nerve::util::ts(secs)
                .replace([':', '-'], "")
                .chars()
                .take(15)
                .collect::<String>()
        );
        let p = se.join(n);
        std::fs::write(&p, "{}").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    assert_eq!(self_eval_names(&se).len(), 100);
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"live-svc","port":{}}}]"#, SVC()),
    )
    .unwrap();
    let _nerve = Kill(
        Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{}", PORT()))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_REPORT_EVERY", "1")
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", &comps)
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", &PORTS.range())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn nerve"),
    );
    let t = Instant::now();
    while get("/healthz").is_none() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(200));
    }
    // Three cycles at 5 s.
    std::thread::sleep(Duration::from_secs(12));
    let names = self_eval_names(&se);
    let fresh: Vec<&String> = names.iter().filter(|n| n.len() == 35).collect();
    eprintln!("self-eval files: {} total, new: {fresh:?}", names.len());
    assert!(fresh.len() >= 2, "{names:?}");
    assert_eq!(names.len(), 96, "retention keeps 96");
    let now = SystemTime::now() // repo-check: allow (feeds zagreb-time)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    for n in &fresh {
        let at = openmind_nerve::engine::self_eval_file_instant(n).expect(n);
        assert!(now.abs_diff(at) < 60, "{n} is not now");
        let off = &n[25..30];
        assert!(off == "+0200" || off == "+0100", "{n}");
    }
    let r = json_at("/self-eval");
    let ts = r["ts"].as_str().unwrap_or("").to_string();
    eprintln!("/self-eval ts {ts} score {} pass {}", r["score"], r["pass"]);
    assert_eq!(zagreb_time::require_rfc3339(&ts), Ok(()));
    let st = json_at("/api/v1/state");
    let comp = st["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "live-svc")
        .cloned()
        .unwrap();
    assert_eq!(comp["status"], "healthy", "{comp}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Read-only GET of the live nerve `/state` over ssh.
fn vps_state(host: &str, key: Option<&str>) -> Value {
    let mut c = Command::new("ssh");
    if let Some(k) = key {
        c.args(["-i", k, "-o", "IdentitiesOnly=yes"]);
    }
    let out = c
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host])
        .arg("curl -s -m 10 http://127.0.0.1:8670/state")
        .output()
        .expect("ssh");
    serde_json::from_slice(&out.stdout).expect("live /state is JSON")
}

fn snapshots() -> Vec<Value> {
    if let Ok(host) = std::env::var("NERVE_DST_SSH") {
        let key = std::env::var("NERVE_DST_SSH_KEY").ok();
        let mut v = vec![];
        for i in 0..3 {
            if i > 0 {
                std::thread::sleep(Duration::from_secs(30));
            }
            v.push(vps_state(&host, key.as_deref()));
        }
        return v;
    }
    let dir = PathBuf::from(
        std::env::var("NERVE_DST_CAPTURE").expect("NERVE_DST_SSH or NERVE_DST_CAPTURE"),
    );
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap())
        .collect()
}

/// Tunnel counters per 30 s cycle, from the first to the last real snapshot,
/// so cycling the snapshots keeps the cumulative counters monotonic (a
/// counter going backwards would be a harness artifact, not traffic).
fn tunnel_rates(snaps: &[Value]) -> std::collections::HashMap<String, (f64, f64)> {
    let mut m = std::collections::HashMap::new();
    let (Some(a), Some(b)) = (snaps.first(), snaps.last()) else {
        return m;
    };
    let t = |s: &Value| {
        s["ts"]
            .as_str()
            .and_then(openmind_nerve::util::parse_rfc3339)
    };
    let cycles = match (t(a), t(b)) {
        (Some(x), Some(y)) if y > x => (y - x) as f64 / 30.0,
        _ => return m,
    };
    for ca in a["components"].as_array().into_iter().flatten() {
        let name = ca["name"].as_str().unwrap_or("");
        let cb = b["components"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["name"] == name);
        let (Some(cb), true) = (cb, ca["tunnel"].is_object()) else {
            continue;
        };
        let d = |k: &str| {
            (cb["tunnel"][k].as_u64().unwrap_or(0) as f64
                - ca["tunnel"][k].as_u64().unwrap_or(0) as f64)
                .max(0.0)
                / cycles
        };
        m.insert(name.to_string(), (d("requests"), d("request_errors")));
    }
    m
}

/// Write a capture of `n` cycles every 30 s from `t0`, cycling the real
/// snapshots, plus a CLF access log with each line's own Zagreb offset.
fn capture(dir: &Path, snaps: &[Value], t0: u64, n: u64) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let rates = tunnel_rates(snaps);
    for k in 0..n {
        let t = t0 + k * 30;
        let mut s = snaps[(k as usize) % snaps.len()].clone();
        for c in s["components"].as_array_mut().into_iter().flatten() {
            let name = c["name"].as_str().unwrap_or("").to_string();
            if let (Some((r, e)), Some(base)) = (
                rates.get(&name),
                snaps[0]["components"]
                    .as_array()
                    .and_then(|v| v.iter().find(|x| x["name"] == name.as_str())),
            ) {
                let r0 = base["tunnel"]["requests"].as_u64().unwrap_or(0);
                let e0 = base["tunnel"]["request_errors"].as_u64().unwrap_or(0);
                c["tunnel"]["requests"] = json!(r0 + (r * k as f64) as u64);
                c["tunnel"]["request_errors"] = json!(e0 + (e * k as f64) as u64);
            }
        }
        s["ts"] = json!(openmind_nerve::util::ts(t));
        std::fs::write(dir.join(format!("state-{k:04}.json")), s.to_string()).unwrap();
    }
    let mut log = String::new();
    for s in (t0..t0 + n * 30).step_by(2) {
        let z = openmind_nerve::util::ts(s);
        log.push_str(&format!(
            "203.0.113.{} - - [{}/Oct/{}:{} {}] \"GET / HTTP/1.1\" 200 9 \"-\" \"x\"\n", // repo-check: allow (TEST-NET-3 fixture, never dialled)
            1 + s % 50,
            &z[8..10],
            &z[0..4],
            &z[11..19],
            z[19..].replace(':', "")
        ));
    }
    std::fs::write(dir.join("access-tuner.log"), log).unwrap();
}

struct Out {
    ts: String,
    score: Value,
    ids: BTreeSet<String>,
    causes: BTreeSet<String>,
    notes: String,
}

fn replay(dir: &Path) -> Out {
    let out = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"))
        .arg("replay")
        .arg(dir)
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_REFLEX", "0")
        .output()
        .expect("run replay");
    let r: Value = serde_json::from_slice(&out.stdout).expect("report JSON");
    let ids = r["cortex"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|x| x["id"].as_str().map(str::to_string))
        .collect();
    let causes = r["incidents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["cause"].as_str().map(str::to_string))
        .collect();
    Out {
        ts: r["ts"].as_str().unwrap_or("").into(),
        score: r["score"].clone(),
        ids,
        causes,
        notes: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn real_vps_state_replayed_through_the_fold_matches_an_ordinary_night() {
    if !real() {
        return;
    }
    let snaps = snapshots();
    assert!(!snaps.is_empty());
    for s in &snaps {
        eprintln!(
            "live snapshot ts {} components {} system {}",
            s["ts"],
            s["components"].as_array().map_or(0, Vec::len),
            s["system"]["status"]
        );
    }
    let base = std::env::temp_dir().join(format!("nerve-real-dst-replay-{}", std::process::id()));
    let n = 141; // 70 min at 30 s
    let fold_dir = base.join("fold");
    let ctl_dir = base.join("control");
    capture(&fold_dir, &snaps, FOLD - 35 * 60, n);
    capture(&ctl_dir, &snaps, CONTROL, n);
    let f = replay(&fold_dir);
    let c = replay(&ctl_dir);
    eprintln!(
        "fold: ts {} score {} findings {:?} causes {:?}\n{}",
        f.ts,
        f.score,
        f.ids,
        f.causes,
        f.notes.trim()
    );
    eprintln!(
        "control: ts {} score {} findings {} causes {:?}\n{}",
        c.ts,
        c.score,
        c.ids.len(),
        c.causes,
        c.notes.trim()
    );
    assert_eq!(f.ts, "2026-10-25T02:35:00+01:00");
    assert_eq!(c.ts, "2026-10-21T03:35:00+02:00");
    assert!(
        f.notes
            .contains("2026-10-25T02:25:00+02:00 .. 2026-10-25T02:35:00+01:00"),
        "{}",
        f.notes
    );
    assert_eq!(f.score, c.score, "score differs at the fold");
    assert_eq!(f.ids, c.ids, "findings differ at the fold");
    assert_eq!(f.causes, c.causes, "incidents differ at the fold");
    for c in &f.causes {
        assert!(!c.starts_with("ddos:"), "rate false alarm {c}");
    }
    for id in &f.ids {
        assert!(
            !id.starts_with("restarts-") && !id.starts_with("memory-trend-"),
            "false alarm {id}"
        );
    }
    let _ = std::fs::remove_dir_all(&base);
}
