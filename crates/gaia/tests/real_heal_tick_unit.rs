// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL test (GALION_REAL=1), box only, wave-4 slot 188: runs the **exact
//! `ExecStart=` of the staged unit** `deploy/staged/gaia-heal-tick@.service`
//! (expanded for the `gaia-canary` instance with the staged env file, as
//! systemd would) on real release binaries. Nothing is installed (D-L1).
//!
//! Only these rewrites are made, so the run fits a throwaway box dir:
//! `/var/lib/gaia` and `/etc/galion` move under a temp root, the live nerve
//! `127.0.0.1:8670` becomes a fake nerve on 127.0.0.1:20580, the two
//! `/opt/galion/rust/bin` programs become the box builds, and the 30 s
//! interval / 5 s jitter shrink to 1 s / 0.2 s. The canary port 19880 stays.
//!
//! Phases (what systemd would do around the loop):
//! 1. timer start: canary down, 1 admin-minted token -> one restart, token
//!    `.spent`, `gaia.heal.applied` on the nerve;
//! 2. `systemctl stop` with `KillMode=process` (SIGTERM to the gaia main pid
//!    only): gaia exits, the healed canary keeps serving (own process group);
//! 3. timer re-arm: healthy ticks, no token used;
//! 4. D-L2: nerve says 2 restarts + Gaia's 1 = 3 -> capped, token kept;
//!    nerve back to 0 -> restart allowed;
//! 5. stop file (the unit's ConditionPathExists path) -> loop exits in < 2 s
//!    and leaves the canary running;
//! 6. nerve `/state` broken -> fails closed, exit 7 after 5 errors, 0 tokens;
//! 7. real `openmind-nerve serve` (GAIA_NERVE_BIN, 127.0.0.1:20581) `/state`
//!    parses via `gaia nerve-restarts`; optional `GAIA_VPS_STATE` = a saved
//!    read-only `GET /state` from the live VPS nerve.
//!
//! Every file the loop wrote is under the unit's StateDirectory
//! (`/var/lib/gaia/...`); `/etc/galion` is unchanged (ReadOnlyPaths). No
//! token string appears in any output or nerve event.
//!
//! Run: `GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve cargo test
//! --release -p gaia --test real_heal_tick_unit -- --nocapture`.

mod unit_text;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const FAKE: &str = "127.0.0.1:20580";
const REAL: &str = "127.0.0.1:20581";
const CANARY: u16 = 19880;
const SVC: &str = "gaia-canary";

fn stamp() -> String {
    zagreb_time::now_rfc3339()
}

#[derive(Clone, Default)]
struct Fake {
    log: Arc<Mutex<Vec<Value>>>,
    state: Arc<Mutex<(u16, String)>>,
}

impl Fake {
    fn set(&self, code: u16, body: String) {
        *self.state.lock().unwrap() = (code, body);
    }
    fn restarts(&self, n: u64) {
        self.set(
            200,
            json!({"cycle": 7, "interval_s": 30, "mode": "live",
                   "components": [{"name": SVC, "unit": format!("{SVC}.service"), "status": "degrading",
                                   "restarts_1h": n, "reflex_restarts_per_hour": 3}],
                   "reflex": {"budgets": {format!("{SVC}.service"): {"dry_1h": 0, "live_1h": n}}}})
            .to_string(),
        );
    }
    fn count(&self, kind: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == kind && e["data"]["service"] == SVC)
            .count()
    }
    fn dump(&self) -> String {
        Value::Array(self.log.lock().unwrap().clone()).to_string()
    }
}

fn start_fake() -> Fake {
    let l = TcpListener::bind(FAKE).expect("bind fake nerve 20580");
    let f = Fake::default();
    f.restarts(0);
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
            let (code, resp) = if first.starts_with("GET /state") {
                f2.state.lock().unwrap().clone()
            } else if first.starts_with("POST /event") {
                let mut v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                v["ts"] = Value::from(stamp());
                f2.log.lock().unwrap().push(v);
                (200, r#"{"ok":true}"#.to_string())
            } else if first.starts_with("GET /health") {
                (200, r#"{"status":"ok"}"#.to_string())
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

fn canary_health() -> Option<Value> {
    let mut s = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], CANARY)),
        Duration::from_millis(300),
    )
    .ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        s,
        "GET /health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut b = String::new();
    s.read_to_string(&mut b).ok()?;
    serde_json::from_str(b.split_once("\r\n\r\n")?.1).ok()
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) -> Duration {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    t0.elapsed()
}

