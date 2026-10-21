// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for security S3.4 (slot 219, sec-ledger-race): the replay
//! ledger of the built `hades-gate` binary stays single-use when several
//! processes present the same token at the same instant, and fails closed on a
//! write that the kernel cuts short (a full-disk style short write).
//!
//! Runs only with `GALION_REAL=1`; build with `--release` for the real release
//! binary. Binds 127.0.0.1:20890 and :20891 (slot 219 range 20890-20899;
//! `HADES_REAL_RACE_PORT` moves the base). Throwaway keys in a temp dir; token
//! strings are never printed.
//!
//! Start barrier: every `check` process reads its token from its own FIFO, so
//! it loads the config, the admin key and the ledger, then blocks in `open()`
//! of the FIFO. The test opens all write ends (that only succeeds once each
//! reader is waiting), writes the token into each and closes them together:
//! all processes reach the ledger within microseconds of each other.
//!
//! Builds on slot 125 (hades-ledger-fail): the stderr lines, exit 3 and
//! `/health` 503 for a failing ledger are tested there and not repeated here.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_hades-gate");
const PLAN: &str = "restart galion-tuner";
/// `O_NONBLOCK` on Linux x86_64 and aarch64 (std has no named constant).
const O_NONBLOCK: i32 = 0o4000;

fn enabled() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn port_base() -> u16 {
    std::env::var("HADES_REAL_RACE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(20890)
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn json(b: &[u8]) -> serde_json::Value {
    serde_json::from_slice(b).unwrap_or(serde_json::Value::Null)
}

fn now_zagreb() -> String {
    zagreb_time::now_rfc3339()
}

/// Temp dir, removed on drop.
struct Gate {
    dir: PathBuf,
    seed: PathBuf,
    cfg: PathBuf,
    plan: PathBuf,
    ledger: PathBuf,
    minted: std::cell::Cell<u32>,
}

impl Drop for Gate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Gate {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "hades-real-ledger-race-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (seed, public) = (dir.join("admin.seed"), dir.join("admin.pub"));
        let o = run(&["keygen", "--seed", s(&seed), "--pub", s(&public)]);
        assert!(o.status.success(), "{}", text(&o.stderr));
        let ledger = dir.join("ledger.jsonl");
        let cfg = dir.join("hades.toml");
        std::fs::write(
            &cfg,
            format!(
                "admin_pubkey = \"{}\"\nledger = \"{}\"\ndisable_file = \"{}\"\n",
                s(&public),
                s(&ledger),
                s(&dir.join("DISABLED"))
            ),
        )
        .unwrap();
        let plan = dir.join("plan.txt");
        std::fs::write(&plan, PLAN).unwrap();
        Self {
            dir,
            seed,
            cfg,
            plan,
            ledger,
            minted: std::cell::Cell::new(0),
        }
    }

    /// Mint a fresh token; returns (token text, nonce hex).
    fn mint(&self) -> (String, String) {
        let n = self.minted.get() + 1;
        self.minted.set(n);
        let out = self.dir.join(format!("t{n}.tok"));
        let o = run(&[
            "mint",
            "--seed",
            s(&self.seed),
            "--action",
            "heal-apply",
            "--target",
            "galion-tuner",
            "--plan-file",
            s(&self.plan),
            "--ttl",
            "600",
            "--out",
            s(&out),
        ]);
        assert!(o.status.success(), "{}", text(&o.stderr));
        let nonce = json(&o.stdout)["nonce"].as_str().unwrap().to_owned();
        let token = std::fs::read_to_string(&out).unwrap();
        std::fs::remove_file(&out).unwrap();
        (token, nonce)
    }

    fn check_args<'a>(&'a self, token_file: &'a Path) -> Vec<&'a str> {
        vec![
            "check",
            "--config",
            s(&self.cfg),
            "--action",
            "heal-apply",
            "--target",
            "galion-tuner",
            "--plan-file",
            s(&self.plan),
            "--token-file",
            s(token_file),
        ]
    }

    /// Start `n` `check` processes, each blocked on its own token FIFO.
    fn spawn_blocked(&self, round: usize, n: usize) -> Vec<(PathBuf, Child)> {
        (0..n)
            .map(|i| {
                let fifo = self.dir.join(format!("r{round}-p{i}.fifo"));
                let st = Command::new("mkfifo")
                    .args(["-m", "600", s(&fifo)])
                    .status()
                    .unwrap();
                assert!(st.success(), "mkfifo");
                let child = Command::new(BIN)
                    .args(self.check_args(&fifo))
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                (fifo, child)
            })
            .collect()
    }

    fn ledger_lines(&self) -> Vec<serde_json::Value> {
        let t = std::fs::read_to_string(&self.ledger).unwrap();
        assert!(t.is_empty() || t.ends_with('\n'), "torn ledger tail");
        t.lines()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|_| panic!("bad ledger line {l:?}")))
            .collect()
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("run hades-gate")
}

