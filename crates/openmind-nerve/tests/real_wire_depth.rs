// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for wave-4 slot 327 (nerve-wire-gaps-2): wiring DEPTH.
//!
//! The dependency gate (repo-check `nerve`) was at 0 gaps, but two kinds of
//! signal never reached a real nerve:
//!
//! 1. One-shot CLIs (desk-hermes, persona-plex-relay, galion-backup,
//!    galion-boot, galion-deploy one-shot paths) queued `started` and then
//!    exited before the nerve-client push worker drained the queue. They now
//!    call the bounded `nerve_client::flush` before exit.
//! 2. galion-one (live :8000 + mouth :8002) serves HTTP but never called
//!    `observe_request`, so its 5xx (e.g. mouth `/health` 503) and slow
//!    requests were invisible as pushed signals. It now reports per request.
//!
//! Ports 21970-21979 (override with `REAL_WIRE_DEPTH_PORT_BASE`):
//!
//! | port  | process |
//! |-------|---------|
//! | 21970 | openmind-nerve serve |
//! | 21971 | galion-one (`ONE_BIND`) |
//! | 21972 | galion-one mouth (`ONE_MOUTH_BIND`) |
//!
//! Gated by `GALION_REAL=1`. `NERVE_WIRING_BIN_DIR` skips the build.
//! `WIRE_DEPTH_CONTROL_BIN_DIR` (optional) runs the same one-shots from an
//! older build and prints how many `started` events they lost (not asserted).

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn base() -> u16 {
    std::env::var("REAL_WIRE_DEPTH_PORT_BASE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21970)
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status();
        let _ = self.0.wait();
    }
}

fn http(port: u16, req: &str, pause: Duration) -> (u16, String) {
    let addr = format!("127.0.0.1:{port}").parse().unwrap();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    std::thread::sleep(pause);
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn get(port: u16, path: &str) -> (u16, String) {
    http(
        port,
        &format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"),
        Duration::ZERO,
    )
}

fn wait_up(port: u16, path: &str) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if get(port, path).0 != 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("port {port} path {path} did not come up");
}

fn signals(nerve: u16) -> Value {
    let (code, body) = get(nerve, "/api/v1/signals");
    assert_eq!(code, 200, "signals -> {code} {body}");
    serde_json::from_str(&body).unwrap_or(Value::Null)
}

fn source(sig: &Value, name: &str) -> Value {
    sig["sources"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["source"] == name).cloned())
        .unwrap_or(Value::Null)
}

fn recent_has(sig: &Value, src: &str, kind: &str, name: &str, msg_part: &str) -> bool {
    source(sig, src)["recent"].as_array().is_some_and(|r| {
        r.iter().any(|e| {
            e["kind"] == kind
                && e["name"] == name
                && e["msg"].as_str().is_some_and(|m| m.contains(msg_part))
        })
    })
}

fn started_count(nerve: u16, src: &str) -> usize {
    source(&signals(nerve), src)["recent"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter(|e| e["kind"] == "event" && e["name"] == "started")
                .count()
        })
        .unwrap_or(0)
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
}

fn binaries() -> PathBuf {
    if let Some(d) = std::env::var_os("NERVE_WIRING_BIN_DIR") {
        return PathBuf::from(d);
    }
    let td = target_dir().join("real-nerve-wire-depth");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut c = Command::new(cargo);
    c.current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .env("CARGO_TARGET_DIR", &td)
        .args(["build", "-q", "-j", "2", "--release"]);
    for p in [
        "openmind-nerve",
        "galion-one",
        "desk-hermes",
        "persona-plex-relay",
        "galion-backup",
        "galion-boot",
        "galion-deploy",
    ] {
        c.args(["-p", p]);
    }
    assert!(
        c.status().expect("run cargo build").success(),
        "release build failed"
    );
    td.join("release")
}

/// (source, binary, args, extra env) for one one-shot.
type OneShot = (
    &'static str,
    &'static str,
    Vec<String>,
    Vec<(String, String)>,
);

