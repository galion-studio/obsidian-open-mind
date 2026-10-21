// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Slot 206: POSEIDON (`openmind-queue`) crash and soak suite against the
//! real release binary. Gated: runs only with `GALION_REAL=1`. Binary from
//! `POSEIDON_BIN`, default `$CARGO_TARGET_DIR/release/openmind-queue`.
//! Loopback ports 20761-20768 only (slot 206 band 20760-20769). Box only.
//!
//! * `real_kill9_loop_during_ack`: producers, workers (lease + ack, with
//!   deliberate stale acks) and a quota / budget / backpressure mix run
//!   while the server is SIGKILLed at random moments, then restarted on
//!   the same data dir. After every restart: no lost job, no resurrected
//!   ack, exact ack and spend totals, caps and quotas intact, refusal
//!   counters never go backwards and match what clients saw, lease tokens
//!   stay fenced, and the server state equals an independent replay of
//!   the log the dead process left. Compaction runs under that load
//!   (`--compact-min 300`).
//! * `real_kill9_loop_compaction_hammer`: the same with `--compact-min 100`
//!   and a thread POSTing `/admin/compact` every 25 ms, so kills land
//!   inside compactions.
//! * `real_torn_tail`: a real log cut at random byte offsets, plus garbage,
//!   half lines and a 4 KiB zero block; each copy must start, replay
//!   exactly the complete lines and keep working after the repair. A bad
//!   line in the middle must make the binary refuse to start.
//! * `real_flock_two_servers`: a second server on the same data dir is
//!   refused (8 at once), does not bind, does not touch the log; after a
//!   SIGKILL of the first the lock is free at once.
//! * `real_soak_10k`: 3 rounds of 10,000 jobs enqueued then drained, RSS
//!   sampled every 250 ms, one SIGKILL mid-drain with a full no-loss check
//!   and replay timing; RSS must stay bounded across rounds.
//!
//! Tunables: `SOAK_SEED`, `SOAK_KILLS` (default 40), `SOAK_JOBS` (10000),
//! `SOAK_ROUNDS` (3). Results go to stderr (run with --nocapture).

use openmind_queue::engine::{Config, Engine, Stats};
use openmind_queue::{http, store};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn real() -> bool {
    if std::env::var("GALION_REAL").as_deref() == Ok("1") {
        return true;
    }
    eprintln!("skipped: set GALION_REAL=1 to run the POSEIDON crash/soak suite");
    false
}

fn bin() -> String {
    std::env::var("POSEIDON_BIN").unwrap_or_else(|_| {
        let t = std::env::var("CARGO_TARGET_DIR")
            .unwrap_or_else(|_| "/workspace/swarm/target/poseidon-crash-soak".into());
        format!("{t}/release/openmind-queue")
    })
}

fn env_num(k: &str, d: u64) -> u64 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

fn stamp() -> String {
    openmind_queue::zagreb_ms(openmind_queue::now_ms())
}

/// xorshift64*: no rand crate.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo).max(1)
    }
}

fn seed() -> u64 {
    env_num("SOAK_SEED", openmind_queue::now_ms())
}

