// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL test (GALION_REAL=1), box only, of Mate's accepted decisions for
//! slot 160 (2026-10-21), wave-4 slot 175:
//!
//! * D-L2: a fake nerve (in this test, real HTTP on 127.0.0.1:20450) reports
//!   its own restarts in `GET /state`; the real `gaia heal-tick --loop`
//!   (release) on a flapping `gaia-dummy` restarts only `3 - nerve` times,
//!   and the cap lifts as soon as the nerve's count drops. Nerve 3 caps a
//!   one-shot tick before its token is used. A broken `/state` fails closed.
//! * D-L3: `gaia tokens request` / `mint` (admin side, throwaway key) /
//!   `status` through the real binary; batch ceiling 3, lifetime <= 1 h,
//!   expired tokens moved aside by the loop and never consumed.
//! * D-L4: a hung `gaia-dummy --mode hang` is reported as `gaia.heal.hung`
//!   every tick with its real pid; never restarted, never killed.
//! * The real `openmind-nerve serve` (GAIA_NERVE_BIN, 127.0.0.1:20451): its
//!   `/state` parses and the heal path works against it.
//!
//! Ports: all in slot 175's band 20450-20459 (nerves 20450-20451, dummies
//! 20452-20456; slot 199 moved them from 20750/19750, D-FI4). The heal
//! apply path refuses live VPS ports (`gaia_live::heal::LIVE_PORTS`). D-L1:
//! nothing is installed anywhere.
//!
//! Run: `GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve cargo test
//! --release -p gaia --test real_gaia_decisions -- --nocapture`.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FAKE: &str = "127.0.0.1:20450";
const REAL: &str = "127.0.0.1:20451";
const ALLOW: &str = "20452-20456";
const FLAP: u16 = 20452;
const NCAP: u16 = 20453;
const HANG: u16 = 20454;
const EXPIRY: u16 = 20455;
const REALSVC: u16 = 20456;
const PORTS: [u16; 7] = [20450, 20451, FLAP, NCAP, HANG, EXPIRY, REALSVC];

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

#[derive(Clone, Default)]
struct Fake {
    log: Arc<Mutex<Vec<Value>>>,
    state: Arc<Mutex<(u16, String)>>,
    state_gets: Arc<Mutex<u64>>,
}

impl Fake {
    fn set(&self, code: u16, v: Value) {
        *self.state.lock().unwrap() = (code, v.to_string());
    }
    /// Nerve-reported restarts in the last hour per service.
    fn restarts(&self, per: &[(&str, u64)]) {
        let comps: Vec<Value> = per
            .iter()
            .map(|(s, n)| json!({"name": s, "unit": format!("{s}.service"), "status": "degrading", "restarts_1h": n, "reflex_restarts_per_hour": 3}))
            .collect();
        let budgets: serde_json::Map<String, Value> = per
            .iter()
            .map(|(s, n)| {
                (
                    format!("{s}.service"),
                    json!({"dry_1h": 0, "live_1h": n, "last_rollback": null}),
                )
            })
            .collect();
        self.set(
            200,
            json!({"cycle": 42, "interval_s": 30, "components": comps,
                   "reflex": {"budgets": budgets, "limits": {"restarts_per_hour_default": 3}}}),
        );
    }
    fn events(&self, svc: &str, kind: &str) -> Vec<Value> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == kind && e["data"]["service"] == svc)
            .cloned()
            .collect()
    }
}

fn start_fake() -> Fake {
    let l = TcpListener::bind(FAKE).expect("bind fake nerve 20450");
    let f = Fake::default();
    f.set(200, json!({"cycle": 1, "components": []}));
    let f2 = f.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let Ok(c) = s.try_clone() else { continue };
            let mut r = BufReader::new(c);
            let mut first = String::new();
            if r.read_line(&mut first).is_err() {
                continue;
            }
            let mut len = 0usize;
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
                if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            let _ = r.read_exact(&mut body);
            let (code, resp) = if first.starts_with("GET /health") {
                (200, r#"{"status":"ok"}"#.to_string())
            } else if first.starts_with("GET /state") {
                *f2.state_gets.lock().unwrap() += 1;
                f2.state.lock().unwrap().clone()
            } else if first.starts_with("POST /event") {
                let mut v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                v["ts"] = Value::from(stamp());
                f2.log.lock().unwrap().push(v);
                (200, r#"{"ok":true}"#.to_string())
            } else {
                (
                    200,
                    Value::Array(f2.log.lock().unwrap().clone()).to_string(),
                )
            };
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}",
                resp.len()
            );
        }
    });
    f
}