/// Open every FIFO write end (waits until each process is blocked reading it).
fn open_writers(fifos: &[PathBuf]) -> Vec<File> {
    let deadline = Instant::now() + Duration::from_secs(60);
    fifos
        .iter()
        .map(|f| loop {
            match OpenOptions::new()
                .write(true)
                .custom_flags(O_NONBLOCK)
                .open(f)
            {
                Ok(w) => break w,
                // ENXIO: the reader has not opened the FIFO yet.
                Err(e) if e.raw_os_error() == Some(6) && Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) => panic!("open {}: {e}", f.display()),
            }
        })
        .collect()
}

/// Release all blocked processes: write the token into every FIFO, then close
/// them back to back.
fn release(writers: Vec<File>, token: &str) -> Instant {
    let mut writers = writers;
    for w in &mut writers {
        w.write_all(token.as_bytes()).unwrap();
    }
    let at = Instant::now();
    drop(writers);
    at
}

#[derive(Debug, Default)]
struct Tally {
    approved: usize,
    replayed: usize,
    other: Vec<String>,
}

fn tally_cli(children: Vec<(PathBuf, Child)>, wins: &mut Vec<usize>) -> Tally {
    let mut t = Tally::default();
    for (i, (_, c)) in children.into_iter().enumerate() {
        let o = c.wait_with_output().unwrap();
        let v = json(&o.stdout);
        match (o.status.code(), v["decision"].as_str(), v["code"].as_str()) {
            (Some(0), Some("WouldApply"), _) => {
                t.approved += 1;
                wins.push(i);
            }
            (Some(2), Some("Refused"), Some("replayed")) => t.replayed += 1,
            _ => t.other.push(format!(
                "p{i}: exit {:?} signal {:?} stdout {} stderr {}",
                o.status.code(),
                o.status.signal(),
                text(&o.stdout).trim(),
                text(&o.stderr).trim()
            )),
        }
    }
    t
}

