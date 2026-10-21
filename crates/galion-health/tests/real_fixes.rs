// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Real-life test for galion-health v0.2.0 (swarm/health-fixes, wave-4 slot 149, ports 20190-20199).
//! Builds the RELEASE binary (or uses GALION_HEALTH_BIN), starts it on 127.0.0.1:20190 with a fake
//! HTTP upstream (20191), a black-hole upstream that accepts and never answers (20192), raw TCP
//! (20193) and a closed port (20194), runs every scenario, prints one PASS/FAIL line per scenario
//! with timings, then kills everything. Run:
//! `GALION_REAL=1 cargo test -p galion-health --test real_fixes -- --ignored --nocapture`.
//! Pointing GALION_HEALTH_BIN at the v0.1.0 (health-land f4a66ea) binary shows which scenarios it
//! fails (findings 1 and 2); the test then fails, as it should.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:20190";
const UP: u16 = 20191;
const HOLE: u16 = 20192;
const RAW: u16 = 20193;
const CLOSED: u16 = 20194;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn release_bin() -> PathBuf {
    if let Ok(p) = std::env::var("GALION_HEALTH_BIN") {
        return PathBuf::from(p);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let st = Command::new(cargo)
        .args(["build", "--release", "-j", "2", "-p", "galion-health"])
        .status()
        .expect("spawn cargo build");
    assert!(st.success(), "release build failed");
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    target.join("release/galion-health")
}

fn start(bin: &PathBuf, checks: &str, workers: Option<&str>) -> Kill {
    let mut c = Command::new(bin);
    c.env("GALION_HEALTH_ADDR", ADDR)
        .env("GALION_HEALTH_CHECKS", checks)
        .env("GALION_LOG_FORMAT", "json")
        .stderr(Stdio::null());
    if let Some(w) = workers {
        c.env("GALION_HEALTH_WORKERS", w);
    }
    let k = Kill(c.spawn().expect("spawn galion-health"));
    let t0 = Instant::now();
    loop {
        if let Ok(s) = TcpStream::connect(ADDR) {
            drop(s); // an empty connection: v0.2.0 closes it silently
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(5), "no listen");
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(100));
    k
}

/// Sends `parts` with `gap` between them, then reads the whole reply.
/// Err carries the I/O error text (e.g. "Connection reset by peer").
fn raw(parts: &[&[u8]], gap: Duration, timeout: Duration) -> Result<(String, String), String> {
    let mut s = TcpStream::connect(ADDR).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(timeout)).unwrap();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(gap);
        }
        s.write_all(p).map_err(|e| format!("write {i}: {e}"))?;
    }
    let mut out = Vec::new();
    s.read_to_end(&mut out).map_err(|e| format!("read: {e}"))?;
    let out = String::from_utf8_lossy(&out).to_string();
    let (h, b) = out
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("no http head in {out:?}"))?;
    Ok((h.to_string(), b.to_string()))
}

fn get(path: &str) -> Result<(String, String), String> {
    raw(
        &[format!("GET {path} HTTP/1.0\r\nHost: x\r\n\r\n").as_bytes()],
        Duration::ZERO,
        Duration::from_secs(40),
    )
}

fn status(h: &str) -> &str {
    h.lines().next().unwrap_or("")
}

fn upstream_ok(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(("127.0.0.1", UP)).expect("bind 20191");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((mut s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut b = [0u8; 512];
                    let _ = s.read(&mut b);
                    let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    })
}

/// Accepts and holds connections without ever answering (a hung upstream).
fn upstream_hole(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let l = TcpListener::bind(("127.0.0.1", HOLE)).expect("bind 20192");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            match l.accept() {
                Ok((s, _)) => held.push(s),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    })
}

struct Report(Vec<(String, bool, String)>);
impl Report {
    fn add(&mut self, name: &str, ok: bool, detail: String) {
        eprintln!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        self.0.push((name.into(), ok, detail));
    }
}