/// Live gaia-dummy processes on the canary port (not zombies).
fn canaries() -> Vec<u32> {
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
        let zombie = stat(pid).is_some_and(|s| s.0 == 'Z');
        if args.first().is_some_and(|a| a.ends_with("/gaia-dummy"))
            && args.iter().any(|a| a == &CANARY.to_string())
            && !zombie
        {
            v.push(pid);
        }
    }
    v
}

/// (state, pgid) from /proc/<pid>/stat.
fn stat(pid: u32) -> Option<(char, u32)> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = s.rsplit_once(") ")?.1;
    let f: Vec<&str> = rest.split(' ').collect();
    Some((f[0].chars().next()?, f[2].parse().ok()?))
}

fn kill(pid: u32, sig: &str) {
    // Test harness only: Gaia itself never signals anything.
    let _ = Command::new("kill").args([sig, &pid.to_string()]).status();
}

/// Kills, on any exit (also a failed assert), everything the test started:
/// the real nerve, every gaia loop it started (matched by its temp dir in
/// /proc/<pid>/cmdline, so a reused pid is never hit) and canaries.
struct Guard {
    kids: Vec<Child>,
    loops: Vec<u32>,
    tag: String,
}
impl Drop for Guard {
    fn drop(&mut self) {
        for c in &mut self.kids {
            let _ = c.kill();
            let _ = c.wait();
        }
        for pid in &self.loops {
            let ours = std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(&self.tag));
            if ours {
                kill(*pid, "-KILL");
            }
        }
        for pid in canaries() {
            kill(pid, "-TERM");
        }
    }
}

fn gaia(args: &[String]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_gaia"))
        .args(args)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let err = String::from_utf8_lossy(&o.stderr).to_string();
    assert!(
        !out.contains("hades1.") && !err.contains("hades1."),
        "token printed"
    );
    (o.status.code().unwrap_or(-1), format!("{out}{err}"))
}

fn last_result(out: &str) -> Value {
    let last = out
        .lines()
        .rev()
        .find(|l| l.contains("\"result\""))
        .unwrap_or("{}");
    serde_json::from_str::<Value>(last).unwrap()["result"].clone()
}

/// Files (path -> (len, mtime)) under `root`.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime)> {
    let mut m = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if md.is_dir() {
                stack.push(p);
            } else {
                m.insert(p, (md.len(), md.modified().unwrap()));
            }
        }
    }
    m
}

struct Unit {
    argv: Vec<String>,
    root: PathBuf,
}

impl Unit {
    fn state(&self, rel: &str) -> PathBuf {
        self.root.join("var/lib/gaia").join(rel)
    }
    fn start(&self, dir: &Path, n: u32) -> (Child, PathBuf) {
        let out = dir.join(format!("loop-{n}.out"));
        let f = std::fs::File::create(&out).unwrap();
        let c = Command::new(&self.argv[0])
            .args(&self.argv[1..])
            .stdout(Stdio::from(f.try_clone().unwrap()))
            .stderr(Stdio::from(f))
            .spawn()
            .unwrap();
        eprintln!("[{}] loop {n} started pid {}", stamp(), c.id());
        (c, out)
    }
    fn tokens(&self, suffix: &str) -> usize {
        std::fs::read_dir(self.state(&format!("{SVC}/tokens")))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(suffix))
            .count()
    }
    fn consumed(&self) -> usize {
        std::fs::read_to_string(self.state("hades/ledger.jsonl"))
            .unwrap_or_default()
            .matches("\"event\":\"consumed\"")
            .count()
    }
}

