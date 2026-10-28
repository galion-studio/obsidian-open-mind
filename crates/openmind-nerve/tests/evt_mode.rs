// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Event mode, end to end against the real `openmind-nerve` binary
//! (`serve --mode event`), with simulated events through every source:
//! journal (JSON lines into a FIFO via `NERVE_EVT_JOURNALCTL=replay:`),
//! inotify (task dir + record file), pidfd (`NERVE_EVT_PIDS`), HTTP
//! (`POST /event`, `POST /actions`), and the paid-escalation slot against
//! an in-test stub (no real provider, no key, loopback only).
//! Runs on the box (no sandbox, no root needed).
//!
//! Ports (slot 390, nerve-evt-mode-port, 2026-10-28 CET +01:00; F-373-2):
//! the suite used fixed ports 20760-20769 shared by every swarm slot, so two
//! concurrent workspace runs collided (a failed stub bind, or a health probe
//! answered by another slot's nerve) and the e2e test could hang forever on
//! the blocking journal-FIFO open once its nerve was gone. Now:
//! - default: ephemeral loopback ports. The stub binds `127.0.0.1:0`; each
//!   nerve gets a kernel-picked free port and the test waits for the nerve's
//!   own `listening on <addr>` stderr line (so a foreign listener can never
//!   pass the health wait); a lost bind race is retried on a new port.
//! - `NERVE_EVT_IT_PORT_BASE=<base>` (via `galion_realtest::ports`): fixed
//!   block `base..base+9` (e2e nerve +0, paid stub +2, paid nerve +3,
//!   dead component +9) for runs that must stay inside a slot range.
//! - the FIFO writer open is bounded (30 s) and fails fast if the nerve died.

use galion_realtest::ports::parse_base;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");
const PORT_ENV: &str = "NERVE_EVT_IT_PORT_BASE";
const PORT_SPAN: u16 = 10;
const OFF_E2E: u16 = 0;
const OFF_STUB: u16 = 2;
const OFF_PAID: u16 = 3;
const OFF_DEAD: u16 = 9;

/// Fixed block base from `NERVE_EVT_IT_PORT_BASE`, or `None` = ephemeral.
fn fixed_base() -> Option<u16> {
    let raw = std::env::var(PORT_ENV).ok()?;
    // default is unused when the var is set; parse_base validates the span
    Some(parse_base(Some(&raw), 0, PORT_SPAN).unwrap_or_else(|e| panic!("{PORT_ENV}: {e}")))
}

/// A loopback port nothing listens on right now (kernel-picked, then freed).
fn ephemeral_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("ephemeral bind")
        .port()
}

/// Port for block offset `off`: fixed block when the env is set, else ephemeral.
fn port_for(off: u16) -> u16 {
    match fixed_base() {
        Some(b) => b + off,
        None => ephemeral_port(),
    }
}

fn http(port: u16, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1.to_string())
        .unwrap_or_default();
    (code, body)
}

fn get_json(port: u16, path: &str) -> Value {
    let (c, b) = http(port, "GET", path, "");
    assert_eq!(c, 200, "{path}: {b}");
    serde_json::from_str(&b).unwrap_or_else(|e| panic!("{path}: {e}: {b}"))
}