fn threads_of(pid: u32) -> (u64, u64) {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let field = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (field("Threads:"), field("VmRSS:"))
}

#[test]
#[ignore]
fn real_v020_fixes() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skip: set GALION_REAL=1");
        return;
    }
    let bin = release_bin();
    let stop = Arc::new(AtomicBool::new(false));
    let up = upstream_ok(stop.clone());
    let hole = upstream_hole(stop.clone());
    let _raw = TcpListener::bind(("127.0.0.1", RAW)).expect("bind 20193");
    let mut r = Report(Vec::new());

    // --version
    // v0.1.0 has no --version and would start serving: pin its address to ours and kill it after 2 s.
    let mut vc = Command::new(&bin)
        .arg("--version")
        .env("GALION_HEALTH_ADDR", ADDR)
        .env("GALION_HEALTH_CHECKS", "n|-|0|-")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t = Instant::now();
    while vc.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = vc.kill();
    let v = vc.wait_with_output().unwrap();
    let vs = String::from_utf8_lossy(&v.stdout).trim().to_string();
    r.add("version", vs == "galion-health 0.2.0", format!("{vs:?}"));

    // A) parity + finding 1 on an all-green spec.
    let green = format!("up|-|{UP}|/health;raw|-|{RAW}|-;noprobe|-|0|-");
    {
        let _k = start(&bin, &green, None);
        let a = get("/status.json");
        let ok = matches!(&a, Ok((h, b)) if h.starts_with("HTTP/1.1 200 OK")
            && h.contains("Content-Type: application/json") && h.contains("Cache-Control: no-store")
            && b.starts_with("{\"ok\":true,\"checked_at\":\"")
            && b.contains("\"name\":\"up\",\"unit\":\"-\",\"unit_state\":\"n/a\",\"port\":20191,\"http\":\"200\"")
            && b.contains("\"http\":\"tcp-open\""));
        r.add(
            "parity /status.json",
            ok,
            format!("{:?}", a.as_ref().map(|x| status(&x.0).to_string())),
        );
        let j = get("/json");
        r.add(
            "parity /json == /status.json (5 s cache)",
            matches!((&a, &j), (Ok(x), Ok(y)) if x.1 == y.1),
            String::new(),
        );
        let h = get("/healthz");
        r.add(
            "parity /healthz",
            matches!(&h, Ok((h, b)) if h.starts_with("HTTP/1.1 200") && b == "ok\n"),
            format!("{h:?}"),
        );
        let p = get("/");
        r.add("parity / html", matches!(&p, Ok((h, b)) if h.contains("text/html; charset=utf-8") && b.contains("Galion health: 3/3 ok")), String::new());
        let n = get("/status");
        r.add(
            "parity /status 404",
            matches!(&n, Ok((h, b)) if h.starts_with("HTTP/1.1 404") && b == "not found\n"),
            String::new(),
        );

        let gap = Duration::from_millis(200);
        let to = Duration::from_secs(10);
        let s1 = raw(
            &[b"GET /stat", b"us.json HTTP/1.0\r\nHost: x\r\n\r\n"],
            gap,
            to,
        );
        r.add("split mid-path -> /status.json", matches!(&s1, Ok((h, b)) if h.starts_with("HTTP/1.1 200") && b.starts_with("{\"ok\":true")), format!("{:?}", s1.as_ref().map(|x| status(&x.0).to_string())));
        let s2 = raw(&[b"GE", b"T /healthz HTTP/1.0\r\n\r\n"], gap, to);
        r.add(
            "split in method -> /healthz",
            matches!(&s2, Ok((h, b)) if h.starts_with("HTTP/1.1 200") && b == "ok\n"),
            format!("{:?}", s2.as_ref().map(|x| status(&x.0).to_string())),
        );
        let s3 = raw(
            &[b"GET /healthz HTTP/1.0\r\n", b"Host: x\r\n", b"\r\n"],
            gap,
            to,
        );
        r.add(
            "split headers, 2nd/3rd write accepted (no RST)",
            matches!(&s3, Ok((_, b)) if b == "ok\n"),
            format!("{:?}", s3.as_ref().map(|x| status(&x.0).to_string())),
        );
        let req = b"GET /healthz HTTP/1.0\r\nHost: x\r\n\r\n";
        let bytes: Vec<&[u8]> = req.chunks(1).collect();
        let t = Instant::now();
        let s4 = raw(&bytes, Duration::from_millis(15), to);
        r.add(
            "byte-by-byte head (34 writes)",
            matches!(&s4, Ok((_, b)) if b == "ok\n"),
            format!(
                "{:?} in {:?}",
                s4.as_ref().map(|x| status(&x.0).to_string()),
                t.elapsed()
            ),
        );
        let mut big = b"POST /healthz HTTP/1.0\r\nHost: x\r\nContent-Length: 6000\r\n\r\n".to_vec();
        big.extend_from_slice(&[b'z'; 6000]);
        let s5 = raw(&[&big], Duration::ZERO, to);
        r.add(
            "unread request body -> clean close, no RST",
            matches!(&s5, Ok((_, b)) if b == "ok\n"),
            format!("{:?}", s5.as_ref().map(|x| status(&x.0).to_string())),
        );
        let s6 = raw(&[b"hello\r\n\r\n"], Duration::ZERO, to);
        r.add(
            "garbage request -> 400",
            matches!(&s6, Ok((h, _)) if h.starts_with("HTTP/1.1 400")),
            format!("{:?}", s6.as_ref().map(|x| status(&x.0).to_string())),
        );
        let mut huge = b"GET / HTTP/1.0\r\nX: ".to_vec();
        huge.extend_from_slice(&[b'a'; 9000]);
        let s7 = raw(&[&huge], Duration::ZERO, to);
        r.add(
            "9 KB head -> 431",
            matches!(&s7, Ok((h, _)) if h.starts_with("HTTP/1.1 431")),
            format!("{:?}", s7.as_ref().map(|x| status(&x.0).to_string())),
        );

        // Finding 2a: idle (slow-loris) clients must not block others.
        let idle: Vec<TcpStream> = (0..2).map(|_| TcpStream::connect(ADDR).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(100));
        let t = Instant::now();
        let q = get("/healthz");
        let took = t.elapsed();
        r.add(
            "2 idle clients do not block a 3rd",
            q.is_ok() && took < Duration::from_millis(1000),
            format!("{took:?}"),
        );
        let t = Instant::now();
        let mut i0 = idle.into_iter().next().unwrap();
        i0.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut b = String::new();
        let _ = i0.read_to_string(&mut b);
        r.add(
            "idle client gets 408 after the 3 s head deadline",
            b.starts_with("HTTP/1.1 408"),
            format!(
                "{:?} after {:?}",
                b.lines().next().unwrap_or(""),
                t.elapsed()
            ),
        );
    }

    // B) finding 2b: 12 hung upstreams + 10 concurrent clients with a cold cache.
    std::thread::sleep(Duration::from_millis(200));
    let hung: Vec<String> = (0..12).map(|i| format!("h{i}|-|{HOLE}|/health")).collect();
    let hung = hung.join(";");
    {
        let k = start(&bin, &hung, None);
        let pid = k.0.id();
        let t0 = Instant::now();
        let clients: Vec<_> = (0..10)
            .map(|_| {
                std::thread::spawn(move || {
                    let t = Instant::now();
                    let res = get("/healthz");
                    (
                        t.elapsed(),
                        res.map(|x| status(&x.0).to_string() + " " + x.1.trim()),
                    )
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(1000));
        let (threads, rss) = threads_of(pid);
        let t = Instant::now();
        let nf = get("/nope");
        let nf_took = t.elapsed();
        let res: Vec<_> = clients.into_iter().map(|c| c.join().unwrap()).collect();
        let worst = res.iter().map(|x| x.0).max().unwrap();
        let all_503 = res.iter().all(|x| matches!(&x.1, Ok(s) if s.starts_with("HTTP/1.1 503") && s.contains("failing: h0,h1")));
        r.add(
            "cold cache, 12 hung upstreams, 10 clients",
            all_503 && worst < Duration::from_secs(5),
            format!(
                "worst {worst:?}, wall {:?}, first {:?}",
                t0.elapsed(),
                res[0].1
            ),
        );
        r.add(
            // 3 workers (TasksMax=8 budget) may all be waiting on the round: bound is one round.
            "404 during a probe round served within one round",
            nf.is_ok() && nf_took < Duration::from_millis(3000),
            format!("{nf_took:?}"),
        );
        r.add(
            "threads <= 5 (TasksMax=8 budget)",
            threads > 0 && threads <= 5,
            format!("Threads={threads} VmRSS={rss} kB"),
        );
        let b = get("/status.json").map(|x| x.1).unwrap_or_default();
        r.add(
            "hung upstream reads no-reply",
            b.matches("\"http\":\"no-reply\"").count() == 12,
            String::new(),
        );
        // Expired cache: the next request triggers one refresh; a second client during it does not start another.
        std::thread::sleep(Duration::from_millis(5200));
        let t = Instant::now();
        let a = std::thread::spawn(|| get("/status.json"));
        std::thread::sleep(Duration::from_millis(200));
        let b2 = get("/healthz");
        let a = a.join().unwrap();
        let took = t.elapsed();
        r.add(
            "expired cache: refresh served within STALE_WAIT",
            a.is_ok() && b2.is_ok() && took < Duration::from_millis(4500),
            format!("{took:?}"),
        );
    }

    // C) queue full -> 503 busy at once (1 worker, 32 queued idle clients).
    std::thread::sleep(Duration::from_millis(200));
    {
        let _k = start(&bin, &green, Some("1"));
        let idle: Vec<TcpStream> = (0..33).map(|_| TcpStream::connect(ADDR).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        let q = raw(
            &[b"GET /healthz HTTP/1.0\r\n\r\n"],
            Duration::ZERO,
            Duration::from_secs(15),
        );
        let took = t.elapsed();
        r.add(
            "full queue -> 503 busy without waiting",
            matches!(&q, Ok((h, b)) if h.starts_with("HTTP/1.1 503") && b == "busy\n")
                && took < Duration::from_millis(1000),
            format!(
                "{:?} in {took:?}",
                q.as_ref().map(|x| x.1.trim().to_string())
            ),
        );
        drop(idle);
    }

    // D) closed port -> down, /healthz 503 (parity with v0.1.0 test 2).
    std::thread::sleep(Duration::from_millis(200));
    {
        let _k = start(&bin, &format!("gone|-|{CLOSED}|/health;up|-|{UP}|/"), None);
        let h = get("/healthz");
        r.add(
            "closed port -> 503 failing: gone",
            matches!(&h, Ok((h, b)) if h.starts_with("HTTP/1.1 503") && b == "failing: gone\n"),
            format!("{h:?}"),
        );
        let j = get("/status.json").map(|x| x.1).unwrap_or_default();
        r.add(
            "closed port -> http down",
            j.starts_with("{\"ok\":false,") && j.contains("\"port\":20194,\"http\":\"down\""),
            String::new(),
        );
    }

    stop.store(true, Ordering::Relaxed);
    up.join().unwrap();
    hole.join().unwrap();
    let failed: Vec<&str> = r.0.iter().filter(|x| !x.1).map(|x| x.0.as_str()).collect();
    eprintln!(
        "SUMMARY {} pass, {} fail",
        r.0.len() - failed.len(),
        failed.len()
    );
    assert!(failed.is_empty(), "failed: {failed:?}");
}