/// The staged ExecStart with the documented box rewrites.
fn unit_argv(root: &Path) -> Vec<String> {
    let staged = unit_text::canary_argv();
    let mut out = Vec::new();
    let mut prev = String::new();
    for (i, a) in staged.iter().enumerate() {
        let r = root.display().to_string();
        let mut v = if i == 0 {
            assert_eq!(a, "/opt/galion/rust/bin/gaia");
            env!("CARGO_BIN_EXE_gaia").to_string()
        } else if a == "/opt/galion/rust/bin/gaia-dummy" {
            env!("CARGO_BIN_EXE_gaia-dummy").to_string()
        } else if a == "127.0.0.1:8670" {
            FAKE.to_string()
        } else if let Some(rest) = a.strip_prefix("/var/lib/gaia") {
            format!("{r}/var/lib/gaia{rest}")
        } else if let Some(rest) = a.strip_prefix("/etc/galion") {
            format!("{r}/etc/galion{rest}")
        } else {
            a.clone()
        };
        if prev == "--interval-ms" {
            assert_eq!(a, "30000");
            v = "1000".into();
        }
        if prev == "--jitter-ms" {
            assert_eq!(a, "5000");
            v = "200".into();
        }
        assert!(
            !v.starts_with("/opt/") && !v.starts_with("/var/lib/") && !v.starts_with("/etc/"),
            "unrewritten path {v}"
        );
        prev = a.clone();
        out.push(v);
    }
    out
}