fn open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

fn wait_port(port: u16) {
    for _ in 0..50 {
        if open(port) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("port {port} never opened");
}

fn http_get(addr: &str, path: &str) -> String {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut b = String::new();
    s.read_to_string(&mut b).unwrap();
    b.split_once("\r\n\r\n").unwrap().1.to_string()
}

/// gaia-dummy processes on our dummy ports (by /proc cmdline), not zombies.
fn our_dummies() -> Vec<u32> {
    let mut v = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(cmd) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let args: Vec<String> = cmd
            .split(|b| *b == 0)
            .map(|s| String::from_utf8_lossy(s).to_string())
            .collect();
        let dummy = args.first().is_some_and(|a| a.ends_with("/gaia-dummy"));
        let ours = args
            .iter()
            .any(|a| PORTS[2..].iter().any(|p| a == &p.to_string()));
        let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.rsplit_once(") ").is_some_and(|x| x.1.starts_with('Z')))
            .unwrap_or(false);
        if dummy && ours && !zombie {
            v.push(pid);
        }
    }
    v
}

struct Guard {
    kids: Vec<Child>,
    dir: PathBuf,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::write(self.dir.join("STOP"), b"");
        for c in &mut self.kids {
            let _ = c.kill();
            let _ = c.wait();
        }
        // Test teardown only (Gaia itself never signals anything).
        for pid in our_dummies() {
            let _ = Command::new("kill")
                .arg(pid.to_string())
                .stderr(Stdio::null())
                .status();
        }
    }
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

fn gaia(args: &[&str]) -> Run {
    let o = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).to_string(),
        err: String::from_utf8_lossy(&o.stderr).to_string(),
    }
}

fn result(r: &Run) -> Value {
    let last = r.out.lines().last().unwrap_or("{}");
    serde_json::from_str::<Value>(last).unwrap()["result"].clone()
}

struct Ctx {
    dir: PathBuf,
    config: String,
    seed: String,
    dummy: String,
}