fn one_shots(work: &Path) -> Vec<OneShot> {
    let w = |p: &str| work.join(p).display().to_string();
    vec![
        (
            "desk-hermes",
            "desk-hermes",
            vec![],
            vec![("HERMES_HOME".into(), w("hermes-home"))],
        ),
        (
            "persona-plex-relay",
            "persona-plex-relay",
            vec![],
            vec![("PERSONA_PLEX_RELAY_CONFIG".into(), w("no-relay.toml"))],
        ),
        (
            "galion-backup",
            "galion-backup",
            vec!["verify".into(), w("no-such.db")],
            vec![],
        ),
        (
            "galion-boot",
            "galion-boot",
            vec!["status".into(), "--config".into(), w("no-boot.toml")],
            vec![],
        ),
        (
            "galion-deploy",
            "galion-deploy",
            vec!["backup-verify".into()],
            vec![],
        ),
    ]
}

/// Run every one-shot once; returns (source, exit code, ms, started delta).
fn run_one_shots(bin: &Path, work: &Path, nerve: u16) -> Vec<(String, i32, u128, usize)> {
    let url = format!("http://127.0.0.1:{nerve}");
    let mut out = Vec::new();
    for (src, exe, args, envs) in one_shots(work) {
        let before = started_count(nerve, src);
        let t0 = Instant::now();
        let st = Command::new(bin.join(exe))
            .args(&args)
            .envs(envs)
            .env("NERVE_URL", &url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap_or_else(|e| panic!("run {exe}: {e}"));
        let ms = t0.elapsed().as_millis();
        // No grace wait: flush() returned only after the nerve answered.
        let after = started_count(nerve, src);
        out.push((
            src.to_string(),
            st.code().unwrap_or(-1),
            ms,
            after.saturating_sub(before),
        ));
    }
    out
}

#[test]
fn one_shots_flush_and_galion_one_observes_requests() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run the real-life wire-depth test");
        return;
    }
    let bin = binaries();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let work = std::env::temp_dir().join(format!("nerve-wire-depth-{}-{now}", std::process::id()));
    std::fs::create_dir_all(work.join("data")).unwrap();
    let (nerve, one, mouth) = (base(), base() + 1, base() + 2);
    std::fs::write(
        work.join("components.json"),
        format!(
            r#"[{{"name":"galion-one","port":{one},"path":"/health","gaia":"AETHER","system":"one"}}]"#
        ),
    )
    .unwrap();
    let log = |n: &str| {
        std::fs::File::create(work.join(n))
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null())
    };
    let _nerve = Proc(
        Command::new(bin.join("openmind-nerve"))
            .arg("serve")
            .env("NERVE_ADDR", format!("127.0.0.1:{nerve}"))
            .env("NERVE_INTERVAL", "5")
            .env("NERVE_DATA_DIR", work.join("data"))
            .env("NERVE_COMPONENTS", work.join("components.json"))
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_REFLEX", "0")
            .env("NERVE_LISTEN_SCOPE", format!("{}-{}", base(), base() + 9))
            .stdout(Stdio::null())
            .stderr(log("nerve.log"))
            .spawn()
            .expect("spawn openmind-nerve"),
    );
    wait_up(nerve, "/healthz");
    println!("== {} nerve up on {nerve}", zagreb_time::now_rfc3339());

    // --- 1. one-shots: started must be in the nerve when the process exits.
    let runs = run_one_shots(&bin, &work, nerve);
    for (src, code, ms, got) in &runs {
        println!("one-shot {src:<20} exit={code:<3} {ms:>4} ms started_delivered={got}");
    }
    if let Some(ctl) = std::env::var_os("WIRE_DEPTH_CONTROL_BIN_DIR") {
        let ctl = run_one_shots(Path::new(&ctl), &work, nerve);
        let lost = ctl.iter().filter(|r| r.3 == 0).count();
        for (src, code, ms, got) in &ctl {
            println!("CONTROL  {src:<20} exit={code:<3} {ms:>4} ms started_delivered={got}");
        }
        println!("CONTROL lost started: {lost}/{}", ctl.len());
    }
    for (src, _, ms, got) in &runs {
        assert_eq!(*got, 1, "{src}: started not delivered before exit");
        assert!(*ms < 5000, "{src}: took {ms} ms");
    }
    // Nerve down: flush is bounded, the CLI still exits fast.
    let dead = base() + 9;
    for (src, exe, args, envs) in one_shots(&work) {
        let t0 = Instant::now();
        let _ = Command::new(bin.join(exe))
            .args(&args)
            .envs(envs)
            .env("NERVE_URL", format!("http://127.0.0.1:{dead}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        let ms = t0.elapsed().as_millis();
        println!("dead-nerve {src:<20} {ms:>4} ms");
        assert!(ms < 2000, "{src} with dead nerve took {ms} ms");
    }

    // --- 2. galion-one observe_request.
    let _one = Proc(
        Command::new(bin.join("galion-one"))
            .env("ONE_BIND", format!("127.0.0.1:{one}"))
            .env("ONE_MOUTH_BIND", format!("127.0.0.1:{mouth}"))
            .env("ONE_NO_WARM", "1")
            .env("NERVE_URL", format!("http://127.0.0.1:{nerve}"))
            .env("NERVE_HEARTBEAT_S", "3600")
            .env("NERVE_SLOW_MS", "300")
            .env_remove("GALION_MOUTH")
            .stdout(Stdio::null())
            .stderr(log("one.log"))
            .spawn()
            .expect("spawn galion-one"),
    );
    wait_up(one, "/health");
    wait_up(mouth, "/health");
    // Let startup pushes (started, first heartbeat, the wait_up 503 on the
    // mouth) land before the baseline.
    std::thread::sleep(Duration::from_millis(700));
    // Fast 200s are not pushed.
    let before = source(&signals(nerve), "galion-one")["total"]
        .as_u64()
        .unwrap_or(0);
    for _ in 0..5 {
        assert_eq!(get(one, "/api/v1/awake").0, 200);
    }
    std::thread::sleep(Duration::from_millis(300));
    let after = source(&signals(nerve), "galion-one")["total"]
        .as_u64()
        .unwrap_or(0);
    println!("galion-one total before={before} after 5 fast 200s={after}");
    assert_eq!(before, after, "fast 2xx must not be pushed");

    // Mouth without a backend answers 503 -> http_5xx.
    let (code, body) = get(mouth, "/health");
    println!("mouth /health -> {code} {body}");
    assert_eq!(code, 503);
    // Slow: hold the connection 450 ms before sending the request line.
    let (code, _) = http(
        one,
        "GET /api/v1/awake HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
        Duration::from_millis(450),
    );
    assert_eq!(code, 200);
    let t0 = Instant::now();
    let mut sig = Value::Null;
    while t0.elapsed() < Duration::from_secs(5) {
        sig = signals(nerve);
        if recent_has(
            &sig,
            "galion-one",
            "error",
            "http_5xx",
            "mouth GET /health -> 503",
        ) && recent_has(
            &sig,
            "galion-one",
            "event",
            "slow_request",
            "GET /api/v1/awake -> 200",
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("galion-one pushed: {}", source(&sig, "galion-one"));
    assert!(
        recent_has(
            &sig,
            "galion-one",
            "error",
            "http_5xx",
            "mouth GET /health -> 503"
        ),
        "missing http_5xx"
    );
    assert!(
        recent_has(
            &sig,
            "galion-one",
            "event",
            "slow_request",
            "GET /api/v1/awake -> 200"
        ),
        "missing slow_request"
    );
    // The nerve turns pushed errors into incident signals: /state should
    // carry them for galion-one within an interval or two.
    let (sc, state) = get(nerve, "/state");
    println!(
        "nerve /state -> {sc}, mentions http_5xx: {}",
        state.contains("http_5xx")
    );
    println!(
        "== {} PASS: wire depth (slot 327)",
        zagreb_time::now_rfc3339()
    );
    let _ = std::fs::remove_dir_all(&work);
}