fn wait_until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(secs),
            "timeout waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Nerve {
    child: Child,
    port: u16,
    /// nerve stderr, drained by a thread (an undrained pipe could block it)
    err: Arc<Mutex<String>>,
}
impl Drop for Nerve {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Nerve {
    /// Panics with the nerve's stderr if the process has exited.
    fn assert_alive(&mut self, what: &str) {
        if let Ok(Some(st)) = self.child.try_wait() {
            panic!(
                "nerve on :{} exited ({st}) during {what}; stderr:\n{}",
                self.port,
                self.err.lock().unwrap()
            );
        }
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nerve-evt-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Start an event-mode nerve on block offset `off` (or an ephemeral port).
/// Ready = its own `listening on 127.0.0.1:<port>` line was printed (bind
/// succeeded in THIS process) and `/health` is 200 while it is still alive.
/// An ephemeral port lost to a race (bind error) is retried on a new port.
fn start(off: u16, dir: &Path, extra: &[(&str, String)]) -> Nerve {
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let dead = port_for(OFF_DEAD);
    std::fs::write(
        dir.join("components.json"),
        format!(r#"[{{"name":"evt-it-dead","port":{dead},"path":"/health","gaia":"DEMETER","system":"tuner"}}]"#),
    )
    .unwrap();
    let fixed = fixed_base().is_some();
    let tries = if fixed { 1 } else { 5 };
    let mut last = String::new();
    for _ in 0..tries {
        let port = port_for(off);
        let mut c = Command::new(BIN);
        c.args([
            "serve",
            "--mode",
            "event",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--log",
            &dir.join("events.jsonl").display().to_string(),
        ])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NERVE_DATA_DIR", dir.join("data"))
        .env("NERVE_COMPONENTS", dir.join("components.json"))
        .env("NERVE_DRY_RUN", "1")
        .env("NERVE_SYSTEMD", "0")
        .env("NERVE_JOURNAL", "0")
        .env("NERVE_GALION_HEALTH_PORT", "0")
        .env("NERVE_EVT_DEBOUNCE", "1")
        .env("NERVE_EVT_MAX_WAIT", "4")
        .env("NERVE_EVT_CYCLE_DEBOUNCE", "1")
        .env("NERVE_EVT_CYCLE_MIN_GAP", "5")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        for (k, v) in extra {
            c.env(k, v);
        }
        let mut child = c.spawn().expect("spawn nerve");
        let err = Arc::new(Mutex::new(String::new()));
        let pipe = child.stderr.take().unwrap();
        let e2 = err.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(pipe).lines().map_while(Result::ok) {
                let mut g = e2.lock().unwrap();
                g.push_str(&l);
                g.push('\n');
            }
        });
        let mut n = Nerve { child, port, err };
        let want = format!("listening on 127.0.0.1:{port} ");
        let t = Instant::now();
        let mut exited = false;
        while !n.err.lock().unwrap().contains(&want) {
            if let Ok(Some(_)) = n.child.try_wait() {
                exited = true;
                break;
            }
            assert!(
                t.elapsed() < Duration::from_secs(30),
                "timeout waiting for nerve listen line on :{port}; stderr:\n{}",
                n.err.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        if exited {
            // give the drain thread a moment to collect the last lines
            std::thread::sleep(Duration::from_millis(100));
            last = n.err.lock().unwrap().clone();
            if !fixed && last.contains("bind 127.0.0.1:") {
                eprintln!("evt_mode: port {port} lost to a bind race, retrying");
                continue;
            }
            panic!("nerve on :{port} exited at start; stderr:\n{last}");
        }
        wait_until("health", 30, || {
            TcpStream::connect(("127.0.0.1", port)).is_ok()
                && http(port, "GET", "/health", "").0 == 200
        });
        n.assert_alive("start");
        return n;
    }
    panic!("nerve could not bind after {tries} ephemeral ports; last stderr:\n{last}");
}

/// Open the journal FIFO for write without hanging forever: the open blocks
/// until the nerve opens the read end, so do it on a helper thread and give
/// up (with the nerve's stderr) after 30 s or as soon as the nerve exits.
fn open_fifo_writer(fifo: &Path, n: &mut Nerve) -> std::fs::File {
    let (tx, rx) = std::sync::mpsc::channel();
    let p = fifo.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(std::fs::OpenOptions::new().write(true).open(&p));
    });
    let t = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(r) => return r.expect("open journal fifo for write"),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                n.assert_alive("journal fifo open");
                assert!(
                    t.elapsed() < Duration::from_secs(30),
                    "journal fifo open timed out (nerve never opened the read end); stderr:\n{}",
                    n.err.lock().unwrap()
                );
            }
            Err(e) => panic!("fifo open thread: {e}"),
        }
    }
}

fn mkfifo(p: &Path) {
    assert!(Command::new("mkfifo").arg(p).status().unwrap().success());
}

