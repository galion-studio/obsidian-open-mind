// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! REAL test (GALION_REAL=1) of the S3.1 supervised loop, box only.
//!
//! Real processes: `openmind-nerve serve` (GAIA_NERVE_BIN) on
//! 127.0.0.1:20300 (wave-4 slot 160 range 20300-20309) and three
//! `gaia-dummy` fault services on 127.0.0.1:19601-19603. The dummies sit in
//! 19600-19609 because the heal apply path only ever touches 19000-19999
//! (`gaia_live::heal::TEST_PORTS`, a safety guard this test does not widen).
//!
//! Three `gaia heal-tick --loop` processes run at once, sharing one
//! hades-gate (one replay ledger, one global restart ledger), each with its
//! own `--state` dir and a dir of five pre-minted single-use tokens:
//! * hang  (accepts, never answers): probe times out, every restart fails
//!   (the hung process keeps the port; Gaia never signals it): 3 x
//!   `gaia.heal.failed`, then `gaia.heal.capped` with no token consumed;
//! * flap  (every instance exits after 1.2 s): 3 x `gaia.heal.applied`,
//!   then capped;
//! * crash (the first instance exits once): 1 x applied, then healthy.
//!
//! Then: no zombies under the loops, the stop file ends all three, a
//! one-shot tick and a fourth loop with brand-new `--state` dirs are still
//! capped by the global ledger (tokens not consumed), the replay ledger has
//! exactly 7 consumed tokens, no token text reaches the nerve, every stamp
//! is Europe/Zagreb, and teardown leaves no process and no open port.
//!
//! Run: `GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve cargo test
//! --release -p gaia --test real_gaia_loop -- --nocapture`.

use gaia::live_heal::heal_plan;
use gaia_live::heal::TestTarget;
use hades_gate::adapter::gaia_plan_bytes;
use hades_gate::approval::unix_now;
use hades_gate::token::{plan_hash, Claims};
use serde_json::Value;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NERVE: &str = "127.0.0.1:20300";
const NERVE_PORT: u16 = 20300;
const ALLOW: &str = "19600-19609";
const SVCS: [(&str, u16); 3] = [
    ("dummy-hang", 19601),
    ("dummy-flap", 19602),
    ("dummy-crash", 19603),
];