fn workdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("poseidon-206-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// How a request ended.
enum Out {
    Done(u16, Value),
    /// Connection refused: the request never reached a server.
    NotSent,
    /// Sent (or maybe sent) but no answer: the server died under it.
    Ambiguous,
}

fn req(addr: &str, m: &str, p: &str, b: Option<&Value>) -> Out {
    let body = b.map(|v| v.to_string());
    match http::request(addr, m, p, body.as_deref(), Duration::from_secs(5)) {
        Ok((c, s)) => Out::Done(c, serde_json::from_str(&s).unwrap_or(Value::Null)),
        Err(e) if e.kind() == ErrorKind::ConnectionRefused => Out::NotSent,
        Err(_) => Out::Ambiguous,
    }
}

fn call(addr: &str, m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    match req(addr, m, p, b.as_ref()) {
        Out::Done(c, v) => (c, v),
        _ => panic!("{m} {p} at {addr}: no answer"),
    }
}

struct Server {
    child: Child,
    addr: String,
    pid: u32,
}

fn spawn(addr: &str, data: &Path, extra: &[&str], err_log: &Path) -> std::io::Result<Child> {
    let err = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(err_log)?;
    Command::new(bin())
        .args(["serve", "--bind", addr, "--data"])
        .arg(data)
        .args(extra)
        .stdout(Stdio::null())
        .stderr(err)
        .spawn()
}

/// Start and wait for /healthz. Returns the server and how long it took.
fn start(addr: &str, data: &Path, extra: &[&str], err_log: &Path) -> (Server, Duration) {
    let t0 = Instant::now();
    let mut child =
        spawn(addr, data, extra, err_log).expect("start release binary (build it first)");
    let pid = child.id();
    for _ in 0..600 {
        if http::request(addr, "GET", "/healthz", None, Duration::from_millis(200)).is_ok() {
            let addr = addr.to_string();
            return (Server { child, addr, pid }, t0.elapsed());
        }
        if let Ok(Some(st)) = child.try_wait() {
            panic!(
                "server exited early on {addr}: {st}; see {}",
                err_log.display()
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("server did not come up on {addr}");
}

impl Server {
    fn kill9(mut self) {
        let _ = self.child.kill(); // SIGKILL
        let _ = self.child.wait();
    }
    fn stats(&self) -> Value {
        let (c, v) = call(&self.addr, "GET", "/stats", None);
        assert_eq!(c, 200);
        v
    }
}

/// VmRSS and VmHWM of a process in KiB.
fn rss_kib(pid: u32) -> (u64, u64) {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (get("VmRSS:"), get("VmHWM:"))
}

/// Replay a log file the way the server does (complete, valid lines up to
/// the first bad one) with the library engine: an independent check of
/// what the restarted binary reports.
fn replay_file(path: &Path, cfg: Config) -> (Engine, usize) {
    let buf = std::fs::read(path).unwrap_or_default();
    let mut e = Engine::new(cfg);
    let mut n = 0;
    let mut pos = 0;
    while let Some(i) = buf[pos..].iter().position(|&b| b == b'\n') {
        let line = &buf[pos..pos + i];
        match std::str::from_utf8(line).ok().and_then(store::decode) {
            Some(r) => {
                e.apply(&r);
                n += 1;
            }
            None => break,
        }
        pos += i + 1;
    }
    (e, n)
}

fn stats_of(v: &Value) -> Stats {
    serde_json::from_value(v["stats"].clone()).unwrap()
}

// ------------------------------------------------------------ kill loop

const AGENTS: [(&str, u64); 3] = [("spend", 1), ("tight", 5), ("qq", 2)];
const TIGHT_CAP: u64 = 400;
const QQ_PENDING: u32 = 5;

#[derive(Default)]
struct Ledger {
    /// Enqueues answered 201: id -> (agent, cost).
    enq: BTreeMap<u64, (String, u64)>,
    enq_ambiguous: u64,
    /// Acks known applied: id -> charge (answered 200, or ambiguous and
    /// later found gone).
    acked: BTreeMap<u64, u64>,
    /// Acks sent with no answer, not yet resolved.
    ack_pending: BTreeSet<u64>,
    /// What a lease said about its job: id -> (agent, cost). Covers jobs
    /// whose enqueue answer was lost but which were durable anyway.
    leased: BTreeMap<u64, (String, u64)>,
    ack_ambiguous_total: u64,
    acked_since_check: Vec<u64>,
    lease_ambiguous: u64,
    other_ambiguous: u64,
    /// Refusals clients saw (confirmed answers).
    seen_budget: u64,
    seen_quota: u64,
    seen_backpressure: u64,
    seen_stale: u64,
    max_token: u64,
    ok_requests: u64,
}

struct Shared {
    addr: String,
    led: Mutex<Ledger>,
    paused: AtomicBool,
    stop: AtomicBool,
    inflight: AtomicUsize,
    ops: AtomicU64,
}

fn guarded<F: FnOnce() -> Out>(s: &Shared, f: F) -> Option<Out> {
    if s.paused.load(Ordering::SeqCst) {
        return None;
    }
    s.inflight.fetch_add(1, Ordering::SeqCst);
    // re-check after announcing, so the killer's quiesce wait is exact
    if s.paused.load(Ordering::SeqCst) {
        s.inflight.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    let o = f();
    s.inflight.fetch_sub(1, Ordering::SeqCst);
    s.ops.fetch_add(1, Ordering::Relaxed);
    Some(o)
}

fn producer(s: Arc<Shared>, seed: u64) {
    let mut rng = Rng::new(seed);
    while !s.stop.load(Ordering::SeqCst) {
        let (agent, cost) = AGENTS[(rng.next() % 3) as usize];
        let body = json!({"agent": agent, "cost": cost, "priority": rng.range(0, 4),
                          "payload": format!("p{}", rng.next())});
        let Some(o) = guarded(&s, || req(&s.addr, "POST", "/jobs", Some(&body))) else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        let mut l = s.led.lock().unwrap();
        let accepted = match o {
            Out::Done(201, v) => {
                l.enq
                    .insert(v["id"].as_u64().unwrap(), (agent.to_string(), cost));
                l.ok_requests += 1;
                true
            }
            Out::Done(402, _) => {
                l.seen_budget += 1;
                false
            }
            Out::Done(429, _) => {
                l.seen_quota += 1;
                false
            }
            Out::Done(503, v) if v["error"] == "backpressure" => {
                l.seen_backpressure += 1;
                false
            }
            Out::Done(c, v) => panic!("enqueue: unexpected {c} {v}"),
            Out::Ambiguous => {
                l.enq_ambiguous += 1;
                false
            }
            Out::NotSent => false,
        };
        drop(l);
        if !accepted {
            thread::sleep(Duration::from_millis(2));
        }
    }
}

fn worker(s: Arc<Shared>, seed: u64, wid: usize) {
    let mut rng = Rng::new(seed);
    let worker = format!("w{wid}");
    while !s.stop.load(Ordering::SeqCst) {
        let body = json!({"worker": worker, "wait_ms": 0, "lease_ms": 600_000});
        let Some(o) = guarded(&s, || req(&s.addr, "POST", "/lease", Some(&body))) else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        let (id, token) = match o {
            Out::Done(200, v) if v["id"].is_u64() => {
                let id = v["id"].as_u64().unwrap();
                let spec = &v["job"]["spec"];
                let info = (
                    spec["agent"].as_str().unwrap_or_default().to_string(),
                    spec["cost"].as_u64().unwrap_or(0),
                );
                s.led.lock().unwrap().leased.insert(id, info);
                (id, v["token"].as_u64().unwrap())
            }
            Out::Done(200, v) => {
                if v["miss"] == "budget_hard_stop" {
                    s.led.lock().unwrap().seen_budget += 1;
                }
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Out::Done(c, v) => panic!("lease: unexpected {c} {v}"),
            Out::Ambiguous => {
                s.led.lock().unwrap().lease_ambiguous += 1;
                continue;
            }
            Out::NotSent => continue,
        };
        {
            let mut l = s.led.lock().unwrap();
            l.max_token = l.max_token.max(token);
        }
        let path = format!("/jobs/{id}/ack");
        if rng.next() % 10 == 0 {
            // deliberate stale ack
            let bad = json!({"token": token + 1_000_000});
            match guarded(&s, || req(&s.addr, "POST", &path, Some(&bad))) {
                Some(Out::Done(409, _)) => s.led.lock().unwrap().seen_stale += 1,
                Some(Out::Done(c, v)) => panic!("stale ack: unexpected {c} {v}"),
                Some(Out::Ambiguous) => s.led.lock().unwrap().other_ambiguous += 1,
                _ => {}
            }
        }
        let good = json!({"token": token});
        // retry only when the request certainly never reached a server
        loop {
            if s.stop.load(Ordering::SeqCst) {
                return;
            }
            let Some(o) = guarded(&s, || req(&s.addr, "POST", &path, Some(&good))) else {
                thread::sleep(Duration::from_millis(5));
                continue;
            };
            let mut l = s.led.lock().unwrap();
            match o {
                Out::Done(200, v) => {
                    let ch = v["charged"].as_u64().unwrap();
                    l.acked.insert(id, ch);
                    l.acked_since_check.push(id);
                    l.ok_requests += 1;
                }
                Out::Done(c, v) => panic!("ack {id}: unexpected {c} {v}"),
                Out::Ambiguous => {
                    l.ack_pending.insert(id);
                    l.ack_ambiguous_total += 1;
                }
                Out::NotSent => {
                    drop(l);
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
            }
            break;
        }
    }
}

fn compactor(s: Arc<Shared>) {
    while !s.stop.load(Ordering::SeqCst) {
        match guarded(&s, || req(&s.addr, "POST", "/admin/compact", None)) {
            Some(Out::Done(200, _)) | Some(Out::NotSent) | None => {}
            Some(Out::Done(c, v)) => panic!("compact: unexpected {c} {v}"),
            Some(Out::Ambiguous) => s.led.lock().unwrap().other_ambiguous += 1,
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[derive(Default, Debug)]
struct LoopReport {
    kills: u64,
    ops: u64,
    enq_ok: usize,
    acked: usize,
    ack_ambiguous: u64,
    ack_ambiguous_applied: u64,
    enq_ambiguous: u64,
    lease_ambiguous: u64,
    compactions: u64,
    compaction_errors: u64,
    truncated_bytes: u64,
    restart_ms_max: u128,
    restart_ms_sum: u128,
    lost: usize,
    resurrected: usize,
    max_rss_kib: u64,
    final_stats: Value,
}

/// Verify everything after a restart while clients are parked.
fn verify(
    s: &Shared,
    srv: &Server,
    prev: &mut Option<Stats>,
    cfg: &Config,
    log_copy: &Path,
    rep: &mut LoopReport,
) {
    let st = srv.stats();
    let stats = stats_of(&st);
    let mut l = s.led.lock().unwrap();

    // resolve ambiguous acks: gone => applied
    let pending: Vec<u64> = l.ack_pending.iter().copied().collect();
    for id in pending {
        let (c, _) = call(&srv.addr, "GET", &format!("/jobs/{id}"), None);
        if c == 404 {
            let cost = l
                .leased
                .get(&id)
                .or(l.enq.get(&id))
                .map(|e| e.1)
                .unwrap_or(0);
            l.acked.insert(id, cost);
            l.acked_since_check.push(id);
            rep.ack_ambiguous_applied += 1;
        } else {
            assert_eq!(c, 200);
        }
        l.ack_pending.remove(&id);
    }
    // 1. no lost job: every 201 not known acked is live
    let outstanding: Vec<u64> = l
        .enq
        .keys()
        .filter(|id| !l.acked.contains_key(id))
        .copied()
        .collect();
    for id in &outstanding {
        let (c, v) = call(&srv.addr, "GET", &format!("/jobs/{id}"), None);
        if c != 200 {
            rep.lost += 1;
            eprintln!("LOST job {id}: {c} {v}");
        }
    }
    // 2. no resurrected ack
    for id in std::mem::take(&mut l.acked_since_check) {
        let (c, _) = call(&srv.addr, "GET", &format!("/jobs/{id}"), None);
        if c != 404 {
            rep.resurrected += 1;
            eprintln!("RESURRECTED acked job {id}: {c}");
        }
    }
    // 3. exact totals
    assert_eq!(stats.acked as usize, l.acked.len(), "acked total");
    let live = {
        let d = &st["depth"];
        ["ready", "delayed", "leased", "dead"]
            .iter()
            .map(|k| d[*k].as_u64().unwrap())
            .sum::<u64>()
    };
    assert_eq!(
        stats.enqueued - stats.acked,
        live,
        "enqueued - acked == live"
    );
    assert!(stats.enqueued as usize >= l.enq.len());
    assert!(
        stats.enqueued <= l.enq.len() as u64 + l.enq_ambiguous,
        "phantom enqueues"
    );
    // 4. spend: exact per agent, global = sum, tight cap never crossed
    let mut want: BTreeMap<String, u64> = BTreeMap::new();
    for (id, ch) in &l.acked {
        let agent = l
            .leased
            .get(id)
            .or(l.enq.get(id))
            .map(|e| e.0.clone())
            .unwrap_or_default();
        *want.entry(agent).or_default() += ch;
    }
    let mut global = 0;
    for (agent, _) in AGENTS {
        let a = &st["agents"][agent];
        let spent = a["spent"].as_u64().unwrap_or(0);
        assert_eq!(
            spent,
            want.get(agent).copied().unwrap_or(0),
            "spent {agent}"
        );
        global += spent;
    }
    assert_eq!(
        st["global"]["spent"].as_u64().unwrap(),
        global,
        "global spent"
    );
    let t = &st["agents"]["tight"];
    assert_eq!(t["cap"].as_u64(), Some(TIGHT_CAP), "tight cap survived");
    assert!(
        t["spent"].as_u64().unwrap() + t["reserved"].as_u64().unwrap() <= TIGHT_CAP,
        "hard cap crossed"
    );
    assert_eq!(
        st["agents"]["qq"]["quota"],
        json!([QQ_PENDING, 2]),
        "qq quota survived"
    );
    // 5. refusal counters: never backwards, and within what clients saw
    if let Some(p) = prev.as_ref() {
        for (n, a, b) in [
            ("budget", p.budget_refusals, stats.budget_refusals),
            ("quota", p.quota_refusals, stats.quota_refusals),
            (
                "backpressure",
                p.backpressure_refusals,
                stats.backpressure_refusals,
            ),
            ("stale", p.stale_acks, stats.stale_acks),
        ] {
            assert!(
                b >= a,
                "{n} refusal counter went backwards across restart: {a} -> {b}"
            );
        }
    }
    let amb = l.enq_ambiguous + l.lease_ambiguous + l.other_ambiguous;
    for (n, seen, got) in [
        ("budget", l.seen_budget, stats.budget_refusals),
        ("quota", l.seen_quota, stats.quota_refusals),
        (
            "backpressure",
            l.seen_backpressure,
            stats.backpressure_refusals,
        ),
        ("stale", l.seen_stale, stats.stale_acks),
    ] {
        assert!(
            got >= seen && got <= seen + amb,
            "{n} refusals: server {got}, clients saw {seen} (+{amb} unanswered)"
        );
    }
    *prev = Some(stats.clone());
    // 6. the restarted server equals an independent replay of the log the
    // killed process left
    let (eng, _) = replay_file(log_copy, cfg.clone());
    assert_eq!(
        eng.stats, stats,
        "binary stats != library replay of the dead process's log"
    );
    assert_eq!(eng.live_jobs() as u64, live);
    // 7. tokens stay fenced: a fresh lease token is above anything seen
    rep.truncated_bytes += st["store"]["truncated_bytes"].as_u64().unwrap_or(0);
    rep.enq_ok = l.enq.len();
    rep.acked = l.acked.len();
    drop(l);
}

fn kill_loop(name: &str, port: u16, compact_min: u64, hammer: bool) {
    let kills = env_num("SOAK_KILLS", 40);
    let seed = seed();
    let dir = workdir(name);
    let data = dir.join("data");
    let errlog = dir.join("server.err");
    let addr = format!("127.0.0.1:{port}");
    let cm = compact_min.to_string();
    let extra = ["--compact-min", cm.as_str(), "--max-depth", "400"];
    let cfg = Config {
        max_depth: 400,
        ..Config::default()
    };
    eprintln!(
        "{} {name}: seed {seed} kills {kills} compact_min {compact_min} hammer {hammer} bin {}",
        stamp(),
        bin()
    );
    let (mut srv, _) = start(&addr, &data, &extra, &errlog);
    call(
        &addr,
        "POST",
        "/caps",
        Some(json!({"scope":"tight","cap":TIGHT_CAP})),
    );
    call(
        &addr,
        "POST",
        "/quotas",
        Some(json!({"agent":"qq","max_pending":QQ_PENDING,"max_inflight":2})),
    );
    let s = Arc::new(Shared {
        addr: addr.clone(),
        led: Mutex::new(Ledger::default()),
        paused: AtomicBool::new(false),
        stop: AtomicBool::new(false),
        inflight: AtomicUsize::new(0),
        ops: AtomicU64::new(0),
    });
    let mut threads = Vec::new();
    for i in 0..4 {
        let sc = s.clone();
        threads.push(thread::spawn(move || producer(sc, seed ^ (i + 1) * 7919)));
    }
    for i in 0..4 {
        let sc = s.clone();
        threads.push(thread::spawn(move || {
            worker(sc, seed ^ (i as u64 + 11) * 104_729, i)
        }));
    }
    if hammer {
        let sc = s.clone();
        threads.push(thread::spawn(move || compactor(sc)));
    }
    let mut rng = Rng::new(seed ^ 0xABCD);
    let mut rep = LoopReport::default();
    let mut prev: Option<Stats> = None;
    let log_copy = dir.join("log-at-kill");
    let t_all = Instant::now();
    for k in 0..kills {
        thread::sleep(Duration::from_millis(rng.range(150, 650)));
        let pre = srv.stats();
        rep.compactions += pre["store"]["compactions"].as_u64().unwrap_or(0);
        rep.compaction_errors += pre["store"]["compaction_errors"].as_u64().unwrap_or(0);
        rep.max_rss_kib = rep.max_rss_kib.max(rss_kib(srv.pid).0);
        let in_ack = s.inflight.load(Ordering::SeqCst);
        srv.kill9();
        s.paused.store(true, Ordering::SeqCst);
        let q0 = Instant::now();
        while s.inflight.load(Ordering::SeqCst) > 0 {
            assert!(
                q0.elapsed() < Duration::from_secs(15),
                "clients did not quiesce"
            );
            thread::sleep(Duration::from_millis(2));
        }
        std::fs::copy(data.join("queue.log"), &log_copy).unwrap();
        let (n, took) = start(&addr, &data, &extra, &errlog);
        srv = n;
        rep.kills += 1;
        rep.restart_ms_max = rep.restart_ms_max.max(took.as_millis());
        rep.restart_ms_sum += took.as_millis();
        let max_token_before = s.led.lock().unwrap().max_token;
        verify(&s, &srv, &mut prev, &cfg, &log_copy, &mut rep);
        assert_eq!(rep.lost, 0, "lost jobs after kill {k}");
        assert_eq!(rep.resurrected, 0, "resurrected acks after kill {k}");
        // fencing: the next token must be above every token handed out
        // before the kill (probe through a throwaway queue)
        let probe = json!({"queue":"probe","agent":"probe","payload":"fence"});
        let (c, pj) = call(&addr, "POST", "/jobs", Some(probe));
        assert_eq!(c, 201);
        let (_, pl) = call(
            &addr,
            "POST",
            "/lease",
            Some(json!({"worker":"fence","queue":"probe"})),
        );
        let tok = pl["token"].as_u64().unwrap();
        assert!(
            tok > max_token_before,
            "token {tok} not above {max_token_before} after restart"
        );
        let (c, _) = call(
            &addr,
            "POST",
            &format!("/jobs/{}/ack", pj["id"]),
            Some(json!({"token": tok})),
        );
        assert_eq!(c, 200);
        {
            // the probe counts toward the ledger as an enqueue + ack
            let mut l = s.led.lock().unwrap();
            let id = pj["id"].as_u64().unwrap();
            l.enq.insert(id, ("probe".into(), 0));
            l.acked.insert(id, 0);
            l.acked_since_check.push(id);
        }
        prev = Some(stats_of(&srv.stats()));
        if k % 10 == 9 {
            eprintln!(
                "{} {name}: kill {} ok, {in_ack} requests in flight at kill, ops {}",
                stamp(),
                k + 1,
                s.ops.load(Ordering::Relaxed)
            );
        }
        s.paused.store(false, Ordering::SeqCst);
    }
    s.stop.store(true, Ordering::SeqCst);
    for t in threads {
        t.join().unwrap();
    }
    // final: every acked id is gone, every other 201 is live
    let fin = srv.stats();
    rep.compactions += fin["store"]["compactions"].as_u64().unwrap_or(0);
    rep.compaction_errors += fin["store"]["compaction_errors"].as_u64().unwrap_or(0);
    srv.kill9();
    s.paused.store(true, Ordering::SeqCst);
    std::fs::copy(data.join("queue.log"), &log_copy).unwrap();
    let (srv, _) = start(&addr, &data, &extra, &errlog);
    verify(&s, &srv, &mut prev, &cfg, &log_copy, &mut rep);
    let all_acked: Vec<u64> = s.led.lock().unwrap().acked.keys().copied().collect();
    for id in all_acked {
        let (c, _) = call(&addr, "GET", &format!("/jobs/{id}"), None);
        if c != 404 {
            rep.resurrected += 1;
        }
    }
    {
        let l = s.led.lock().unwrap();
        rep.ack_ambiguous = l.ack_ambiguous_total;
        rep.enq_ambiguous = l.enq_ambiguous;
        rep.lease_ambiguous = l.lease_ambiguous;
        rep.final_stats = json!({"stats": fin["stats"], "global": fin["global"], "seen": {
            "budget": l.seen_budget, "quota": l.seen_quota, "backpressure": l.seen_backpressure,
            "stale": l.seen_stale}});
    }
    rep.ops = s.ops.load(Ordering::Relaxed);
    srv.kill9();
    let err = std::fs::read_to_string(&errlog).unwrap_or_default();
    eprintln!(
        "{} {name}: RESULT kills {} in {:.1} s, ops {}, enq_ok {}, acked {}, ack_ambiguous {} (applied {}), enq_ambiguous {}, lease_ambiguous {}, compactions {} (errors {}), truncated_bytes {}, restart max {} ms avg {} ms, max RSS {} KiB, lost {}, resurrected {}, server stderr lines {}\n  final {}",
        stamp(), rep.kills, t_all.elapsed().as_secs_f64(), rep.ops, rep.enq_ok, rep.acked, rep.ack_ambiguous,
        rep.ack_ambiguous_applied, rep.enq_ambiguous, rep.lease_ambiguous, rep.compactions, rep.compaction_errors,
        rep.truncated_bytes, rep.restart_ms_max, rep.restart_ms_sum / u128::from(rep.kills.max(1)), rep.max_rss_kib,
        rep.lost, rep.resurrected, err.lines().count(), rep.final_stats
    );
    assert_eq!(rep.lost, 0);
    assert_eq!(rep.resurrected, 0);
    assert_eq!(rep.compaction_errors, 0, "compaction errors: {err}");
    assert!(rep.compactions > 0, "no compaction happened under load");
    assert!(rep.acked > 100, "too little work done: {}", rep.acked);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_kill9_loop_during_ack() {
    if real() {
        kill_loop("kill9", 20761, 300, false);
    }
}

#[test]
fn real_kill9_loop_compaction_hammer() {
    if real() {
        kill_loop("compact", 20762, 100, true);
    }
}

// ------------------------------------------------------------ torn tail

#[test]
fn real_torn_tail() {
    if !real() {
        return;
    }
    let seed = seed();
    let mut rng = Rng::new(seed);
    let dir = workdir("torn");
    let data = dir.join("data");
    let errlog = dir.join("server.err");
    let addr = "127.0.0.1:20763";
    // build a real log: caps, quotas, enqueues, leases, acks, nacks
    let (srv, _) = start(addr, &data, &["--backoff-base-ms", "5"], &errlog);
    call(addr, "POST", "/caps", Some(json!({"scope":"t","cap":1000})));
    call(
        addr,
        "POST",
        "/quotas",
        Some(json!({"agent":"t","max_pending":500,"max_inflight":50})),
    );
    for i in 0..120 {
        let (c, _) = call(
            addr,
            "POST",
            "/jobs",
            Some(json!({"agent":"t","cost":3,"priority": i % 5})),
        );
        assert_eq!(c, 201);
    }
    for i in 0..60 {
        let (_, l) = call(addr, "POST", "/lease", Some(json!({"worker":"w"})));
        let action = if i % 4 == 0 { "nack" } else { "ack" };
        let (c, _) = call(
            addr,
            "POST",
            &format!("/jobs/{}/{action}", l["id"]),
            Some(json!({"token": l["token"]})),
        );
        assert_eq!(c, 200);
    }
    srv.kill9();
    let log = std::fs::read(data.join("queue.log")).unwrap();
    let ends: Vec<usize> = log
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .map(|(i, _)| i + 1)
        .collect();
    let cfg = Config {
        backoff_base_ms: 5,
        ..Config::default()
    };
    eprintln!(
        "{} torn: seed {seed}, log {} bytes, {} lines",
        stamp(),
        log.len(),
        ends.len()
    );

    // cases: (label, bytes)
    let mut cases: Vec<(String, Vec<u8>)> = Vec::new();
    for _ in 0..30 {
        let cut = rng.range(1, log.len() as u64) as usize;
        cases.push((format!("cut@{cut}"), log[..cut].to_vec()));
    }
    for back in 1..=5 {
        // cut inside each of the last 5 lines
        let s = ends[ends.len() - back - 1];
        let e = ends[ends.len() - back];
        let cut = s + (e - s) / 2;
        cases.push((
            format!("mid-line-{back}-from-end@{cut}"),
            log[..cut].to_vec(),
        ));
        cases.push((
            format!("no-newline-{back}@{}", e - 1),
            log[..e - 1].to_vec(),
        ));
    }
    let mut g = log.clone();
    g.extend_from_slice(&[0xff, 0xfe, 0x00, b'{', 0x80]);
    cases.push(("garbage-tail".into(), g));
    let mut z = log.clone();
    z.extend_from_slice(&[0u8; 4096]);
    cases.push(("zero-block-4k".into(), z));
    let mut h = log.clone();
    h.extend_from_slice(b"deadbeef {\"t\":\"ack\",\"id\":1,");
    cases.push(("half-json".into(), h));
    let mut bc = log.clone();
    bc.extend_from_slice(b"00000000 {\"t\":\"cap\",\"scope\":\"t\",\"cap\":1}\n");
    cases.push(("bad-crc-complete-last-line".into(), bc));

    let mut n_ok = 0;
    for (i, (label, bytes)) in cases.iter().enumerate() {
        let d = dir.join(format!("case{i}"));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("queue.log");
        std::fs::write(&p, bytes).unwrap();
        let (want, want_lines) = replay_file(&p, cfg.clone());
        let (srv, took) = start(addr, &d, &["--backoff-base-ms", "5"], &errlog);
        let st = srv.stats();
        let replayed = st["store"]["replayed"].as_u64().unwrap() as usize;
        let cut = st["store"]["truncated_bytes"].as_u64().unwrap() as usize;
        assert_eq!(replayed, want_lines, "{label}: replayed");
        let good_end = if want_lines == 0 {
            0
        } else {
            ends[want_lines - 1]
        };
        assert_eq!(
            cut,
            bytes.len() - good_end.min(bytes.len()),
            "{label}: truncated bytes"
        );
        assert_eq!(
            stats_of(&st),
            want.stats,
            "{label}: stats != library replay"
        );
        // the repaired log takes new writes and replays cleanly again
        let (c, _) = call(
            addr,
            "POST",
            "/jobs",
            Some(json!({"agent":"t","payload":"after-repair"})),
        );
        assert_eq!(c, 201, "{label}: enqueue after repair");
        let enq = stats_of(&srv.stats()).enqueued;
        srv.kill9();
        let (srv, _) = start(addr, &d, &["--backoff-base-ms", "5"], &errlog);
        let st2 = srv.stats();
        assert_eq!(
            st2["store"]["truncated_bytes"], 0,
            "{label}: second open still torn"
        );
        assert_eq!(
            stats_of(&st2).enqueued,
            enq,
            "{label}: after-repair job lost"
        );
        srv.kill9();
        n_ok += 1;
        if i % 10 == 0 || i + 1 == cases.len() {
            eprintln!(
                "{} torn: {label}: replayed {replayed}/{} lines, cut {cut} B, start {} ms",
                stamp(),
                ends.len(),
                took.as_millis()
            );
        }
    }
    // a bad line with good lines after it is corruption: refuse to start
    let d = dir.join("mid-corrupt");
    std::fs::create_dir_all(&d).unwrap();
    let mut m = log.clone();
    let at = ends[ends.len() / 2] + 3; // inside the crc of a middle line
    m[at] = if m[at] == b'0' { b'1' } else { b'0' };
    std::fs::write(d.join("queue.log"), &m).unwrap();
    let before = std::fs::read(d.join("queue.log")).unwrap();
    let mut child = spawn("127.0.0.1:20764", &d, &[], &errlog).unwrap();
    let t0 = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().unwrap() {
            break st;
        }
        if t0.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            panic!("server started on a corrupt log");
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success(), "corrupt middle line must fail the start");
    assert!(http::request(
        "127.0.0.1:20764",
        "GET",
        "/healthz",
        None,
        Duration::from_millis(200)
    )
    .is_err());
    assert_eq!(
        std::fs::read(d.join("queue.log")).unwrap(),
        before,
        "refused start modified the log"
    );
    let err = std::fs::read_to_string(&errlog).unwrap_or_default();
    assert!(err.contains("refusing to start"), "stderr: {err}");
    eprintln!("{} torn: RESULT {n_ok}/{} torn/garbage cases replayed exactly and kept working; mid-log corruption refused ({status}), log untouched", stamp(), cases.len());
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ flock

#[test]
fn real_flock_two_servers() {
    if !real() {
        return;
    }
    let dir = workdir("flock");
    let data = dir.join("data");
    let errlog = dir.join("server.err");
    let (a, _) = start("127.0.0.1:20765", &data, &[], &errlog);
    for _ in 0..20 {
        call(&a.addr, "POST", "/jobs", Some(json!({"payload":"x"})));
    }
    let log = data.join("queue.log");
    let before = std::fs::read(&log).unwrap();
    // 8 rival servers at once, each on its own port
    let rivals: Vec<(u16, Child)> = (0..8)
        .map(|i| {
            let port = 20766 + (i % 2) as u16;
            let el = dir.join(format!("rival{i}.err"));
            (
                port,
                spawn(&format!("127.0.0.1:{port}"), &data, &[], &el).unwrap(),
            )
        })
        .collect();
    let mut refused = 0;
    for (i, (port, mut c)) in rivals.into_iter().enumerate() {
        let t0 = Instant::now();
        let st = loop {
            if let Some(st) = c.try_wait().unwrap() {
                break st;
            }
            if t0.elapsed() > Duration::from_secs(5) {
                let _ = c.kill();
                let _ = c.wait();
                panic!("rival {i} on :{port} kept running on a locked data dir");
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(!st.success());
        let e = std::fs::read_to_string(dir.join(format!("rival{i}.err"))).unwrap();
        assert!(e.contains("in use by another process"), "rival {i}: {e}");
        refused += 1;
    }
    for p in [20766, 20767] {
        assert!(
            http::request(
                &format!("127.0.0.1:{p}"),
                "GET",
                "/healthz",
                None,
                Duration::from_millis(200)
            )
            .is_err(),
            ":{p} bound by a refused rival"
        );
    }
    assert_eq!(
        std::fs::read(&log).unwrap(),
        before,
        "a refused rival touched the log"
    );
    let st = a.stats();
    assert_eq!(st["depth"]["ready"], 20);
    // the first server is unharmed
    assert_eq!(call(&a.addr, "POST", "/jobs", Some(json!({}))).0, 201);
    // SIGKILL frees the lock at once: no stale lock
    a.kill9();
    let t0 = Instant::now();
    let (b, took) = start("127.0.0.1:20766", &data, &[], &errlog);
    assert_eq!(b.stats()["depth"]["ready"], 21);
    b.kill9();
    eprintln!("{} flock: RESULT {refused}/8 rivals refused (\"in use by another process\"), none bound, log bytes unchanged; after SIGKILL a new server started in {} ms ({} ms incl. kill), 21/21 jobs replayed", stamp(), took.as_millis(), t0.elapsed().as_millis());
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ soak

#[test]
fn real_soak_10k() {
    if !real() {
        return;
    }
    let jobs = env_num("SOAK_JOBS", 10_000);
    let rounds = env_num("SOAK_ROUNDS", 3);
    let dir = workdir("soak");
    let data = dir.join("data");
    let errlog = dir.join("server.err");
    let addr = "127.0.0.1:20768".to_string();
    let (srv, _) = start(&addr, &data, &[], &errlog);
    let pid = Arc::new(AtomicU64::new(u64::from(srv.pid)));
    let samples: Arc<Mutex<Vec<(u64, u64, &'static str)>>> = Arc::new(Mutex::new(Vec::new()));
    let phase: Arc<Mutex<&'static str>> = Arc::new(Mutex::new("idle"));
    let stop = Arc::new(AtomicBool::new(false));
    let t_start = Instant::now();
    let sampler = {
        let (pid, samples, phase, stop) =
            (pid.clone(), samples.clone(), phase.clone(), stop.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (rss, _) = rss_kib(pid.load(Ordering::SeqCst) as u32);
                let ph = *phase.lock().unwrap();
                samples
                    .lock()
                    .unwrap()
                    .push((t_start.elapsed().as_millis() as u64, rss, ph));
                thread::sleep(Duration::from_millis(250));
            }
        })
    };
    let srv = Arc::new(Mutex::new(Some(srv)));
    let mut round_rows = Vec::new();
    let mut acked_total = 0u64;
    let mut kill_row = Value::Null;
    for round in 0..rounds {
        let rss0 = rss_kib(pid.load(Ordering::SeqCst) as u32).0;
        *phase.lock().unwrap() = "enqueue";
        let t0 = Instant::now();
        let ids: Arc<Mutex<BTreeSet<u64>>> = Arc::new(Mutex::new(BTreeSet::new()));
        let per = jobs / 4;
        let hs: Vec<_> = (0..4)
            .map(|p| {
                let (addr, ids) = (addr.clone(), ids.clone());
                let n = if p == 3 { jobs - per * 3 } else { per };
                thread::spawn(move || {
                    for i in 0..n {
                        let b = json!({"agent": format!("a{}", i % 8), "cost": 1, "priority": i % 4,
                                       "payload": format!("round {round} job {i} {}", "x".repeat(64))});
                        let (c, v) = call(&addr, "POST", "/jobs", Some(b));
                        assert_eq!(c, 201, "{v}");
                        ids.lock().unwrap().insert(v["id"].as_u64().unwrap());
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        let enq_s = t0.elapsed().as_secs_f64();
        let rss_full = rss_kib(pid.load(Ordering::SeqCst) as u32).0;
        *phase.lock().unwrap() = "drain";
        let t1 = Instant::now();
        let acked = Arc::new(Mutex::new(BTreeSet::new()));
        let kill_at = if round == 1 { jobs / 2 } else { u64::MAX };
        let draining = Arc::new(AtomicBool::new(true));
        let paused = Arc::new(AtomicBool::new(false));
        let hs: Vec<_> = (0..8)
            .map(|w| {
                let (addr, acked, draining, paused) = (
                    addr.clone(),
                    acked.clone(),
                    draining.clone(),
                    paused.clone(),
                );
                thread::spawn(move || {
                    while draining.load(Ordering::SeqCst) {
                        if paused.load(Ordering::SeqCst) {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        let body = json!({"worker": format!("s{w}"), "wait_ms": 200});
                        let Out::Done(200, l) = req(&addr, "POST", "/lease", Some(&body)) else {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        };
                        let Some(id) = l["id"].as_u64() else { continue };
                        let p = format!("/jobs/{id}/ack");
                        if let Out::Done(200, _) =
                            req(&addr, "POST", &p, Some(&json!({"token": l["token"]})))
                        {
                            acked.lock().unwrap().insert(id);
                        }
                    }
                })
            })
            .collect();
        loop {
            let n = acked.lock().unwrap().len() as u64;
            if n >= kill_at && kill_row.is_null() {
                // SIGKILL mid-drain, then a full no-loss check
                paused.store(true, Ordering::SeqCst);
                let s = srv.lock().unwrap().take().unwrap();
                let rss_before = rss_kib(s.pid).0;
                s.kill9();
                thread::sleep(Duration::from_millis(300)); // let in-flight requests fail
                let log_bytes = std::fs::metadata(data.join("queue.log"))
                    .map(|m| m.len())
                    .unwrap_or(0);
                let (ns, took) = start(&addr, &data, &[], &errlog);
                pid.store(u64::from(ns.pid), Ordering::SeqCst);
                let st = ns.stats();
                let done = acked.lock().unwrap().clone();
                let all = ids.lock().unwrap().clone();
                let mut lost = 0;
                let mut live = 0;
                for id in all.iter().filter(|i| !done.contains(i)) {
                    match call(&addr, "GET", &format!("/jobs/{id}"), None).0 {
                        200 => live += 1,
                        _ => lost += 1,
                    }
                }
                let mut resurrected = 0;
                for id in &done {
                    if call(&addr, "GET", &format!("/jobs/{id}"), None).0 != 404 {
                        resurrected += 1;
                    }
                }
                let rss_after = rss_kib(ns.pid).0;
                kill_row = json!({"round": round + 1, "acked_before_kill": done.len(), "live_after": live,
                    "lost": lost, "resurrected": resurrected, "replayed_records": st["store"]["replayed"],
                    "log_bytes": log_bytes, "restart_ms": took.as_millis() as u64,
                    "rss_kib_before_kill": rss_before, "rss_kib_after_replay": rss_after,
                    "truncated_bytes": st["store"]["truncated_bytes"]});
                eprintln!("{} soak: SIGKILL mid-drain {kill_row}", stamp());
                assert_eq!(lost, 0, "lost jobs after SIGKILL");
                assert_eq!(resurrected, 0, "resurrected acks after SIGKILL");
                *srv.lock().unwrap() = Some(ns);
                paused.store(false, Ordering::SeqCst);
            }
            // drained when the server says nothing is left
            let (c, st) = call(&addr, "GET", "/stats", None);
            assert_eq!(c, 200);
            let d = &st["depth"];
            if d["ready"] == 0 && d["delayed"] == 0 && d["leased"] == 0 && n >= kill_at.min(jobs) {
                break;
            }
            if d["ready"] == 0 && d["delayed"] == 0 && d["leased"] != 0 {
                // jobs leased by a request the kill cut: wait for nothing,
                // they are live, count them and stop draining this round
                let leased = d["leased"].as_u64().unwrap();
                if t1.elapsed() > Duration::from_secs(5) && leased <= 16 {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        draining.store(false, Ordering::SeqCst);
        for h in hs {
            h.join().unwrap();
        }
        let drain_s = t1.elapsed().as_secs_f64();
        let pidv = pid.load(Ordering::SeqCst) as u32;
        let (rss_end, hwm) = rss_kib(pidv);
        let st = srv.lock().unwrap().as_ref().unwrap().stats();
        let n = acked.lock().unwrap().len() as u64;
        acked_total += n;
        let row = json!({"round": round + 1, "jobs": jobs, "acked": n, "enqueue_s": format!("{enq_s:.2}"),
            "drain_s": format!("{drain_s:.2}"), "rss_kib_start": rss0, "rss_kib_full": rss_full,
            "rss_kib_drained": rss_end, "hwm_kib": hwm, "store_lines": st["store"]["lines"],
            "compactions": st["store"]["compactions"], "leased_left": st["depth"]["leased"]});
        eprintln!("{} soak: {row}", stamp());
        round_rows.push(row);
        *phase.lock().unwrap() = "idle";
    }
    stop.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    let s = srv.lock().unwrap().take().unwrap();
    let st = s.stats();
    s.kill9();
    let samples = samples.lock().unwrap().clone();
    let peak = samples.iter().map(|s| s.1).max().unwrap_or(0);
    let csv: String = samples
        .iter()
        .map(|(t, r, p)| format!("{t},{r},{p}\n"))
        .collect();
    let out =
        std::env::var("SOAK_OUT").unwrap_or_else(|_| dir.parent().unwrap().display().to_string());
    let csv_path = Path::new(&out).join("poseidon-206-soak-rss.csv");
    let _ = std::fs::write(&csv_path, format!("ms,rss_kib,phase\n{csv}"));
    // bounded: the last round must not hold notably more than the first
    let full = |i: usize| round_rows[i]["rss_kib_full"].as_u64().unwrap();
    let drained = |i: usize| round_rows[i]["rss_kib_drained"].as_u64().unwrap();
    let last = round_rows.len() - 1;
    eprintln!(
        "{} soak: RESULT {} rounds x {jobs} jobs, acked {acked_total}, peak RSS {peak} KiB, full r1 {} / r{} {} KiB, drained r1 {} / r{} {} KiB, {} samples -> {}, final stats {}",
        stamp(), round_rows.len(), full(0), last + 1, full(last), drained(0), last + 1, drained(last),
        samples.len(), csv_path.display(), json!({"stats": st["stats"], "depth": st["depth"]})
    );
    assert!(
        full(last) <= full(0) * 3 / 2 + 4096,
        "RSS with {jobs} live jobs grew across rounds"
    );
    assert!(
        drained(last) <= drained(0) * 3 / 2 + 4096,
        "drained RSS grew across rounds"
    );
    assert!(peak < 512 * 1024, "peak RSS {peak} KiB over 512 MiB");
    assert!(
        !kill_row.is_null() || rounds < 2,
        "mid-drain kill did not run"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