fn jline(id: &str, unit: &str, extra: &str) -> String {
    format!(r#"{{"MESSAGE_ID":"{id}","UNIT":"{unit}","MESSAGE":"{unit}: sim","_PID":"1"{extra}}}"#)
}

#[test]
fn event_mode_end_to_end() {
    let dir = tmp("e2e");
    let fifo = dir.join("journal.fifo");
    mkfifo(&fifo);
    let tasks = dir.join("swarm-tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let record = dir.join("swarm-record.md");
    std::fs::write(&record, "- old START line\n").unwrap();
    let mut sleeper = Command::new("sleep").arg("3").spawn().unwrap();
    let mut n = start(
        OFF_E2E,
        &dir,
        &[
            ("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display())),
            (
                "NERVE_EVT_WATCH",
                format!("{},{}", tasks.display(), record.display()),
            ),
            ("NERVE_EVT_PIDS", sleeper.id().to_string()),
        ],
    );
    let p = n.port;
    // the FIFO writer stays open for the whole test
    let mut j = open_fifo_writer(&fifo, &mut n);

    // 1. contract endpoints exist; health stays 200
    wait_until("inventory", 30, || {
        http(p, "GET", "/inventory", "").0 == 200
    });
    let inv = get_json(p, "/inventory");
    assert!(
        inv["groups"].is_object() && inv["totals"]["processes"].as_u64().unwrap() > 0,
        "{inv}"
    );
    let st = get_json(p, "/stats");
    assert_eq!(st["mode"], "event");
    assert_eq!(st["config"]["escalate"]["mode"], "delegate");
    assert_eq!(st["config"]["escalate"]["paid_enabled"], false);

    // 2. idle: no source wakes for 2 s (startup warm-up incident did NOT escalate)
    let w0 = get_json(p, "/stats")["wakes_total"].as_u64().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let st = get_json(p, "/stats");
    assert!(st["wakes_total"].as_u64().unwrap() <= w0 + 1, "{st}"); // +1: the pidfd sleeper may exit
    assert_eq!(
        st["escalations_total"], 0,
        "warm-up incidents never escalate: {st}"
    );

    // 3. stream subscriber from seq 0
    let mut s = TcpStream::connect(("127.0.0.1", p)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(s, "GET /events/stream?since=0 HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut rd = BufReader::new(s);
    let mut status = String::new();
    rd.read_line(&mut status).unwrap();
    assert!(status.contains("200"), "{status}");
    loop {
        let mut h = String::new();
        rd.read_line(&mut h).unwrap();
        if h == "\r\n" {
            break;
        }
    }

    // 4. crash-loop burst through the journal: 12 lines, 1 untracked noise line
    for _ in 0..4 {
        writeln!(
            j,
            "{}",
            jline(
                "98e322203f7a4ed290d09fe03c09fe15",
                "hermes-rs.service",
                r#","EXIT_STATUS":"1""#
            )
        )
        .unwrap();
        writeln!(
            j,
            "{}",
            jline("5eb03494b6584870a536b337290809b3", "hermes-rs.service", "")
        )
        .unwrap();
        writeln!(
            j,
            "{}",
            jline("39f53479d3a045ac8e11786248231fbf", "hermes-rs.service", "")
        )
        .unwrap();
    }
    writeln!(
        j,
        "{}",
        jline(
            "d9b373ed55a64feb8242e02dbe79a49c",
            "hermes-rs.service",
            r#","UNIT_RESULT":"start-limit-hit""#
        )
    )
    .unwrap();
    writeln!(
        j,
        "{}",
        jline("39f53479d3a045ac8e11786248231fbf", "session-1.scope", "")
    )
    .unwrap();
    j.flush().unwrap();

    // 5. the stream delivers the events and exactly one escalation.batch
    let mut kinds = Vec::new();
    let mut batch: Option<Value> = None;
    while batch.is_none() {
        let mut l = String::new();
        rd.read_line(&mut l).expect("stream line");
        let v: Value =
            serde_json::from_str(l.trim()).unwrap_or_else(|e| panic!("ndjson: {e}: {l}"));
        assert_eq!(v["v"], 1);
        assert!(v["seq"].as_u64().unwrap() > 0 && v["ts"].as_str().unwrap().contains("+0"));
        kinds.push(v["kind"].as_str().unwrap().to_string());
        if v["kind"] == "escalation.batch" {
            batch = Some(v);
        }
    }
    for k in [
        "unit.exit",
        "unit.restart_scheduled",
        "unit.started",
        "unit.start_limit",
    ] {
        assert!(kinds.iter().any(|x| x == k), "{k} in {kinds:?}");
    }
    let b = batch.unwrap();
    assert_eq!(b["severity"], "critical");
    assert_eq!(b["data"]["outcome"], "delegated");
    let st = get_json(p, "/stats");
    assert_eq!(st["escalations_total"], 1, "{st}");
    assert!(st["deduped"].as_u64().unwrap() >= 8, "{st}");
    assert!(st["wakes_by"]["journal"].as_u64().unwrap() <= 13, "{st}");
    let esc = get_json(p, "/escalations");
    assert_eq!(esc.as_array().unwrap().len(), 1);

    // 6. inotify: task dir and record file
    std::fs::write(tasks.join("slot-3.failed"), "boom").unwrap();
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&record)
            .unwrap();
        writeln!(
            f,
            "- 2026-10-21 18:40:00 +0200 — slot 211 nerve-event-driven DONE (test)"
        )
        .unwrap();
    }
    wait_until("inotify events", 10, || {
        let ev = get_json(p, "/evt?n=200");
        let ks: Vec<String> = ev
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["kind"].as_str().unwrap_or("").to_string())
            .collect();
        ks.contains(&"task.failed".into()) && ks.contains(&"slot.done".into())
    });

    // 7. pidfd: the sleeper exits -> proc.exit
    let _ = sleeper.wait();
    wait_until("pidfd exit", 10, || {
        get_json(p, "/evt?n=200")
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "proc.exit" && e["source"] == "pidfd")
    });

    // 8. actions: allowlisted -> dry-run; never-touch -> 403; note -> ok
    let (c, body) = http(
        p,
        "POST",
        "/actions",
        r#"{"action":"restart","unit":"hermes-rs.service","dry_run":false,"request_id":"cx-1","source":"cortex"}"#,
    );
    assert_eq!(c, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["result"], "dry-run");
    assert!(v["why"]
        .to_string()
        .contains("NERVE_EVT_ACTIONS is not live"));
    assert_eq!(
        http(
            p,
            "POST",
            "/actions",
            r#"{"action":"restart","unit":"galion-nerve.service"}"#
        )
        .0,
        403
    );
    assert_eq!(
        http(
            p,
            "POST",
            "/actions",
            r#"{"action":"restart","unit":"ssh.service"}"#
        )
        .0,
        403
    );
    assert_eq!(
        http(p, "POST", "/actions", r#"{"action":"rm -rf","unit":"x"}"#).0,
        400
    );
    assert_eq!(get_json(p, "/actions").as_array().unwrap().len(), 3);

    // 9. POST /event (existing API) becomes an event too and stays in /events
    let (c, _) = http(
        p,
        "POST",
        "/event",
        r#"{"source":"swarm","kind":"slot.failed","severity":"error","data":{"slot":"212"}}"#,
    );
    assert_eq!(c, 200);
    wait_until("http event", 10, || {
        get_json(p, "/evt?n=300")
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "slot.failed" && e["source"] == "http")
    });
    let events = get_json(p, "/events?n=500");
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["source"] == "nerve-evt"),
        "medium+ evt events mirrored into the event log"
    );

    // 10. one more burst after the window -> second escalation (task.failed + slot.failed were high too)
    wait_until("2nd batch", 15, || {
        get_json(p, "/stats")["escalations_total"].as_u64().unwrap() >= 2
    });
    let st = get_json(p, "/stats");
    assert_eq!(http(p, "GET", "/health", "").0, 200);
    eprintln!(
        "evt e2e stats: wakes_total={} by={} escalations={} batches={} deduped={}",
        st["wakes_total"], st["wakes_by"], st["escalations_total"], st["batches"], st["deduped"]
    );
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn openrouter_endpoint_refused_at_start() {
    let dir = tmp("or");
    let or_listen = format!("127.0.0.1:{}", fixed_base().map_or(0, |b| b + 1));
    for url in [
        "https://openrouter.ai/api/v1/chat/completions",
        "http://127.0.0.1:8642/v1/chat/completions",
    ] {
        let out = Command::new(BIN)
            // binds before the refusal check: fixed block base+1, else port 0
            .args(["serve", "--mode", "event", "--listen", &or_listen])
            .env("NERVE_DATA_DIR", &dir)
            .env("NERVE_EVT_ESCALATE", "openai")
            .env("NERVE_EVT_LLM_ENDPOINT", url)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{url}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("refused"), "{err}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The paid slot against a loopback stub: one call per batch, key from the
/// named env var in the Authorization header only, per-hour cap honoured.
#[test]
fn paid_slot_against_stub_respects_caps() {
    let dir = tmp("paid");
    let fifo = dir.join("journal.fifo");
    mkfifo(&fifo);
    // fixed block: base+2; default: kernel-picked port (no shared fixed port)
    let stub = match fixed_base() {
        Some(b) => TcpListener::bind(("127.0.0.1", b + OFF_STUB)),
        None => TcpListener::bind("127.0.0.1:0"),
    }
    .expect("stub bind");
    let stub_port = stub.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(bool, bool, String)>::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in stub.incoming() {
            let Ok(mut c) = c else { continue };
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = vec![0u8; 65536];
            let mut got = Vec::new();
            loop {
                let n = c.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&got).to_string();
                if let Some((h, b)) = t.split_once("\r\n\r\n") {
                    let cl = h
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if b.len() >= cl {
                        break;
                    }
                }
            }
            let t = String::from_utf8_lossy(&got).to_string();
            let auth_ok = t.contains("Authorization: Bearer stub-key-not-real");
            let (_, body) = t.split_once("\r\n\r\n").unwrap_or(("", ""));
            s2.lock().unwrap().push((
                auth_ok,
                body.contains("stub-key-not-real"),
                body.to_string(),
            ));
            let resp = r#"{"choices":[{"message":{"content":"likely crash loop; restart hermes-rs.service"}}],"usage":{"prompt_tokens":800,"completion_tokens":60}}"#;
            let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}", resp.len());
        }
    });
    let mut n = start(
        OFF_PAID,
        &dir,
        &[
            ("NERVE_EVT_JOURNALCTL", format!("replay:{}", fifo.display())),
            ("NERVE_EVT_ESCALATE", "openai".into()),
            (
                "NERVE_EVT_LLM_ENDPOINT",
                format!("http://127.0.0.1:{stub_port}/v1/chat/completions"),
            ),
            ("NERVE_EVT_LLM_MODEL", "stub-model".into()),
            ("NERVE_EVT_LLM_KEY_ENV", "NERVE_IT_STUB_KEY".into()),
            ("NERVE_IT_STUB_KEY", "stub-key-not-real".into()),
            ("NERVE_EVT_LLM_BUDGET_USD", "0.05".into()),
            ("NERVE_EVT_LLM_PRICE_IN_PER_MTOK", "0.5".into()),
            ("NERVE_EVT_LLM_PRICE_OUT_PER_MTOK", "1.5".into()),
            ("NERVE_EVT_LLM_CALLS_PER_HOUR", "1".into()),
            ("NERVE_EVT_DEDUPE", "1".into()),
        ],
    );
    let p = n.port;
    let mut j = open_fifo_writer(&fifo, &mut n);
    let st = get_json(p, "/stats");
    assert_eq!(st["config"]["escalate"]["paid_enabled"], true);
    assert!(
        !st.to_string().contains("stub-key-not-real"),
        "key value never exposed"
    );
    for round in 0..2 {
        for _ in 0..5 {
            writeln!(
                j,
                "{}",
                jline(
                    "98e322203f7a4ed290d09fe03c09fe15",
                    "openmind-memory.service",
                    r#","EXIT_STATUS":"1""#
                )
            )
            .unwrap();
        }
        j.flush().unwrap();
        wait_until("batch", 20, || {
            get_json(p, "/stats")["batches"].as_u64().unwrap() > round
        });
        std::thread::sleep(Duration::from_millis(200));
    }
    let st = get_json(p, "/stats");
    let esc = get_json(p, "/escalations");
    let outs: Vec<&str> = esc
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(outs, vec!["called", "refused"], "{esc}");
    assert!(esc[1]["detail"].as_str().unwrap().contains("per-hour"));
    assert_eq!(st["llm_calls"], 1);
    let usd = st["llm_usd"].as_f64().unwrap();
    assert!(
        (usd - (800.0 * 0.5 + 60.0 * 1.5) / 1e6).abs() < 1e-9,
        "{usd}"
    );
    let calls = seen.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "exactly one paid call");
    assert!(
        calls[0].0,
        "Authorization header carried the key from the named env var"
    );
    assert!(!calls[0].1, "key never in the body");
    assert!(calls[0].2.contains("stub-model") && calls[0].2.contains("openmind-memory.service"));
    let spend: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("data/evt-llm-spend.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(spend["calls"], 1);
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
}