fn stamp() -> String {
    zagreb_time::now_rfc3339()
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
        for pid in our_dummies() {
            let _ = Command::new("kill")
                .arg(pid.to_string())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// gaia-dummy processes on our ports (by /proc cmdline), whatever their parent.
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
        let is_dummy = args.first().is_some_and(|a| a.ends_with("/gaia-dummy"));
        let ours = args
            .iter()
            .any(|a| SVCS.iter().any(|(_, p)| a == &p.to_string()));
        let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| {
                s.rsplit_once(") ")
                    .map(|x| x.1.starts_with('Z'))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if is_dummy && ours && !zombie {
            v.push(pid);
        }
    }
    v
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

fn http_get(port: u16, path: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut b = String::new();
    s.read_to_string(&mut b).unwrap();
    b.split_once("\r\n\r\n").unwrap().1.to_string()
}

fn target(name: &str, port: u16, argv: Vec<String>) -> TestTarget {
    TestTarget {
        name: name.into(),
        port,
        health_path: "/health".into(),
        argv,
    }
}

/// Admin side (in-process, same as `hades-gate mint`): one token, 0600 file.
fn mint(seed: &Path, t: &TestTarget, out: &Path) {
    let key = hades_gate::keys::load_seed(seed).unwrap();
    let now = unix_now();
    let tok = Claims {
        action: "heal-apply".into(),
        target: t.name.clone(),
        plan_hash: plan_hash(&gaia_plan_bytes(&heal_plan(t))),
        nonce: hades_gate::keys::os_random::<16>().unwrap(),
        issued_at: now,
        expires_at: now + 600,
    }
    .sign(&key);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(out)
        .unwrap()
        .write_all(tok.as_bytes())
        .unwrap();
}

fn events(log: &Path) -> Vec<Value> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn count(ev: &[Value], svc: &str, kind: &str) -> usize {
    ev.iter()
        .filter(|e| e["kind"] == kind && e["data"]["service"] == svc)
        .count()
}

/// Zombie children of `pid` (from /proc/<pid>/task/*/children).
fn zombies_of(pid: u32) -> (usize, usize) {
    let mut kids = Vec::new();
    if let Ok(rd) = std::fs::read_dir(format!("/proc/{pid}/task")) {
        for t in rd.flatten() {
            if let Ok(s) = std::fs::read_to_string(t.path().join("children")) {
                kids.extend(s.split_whitespace().filter_map(|p| p.parse::<u32>().ok()));
            }
        }
    }
    let z = kids
        .iter()
        .filter(|k| {
            std::fs::read_to_string(format!("/proc/{k}/stat"))
                .map(|s| {
                    s.rsplit_once(") ")
                        .map(|x| x.1.starts_with('Z'))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .count();
    (z, kids.len())
}

#[allow(clippy::too_many_lines)]
#[test]
fn real_supervised_loop_hang_flap_crash_global_budget() {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP: set GALION_REAL=1 and GAIA_NERVE_BIN to run the real loop test");
        return;
    }
    let nerve_bin = std::env::var("GAIA_NERVE_BIN").expect("GAIA_NERVE_BIN");
    let gaia = env!("CARGO_BIN_EXE_gaia");
    let dummy = env!("CARGO_BIN_EXE_gaia-dummy");
    for p in std::iter::once(NERVE_PORT).chain(SVCS.iter().map(|s| s.1)) {
        assert!(!open(p), "port {p} is busy before the test");
    }
    assert!(our_dummies().is_empty(), "leftover dummies before the test");
    let dir = std::env::temp_dir().join(format!("gaia-loop-real-{}", std::process::id()));
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
    let nerve_log = dir.join("nerve.jsonl");
    let once = dir.join("crash.once");
    let argv = |name: &str, port: u16| -> Vec<String> {
        let mut v = vec![dummy.to_string(), "--port".into(), port.to_string()];
        match name {
            "dummy-hang" => v.extend(["--mode".into(), "hang".into()]),
            "dummy-flap" => v.extend([
                "--mode".into(),
                "flap".into(),
                "--up-ms".into(),
                "1200".into(),
            ]),
            _ => v.extend([
                "--mode".into(),
                "crash".into(),
                "--up-ms".into(),
                "800".into(),
                "--once-file".into(),
                once.display().to_string(),
            ]),
        }
        v
    };
    let targets: Vec<TestTarget> = SVCS
        .iter()
        .map(|(n, p)| target(n, *p, argv(n, *p)))
        .collect();
    for t in &targets {
        let td = dir.join(format!("tok-{}", t.name));
        std::fs::create_dir_all(&td).unwrap();
        for i in 1..=5 {
            mint(&seed, t, &td.join(format!("{i:02}.tok")));
        }
    }

    eprintln!("[{}] start openmind-nerve on {NERVE}", stamp());
    let nerve = Command::new(&nerve_bin)
        .args(["serve", "--listen", NERVE, "--log"])
        .arg(&nerve_log)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut g = Guard {
        kids: vec![nerve],
        dir: dir.clone(),
    };
    wait_port(NERVE_PORT);
    eprintln!("nerve GET /health: {}", http_get(NERVE_PORT, "/health"));
    for t in &targets {
        let c = Command::new(&t.argv[0])
            .args(&t.argv[1..])
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        g.kids.push(c);
        wait_port(t.port);
    }
    eprintln!(
        "[{}] dummies up: hang {} flap {} crash {}",
        stamp(),
        targets[0].port,
        targets[1].port,
        targets[2].port
    );

    let loop_cmd = |t: &TestTarget, state: &str, extra: &[&str]| {
        let mut c = Command::new(gaia);
        c.args([
            "heal-tick",
            "--loop",
            "--compact",
            "--service",
            &t.name,
            "--port",
        ])
        .arg(t.port.to_string())
        .args(["--allow-ports", ALLOW, "--nerve", NERVE, "--state"])
        .arg(dir.join(state))
        .arg("--hades-config")
        .arg(&config)
        .arg("--token-dir")
        .arg(dir.join(format!("tok-{}", t.name)))
        .arg("--stop-file")
        .arg(dir.join("STOP"))
        .args(["--wait-ms", "2500"])
        .args(extra)
        .arg("--")
        .args(&t.argv)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        c
    };
    let t_loops = Instant::now();
    let mut loops: Vec<Child> = targets
        .iter()
        .map(|t| {
            loop_cmd(
                t,
                &format!("state-{}", t.name),
                &[
                    "--interval-ms",
                    "300",
                    "--jitter-ms",
                    "200",
                    "--max-ticks",
                    "80",
                    "--max-runtime-s",
                    "60",
                ],
            )
            .spawn()
            .unwrap()
        })
        .collect();
    let loop_pids: Vec<u32> = loops.iter().map(Child::id).collect();
    eprintln!("[{}] 3 loops started, pids {loop_pids:?}", stamp());

    // Wait for the documented outcome of each fault on the real nerve.
    let done = |ev: &[Value]| {
        let crash_up_after = ev
            .iter()
            .skip_while(|e| {
                !(e["kind"] == "gaia.heal.applied" && e["data"]["service"] == "dummy-crash")
            })
            .filter(|e| {
                e["kind"] == "service.probe"
                    && e["data"]["service"] == "dummy-crash"
                    && e["data"]["state"] == "up"
            })
            .count();
        count(ev, "dummy-hang", "gaia.heal.failed") == 3
            && count(ev, "dummy-hang", "gaia.heal.capped") >= 2
            && count(ev, "dummy-flap", "gaia.heal.applied") == 3
            && count(ev, "dummy-flap", "gaia.heal.capped") >= 2
            && count(ev, "dummy-crash", "gaia.heal.applied") == 1
            && crash_up_after >= 3
    };
    let deadline = Instant::now() + Duration::from_secs(55);
    let mut ev = events(&nerve_log);
    while !done(&ev) {
        assert!(
            Instant::now() < deadline,
            "timeout; nerve so far: {}",
            ev.len()
        );
        std::thread::sleep(Duration::from_millis(500));
        ev = events(&nerve_log);
    }
    let settled = t_loops.elapsed();
    eprintln!(
        "[{}] all three outcomes on the nerve after {settled:?}",
        stamp()
    );

    // No zombies under the loops (the flap and failed-hang children exited).
    std::thread::sleep(Duration::from_millis(1200));
    for pid in &loop_pids {
        let (z, n) = zombies_of(*pid);
        eprintln!("loop pid {pid}: children {n}, zombies {z}");
        assert_eq!(z, 0, "loop {pid} has zombie children");
    }
    // The live nerve tail agrees with the log (GET, as an observer would).
    let tail: Value = serde_json::from_str(&http_get(NERVE_PORT, "/events?n=1000")).unwrap();
    assert!(tail
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "gaia.loop.started"));

    // Stop all three with the stop file.
    let t_stop = Instant::now();
    std::fs::write(dir.join("STOP"), b"").unwrap();
    let mut summaries = Vec::new();
    for (i, mut l) in loops.drain(..).enumerate() {
        let out = {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(_s) = l.try_wait().unwrap() {
                    break l.wait_with_output().unwrap();
                }
                assert!(Instant::now() < deadline, "loop {i} did not stop");
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let summary = lines.last().unwrap()["result"].clone();
        let ticks = &lines[..lines.len() - 1];
        assert_eq!(summary["stop"], "stop-file");
        assert_eq!(summary["ticks"].as_u64().unwrap() as usize, ticks.len());
        for s in summary["sleeps_ms"].as_array().unwrap() {
            let s = s.as_u64().unwrap();
            assert!((300..=500).contains(&s), "sleep {s}");
        }
        for e in std::fs::read_dir(dir.join(format!("tok-{}", targets[i].name)))
            .unwrap()
            .flatten()
        {
            let tok = std::fs::read_to_string(e.path()).unwrap();
            assert!(!text.contains(tok.trim()), "token text must not be printed");
        }
        assert!(
            !text.contains(hades_gate::token::TOKEN_PREFIX),
            "no token-shaped text in the loop output"
        );
        summaries.push((targets[i].name.clone(), summary, ticks.to_vec()));
    }
    let stop_ms = t_stop.elapsed().as_millis();
    eprintln!(
        "[{}] all loops stopped {stop_ms} ms after the stop file",
        stamp()
    );
    for (name, s, ticks) in &summaries {
        eprintln!(
            "{name}: ticks {} restarts {} outcomes {} tokens_spent {} reaped {} sleeps {}..{} ms errors {}",
            s["ticks"],
            s["restarts"],
            s["outcomes"],
            s["tokens_spent"],
            s["children_reaped"],
            s["sleeps_ms"].as_array().unwrap().iter().filter_map(Value::as_u64).min().unwrap_or(0),
            s["sleeps_ms"].as_array().unwrap().iter().filter_map(Value::as_u64).max().unwrap_or(0),
            s["errors_total"],
        );
        let first_ms: Vec<u64> = ticks
            .iter()
            .take(6)
            .filter_map(|t| t["ms"].as_u64())
            .collect();
        eprintln!("{name}: first tick durations ms {first_ms:?}");
    }
    let by = |n: &str| &summaries.iter().find(|s| s.0 == n).unwrap().1;
    assert_eq!(by("dummy-hang")["tokens_spent"], 3);
    assert_eq!(by("dummy-hang")["restarts"], 3);
    assert_eq!(by("dummy-hang")["outcomes"]["restarted"], Value::Null);
    assert_eq!(by("dummy-hang")["outcomes"]["apply-failed"], 3);
    assert_eq!(by("dummy-flap")["tokens_spent"], 3);
    assert_eq!(by("dummy-flap")["outcomes"]["restarted"], 3);
    assert_eq!(by("dummy-crash")["tokens_spent"], 1);
    assert_eq!(by("dummy-crash")["outcomes"]["restarted"], 1);
    for (name, s, _) in &summaries {
        assert_eq!(s["errors_total"], 0, "{name}");
        let left = std::fs::read_dir(dir.join(format!("tok-{name}")))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "tok")
            })
            .count();
        let want = if name == "dummy-crash" { 4 } else { 2 };
        assert_eq!(left, want, "{name} unused tokens");
    }
    let consumed = || {
        std::fs::read_to_string(dir.join("gate/ledger.jsonl"))
            .unwrap()
            .matches("\"event\":\"consumed\"")
            .count()
    };
    assert_eq!(consumed(), 7);
    let global = std::fs::read_to_string(dir.join("gate/gaia-restarts.jsonl")).unwrap();
    assert_eq!(global.lines().count(), 7, "{global}");

    // A brand-new --state dir cannot reset the budget: one-shot tick for the
    // (now down) flap service with a fresh token -> capped, token kept.
    let flap = &targets[1];
    let fresh_tok = dir.join("fresh.tok");
    mint(&seed, flap, &fresh_tok);
    let out = Command::new(gaia)
        .args(["heal-tick", "--compact", "--service", &flap.name, "--port"])
        .arg(flap.port.to_string())
        .args(["--allow-ports", ALLOW, "--nerve", NERVE, "--state"])
        .arg(dir.join("state-brand-new"))
        .arg("--hades-config")
        .arg(&config)
        .arg("--token-file")
        .arg(&fresh_tok)
        .arg("--")
        .args(&flap.argv)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(5), "{text}");
    assert!(text.contains("\"outcome\":\"capped\""), "{text}");
    assert!(!dir.join("state-brand-new/restarts.jsonl").exists());
    assert_eq!(consumed(), 7, "capped tick must not consume the token");
    eprintln!(
        "[{}] one-shot heal-tick with a new --state dir: exit 5 capped, token not consumed",
        stamp()
    );

    // And a whole new loop on another new --state dir: capped every tick.
    let _ = std::fs::remove_file(dir.join("STOP"));
    let out = loop_cmd(
        flap,
        "state-second-loop",
        &["--interval-ms", "200", "--max-ticks", "3", "--seed", "160"],
    )
    .output()
    .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let last: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert_eq!(last["result"]["stop"], "max-ticks");
    assert_eq!(last["result"]["outcomes"]["capped"], 3, "{text}");
    assert_eq!(last["result"]["restarts"], 0);
    assert_eq!(consumed(), 7);
    eprintln!(
        "[{}] second flap loop on a new --state dir: outcomes {} (no restart, no token)",
        stamp(),
        last["result"]["outcomes"]
    );

    // Nerve: loop start/stop events, the heal events, Zagreb stamps, no token text.
    let ev = events(&nerve_log);
    let kinds = |k: &str| ev.iter().filter(|e| e["kind"] == k).count();
    assert_eq!(kinds("gaia.loop.started"), 4);
    assert_eq!(kinds("gaia.loop.stopped"), 4);
    assert_eq!(kinds("gaia.heal.applied"), 4);
    assert_eq!(kinds("gaia.heal.failed"), 3);
    assert_eq!(kinds("gaia.heal.approved"), 7);
    let raw = std::fs::read_to_string(&nerve_log).unwrap();
    for t in &targets {
        for e in std::fs::read_dir(dir.join(format!("tok-{}", t.name)))
            .unwrap()
            .flatten()
        {
            let tok = std::fs::read_to_string(e.path()).unwrap();
            assert!(!raw.contains(tok.trim()), "token text reached the nerve");
        }
    }
    for e in &ev {
        let ts = e["ts"].as_str().unwrap();
        assert!(ts.ends_with("+02:00") || ts.ends_with("+01:00"), "{ts}");
    }
    eprintln!(
        "nerve events: {} total; loop.started {} loop.stopped {} heal.applied {} heal.failed {} heal.capped {} heal.approved {} heal.refused {}",
        ev.len(),
        kinds("gaia.loop.started"),
        kinds("gaia.loop.stopped"),
        kinds("gaia.heal.applied"),
        kinds("gaia.heal.failed"),
        kinds("gaia.heal.capped"),
        kinds("gaia.heal.approved"),
        kinds("gaia.heal.refused"),
    );

    // Teardown: our children, then any dummy on our ports (the crash
    // service's restarted instance), then prove nothing is left.
    let left_before = our_dummies();
    eprintln!(
        "dummies alive before teardown (ours): {}",
        left_before.len()
    );
    drop(g);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        our_dummies().is_empty(),
        "leftover dummies: {:?}",
        our_dummies()
    );
    for p in std::iter::once(NERVE_PORT).chain(SVCS.iter().map(|s| s.1)) {
        assert!(!open(p), "port {p} still open after teardown");
    }
    eprintln!(
        "[{}] teardown: 0 dummies, ports 20300 + 19601-19603 closed",
        stamp()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