/// Many processes of the real binary race one token; exactly one wins.
#[test]
fn real_ledger_single_use_under_process_race() {
    if !enabled() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const ROUNDS: usize = 40;
    const PROCS: usize = 12;
    let g = Gate::new("cli");
    eprintln!("start {} bin {BIN}", now_zagreb());
    assert!(!g.ledger.exists(), "round 0 races a ledger that does not exist yet");
    let started = Instant::now();
    let mut wins = Vec::new();
    let mut nonces = Vec::new();
    let mut spread = Duration::ZERO;
    for round in 0..ROUNDS {
        let (token, nonce) = g.mint();
        let kids = g.spawn_blocked(round, PROCS);
        let fifos: Vec<PathBuf> = kids.iter().map(|(f, _)| f.clone()).collect();
        let writers = open_writers(&fifos);
        let at = release(writers, &token);
        let t = tally_cli(kids, &mut wins);
        spread = spread.max(at.elapsed());
        assert!(t.other.is_empty(), "round {round}: {:?}", t.other);
        assert_eq!(
            (t.approved, t.replayed),
            (1, PROCS - 1),
            "round {round}: exactly one approval"
        );
        nonces.push(nonce);
    }
    let lines = g.ledger_lines();
    let mut consumed: HashMap<String, usize> = HashMap::new();
    let mut refused_replay = 0;
    for l in &lines {
        match l["event"].as_str() {
            Some("consumed") => *consumed.entry(l["nonce"].as_str().unwrap().into()).or_default() += 1,
            Some("refused") if l["reason"].as_str().unwrap_or("").starts_with("replayed") => {
                refused_replay += 1;
            }
            other => panic!("unexpected ledger event {other:?}"),
        }
    }
    for n in &nonces {
        assert_eq!(consumed.get(n), Some(&1), "nonce consumed exactly once");
    }
    assert_eq!(consumed.len(), ROUNDS);
    assert_eq!(refused_replay, ROUNDS * (PROCS - 1));
    let mut per_slot = [0usize; PROCS];
    for w in &wins {
        per_slot[*w] += 1;
    }
    eprintln!(
        "process race: {ROUNDS} rounds x {PROCS} processes = {} checks; approvals {} (1 per round); \
         replay refusals {refused_replay}; ledger lines {} all well-formed; \
         winner by spawn slot {per_slot:?}; longest release-to-all-exited {spread:?}; elapsed {:?}; end {}",
        ROUNDS * PROCS,
        wins.len(),
        lines.len(),
        started.elapsed(),
        now_zagreb()
    );
    // A winner that is not always the first-spawned process shows the
    // processes really overlapped (the lock, not spawn order, decided).
    assert!(
        per_slot.iter().filter(|c| **c > 0).count() > 1,
        "winner always the same slot: {per_slot:?}"
    );
}

struct Server(Option<Child>);
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn http(port: u16, method: &str, path: &str, body: &str) -> String {
    let mut c = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        c,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    out
}

fn body_of(resp: &str) -> serde_json::Value {
    serde_json::from_str(resp.split("\r\n\r\n").nth(1).unwrap_or("")).unwrap_or_default()
}

fn approve_body(token: &str) -> String {
    serde_json::json!({
        "action": "heal-apply", "target": "galion-tuner", "plan": PLAN, "token": token,
    })
    .to_string()
}

