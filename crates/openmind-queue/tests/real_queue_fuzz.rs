// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary fuzz test (slot 221), gated: runs only with `GALION_REAL=1`.
//!
//! Starts the release `openmind-queue` (`POSEIDON_BIN`, default
//! `$CARGO_TARGET_DIR/release/openmind-queue`; build it first) on
//! 127.0.0.1:20911 (slot 221 range 20910-20919). The release profile has
//! `panic = "abort"`, so any panic reachable from the wire kills the
//! process; the test checks it never does.
//!
//! 1. Wire fuzz: `REAL_FUZZ_CASES` (default 3000) mutated requests from a
//!    corpus of real API calls over real TCP, the write side shut after
//!    each so a short body ends at EOF. Every answer must be a well-formed
//!    HTTP response with a known status, never 500, and the process must
//!    stay up.
//! 2. The finds over the wire: conflicting / signed Content-Length -> 400,
//!    cost over MAX_COST -> 400, two devices with cents_per_hour u64::MAX
//!    -> /stats answers with a saturated (not wrapped) price.
//! 3. A hostile but CRC-valid queue.log (duplicate enqueue id) and a
//!    refusals.dat slot at seq u64::MAX: the server starts, leases, acks,
//!    leases again without aborting, and a refusal survives a SIGKILL.
//! 4. SIGKILL + restart after the wire fuzz: the replayed depth equals the
//!    depth before the kill.

mod fuzz;

use fuzz::{mutate, Rng};
use openmind_queue::http;
use openmind_queue::store::crc32;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:20911";

fn bin() -> String {
    std::env::var("POSEIDON_BIN").unwrap_or_else(|_| {
        let t = std::env::var("CARGO_TARGET_DIR")
            .unwrap_or_else(|_| "/workspace/swarm/target/gaia-queue-fuzz".into());
        format!("{t}/release/openmind-queue")
    })
}

fn call(m: &str, p: &str, b: Option<Value>) -> (u16, Value) {
    let body = b.map(|v| v.to_string());
    let (c, s) = http::request(ADDR, m, p, body.as_deref(), Duration::from_secs(5)).unwrap();
    (c, serde_json::from_str(&s).unwrap_or(Value::Null))
}

/// Kills the server when dropped, so a failed assertion never leaves a
/// process on the port.
struct Srv(Child);

impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(data: &Path) -> Srv {
    assert!(
        TcpStream::connect(ADDR).is_err(),
        "something is already listening on {ADDR}"
    );
    let mut child = Command::new(bin())
        .args(["serve", "--bind", ADDR, "--data"])
        .arg(data)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start release binary (cargo build --release -p openmind-queue first)");
    for _ in 0..100 {
        if http::request(ADDR, "GET", "/healthz", None, Duration::from_millis(200)).is_ok() {
            return Srv(child);
        }
        if let Ok(Some(st)) = child.try_wait() {
            panic!("server exited at start: {st}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("server did not come up on {ADDR}");
}

fn alive(c: &mut Srv) -> bool {
    matches!(c.0.try_wait(), Ok(None))
}

/// SIGKILL (std `Child::kill`) and reap.
fn kill(c: Srv) {
    drop(c);
}

/// Send raw bytes, shut the write side, read the whole answer.
fn raw(bytes: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut s = TcpStream::connect(ADDR)?;
    s.set_read_timeout(Some(Duration::from_secs(35)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    // a server that already answered (e.g. a 400 on the request line) may
    // reset the rest of a long write; that is fine, read what came back
    let _ = s.write_all(bytes);
    let _ = s.shutdown(Shutdown::Write);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    Ok(out)
}

fn req(method: &str, path: &str, body: &str) -> Vec<u8> {
    format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

const KNOWN: &[u16] = &[200, 201, 204, 400, 402, 403, 404, 405, 409, 413, 429, 503];

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("q221-real-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn line(json: &str) -> String {
    format!("{:08x} {json}\n", crc32(json.as_bytes()))
}

#[test]
fn real_queue_fuzz() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-binary fuzz test");
        return;
    }
    let cases: u64 = std::env::var("REAL_FUZZ_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let seed: u64 = std::env::var("GALION_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x221_0911);
    // REAL_FUZZ_PARTS=hostile runs only part 3 (used for the negative
    // control against the base binary, which fails part 2 first)
    let parts = std::env::var("REAL_FUZZ_PARTS").unwrap_or_else(|_| "all".into());
    eprintln!("real_queue_fuzz: bin {} seed {seed} cases {cases}", bin());
    if parts == "hostile" {
        hostile();
        return;
    }

    // ---------------------------------------------------------- 1 wire fuzz
    let data = tmp("wire");
    let mut s = start(&data);
    let corpus: Vec<Vec<u8>> = vec![
        req("GET", "/healthz", ""),
        req("GET", "/stats", ""),
        req(
            "POST",
            "/jobs",
            r#"{"kind":"echo","priority":5,"agent":"a","cost":2}"#,
        ),
        req(
            "POST",
            "/jobs",
            r#"{"queue":"gpu","kind":"llm","vram_gb":8,"cpu_fallback":true}"#,
        ),
        req("POST", "/lease", r#"{"worker":"w","lease_ms":200}"#),
        req("POST", "/jobs/1/ack", r#"{"token":1,"cost":1}"#),
        req("POST", "/jobs/2/nack", r#"{"token":2,"error":"x"}"#),
        req("POST", "/jobs/1/extend", r#"{"token":1,"lease_ms":500}"#),
        req("POST", "/caps", r#"{"scope":"a","cap":50}"#),
        req(
            "POST",
            "/quotas",
            r#"{"agent":"a","max_pending":50,"max_inflight":5}"#,
        ),
        req(
            "POST",
            "/vram/devices",
            r#"{"id":"g1","kind":"gpu","vram_gb":24,"state":"approved"}"#,
        ),
        req("GET", "/dlq", ""),
        req("GET", "/vram", ""),
        req(
            "POST",
            "/gaia/dispatch",
            r#"{"system":"poseidon","verb":"status"}"#,
        ),
        req("GET", "/jobs/1", ""),
    ];
    let tokens: &[&[u8]] = &[
        b"\r\n",
        b"Content-Length: 1\r\nContent-Length: 2\r\n",
        b"Content-Length: +4\r\n",
        b"Transfer-Encoding: chunked\r\n",
        b"18446744073709551615",
        b"\"cost\":18446744073709551615,",
        b"\"cents_per_hour\":18446744073709551615,",
        b"\"vram_gb\":4294967295,",
        b"\"priority\":-2147483648,",
        b"\"agent\":\"*\",",
        b"/../",
        b"%00",
        b"\xff",
    ];
    let mut r = Rng(seed);
    let t0 = Instant::now();
    let mut by_code = std::collections::BTreeMap::<u16, u64>::new();
    let mut empty = 0u64;
    for n in 0..cases {
        let base = &corpus[r.below(corpus.len())];
        let m = if r.chance(5) {
            base.clone()
        } else {
            mutate(&mut r, base, tokens)
        };
        let out = raw(&m).expect("connect");
        if !alive(&mut s) {
            panic!("server died on case {n} (seed {seed}): {}", fuzz::show(&m));
        }
        if out.is_empty() {
            // the server closed without answering: only allowed when the
            // request never completed its first line (no LF at all)
            empty += 1;
            assert!(
                !m.contains(&b'\n') || m.is_empty(),
                "no answer to case {n}: {}",
                fuzz::show(&m)
            );
            continue;
        }
        let (code, _) = http::parse_response(&out)
            .unwrap_or_else(|e| panic!("malformed answer to case {n} ({e}): {}", fuzz::show(&m)));
        assert!(
            KNOWN.contains(&code),
            "status {code} on case {n}: {}",
            fuzz::show(&m)
        );
        *by_code.entry(code).or_default() += 1;
    }
    let (c, st) = call("GET", "/stats", None);
    assert_eq!(c, 200);
    let depth_before = st["depth"].clone();
    // time moves (leases expire, backoffs end), so compare what time cannot
    // change: the number of tracked jobs and the enqueue/ack counts
    let total = |s: &Value| {
        ["ready", "delayed", "leased", "dead"]
            .iter()
            .map(|k| s["depth"][k].as_u64().unwrap())
            .sum::<u64>()
    };
    let keep = |s: &Value| {
        (
            total(s),
            s["stats"]["enqueued"].clone(),
            s["stats"]["acked"].clone(),
        )
    };
    let before = keep(&st);
    eprintln!(
        "wire fuzz: {cases} cases in {} ms, codes {by_code:?}, closed-without-answer {empty}, depth {depth_before}",
        t0.elapsed().as_millis()
    );

    // ---------------------------------------------------- 2 finds over wire
    let out =
        raw(b"POST /jobs HTTP/1.1\r\nContent-Length: 15\r\nContent-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!(
        http::parse_response(&out).unwrap().0,
        400,
        "conflicting Content-Length"
    );
    let out = raw(b"POST /jobs HTTP/1.1\r\nContent-Length: +2\r\n\r\n{}").unwrap();
    assert_eq!(
        http::parse_response(&out).unwrap().0,
        400,
        "signed Content-Length"
    );
    let (c, v) = call(
        "POST",
        "/jobs",
        Some(json!({"kind":"echo","cost": u64::MAX})),
    );
    assert_eq!(c, 400, "cost over MAX_COST: {v}");
    for id in ["z1", "z2"] {
        let (c, v) = call(
            "POST",
            "/vram/devices",
            Some(
                json!({"id":id,"kind":"remote","vram_gb":8,"cents_per_hour":u64::MAX,"state":"planned"}),
            ),
        );
        assert!(c == 200 || c == 201, "device {id}: {c} {v}");
    }
    let (c, st) = call("GET", "/stats", None);
    assert_eq!(c, 200);
    assert!(alive(&mut s));
    assert_eq!(
        st["vram"]["planned_cents_per_hour"].as_u64(),
        Some(u64::MAX),
        "price sum saturates, not wraps: {}",
        st["vram"]
    );
    eprintln!(
        "finds over wire: CL conflict 400, CL +2 400, cost u64::MAX 400, planned_cents_per_hour {}",
        st["vram"]["planned_cents_per_hour"]
    );

    // ----------------------------------------------- 4 SIGKILL and replay
    kill(s);
    let s = start(&data);
    let (_, st2) = call("GET", "/stats", None);
    assert_eq!(keep(&st2), before, "jobs and counts replayed after SIGKILL");
    eprintln!(
        "sigkill replay: (jobs, enqueued, acked) {:?} == before; depth {} (before {}), replayed {} records",
        keep(&st2), st2["depth"], depth_before, st2["store"]["replayed"]
    );
    kill(s);
    let _ = std::fs::remove_dir_all(&data);

    hostile();
}

fn hostile() {
    // ------------------------------------- 3 hostile log + sidecar at MAX
    let data = tmp("hostile");
    std::fs::create_dir_all(&data).unwrap();
    let spec = |p: i32| {
        format!(
            r#"{{"queue":"default","agent":"a","kind":"echo","priority":{p},"max_attempts":3,"cost":0,"lease_ms":60000,"payload":"","vram_gb":0,"cpu_fallback":false}}"#
        )
    };
    let mut log = String::new();
    log.push_str(&line(&format!(
        r#"{{"t":"enqueue","id":1,"now":1,"spec":{}}}"#,
        spec(0)
    )));
    log.push_str(&line(&format!(
        r#"{{"t":"enqueue","id":1,"now":2,"spec":{}}}"#,
        spec(5)
    )));
    std::fs::write(data.join("queue.log"), log).unwrap();
    let sc = |seq: u64, q: u64| {
        let j = format!(
            r#"{{"seq":{seq},"c":{{"quota_refusals":{q},"budget_refusals":0,"backpressure_refusals":0,"stale_acks":0,"charge_clamped":0}}}}"#
        );
        let mut b = line(&j).into_bytes();
        b.resize(512, 0);
        b
    };
    let mut side = sc(u64::MAX - 1, 1);
    side.extend(sc(u64::MAX, 7));
    std::fs::write(data.join("refusals.dat"), side).unwrap();
    let mut s = start(&data);
    let (_, st) = call("GET", "/stats", None);
    assert_eq!(
        st["depth"]["ready"], 1,
        "duplicate id replayed once: {}",
        st["depth"]
    );
    assert_eq!(
        st["stats"]["quota_refusals"], 7,
        "sidecar at seq u64::MAX restored"
    );
    let (c, l) = call("POST", "/lease", Some(json!({"worker":"w"})));
    assert_eq!(c, 200, "{l}");
    let (c, _) = call(
        "POST",
        &format!("/jobs/{}/ack", l["id"]),
        Some(json!({"token": l["token"]})),
    );
    assert_eq!(c, 200);
    let (c, l2) = call("POST", "/lease", Some(json!({"worker":"w"})));
    assert!(alive(&mut s), "server aborted on the second lease");
    assert!(
        c == 200 && l2["id"].is_null(),
        "queue empty after ack: {c} {l2}"
    );
    // one quota refusal, then SIGKILL: the sidecar must still persist it
    let (c, _) = call(
        "POST",
        "/quotas",
        Some(json!({"agent":"a","max_pending":0,"max_inflight":1})),
    );
    assert_eq!(c, 200);
    let (c, _) = call("POST", "/jobs", Some(json!({"agent":"a","kind":"echo"})));
    assert_eq!(c, 429);
    kill(s);
    let s = start(&data);
    let (_, st) = call("GET", "/stats", None);
    assert_eq!(
        st["stats"]["quota_refusals"], 8,
        "refusal after rebase survives SIGKILL"
    );
    eprintln!(
        "hostile log: ready 1 (duplicate ignored), lease+ack+lease ok, sidecar seq {} quota_refusals {}",
        st["refusals_sidecar"]["seq"], st["stats"]["quota_refusals"]
    );
    kill(s);
    let _ = std::fs::remove_dir_all(&data);
}
