// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life T3.6 consumer-compatibility test (wave-4 slot 180, ports
//! 20500-20509), Rust only, gated by `GALION_REAL=1`. Run it release:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p openmind-nerve --test real_consumer_compat -- --nocapture --test-threads 1
//! GALION_REAL=1 GALION_REAL_VPS=1 cargo test ... vps_live   # read-only GETs on the VPS over ssh
//! ```
//!
//! `box_binary`: starts the real `openmind-nerve serve` binary (the test's
//! own CARGO_BIN_EXE, release under `--release`) on 127.0.0.1:20500 with a
//! one-component table (a box-local dummy on 20501), dry-run, no systemd,
//! no journal, an event log in a temp dir; then replays every consumer with
//! its own client code: galion-realtest live rows + ingest scenario,
//! nerve-client, gaia-live post/tail/healthy, galion-deploy's health path,
//! the capture/install routes and the gaia restart-counter contract on the
//! engine's real `/state`. Then SIGKILLs the nerve, leaves a torn line in
//! the event log, restarts it and checks gaia-live still reads its tail.
//! A second nerve without an event log runs on 20502 (B2). Everything is
//! killed and removed at the end.
//!
//! `vps_live`: over `ssh slavik-vps`, a fixed GET-only curl script against
//! the live galion-nerve (127.0.0.1:8670); the realtest rows and the /state
//! contract are judged on the real answers, B1/B2 are reported.

use galion_realtest::live::{self, Got};
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use galion_realtest::ports::Block;

/// Slot 383 (real-ports-batch-3): NERVE +0, DUMMY +1, NERVE_NOLOG +2; `NERVE_CONSUMER_REAL_PORT_BASE`
/// moves the block, unset keeps the old
/// 20500-20509.
static PORTS: Block = Block::new("NERVE_CONSUMER_REAL_PORT_BASE", 20500, 10);

#[allow(non_snake_case)]
fn NERVE() -> u16 {
    PORTS.port(0)
}
#[allow(non_snake_case)]
fn DUMMY() -> u16 {
    PORTS.port(1)
}
#[allow(non_snake_case)]
fn NERVE_NOLOG() -> u16 {
    PORTS.port(2)
}

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn addr(p: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], p))
}

fn got(a: SocketAddr, path: &str) -> Got {
    let Ok(mut s) = TcpStream::connect_timeout(&a, Duration::from_secs(2)) else {
        return Got {
            error: "connect refused".into(),
            ..Got::default()
        };
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {a}\r\nConnection: close\r\n\r\n"
    );
    let mut raw = String::new();
    let _ = s.read_to_string(&mut raw);
    live::parse_curl(&raw)
}

fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    rest.find('/').map(|i| &rest[i..]).unwrap_or("/")
}

/// Box-local dummy service: every request gets 200 {"ok":true}.
fn dummy(port: u16) {
    let l = TcpListener::bind(addr(port)).expect("dummy port slot port free");
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut s) = c else { continue };
            let mut b = [0u8; 2048];
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = s.read(&mut b);
            let _ = s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}",
            );
        }
    });
}

fn start_nerve(port: u16, dir: &Path, log: bool) -> Proc {
    let comps = dir.join("components.json");
    std::fs::write(
        &comps,
        format!(r#"[{{"name":"t36-dummy","port":{},"path":"/health","gaia":"DEMETER","unit":"t36-dummy.service","restarts_per_hour":1}}]"#, DUMMY()),
    )
    .unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_openmind-nerve"));
    c.arg("serve")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", "Europe/Zagreb")
        .env("NERVE_ADDR", format!("127.0.0.1:{port}"))
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_INTERVAL", "5")
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_GAIA_BIN", "/bin/false")
        .env("NERVE_DEPLOY_BIN", "/bin/false")
        .env("NERVE_DEPLOY_BACKUPS", dir.join("backups"))
        .env("NERVE_SWITCH_DIR", dir.join("switch"))
        .env("NERVE_TELEGRAM_ENV", dir.join("absent.env"))
        .env("NERVE_VOICE_UNIT", "t36-none.service")
        .env("NERVE_DISK_PATH", dir)
        .env("NERVE_COMPONENTS", &comps)
        .stdout(Stdio::null())
        .stderr(Stdio::from(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join(format!("nerve-{port}.log")))
                .unwrap(),
        ));
    if log {
        c.env("NERVE_EVENT_LOG", dir.join("events.jsonl"));
    }
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let p = Proc(c.spawn().expect("start openmind-nerve"));
    let t = Instant::now();
    while got(addr(port), "/healthz").status != Some(200) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "nerve {port} did not come up"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    p
}

