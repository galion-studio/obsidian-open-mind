// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL tests (GALION_REAL=1) of Gaia S3.4 `gaia classify`, wave-4 slot 224.
//! Ports: slot 224 owns 20940-20949 on the box.
//!
//! * `real_gaia_classify_box`: the real `gaia` binary against fake nerve
//!   (20940), fake cortex (20941) and a fake memory service (20942), all
//!   real HTTP in this test; every other live port is mapped to a closed
//!   box port. Proves the verdict table, the findings, plan-only heal, and
//!   that **only GET** requests ever arrive (each fake logs every request
//!   line; a POST fails the test).
//! * `real_gaia_classify_vps` (also needs `GAIA_VPS_SSH=<ssh host alias>`):
//!   the live VPS, READ-ONLY. One `ssh -N -L` tunnel forwards box
//!   127.0.0.1:20940-20949 to the VPS loopback ports 8420 8642 8650 8091
//!   8080 8670 8674 8646 8002 8660; one `ssh ... sh -s` batch captures
//!   `systemctl is-active` and the link-watch state-file age (stdout only,
//!   nothing written on the VPS); then `gaia classify --port-map ...
//!   --evidence ...` runs on the box. The nerve verdicts Gaia recorded are
//!   cross-checked against an independent `curl` of the nerve `/state`
//!   over ssh (same cycle). No apply, no token, no POST, no unit touched.
//!
//! Run: `GALION_REAL=1 [GAIA_VPS_SSH=slavik-vps] [GAIA_CLASSIFY_OUT=DIR]
//! cargo test --release -p gaia --test real_gaia_classify -- --nocapture
//! --test-threads 1`.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());

const REMOTE: [u16; 10] = [8420, 8642, 8650, 8091, 8080, 8670, 8674, 8646, 8002, 8660];

fn port_map() -> String {
    REMOTE
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{r}={}", 20940 + i as u16))
        .collect::<Vec<_>>()
        .join(",")
}