fn start_server(g: &Gate, port: u16) -> Server {
    let child = Command::new(BIN)
        .args(["serve", "--config", s(&g.cfg), "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let srv = Server(Some(child));
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return srv;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server on {port} did not start");
}

/// Two `serve` processes and several `check` processes share one ledger and
/// race the same token; exactly one of them approves.
#[test]
fn real_ledger_single_use_under_mixed_serve_and_cli_race() {
    if !enabled() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    const ROUNDS: usize = 25;
    const CLI: usize = 6;
    const HTTP_PER_SERVER: usize = 3;
    let g = Gate::new("mixed");
    let (pa, pb) = (port_base(), port_base() + 1);
    let mut sa = start_server(&g, pa);
    let mut sb = start_server(&g, pb);
    eprintln!("start {} servers on {pa} and {pb}", now_zagreb());
    let started = Instant::now();
    let mut by_kind: HashMap<String, usize> = HashMap::new();
    let mut winners = Vec::new();
    for round in 0..ROUNDS {
        let (token, _) = g.mint();
        let kids = g.spawn_blocked(1000 + round, CLI);
        let fifos: Vec<PathBuf> = kids.iter().map(|(f, _)| f.clone()).collect();
        let writers = open_writers(&fifos);
        let n_http = 2 * HTTP_PER_SERVER;
        let barrier = Arc::new(Barrier::new(n_http + 1));
        let threads: Vec<_> = (0..n_http)
            .map(|i| {
                let port = if i % 2 == 0 { pa } else { pb };
                let body = approve_body(&token);
                let b = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
                    c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
                    let req = format!(
                        "POST /v1/approve HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    b.wait();
                    c.write_all(req.as_bytes()).unwrap();
                    let mut out = String::new();
                    c.read_to_string(&mut out).unwrap();
                    (port, out)
                })
            })
            .collect();
        barrier.wait();
        release(writers, &token);
        let mut wins_cli = Vec::new();
        let t = tally_cli(kids, &mut wins_cli);
        assert!(t.other.is_empty(), "round {round}: {:?}", t.other);
        let mut approved = t.approved;
        if t.approved == 1 {
            winners.push("cli".to_owned());
        }
        for th in threads {
            let (port, r) = th.join().unwrap();
            let v = body_of(&r);
            if r.starts_with("HTTP/1.1 200") && v["decision"] == "WouldApply" {
                approved += 1;
                winners.push(format!("serve:{port}"));
            } else {
                assert!(
                    r.starts_with("HTTP/1.1 403") && v["code"] == "replayed",
                    "round {round}: {r}"
                );
            }
        }
        assert_eq!(approved, 1, "round {round}: exactly one approval across processes");
    }
    for w in &winners {
        *by_kind.entry(w.clone()).or_default() += 1;
    }
    // Every token once more at each server: all replayed.
    let lines = g.ledger_lines();
    let consumed = lines.iter().filter(|l| l["event"] == "consumed").count();
    assert_eq!(consumed, ROUNDS);
    for port in [pa, pb] {
        let r = http(port, "GET", "/health", "");
        let h = body_of(&r);
        eprintln!("health {port}: {} ledger {}", r.lines().next().unwrap_or(""), h["ledger"]);
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        assert_eq!(h["ledger"]["consumed"], ROUNDS);
        assert_eq!(h["ledger"]["audit_failures_total"], 0);
    }
    for (srv, port) in [(&mut sa, pa), (&mut sb, pb)] {
        assert!(http(port, "POST", "/v1/stop", "").contains("stopping"));
        let out = srv.0.take().unwrap().wait_with_output().unwrap();
        assert!(out.status.success());
        let j = text(&out.stderr);
        assert!(
            !j.contains("ledger unavailable") && !j.contains("ledger corrupt"),
            "{j}"
        );
    }
    eprintln!(
        "mixed race: {ROUNDS} rounds x ({CLI} cli + {} http on 2 servers) = {} attempts; \
         approvals {} (1 per round), winners {by_kind:?}; ledger lines {} well-formed; elapsed {:?}; end {}",
        2 * HTTP_PER_SERVER,
        ROUNDS * (CLI + 2 * HTTP_PER_SERVER),
        winners.len(),
        lines.len(),
        started.elapsed(),
        now_zagreb()
    );
}

/// Run `check` under a file-size limit (`prlimit --fsize`): the kernel cuts the
/// ledger write short exactly like a disk that fills up mid-line, then kills
/// the process with SIGXFSZ on the next write.
fn check_limited(g: &Gate, token: &str, fsize: u64) -> Output {
    let tok = g.dir.join("limited.tok");
    std::fs::write(&tok, token).unwrap();
    let mut args = vec![format!("--fsize={fsize}"), BIN.to_owned()];
    args.extend(g.check_args(&tok).into_iter().map(str::to_owned));
    let o = Command::new("prlimit").args(&args).output().unwrap();
    std::fs::remove_file(&tok).unwrap();
    o
}

fn check(g: &Gate, token: &str) -> Output {
    let tok = g.dir.join("plain.tok");
    std::fs::write(&tok, token).unwrap();
    let o = run(&g.check_args(&tok));
    std::fs::remove_file(&tok).unwrap();
    o
}

fn show(label: &str, o: &Output) {
    eprintln!(
        "{label}: exit {:?} signal {:?} stdout {} stderr {}",
        o.status.code(),
        o.status.signal(),
        text(&o.stdout).trim(),
        text(&o.stderr).trim()
    );
}

/// A short (torn) ledger write never approves, and the gate fails closed on
/// the torn line afterwards until an admin repairs the ledger.
#[test]
fn real_ledger_short_write_fails_closed() {
    if !enabled() {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let g = Gate::new("short");
    eprintln!("start {}", now_zagreb());
    // Healthy first approval.
    let (t0, _) = g.mint();
    let o = check(&g, &t0);
    show("healthy approval", &o);
    assert_eq!(o.status.code(), Some(0));
    let size = std::fs::metadata(&g.ledger).unwrap().len();

    // (a) No room at all: the first write fails, nothing is written.
    let (t1, n1) = g.mint();
    let o = check_limited(&g, &t1, size);
    show("no room (fsize = ledger size)", &o);
    assert_ne!(o.status.code(), Some(0), "never approved");
    assert!(!text(&o.stdout).contains("WouldApply"));
    assert_eq!(std::fs::metadata(&g.ledger).unwrap().len(), size);
    // Room again: the same token was never consumed, so it passes once.
    let o = check(&g, &t1);
    show("same token, room again", &o);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(json(&o.stdout)["approved"]["nonce"], n1.as_str());
    let o = check(&g, &t1);
    assert_eq!(json(&o.stdout)["code"], "replayed");

    // (b) 40 bytes of room: the consumed line is torn mid-way.
    let size = std::fs::metadata(&g.ledger).unwrap().len();
    let (t2, n2) = g.mint();
    let o = check_limited(&g, &t2, size + 40);
    show("torn write (fsize = ledger size + 40)", &o);
    assert_ne!(o.status.code(), Some(0), "a torn consume never approves");
    assert!(!text(&o.stdout).contains("WouldApply"));
    let after = std::fs::metadata(&g.ledger).unwrap().len();
    eprintln!("ledger {size} -> {after} bytes (torn tail {} bytes)", after - size);
    assert_eq!(after, size + 40, "kernel short write landed");

    // Every later decision fails closed on the torn line, loudly.
    let (t3, _) = g.mint();
    for (label, tok) in [("fresh token after torn write", &t3), ("torn token again", &t2)] {
        let o = check(&g, tok);
        show(label, &o);
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(json(&o.stdout)["code"], "ledger");
        assert!(text(&o.stderr).contains("ledger corrupt"), "loud on stderr");
    }
    let o = run(&["status", "--config", s(&g.cfg)]);
    show("status after torn write", &o);
    assert_eq!(o.status.code(), Some(3));

    // Admin repair. The torn fragment has no newline, so the refusal notes
    // written after it were glued onto it: the corrupt line the gate names is
    // fragment + first later note. Deleting that one line repairs the ledger
    // (and loses that one refusal note). The torn token was never approved,
    // so it may pass exactly once afterwards.
    let bad: usize = json(&o.stdout)["ledger"]["error"]
        .as_str()
        .and_then(|e| e.strip_prefix("ledger corrupt at line "))
        .and_then(|e| e.split(':').next())
        .and_then(|n| n.parse().ok())
        .expect("corrupt line number");
    let t = std::fs::read_to_string(&g.ledger).unwrap();
    let glued = t.lines().nth(bad - 1).unwrap();
    eprintln!(
        "corrupt line {bad}: {} bytes, torn fragment glued to the next record: {}",
        glued.len(),
        glued.matches("{\"action\"").count() == 2
    );
    assert_eq!(glued.matches("{\"action\"").count(), 2, "fragment + one note");
    let kept: String = t
        .lines()
        .enumerate()
        .filter(|(i, _)| *i + 1 != bad)
        .map(|(_, l)| format!("{l}\n"))
        .collect();
    std::fs::write(&g.ledger, kept).unwrap();
    let o = check(&g, &t2);
    show("torn token after admin repair", &o);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(json(&o.stdout)["approved"]["nonce"], n2.as_str());
    let o = check(&g, &t2);
    assert_eq!(json(&o.stdout)["code"], "replayed");
    let o = check(&g, &t3);
    assert_eq!(o.status.code(), Some(0));
    let lines = g.ledger_lines();
    let consumed = lines.iter().filter(|l| l["event"] == "consumed").count();
    assert_eq!(consumed, 4, "t0, t1, t2, t3 each once");
    eprintln!("ledger lines {} well-formed, consumed {consumed}; end {}", lines.len(), now_zagreb());
}