impl Ctx {
    fn argv(&self, port: u16, mode: &[&str]) -> Vec<String> {
        let mut v = vec![self.dummy.clone(), "--port".into(), port.to_string()];
        v.extend(mode.iter().map(|s| s.to_string()));
        v
    }
    fn toks(&self, svc: &str) -> String {
        self.dir.join(format!("tok-{svc}")).display().to_string()
    }
    fn target_args(&self, svc: &str, port: u16, argv: &[String]) -> Vec<String> {
        let mut a: Vec<String> = [
            "--service",
            svc,
            "--port",
            &port.to_string(),
            "--allow-ports",
            ALLOW,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        a.push("--".into());
        a.extend(argv.iter().cloned());
        a
    }
    /// request + admin-side mint through the real CLI; returns the mint result.
    fn batch(&self, svc: &str, port: u16, argv: &[String], count: u64, ttl: u64) -> Value {
        let toks = self.toks(svc);
        let mut a = vec![
            "tokens".to_string(),
            "request".into(),
            "--compact".into(),
            "--token-dir".into(),
            toks,
            "--count".into(),
            count.to_string(),
            "--ttl-s".into(),
            ttl.to_string(),
        ];
        a.extend(self.target_args(svc, port, argv));
        let r = gaia(&a.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r.code, 0, "request: {} {}", r.out, r.err);
        let req = result(&r);
        let m = gaia(&[
            "tokens",
            "mint",
            "--compact",
            "--request",
            req["request"].as_str().unwrap(),
            "--seed",
            &self.seed,
        ]);
        assert_eq!(m.code, 0, "mint: {} {}", m.out, m.err);
        assert!(!m.out.contains("hades1."), "mint printed a token");
        result(&m)
    }
    fn status(&self, svc: &str) -> Value {
        let r = gaia(&[
            "tokens",
            "status",
            "--compact",
            "--token-dir",
            &self.toks(svc),
            "--hades-config",
            &self.config,
        ]);
        assert_eq!(r.code, 0, "{} {}", r.out, r.err);
        assert!(!r.out.contains("hades1."), "status printed a token");
        result(&r)
    }
    fn heal_args(&self, nerve: &str, state: &str) -> Vec<String> {
        vec![
            "heal-tick".into(),
            "--compact".into(),
            "--nerve".into(),
            nerve.into(),
            "--state".into(),
            self.dir.join(state).display().to_string(),
            "--hades-config".into(),
            self.config.clone(),
            "--wait-ms".into(),
            "2500".into(),
            "--health".into(),
            "/health".into(),
        ]
    }
    fn consumed(&self) -> usize {
        std::fs::read_to_string(self.dir.join("gate/ledger.jsonl"))
            .unwrap_or_default()
            .matches("\"event\":\"consumed\"")
            .count()
    }
    fn files(&self, svc: &str, suffix: &str) -> usize {
        std::fs::read_dir(self.toks(svc))
            .map(|r| {
                r.flatten()
                    .filter(|e| e.path().to_string_lossy().ends_with(suffix))
                    .count()
            })
            .unwrap_or(0)
    }
}

/// Run a heal command (one-shot or loop) built from parts.
fn heal(
    c: &Ctx,
    nerve: &str,
    svc: &str,
    port: u16,
    argv: &[String],
    state: &str,
    extra: &[&str],
) -> Run {
    let mut a = c.heal_args(nerve, state);
    a.extend(extra.iter().map(|s| s.to_string()));
    a.extend(c.target_args(svc, port, argv));
    gaia(&a.iter().map(String::as_str).collect::<Vec<_>>())
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    t0.elapsed()
}

#[allow(clippy::too_many_lines)]
#[test]
fn real_gaia_loop_decisions_dl2_dl3_dl4() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 and GAIA_NERVE_BIN to run the slot 175 decisions test");
        return;
    }
    let nerve_bin = std::env::var("GAIA_NERVE_BIN").expect("GAIA_NERVE_BIN");
    for p in PORTS {
        assert!(!open(p), "port {p} is busy before the test");
    }
    assert!(our_dummies().is_empty(), "leftover dummies before the test");
    let dir = std::env::temp_dir().join(format!("gaia-decisions-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("gate")).unwrap();
    let (seed, public) = (dir.join("admin.seed"), dir.join("gate/admin.pub"));
    hades_gate::keys::generate(&seed, &public).unwrap();
    let config = dir.join("gate/hades.toml");
    std::fs::write(
        &config,
        format!(
            "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
            public.display(),
            dir.join("gate/ledger.jsonl").display(),
            dir.join("gate/DISABLED").display()
        ),
    )
    .unwrap();
    let c = Ctx {
        dir: dir.clone(),
        config: config.display().to_string(),
        seed: seed.display().to_string(),
        dummy: env!("CARGO_BIN_EXE_gaia-dummy").to_string(),
    };
    let fake = start_fake();
    wait_port(20450);
    eprintln!("[{}] fake nerve on {FAKE}", stamp());
    let real = Command::new(&nerve_bin)
        .args(["serve", "--listen", REAL, "--log"])
        .arg(dir.join("real-nerve.jsonl"))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut g = Guard {
        kids: vec![real],
        dir: dir.clone(),
    };
    wait_port(20451);

    // ---------------- D-L3: batch limits through the real CLI.
    let flap_argv = c.argv(FLAP, &["--mode", "flap", "--up-ms", "1200"]);
    let m = c.batch("dummy-flap", FLAP, &flap_argv, 3, 900);
    assert_eq!(m["minted"], 3);
    let s = c.status("dummy-flap");
    assert_eq!(s["ready"], 3);
    for row in s["tokens"].as_array().unwrap() {
        assert_eq!(row["signature_ok"], true);
        assert_eq!(row["consumed_in_ledger"], false);
    }
    let mut over = vec![
        "tokens".to_string(),
        "request".into(),
        "--token-dir".into(),
        c.toks("dummy-flap"),
        "--count".into(),
        "1".into(),
    ];
    over.extend(c.target_args("dummy-flap", FLAP, &flap_argv));
    let r = gaia(&over.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(r.code, 2, "a 4th live token must be refused: {}", r.out);
    assert!(r.err.contains("no stockpiling"), "{}", r.err);
    for (k, v) in [("--count", "4"), ("--ttl-s", "3601")] {
        let mut a = vec![
            "tokens".to_string(),
            "request".into(),
            "--token-dir".into(),
            c.toks("other"),
            k.into(),
            v.into(),
        ];
        a.extend(c.target_args("dummy-flap", FLAP, &flap_argv));
        let r = gaia(&a.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(r.code, 2, "{k} {v}: {}", r.out);
    }
    eprintln!("[{}] D-L3: batch of 3 minted (ttl 900 s), 4th refused (no stockpiling), --count 4 / --ttl-s 3601 refused", stamp());

    // ---------------- D-L2: flapping service, nerve already restarted it twice.
    fake.restarts(&[("dummy-flap", 2), ("dummy-ncap", 3)]);
    let flap = Command::new(&flap_argv[0])
        .args(&flap_argv[1..])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    g.kids.push(flap);
    wait_port(FLAP);
    let loop_child = {
        let mut a = c.heal_args(FAKE, "state-flap");
        a.insert(1, "--loop".into());
        for x in [
            "--interval-ms",
            "300",
            "--jitter-ms",
            "100",
            "--max-runtime-s",
            "90",
            "--token-dir",
        ] {
            a.push(x.into());
        }
        a.push(c.toks("dummy-flap"));
        a.push("--stop-file".into());
        a.push(dir.join("STOP").display().to_string());
        a.extend(c.target_args("dummy-flap", FLAP, &flap_argv));
        Command::new(env!("CARGO_BIN_EXE_gaia"))
            .args(&a)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let t1 = wait_for("1 restart then combined cap", 40, || {
        fake.events("dummy-flap", "gaia.heal.applied").len() == 1
            && fake.events("dummy-flap", "gaia.heal.capped").len() >= 2
    });
    std::thread::sleep(Duration::from_secs(2));
    let applied = fake.events("dummy-flap", "gaia.heal.applied");
    assert_eq!(
        applied.len(),
        1,
        "nerve 2 leaves exactly 1 restart for Gaia"
    );
    assert_eq!(applied[0]["data"]["nerve_restarts_last_hour"], 2);
    assert_eq!(applied[0]["data"]["restarts_last_hour"], 3);
    let capped = fake.events("dummy-flap", "gaia.heal.capped");
    let cl = capped.last().unwrap();
    assert_eq!(
        (
            cl["data"]["gaia_restarts_last_hour"].as_u64(),
            cl["data"]["nerve_restarts_last_hour"].as_u64()
        ),
        (Some(1), Some(2))
    );
    assert_eq!(cl["data"]["token_consumed"], false);
    assert_eq!(c.consumed(), 1);
    eprintln!(
        "[{}] D-L2: nerve 2 -> 1 Gaia restart, then capped ({} capped ticks, combined 1+2=3) after {t1:?}; consumed tokens 1",
        stamp(),
        capped.len()
    );
    // The nerve's count drops to 0: Gaia may use its own 3 now (2 more).
    fake.restarts(&[("dummy-flap", 0), ("dummy-ncap", 3)]);
    let t2 = wait_for("2 more restarts then Gaia-only cap", 40, || {
        fake.events("dummy-flap", "gaia.heal.applied").len() == 3
            && fake
                .events("dummy-flap", "gaia.heal.capped")
                .last()
                .is_some_and(|e| e["data"]["gaia_restarts_last_hour"] == 3)
    });
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(fake.events("dummy-flap", "gaia.heal.applied").len(), 3);
    std::fs::write(dir.join("STOP"), b"").unwrap();
    let out = loop_child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sum: Value =
        serde_json::from_str::<Value>(text.lines().last().unwrap()).unwrap()["result"].clone();
    assert_eq!(sum["restarts"], 3);
    assert_eq!(sum["tokens_spent"], 3);
    assert_eq!(sum["errors_total"], 0);
    assert!(!text.contains("hades1."));
    let _ = std::fs::remove_file(dir.join("STOP"));
    eprintln!(
        "[{}] D-L2: nerve -> 0, cap lifted after {t2:?}: Gaia restarts 3 total, then capped 3+0; loop summary ticks {} outcomes {} tokens_spent {}",
        stamp(), sum["ticks"], sum["outcomes"], sum["tokens_spent"]
    );

    // ---------------- D-L2: nerve 3 caps a one-shot tick before its token.
    let ncap_argv = c.argv(NCAP, &[]);
    c.batch("dummy-ncap", NCAP, &ncap_argv, 1, 300);
    let before = c.consumed();
    let tok = std::fs::read_dir(c.toks("dummy-ncap"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "tok"))
        .unwrap();
    let r = heal(
        &c,
        FAKE,
        "dummy-ncap",
        NCAP,
        &ncap_argv,
        "state-ncap",
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 5, "{} {}", r.out, r.err);
    let res = result(&r);
    assert!(
        res["detail"].as_str().unwrap().contains("0 Gaia + 3 nerve"),
        "{res}"
    );
    assert_eq!(
        c.consumed(),
        before,
        "nerve-capped tick must not consume the token"
    );
    assert!(!open(NCAP), "nothing was started");
    eprintln!(
        "[{}] D-L2: nerve 3 -> one-shot exit 5: {}",
        stamp(),
        res["detail"]
    );

    // ---------------- D-L2: broken /state fails closed.
    fake.set(500, json!({"error": "nerve state unavailable"}));
    let r = heal(
        &c,
        FAKE,
        "dummy-ncap",
        NCAP,
        &ncap_argv,
        "state-ncap",
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 2, "{} {}", r.out, r.err);
    assert!(
        r.err.contains("D-L2") && r.err.contains("HTTP 500"),
        "{}",
        r.err
    );
    assert_eq!(c.consumed(), before);
    eprintln!(
        "[{}] D-L2: /state HTTP 500 -> exit 2 fail closed, token kept: {}",
        stamp(),
        r.err.lines().next().unwrap_or("")
    );
    fake.restarts(&[]);

    // ---------------- D-L4: hung service.
    let hang_argv = c.argv(HANG, &["--mode", "hang"]);
    let hang = Command::new(&hang_argv[0])
        .args(&hang_argv[1..])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let hang_pid = hang.id();
    g.kids.push(hang);
    wait_port(HANG);
    c.batch("dummy-hang", HANG, &hang_argv, 3, 900);
    let before = c.consumed();
    let t0 = Instant::now();
    let mut a = c.heal_args(FAKE, "state-hang");
    a.insert(1, "--loop".into());
    for x in ["--interval-ms", "300", "--max-ticks", "4", "--token-dir"] {
        a.push(x.into());
    }
    a.push(c.toks("dummy-hang"));
    a.extend(c.target_args("dummy-hang", HANG, &hang_argv));
    let r = gaia(&a.iter().map(String::as_str).collect::<Vec<_>>());
    let took = t0.elapsed();
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let sum = result(&r);
    assert_eq!(sum["outcomes"]["hung"], 4, "{sum}");
    assert_eq!(sum["restarts"], 0);
    assert_eq!(sum["tokens_spent"], 0);
    assert_eq!(c.consumed(), before);
    assert_eq!(c.files("dummy-hang", ".tok"), 3);
    let hung = fake.events("dummy-hang", "gaia.heal.hung");
    assert_eq!(hung.len(), 4);
    for e in &hung {
        assert_eq!(e["data"]["listener_pids"], json!([hang_pid]), "{e}");
        assert_eq!(e["data"]["kill"], false);
        assert_eq!(e["data"]["restart"], false);
        assert_eq!(e["data"]["probes"].as_array().unwrap().len(), 2);
    }
    assert!(
        Path::new(&format!("/proc/{hang_pid}")).exists(),
        "hung dummy must still run"
    );
    assert!(open(HANG));
    eprintln!(
        "[{}] D-L4: 4 ticks in {took:?} -> 4 x gaia.heal.hung (listener pid {hang_pid}, probes {:?}), 0 restarts, 0 tokens, dummy alive",
        stamp(),
        hung[0]["data"]["probes"]
    );

    // ---------------- D-L3: expiry.
    let exp_argv = c.argv(EXPIRY, &[]);
    c.batch("dummy-exp", EXPIRY, &exp_argv, 3, 2);
    std::thread::sleep(Duration::from_millis(3100));
    let s = c.status("dummy-exp");
    assert_eq!(s["ready"], 0);
    assert_eq!(s["expired_unpruned"], 3);
    let before = c.consumed();
    let mut a = c.heal_args(FAKE, "state-exp");
    a.insert(1, "--loop".into());
    for x in ["--interval-ms", "200", "--max-ticks", "2", "--token-dir"] {
        a.push(x.into());
    }
    a.push(c.toks("dummy-exp"));
    a.extend(c.target_args("dummy-exp", EXPIRY, &exp_argv));
    let r = gaia(&a.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let sum = result(&r);
    assert_eq!(sum["tokens_expired"], 3, "{sum}");
    assert_eq!(sum["restarts"], 0);
    assert_eq!(
        sum["outcomes"]["refused"], 2,
        "no token left: dry-run refusal"
    );
    assert_eq!(c.consumed(), before);
    assert_eq!(c.files("dummy-exp", ".tok.expired"), 3);
    assert!(!open(EXPIRY));
    // Expired ones do not block a new batch.
    c.batch("dummy-exp", EXPIRY, &exp_argv, 3, 900);
    eprintln!("[{}] D-L3: ttl 2 s batch expired; loop moved 3 aside (tokens_expired 3), 0 consumed, outcomes {}; new batch of 3 accepted", stamp(), sum["outcomes"]);

    // ---------------- Real openmind-nerve: its /state parses, heal works.
    let t_state = wait_for("real nerve /state components", 30, || {
        serde_json::from_str::<Value>(&http_get(REAL, "/state"))
            .ok()
            .and_then(|s| s.get("components").map(Value::is_array))
            == Some(true)
    });
    let st: Value = serde_json::from_str(&http_get(REAL, "/state")).unwrap();
    let names: Vec<String> = st["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| format!("{}={}", c["name"].as_str().unwrap_or("?"), c["restarts_1h"]))
        .collect();
    eprintln!(
        "[{}] real openmind-nerve /state after {t_state:?}: cycle {} components {names:?}",
        stamp(),
        st["cycle"]
    );
    for svc in ["hermes-rs", "openmind-memory", "dummy-real"] {
        let r = gaia_live::nerve::restarts_for(&st, svc).unwrap();
        eprintln!("  nerve restarts for {svc}: {:?}", r.to_value());
    }
    let real_argv = c.argv(REALSVC, &[]);
    c.batch("dummy-real", REALSVC, &real_argv, 1, 300);
    let tok = std::fs::read_dir(c.toks("dummy-real"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "tok"))
        .unwrap();
    let r = heal(
        &c,
        REAL,
        "dummy-real",
        REALSVC,
        &real_argv,
        "state-real",
        &["--token-file", tok.to_str().unwrap()],
    );
    assert_eq!(r.code, 0, "{} {}", r.out, r.err);
    let res = result(&r);
    assert_eq!(res["outcome"], "restarted", "{res}");
    let cap_step = res["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == "cap")
        .unwrap()
        .clone();
    assert_eq!(cap_step["detail"]["nerve_restarts_last_hour"], 0);
    eprintln!(
        "[{}] real nerve: dummy-real restarted (exit 0), cap step nerve {}",
        stamp(),
        cap_step["detail"]["nerve"]
    );
    let rl = std::fs::read_to_string(dir.join("real-nerve.jsonl")).unwrap();
    assert!(rl.contains("gaia.heal.applied"));

    // ---------------- Evidence: no token text anywhere, Zagreb stamps.
    let all = serde_json::to_string(&*fake.log.lock().unwrap()).unwrap() + &rl;
    for d in std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("tok-"))
    {
        for f in std::fs::read_dir(d.path()).unwrap().flatten() {
            if f.path().to_string_lossy().contains(".tok") {
                let t = std::fs::read_to_string(f.path()).unwrap();
                assert!(!all.contains(t.trim()), "token text reached a nerve");
            }
        }
    }
    let kinds = |k: &str| {
        fake.log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == k)
            .count()
    };
    let n_events = fake.log.lock().unwrap().len();
    let n_gets = *fake.state_gets.lock().unwrap();
    eprintln!(
        "fake nerve: {} events; /state GETs {}; applied {} capped {} hung {} failed {} refused {} loop.started {}; replay ledger consumed {}",
        n_events,
        n_gets,
        kinds("gaia.heal.applied"),
        kinds("gaia.heal.capped"),
        kinds("gaia.heal.hung"),
        kinds("gaia.heal.failed"),
        kinds("gaia.heal.refused"),
        kinds("gaia.loop.started"),
        c.consumed()
    );
    assert_eq!(c.consumed(), 4, "3 flap + 1 real-nerve restart");

    // ---------------- Teardown.
    drop(g);
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        our_dummies().is_empty(),
        "leftover dummies: {:?}",
        our_dummies()
    );
    for p in PORTS.iter().skip(1) {
        assert!(!open(*p), "port {p} still open after teardown");
    }
    eprintln!(
        "[{}] teardown: 0 dummies, 20451 + 20452-20456 closed (20450 is this test's own thread)",
        stamp()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