/// What `StateDirectory=gaia gaia/hades gaia/%i gaia/%i/state gaia/%i/tokens`
/// + `StateDirectoryMode=0700` creates, and the read-only /etc/galion files.
fn lay_out(root: &Path, admin: &Path) {
    use std::os::unix::fs::PermissionsExt;
    for d in [
        "gaia",
        "gaia/hades",
        "gaia/gaia-canary",
        "gaia/gaia-canary/state",
        "gaia/gaia-canary/tokens",
    ] {
        let p = root.join("var/lib").join(d);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let etc = root.join("etc/galion");
    std::fs::create_dir_all(etc.join("hades")).unwrap();
    std::fs::create_dir_all(etc.join("gaia")).unwrap();
    std::fs::write(
        etc.join("gaia/heal-gaia-canary.env"),
        unit_text::read("gaia/heal-gaia-canary.env"),
    )
    .unwrap();
    let r = root.display().to_string();
    let gate = unit_text::read("gaia/gaia-gate.toml")
        .replace("\"/var/lib/gaia", &format!("\"{r}/var/lib/gaia"))
        .replace("\"/etc/galion", &format!("\"{r}/etc/galion"));
    std::fs::write(etc.join("hades/gaia-gate.toml"), gate).unwrap();
    // Admin side (not on the gate host): throwaway key; only .pub goes to /etc.
    std::fs::create_dir_all(admin).unwrap();
    hades_gate::keys::generate(&admin.join("admin.seed"), &etc.join("hades/admin.pub")).unwrap();
}

fn mint(u: &Unit, admin: &Path, count: u64) {
    let toks = u.state(&format!("{SVC}/tokens")).display().to_string();
    let mut a: Vec<String> = [
        "tokens",
        "request",
        "--compact",
        "--token-dir",
        &toks,
        "--count",
        &count.to_string(),
        "--ttl-s",
        "900",
        "--service",
        SVC,
        "--port",
        &CANARY.to_string(),
        "--allow-ports",
        "19880-19880",
        "--",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(env!("CARGO_BIN_EXE_gaia-dummy").into());
    a.extend(["--port".to_string(), CANARY.to_string()]);
    let (code, out) = gaia(&a);
    assert_eq!(code, 0, "request: {out}");
    let req = last_result(&out)["request"].as_str().unwrap().to_string();
    let seed = admin.join("admin.seed").display().to_string();
    let (code, out) = gaia(&[
        "tokens".into(),
        "mint".into(),
        "--compact".into(),
        "--request".into(),
        req,
        "--seed".into(),
        seed,
    ]);
    assert_eq!(code, 0, "mint: {out}");
    assert_eq!(last_result(&out)["minted"], count);
}

fn exit_of(c: &mut Child, secs: u64) -> (i32, Duration) {
    let t0 = Instant::now();
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            return (s.code().unwrap_or(-1), t0.elapsed());
        }
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "loop did not exit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn real_staged_unit_exec_start() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 (and GAIA_NERVE_BIN) to run the slot 188 unit test");
        return;
    }
    let nerve_bin = std::env::var("GAIA_NERVE_BIN").expect("GAIA_NERVE_BIN");
    for p in [20580, 20581, CANARY] {
        assert!(!open(p), "port {p} busy before the test");
    }
    assert!(canaries().is_empty(), "leftover canary before the test");
    let dir = std::env::temp_dir().join(format!("gaia-unit-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("root");
    let admin = dir.join("admin-side");
    lay_out(&root, &admin);
    let u = Unit {
        argv: unit_argv(&root),
        root: root.clone(),
    };
    eprintln!(
        "[{}] unit argv (box-rewritten): {}",
        stamp(),
        u.argv[1..].join(" ")
    );
    let fake = start_fake();
    wait_for("fake nerve", 5, || open(20580));
    let real = Command::new(&nerve_bin)
        .args(["serve", "--listen", REAL, "--log"])
        .arg(dir.join("real-nerve.jsonl"))
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut g = Guard {
        kids: vec![real],
        loops: Vec::new(),
        tag: dir.display().to_string(),
    };
    wait_for("real nerve", 10, || open(20581));

    // ---- 1. timer start: canary down, 1 token -> 1 restart.
    mint(&u, &admin, 1);
    let etc_before = snapshot(&root.join("etc"));
    let all_before = snapshot(&root);
    let (mut l1, out1) = u.start(&dir, 1);
    g.loops.push(l1.id());
    let took = wait_for("canary healed", 20, || canary_health().is_some());
    let h = canary_health().unwrap();
    let canary_pid = h["pid"].as_u64().unwrap() as u32;
    eprintln!(
        "[{}] P1 healed in {} ms, canary pid {canary_pid}",
        stamp(),
        took.as_millis()
    );
    wait_for("token spent", 5, || u.tokens(".tok.spent") == 1);
    assert_eq!(u.tokens(".tok"), 0);
    assert_eq!(u.consumed(), 1);
    wait_for("applied event", 5, || fake.count("gaia.heal.applied") == 1);
    assert!(
        u.state(&format!("{SVC}/state/{SVC}.log")).exists(),
        "child log in StateDirectory"
    );

    // ---- 2. systemctl stop with KillMode=process: SIGTERM to gaia only.
    let l1_pgid = stat(l1.id()).unwrap().1;
    let (canary_state, canary_pgid) = stat(canary_pid).unwrap();
    assert_eq!(
        canary_pgid, canary_pid,
        "canary leads its own process group"
    );
    assert_ne!(canary_pgid, l1_pgid);
    kill(l1.id(), "-TERM");
    let (code1, t1) = exit_of(&mut l1, 10);
    std::thread::sleep(Duration::from_millis(300));
    let after = canary_health();
    assert_eq!(
        after.as_ref().map(|v| v["pid"].as_u64()),
        Some(Some(u64::from(canary_pid))),
        "canary survived gaia's stop"
    );
    eprintln!(
        "[{}] P2 SIGTERM gaia pid {} -> exit {code1} in {} ms; canary pid {canary_pid} ({canary_state}, pgid {canary_pgid} != gaia pgid {l1_pgid}) still answers",
        stamp(), l1.id(), t1.as_millis()
    );

    // ---- 3. timer re-arm: healthy ticks, no token.
    let probes0 = fake.count("service.probe");
    let (mut l2, out2) = u.start(&dir, 2);
    g.loops.push(l2.id());
    wait_for("3 healthy ticks", 15, || {
        fake.count("service.probe") >= probes0 + 3
    });
    assert_eq!(u.consumed(), 1);
    assert_eq!(fake.count("gaia.heal.applied"), 1);
    eprintln!(
        "[{}] P3 re-armed loop: 3+ healthy ticks, consumed still 1",
        stamp()
    );

    // ---- 4. D-L2: nerve 2 + Gaia 1 = 3 -> capped; nerve 0 -> allowed.
    mint(&u, &admin, 2);
    fake.restarts(2);
    let capped0 = fake.count("gaia.heal.capped");
    kill(canary_pid, "-TERM");
    wait_for("canary down", 5, || !open(CANARY));
    wait_for("capped event", 15, || {
        fake.count("gaia.heal.capped") > capped0
    });
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(u.consumed(), 1, "capped before any token is consumed");
    assert_eq!(u.tokens(".tok"), 2);
    assert!(!open(CANARY));
    eprintln!(
        "[{}] P4 nerve restarts_1h=2 + Gaia 1 -> capped, tokens kept 2",
        stamp()
    );
    fake.restarts(0);
    let took = wait_for("healed after cap lift", 20, || canary_health().is_some());
    wait_for("2nd consumed", 5, || u.consumed() == 2);
    wait_for("2nd token renamed .spent", 5, || u.tokens(".tok") == 1);
    assert_eq!(fake.count("gaia.heal.applied"), 2);
    eprintln!(
        "[{}] P4 nerve back to 0 -> healed in {} ms (Gaia 2 in the hour)",
        stamp(),
        took.as_millis()
    );

    // ---- 5. stop file (= the unit's ConditionPathExists path).
    let canary2 = canary_health().unwrap()["pid"].as_u64().unwrap() as u32;
    let stop = u.state(&format!("{SVC}/stop"));
    std::fs::write(&stop, b"").unwrap();
    let (code2, t2) = exit_of(&mut l2, 5);
    let mut s2 = String::new();
    std::fs::File::open(&out2)
        .unwrap()
        .read_to_string(&mut s2)
        .unwrap();
    let sum2 = last_result(&s2);
    assert_eq!(code2, 0);
    assert_eq!(sum2["stop"], "stop-file");
    assert!(t2 < Duration::from_secs(2), "stop file took {t2:?}");
    assert!(
        canary_health().is_some(),
        "stop file leaves the canary running"
    );
    eprintln!(
        "[{}] P5 stop file -> exit 0 in {} ms, stop {}, ticks {}, restarts {}",
        stamp(),
        t2.as_millis(),
        sum2["stop"],
        sum2["ticks"],
        sum2["restarts"]
    );
    // While the stop file exists a start (as the timer would do it if the
    // condition did not exist) stops before the first tick.
    let (mut l2b, _) = u.start(&dir, 22);
    g.loops.push(l2b.id());
    let (c2b, _) = exit_of(&mut l2b, 5);
    assert_eq!(c2b, 0);
    std::fs::remove_file(&stop).unwrap();

    // ---- 6. broken nerve /state -> fail closed, exit 7, no token.
    kill(canary2, "-TERM");
    wait_for("canary down", 5, || !open(CANARY));
    fake.set(500, r#"{"error":"boom"}"#.into());
    let (mut l3, out3) = u.start(&dir, 3);
    g.loops.push(l3.id());
    let (code3, t3) = exit_of(&mut l3, 60);
    let mut s3 = String::new();
    std::fs::File::open(&out3)
        .unwrap()
        .read_to_string(&mut s3)
        .unwrap();
    let sum3 = last_result(&s3);
    assert_eq!(code3, 7, "{s3}");
    assert_eq!(sum3["stop"], "max-errors");
    assert_eq!(u.consumed(), 2, "no token consumed while /state is broken");
    assert_eq!(u.tokens(".tok"), 1);
    assert!(!open(CANARY));
    eprintln!(
        "[{}] P6 /state 500 -> exit 7 after {} ms, errors {}, sleeps {}",
        stamp(),
        t3.as_millis(),
        sum3["errors_total"],
        sum3["sleeps_ms"]
    );

    // ---- 7. real nerve /state and (optional) live VPS capture.
    let (c, out) = gaia(&[
        "nerve-restarts".into(),
        "--compact".into(),
        "--nerve".into(),
        REAL.into(),
        "--service".into(),
        SVC.into(),
    ]);
    assert_eq!(c, 0, "{out}");
    let r = last_result(&out);
    assert_eq!(r["services"][0]["left_for_gaia"], 3, "{r}");
    eprintln!("[{}] P7 real openmind-nerve /state: {r}", stamp());
    if let Ok(f) = std::env::var("GAIA_VPS_STATE") {
        let mut a: Vec<String> = vec![
            "nerve-restarts".into(),
            "--compact".into(),
            "--state-file".into(),
            f,
        ];
        for s in [
            SVC,
            "galion-one",
            "cloudflared",
            "hermes-rs",
            "galion-nerve",
        ] {
            a.extend(["--service".to_string(), s.to_string()]);
        }
        let (c, out) = gaia(&a);
        eprintln!(
            "[{}] P7 VPS /state capture: exit {c} {}",
            stamp(),
            last_result(&out)
        );
        assert_eq!(c, 0, "{out}");
        assert_eq!(last_result(&out)["services"][0]["left_for_gaia"], 3);
    }

    // ---- write set, secrets.
    assert_eq!(
        snapshot(&root.join("etc")),
        etc_before,
        "/etc/galion changed"
    );
    let after = snapshot(&root);
    let mut wrote: Vec<String> = Vec::new();
    for (p, v) in &after {
        if all_before.get(p) != Some(v) {
            assert!(
                p.starts_with(root.join("var/lib/gaia")),
                "write outside StateDirectory: {}",
                p.display()
            );
            wrote.push(p.strip_prefix(&root).unwrap().display().to_string());
        }
    }
    eprintln!(
        "[{}] write set (all under var/lib/gaia): {wrote:?}",
        stamp()
    );
    let mut all = String::new();
    for o in [&out1, &out2, &out3] {
        std::fs::File::open(o)
            .unwrap()
            .read_to_string(&mut all)
            .unwrap();
    }
    assert!(
        !all.contains("hades1.") && !fake.dump().contains("hades1."),
        "token leaked"
    );

    drop(g);
    let _ = std::fs::remove_dir_all(&dir);
    wait_for("canary gone", 5, || canaries().is_empty());
    // The fake nerve is a thread of this test process and goes with it.
    wait_for("real nerve and canary ports free", 5, || {
        !open(20581) && !open(CANARY)
    });
    eprintln!(
        "[{}] done; 20581 and {CANARY} free, no canary left",
        stamp()
    );
}

/// `systemd-analyze verify` + `security --offline` on the staged text in a
/// throwaway root (the box's own /usr/lib/systemd/system for the standard
/// targets, the box release builds at /opt/galion/rust/bin). Box only; the
/// box has no systemd as PID 1, so this is the offline check only.
#[test]
fn real_systemd_analyze_verify() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 to run systemd-analyze on the staged unit");
        return;
    }
    let sa = "/usr/bin/systemd-analyze";
    assert!(Path::new(sa).exists(), "systemd-analyze is not installed");
    let root = std::env::temp_dir().join(format!("gaia-unit-verify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let sys = root.join("etc/systemd/system");
    let bin = root.join("opt/galion/rust/bin");
    for d in [
        &sys,
        &bin,
        &root.join("etc/galion/gaia"),
        &root.join("usr/lib/systemd"),
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    let ok = Command::new("cp")
        .args(["-a", "/usr/lib/systemd/system"])
        .arg(root.join("usr/lib/systemd/"))
        .status()
        .unwrap();
    assert!(ok.success());
    std::fs::copy(env!("CARGO_BIN_EXE_gaia"), bin.join("gaia")).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_gaia-dummy"), bin.join("gaia-dummy")).unwrap();
    std::fs::write(
        root.join("etc/galion/gaia/heal-gaia-canary.env"),
        unit_text::read("gaia/heal-gaia-canary.env"),
    )
    .unwrap();
    let svc = unit_text::read("gaia-heal-tick@.service");
    std::fs::write(
        sys.join("gaia-heal-tick@.timer"),
        unit_text::read("gaia-heal-tick@.timer"),
    )
    .unwrap();
    let run = |text: &str| {
        std::fs::write(sys.join("gaia-heal-tick@.service"), text).unwrap();
        let o = Command::new(sa)
            .arg("verify")
            .arg(format!("--root={}", root.display()))
            .args([
                "--man=no",
                "gaia-heal-tick@gaia-canary.service",
                "gaia-heal-tick@gaia-canary.timer",
            ])
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        (o.status.code().unwrap_or(-1), text)
    };
    let (code, out) = run(&svc);
    eprintln!("[{}] verify staged: exit {code}, output {:?}", stamp(), out);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim().is_empty(), "verify warned: {out}");
    // Negative control: one bad value must be reported.
    let (_, bad) = run(&svc.replace("KillMode=process", "KillMode=bogus"));
    eprintln!("[{}] verify negative control: {:?}", stamp(), bad.trim());
    assert!(
        bad.contains("kill mode"),
        "negative control not caught: {bad}"
    );
    let (_, out) = run(&svc);
    assert!(out.trim().is_empty());
    let o = Command::new(sa)
        .args(["security", "--offline=yes"])
        .arg(format!("--root={}", root.display()))
        .arg("gaia-heal-tick@gaia-canary.service")
        .output()
        .unwrap();
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    let line = s
        .lines()
        .find(|l| l.contains("Overall exposure level"))
        .unwrap_or_default()
        .to_string();
    let score: f64 = line
        .split(": ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| panic!("no exposure score in {s}"));
    eprintln!("[{}] {line}", stamp());
    assert!(score <= 2.0, "exposure {score} > 2.0");
    let _ = std::fs::remove_dir_all(&root);
}