fn wait_cycle(port: u16, min: u64) -> Value {
    let t = Instant::now();
    loop {
        let g = got(addr(port), "/state");
        if let Ok(v) = serde_json::from_str::<Value>(&g.body) {
            if v["cycle"].as_u64().unwrap_or(0) >= min {
                return v;
            }
        }
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "no cycle {min} on {port}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// gaia-loop-decisions b40208a contract: components[].restarts_1h and
/// reflex.budgets[unit].live_1h are counts. Returns (components, budgets).
fn gaia_contract(st: &Value) -> Result<(usize, usize), String> {
    let comps = st["components"].as_array().ok_or("no components list")?;
    for c in comps {
        if !c["restarts_1h"].is_u64() {
            return Err(format!("restarts_1h not a count: {c}"));
        }
    }
    let b = st["reflex"]["budgets"]
        .as_object()
        .ok_or("no reflex.budgets")?;
    for (u, v) in b {
        if !v["live_1h"].is_u64() {
            return Err(format!("{u}.live_1h not a count"));
        }
    }
    Ok((comps.len(), b.len()))
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, u128) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed().as_micros())
}

#[test]
fn box_binary() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let dir: PathBuf = std::env::temp_dir().join(format!("nerve-t36-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    println!(
        "== T3.6 box real test {} binary {}",
        stamp(),
        env!("CARGO_BIN_EXE_openmind-nerve")
    );
    dummy(DUMMY());
    let n = start_nerve(NERVE(), &dir, true);
    let a = addr(NERVE());
    let rows: Vec<_> = live::VPS
        .iter()
        .filter(|p| p.service == "galion-nerve")
        .collect();

    // Phase 0: as early as possible (the first cycle may already be done).
    let st0 = got(a, "/state");
    println!("phase0 /state body before/at first cycle: {:.40}", st0.body);
    for p in &rows {
        let r = live::judge(p, &got(a, path_of(p.url)));
        println!("phase0 realtest {} -> {r:?}", p.name);
        assert!(r.is_ok());
    }

    // Phase 1: after the first real engine cycle.
    let st = wait_cycle(NERVE(), 1);
    println!(
        "phase1 cycle {} mode {} system {}",
        st["cycle"], st["mode"], st["system"]["status"]
    );
    for p in &rows {
        let (r, us) = timed(|| live::judge(p, &got(a, path_of(p.url))));
        println!("realtest live row {} -> {r:?} ({us} us)", p.name);
        assert!(r.is_ok());
    }
    // galion-realtest scenarios::nerve contract with its own client.
    {
        use galion_realtest::http;
        let s = a.to_string();
        let r = http::post_json(
            &s,
            "/signal",
            &[],
            "{\"source\":\"realtest\",\"kind\":\"event\",\"name\":\"realtest-probe\",\"value\":1}",
        )
        .unwrap();
        assert_eq!(r.status, 202);
        assert!(http::get(&s, "/signals")
            .unwrap()
            .text()
            .contains("realtest"));
        assert_eq!(
            http::post_json(&s, "/signal", &[], "{\"kind\":\"nope\"}")
                .unwrap()
                .status,
            400
        );
        println!("realtest ingest scenario: 202 / visible / 400 ok");
    }
    // nerve-client (hermes-rs, openmind-memory) against the real binary.
    {
        let c = nerve_client::Client::start(a, 256).unwrap();
        for i in 0..50 {
            c.push(nerve_client::body(
                "hermes-rs",
                "event",
                "slow_request",
                Some(1000.0 + i as f64),
                "GET / -> 200",
            ));
            c.push(nerve_client::body(
                "openmind-memory",
                "heartbeat",
                "rss_kb",
                Some(4096.0),
                "queued=0 sent=0 dropped=0 failed=0 rejected=0",
            ));
        }
        let t = Instant::now();
        while c.counters.snapshot().sent + c.counters.snapshot().rejected < 100
            && t.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = c.counters.snapshot();
        println!(
            "nerve-client 100 signals in {} ms: {s:?}",
            t.elapsed().as_millis()
        );
        assert_eq!((s.sent, s.rejected, s.failed), (100, 0, 0));
        // Over the per-source limit (120/min): 80 more from hermes-rs.
        for i in 0..80 {
            c.push(nerve_client::body(
                "hermes-rs",
                "error",
                "http_5xx",
                None,
                &format!("burst {i}"),
            ));
        }
        let t = Instant::now();
        while c.counters.snapshot().sent + c.counters.snapshot().rejected < 180
            && t.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let s = c.counters.snapshot();
        let sig: Value = serde_json::from_str(&got(a, "/signals").body).unwrap();
        let nerve_dropped: u64 = sig["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["source"] == "hermes-rs")
            .map(|x| x["dropped"].as_u64().unwrap_or(0))
            .sum();
        println!("nerve-client burst: client {s:?}; nerve hermes-rs dropped {nerve_dropped}");
        assert_eq!(
            s.rejected, nerve_dropped,
            "client and nerve agree on rejected signals"
        );
        assert!(s.rejected >= 10);
    }
    // gaia-live against the real binary.
    {
        use gaia_live::nerve;
        let (h, us) = timed(|| nerve::healthy(a));
        println!("gaia-live healthy() = {h} ({us} us)");
        assert!(h);
        let (r, us) = timed(|| {
            nerve::post(
                a,
                "gaia-live",
                "heal.restart",
                "info",
                serde_json::json!({"service": "t36-dummy", "port": DUMMY()}),
            )
        });
        println!("gaia-live post() = {r:?} ({us} us)");
        r.unwrap();
        let t = nerve::tail(a, 10).unwrap();
        println!(
            "gaia-live tail(10) = {} record(s), last ts {}",
            t.len(),
            t.last().unwrap()["ts"]
        );
        assert_eq!(t.last().unwrap()["kind"], "heal.restart");
    }
    // galion-deploy health path, install.sh / capture routes.
    {
        let url = galion_deploy::deploy::health_table("galion-nerve").unwrap();
        assert_eq!(got(a, path_of(url)).status, Some(200));
        let mut line = String::new();
        for p in [
            "/healthz",
            "/",
            "/health",
            "/state",
            "/self-eval",
            "/self-eval.md",
            "/signals",
            "/events",
            "/incidents",
            "/cortex",
            "/ddos",
            "/alerts",
            "/audit",
            "/api/v1/state",
        ] {
            let g = got(a, p);
            line.push_str(&format!(
                "{p} {} {}B; ",
                g.status.unwrap_or(0),
                g.body.len()
            ));
            assert_eq!(g.status, Some(200), "{p}");
        }
        println!("routes: {line}");
    }
    // gaia restart-counter contract on the engine's real /state.
    let st = wait_cycle(NERVE(), 2);
    let c = gaia_contract(&st);
    println!(
        "gaia /state contract on real engine state (cycle {}): {c:?}",
        st["cycle"]
    );
    assert!(c.is_ok());
    let dummy_row = st["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == "t36-dummy")
        .cloned()
        .unwrap();
    println!(
        "t36-dummy component: status {} http {} restarts_1h {}",
        dummy_row["status"], dummy_row["http"], dummy_row["restarts_1h"]
    );
    assert_eq!(dummy_row["http"], 200);

    // Crash with a torn line, restart, gaia-live still reads the tail.
    drop(n);
    let log = dir.join("events.jsonl");
    let before = nerve_lines(&log);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"{\"ts\":\"2026-10-21T09:0")
        .unwrap();
    let n2 = start_nerve(NERVE(), &dir, true);
    gaia_live::nerve::post(a, "gaia-live", "after-crash", "info", serde_json::json!({})).unwrap();
    let t = gaia_live::nerve::tail(a, 1000).unwrap();
    println!(
        "after SIGKILL + torn line + restart: {} readable records (before {before}), last kind {}",
        t.len(),
        t.last().unwrap()["kind"]
    );
    assert_eq!(t.len(), before + 1);
    assert_eq!(t.last().unwrap()["kind"], "after-crash");
    drop(n2);

    // B2: no event log -> gaia-live POST /event 404.
    let n3 = start_nerve(NERVE_NOLOG(), &dir, false);
    let e = gaia_live::nerve::post(
        addr(NERVE_NOLOG()),
        "gaia-live",
        "k",
        "info",
        serde_json::json!({}),
    )
    .unwrap_err();
    println!("B2 nerve without event log: {e}");
    assert!(e.contains("404"));
    drop(n3);
    let log_tail =
        std::fs::read_to_string(dir.join(format!("nerve-{}.log", NERVE()))).unwrap_or_default();
    for l in log_tail
        .lines()
        .filter(|l| l.contains("listening") || l.contains("skipped"))
    {
        println!("nerve log: {l}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!("== done {}", stamp());
}

fn nerve_lines(p: &Path) -> usize {
    openmind_nerve::read_lossy(p)
        .map(|x| x.0.len())
        .unwrap_or(0)
}

/// Fixed GET-only script run on the VPS (no POST, no writes).
const VPS_SCRIPT: &str = "for p in /healthz / /health /events /state; do echo \"@@ $p\"; curl -s -m 5 -D - http://127.0.0.1:8670$p; echo; done; echo '@@ ss'; ss -ltn | grep -c ':8670 '; echo '@@ active'; systemctl is-active galion-nerve";

fn blocks(out: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vec![];
    for l in out.split_inclusive('\n') {
        if let Some(k) = l.strip_prefix("@@ ") {
            v.push((k.trim().to_string(), String::new()));
        } else if let Some(last) = v.last_mut() {
            last.1.push_str(l);
        }
    }
    v
}

#[test]
fn vps_live() {
    if !real() || std::env::var("GALION_REAL_VPS").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 GALION_REAL_VPS=1");
        return;
    }
    let mut cycles = vec![];
    for round in 0..2 {
        let o = Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "slavik-vps",
                VPS_SCRIPT,
            ])
            .output()
            .expect("ssh");
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        let b = blocks(&out);
        let get = |k: &str| {
            live::parse_curl(
                &b.iter()
                    .find(|x| x.0 == k)
                    .map(|x| x.1.clone())
                    .unwrap_or_default(),
            )
        };
        println!(
            "== VPS round {round} {} (read-only GETs on 127.0.0.1:8670)",
            stamp()
        );
        for p in live::VPS.iter().filter(|p| p.service == "galion-nerve") {
            let r = live::judge(p, &get(path_of(p.url)));
            println!("live realtest row {} -> {r:?}", p.name);
            assert!(
                r.is_ok(),
                "live galion-nerve fails galion-realtest row {}",
                p.name
            );
        }
        let h = get("/health");
        let gaia_healthy = h.status == Some(200) && h.body.contains("\"status\":\"ok\"");
        println!(
            "B1 live GET /health -> {:?}; gaia-live healthy() would be {gaia_healthy}",
            h.status
        );
        let e = get("/events");
        println!(
            "B2 live GET /events -> {:?} (gaia-live tail {})",
            e.status,
            if e.status == Some(200) {
                "works"
            } else {
                "fails"
            }
        );
        let st: Value = serde_json::from_str(&get("/state").body).expect("live /state JSON");
        let c = gaia_contract(&st);
        println!(
            "live /state cycle {} mode {} system {}; gaia contract {c:?}",
            st["cycle"], st["mode"], st["system"]["status"]
        );
        assert!(c.is_ok());
        cycles.push(st["cycle"].as_u64().unwrap_or(0));
        for k in ["ss", "active"] {
            println!(
                "{k}: {}",
                b.iter()
                    .find(|x| x.0 == k)
                    .map(|x| x.1.trim().to_string())
                    .unwrap_or_default()
            );
        }
        if round == 0 {
            std::thread::sleep(Duration::from_secs(35));
        }
    }
    println!("live nerve cycles observed: {cycles:?}");
    assert!(cycles[1] > cycles[0], "live nerve loop advances");
}