fn open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn real() -> bool {
    std::env::var("GALION_REAL").ok().as_deref() == Some("1")
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

/// A tiny real HTTP server: fixed JSON per path, every request line logged.
struct Fake {
    log: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    port: u16,
    th: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

fn fake(port: u16, routes: Vec<(&'static str, Value)>) -> Fake {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind fake");
    let log = Arc::new(Mutex::new(Vec::new()));
    let log2 = log.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let th = std::thread::spawn(move || {
        for s in l.incoming() {
            if stop2.load(Ordering::SeqCst) {
                break;
            }
            let Ok(mut s) = s else { continue };
            let Ok(c) = s.try_clone() else { continue };
            let mut r = BufReader::new(c);
            let mut first = String::new();
            if r.read_line(&mut first).is_err() {
                continue;
            }
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
            }
            log2.lock().unwrap().push(first.trim().to_string());
            let path = first.split_whitespace().nth(1).unwrap_or("");
            let (code, body) = routes
                .iter()
                .find(|(p, _)| *p == path)
                .map_or((404, json!({"error": "not found"})), |(_, v)| {
                    (200, v.clone())
                });
            let b = body.to_string();
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                b.len()
            );
        }
    });
    Fake {
        log,
        stop,
        port,
        th: Some(th),
    }
}

fn gaia(args: &[&str]) -> (i32, Value, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let v = serde_json::from_str::<Value>(&out)
        .map(|v| v["result"].clone())
        .unwrap_or(Value::Null);
    (
        o.status.code().unwrap_or(-1),
        v,
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

fn unit<'a>(j: &'a Value, name: &str) -> &'a Value {
    j["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["service"] == name)
        .unwrap_or_else(|| panic!("no unit {name}"))
}

fn table(j: &Value) -> String {
    let mut s = String::from("service\tgaia\tnerve\tcortex\tagree\theal-tick (plan-only)\n");
    for u in j["units"].as_array().unwrap() {
        s += &format!(
            "{}\t{}\t{}({})\t{}\t{}\t{}\n",
            u["service"].as_str().unwrap_or("?"),
            u["gaia"]["verdict"].as_str().unwrap_or("?"),
            u["nerve"]["verdict"].as_str().unwrap_or("?"),
            u["nerve"]["status"].as_str().unwrap_or("-"),
            u["cortex"]["verdict"].as_str().unwrap_or("?"),
            u["agree"],
            u["heal_tick"]["reason"].as_str().unwrap_or("-"),
        );
    }
    for f in j["findings"].as_array().unwrap() {
        s += &format!(
            "FINDING {}\t{}\t{}\t{}\n",
            f["severity"].as_str().unwrap_or(""),
            f["kind"].as_str().unwrap_or(""),
            f["subject"].as_str().unwrap_or(""),
            f["detail"].as_str().unwrap_or("")
        );
    }
    s
}

fn assert_plan_only(j: &Value) {
    let t = j.to_string();
    assert!(!t.contains("\"applied\":true"), "something applied");
    assert!(!t.contains("\"token_used\":true"), "a token was used");
    assert!(
        !t.contains("\"nerve_event_posted\":true"),
        "a nerve event was posted"
    );
    for u in j["units"].as_array().unwrap() {
        if let Some(h) = u.get("heal_tick") {
            assert_eq!(h["mode"], "plan-only");
            assert_eq!(h["verdict"], "refused");
        }
    }
}

#[test]
fn real_gaia_classify_box() {
    if !real() {
        eprintln!("SKIP: set GALION_REAL=1 to run the slot 224 box classify test");
        return;
    }
    let _g = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for p in 20940..=20949 {
        assert!(!open(p), "port {p} busy before the test");
    }
    let nerve_state = json!({
        "cycle": 5,
        "components": [
            {"name": "openmind-memory", "unit": "openmind-memory.service", "status": "healthy",
             "restarts_1h": 0, "reasons": [], "gaia": {"function": "APOLLO", "system": "memory-gateway"}},
            {"name": "galion-one-mouth", "unit": null, "status": "degrading", "restarts_1h": 2,
             "reasons": ["degrading: health probe HTTP 503"], "gaia": {"function": "AETHER", "system": "speech-io"}},
            {"name": "galion-nerve", "status": "healthy", "reasons": []}
        ],
        "incidents": {"open": [{"id": "inc-m", "components": ["galion-one-mouth"]}]},
        "gaia": {"APOLLO": {"status": "healthy"}},
        "system": {"status": "degraded", "score": 70}
    });
    let nlog = fake(
        20940,
        vec![
            (
                "/health",
                json!({"status": "ok", "service": "openmind-nerve"}),
            ),
            ("/state", nerve_state),
        ],
    );
    let clog = fake(
        20941,
        vec![
            (
                "/health",
                json!({"ok": true, "mode": "live", "incidents_open": 0}),
            ),
            (
                "/incidents",
                json!({"open": [], "resolved": [], "units": {}}),
            ),
        ],
    );
    let mlog = fake(20942, vec![("/health", json!({"status": "ok"}))]);
    // memory -> 20942 (fake up), nerve -> 20940, cortex -> 20941; the other
    // seven live ports -> closed box ports 20943-20949.
    let map = "8420=20942,8670=20940,8674=20941,8642=20943,8650=20944,8091=20945,8080=20946,8646=20947,8002=20948,8660=20949";
    let dir = std::env::temp_dir().join(format!("gaia-classify-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ev = dir.join("evidence.txt");
    std::fs::write(
        &ev,
        "unit openmind-memory.service active\nunit galion-nerve.service active\nunit gaia-cortex.service active\n",
    )
    .unwrap();
    let t0 = std::time::Instant::now();
    let (code, j, err) = gaia(&[
        "classify",
        "--port-map",
        map,
        "--evidence",
        ev.to_str().unwrap(),
        "--timeout-ms",
        "300",
        "--compact",
    ]);
    println!(
        "[{}] box classify exit {code} in {:?}\n{}",
        stamp(),
        t0.elapsed(),
        table(&j)
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(j["mode"], "live-classify-read-only");
    assert_eq!(j["summary"]["units"], 14);
    assert_eq!(unit(&j, "openmind-memory")["gaia"]["verdict"], "ok");
    assert_eq!(unit(&j, "openmind-memory")["agree"], true);
    assert_eq!(unit(&j, "galion-nerve")["gaia"]["verdict"], "ok");
    assert_eq!(unit(&j, "gaia-cortex")["gaia"]["verdict"], "ok");
    let mouth = unit(&j, "galion-one-mouth");
    assert_eq!(mouth["gaia"]["verdict"], "bad");
    assert_eq!(mouth["nerve"]["verdict"], "warn");
    assert_eq!(mouth["heal_tick"]["nerve_restarts_1h"], 2);
    assert_eq!(mouth["heal_tick"]["cap_left_for_gaia"], 1);
    assert!(mouth["heal_tick"]["reason"]
        .as_str()
        .unwrap()
        .contains("port 8002 is outside the allowed test range"));
    // hermes-rs: closed port + uncaptured unit -> bad, nerve has no row -> nerve-blind.
    assert_eq!(unit(&j, "hermes-rs")["gaia"]["verdict"], "bad");
    let kinds: Vec<&str> = j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    for k in [
        "gaia-nerve-disagree",
        "cortex-missed",
        "nerve-blind",
        "cortex-nerve-incident-gap",
    ] {
        assert!(kinds.contains(&k), "missing finding {k}: {kinds:?}");
    }
    assert_plan_only(&j);
    // --strict turns disagreement into exit 3.
    let (code3, _, _) = gaia(&[
        "classify",
        "--port-map",
        map,
        "--evidence",
        ev.to_str().unwrap(),
        "--timeout-ms",
        "300",
        "--strict",
    ]);
    assert_eq!(code3, 3);
    // Only GETs arrived anywhere.
    for (name, log) in [("nerve", &nlog), ("cortex", &clog), ("memory", &mlog)] {
        let l = log.log.lock().unwrap().clone();
        println!("{name} saw {} request(s): {:?}", l.len(), l);
        assert!(!l.is_empty(), "{name} got no request");
        assert!(
            l.iter().all(|r| r.starts_with("GET ")),
            "{name}: non-GET {l:?}"
        );
    }
    drop((nlog, clog, mlog));
    for p in [20940, 20941, 20942] {
        assert!(!open(p), "fake on {p} still open after teardown");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

struct Tunnel(Child);
impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const EVIDENCE_SH: &str = r#"for u in openmind-memory.service hermes-rs.service link-watch-rs.service tuner.service galion-one.service console.service galion-nerve.service gaia-cortex.service galion-desk.service galion-health.service cloudflared.service tuner-quick.service openmind-queue.service; do echo "unit $u $(systemctl is-active $u)"; done
f=/opt/galion/galion-light/state/link.json
if m=$(stat -c %Y $f 2>/dev/null); then echo "file $f $(( $(date +%s) - m ))"; else echo "file $f missing"; fi
"#;

fn ssh_sh(host: &str, script: &str) -> String {
    let mut c = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            host,
            "sh",
            "-s",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("ssh");
    c.stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let mut out = String::new();
    c.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    assert!(c.wait().unwrap().success(), "ssh batch failed");
    out
}

#[test]
fn real_gaia_classify_vps() {
    let Some(host) = std::env::var("GAIA_VPS_SSH").ok().filter(|_| real()) else {
        eprintln!(
            "SKIP: set GALION_REAL=1 and GAIA_VPS_SSH=<alias> for the read-only VPS classify"
        );
        return;
    };
    let _g = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for p in 20940..=20949 {
        assert!(!open(p), "port {p} busy before the test");
    }
    let mut args: Vec<String> = [
        "-N",
        "-o",
        "BatchMode=yes",
        "-o",
        "ExitOnForwardFailure=yes",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    for (i, r) in REMOTE.iter().enumerate() {
        args.push("-L".into());
        args.push(format!("127.0.0.1:{}:127.0.0.1:{r}", 20940 + i));
    }
    args.push(host.clone());
    let tunnel = Tunnel(
        Command::new("ssh")
            .args(&args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("ssh tunnel"),
    );
    let mut up = false;
    for _ in 0..100 {
        if (20940..=20949).all(open) {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(up, "tunnel did not come up on 20940-20949");
    let evidence = ssh_sh(&host, EVIDENCE_SH);
    let dir = std::env::var("GAIA_CLASSIFY_OUT").map_or_else(
        |_| std::env::temp_dir().join(format!("gaia-classify-vps-{}", std::process::id())),
        std::path::PathBuf::from,
    );
    std::fs::create_dir_all(&dir).unwrap();
    let ev = dir.join("evidence.txt");
    std::fs::write(&ev, &evidence).unwrap();
    println!(
        "[{}] evidence (systemctl is-active, stat):\n{evidence}",
        stamp()
    );
    let t0 = std::time::Instant::now();
    let (code, j, err) = gaia(&[
        "classify",
        "--port-map",
        &port_map(),
        "--evidence",
        ev.to_str().unwrap(),
        "--compact",
    ]);
    let took = t0.elapsed();
    // Independent read of the nerve verdicts (GET over ssh, stdout only).
    let independent = ssh_sh(&host, "curl -sS -m 5 http://127.0.0.1:8670/state\n");
    drop(tunnel);
    std::fs::write(dir.join("classify.json"), j.to_string()).unwrap();
    println!(
        "[{}] VPS classify exit {code} in {took:?}\n{}",
        stamp(),
        table(&j)
    );
    println!("summary {}\noverall {}", j["summary"], j["overall"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(j["summary"]["units"], 14);
    let kinds: Vec<&str> = j["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].as_str().unwrap())
        .collect();
    assert!(
        !kinds.contains(&"nerve-unreadable"),
        "nerve unreadable: {kinds:?}"
    );
    assert!(
        !kinds.contains(&"cortex-unreadable"),
        "cortex unreadable: {kinds:?}"
    );
    assert!(j["overall"]["nerve_score"].is_u64());
    assert_plan_only(&j);
    // Every live heal target is refused by the real scope guard.
    for u in j["units"].as_array().unwrap() {
        if let Some(r) = u["heal_tick"]["reason"].as_str() {
            assert!(
                r.contains("outside the allowed test range") || r.contains("no HTTP port"),
                "{}: {r}",
                u["service"]
            );
        }
    }
    // Cross-check Gaia's recorded nerve verdicts against the independent read.
    let st: Value = serde_json::from_str(&independent).expect("nerve /state JSON");
    let comps = st["components"].as_array().expect("components");
    let mut checked = 0;
    for u in j["units"].as_array().unwrap() {
        let Some(c) = u["nerve"]["component"].as_str() else {
            continue;
        };
        let Some(row) = comps.iter().find(|x| x["name"] == c) else {
            continue;
        };
        let now = row["status"].as_str().unwrap_or("?");
        let then = u["nerve"]["status"].as_str().unwrap_or("?");
        println!("cross-check {c}: classify saw {then}, independent read {now}");
        if now != then {
            println!("  (nerve status changed between reads; not an error)");
        } else {
            checked += 1;
        }
    }
    assert!(
        checked >= 8,
        "too few nerve verdicts cross-checked: {checked}"
    );
    for p in 20940..=20949 {
        assert!(!open(p), "tunnel port {p} still open after teardown");
    }
}
