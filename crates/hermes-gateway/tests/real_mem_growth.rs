// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 198 hermes-mem-growth: RSS soak of the real `hermes-gateway`
//! binary (release) under steady chat load. Gated: `GALION_REAL=1`.
//!
//! Two gateways run side by side, both offline (no spend, no network
//! beyond 127.0.0.1):
//!
//! | port  | process |
//! |-------|---------|
//! | 20680 | `hermes-gateway --mock` (the shadow / smoke-test mode slot 187 soaked) |
//! | 20681 | `hermes-gateway` (production code path: CurlProvider) -> stub upstream |
//! | 20682 | stub OpenAI-compatible upstream inside this test (SSE, no model) |
//!
//! Load per gateway: paced chat completions (`hi <i>` new session; every
//! 4th `mock-fail` on the mock gateway = 502 path; every 8th a `/v1/runs`
//! run; every 16th a 6-turn session continuation; `/health` + `/v1/models`
//! GETs). VmRSS is sampled from `/proc/<pid>/status` every 15 s.
//!
//! Knobs: `HMG_SOAK_MIN` (default 20), `HMG_WARM_S` (default 120),
//! `HMG_RPS` (chats/s per gateway, default 30), `HMG_BIN` (binary to test,
//! default this crate's own release build), `HMG_EXPECT_GROWTH=1` (positive
//! control on an old binary: the mock gateway must grow), `HMG_PORT_BASE`
//! (default 20680), `HMG_ONLY` (`mock` or `stub`).
//!
//! Pass rule (flat): after warm-up, least-squares RSS slope < 0.25 MiB/min
//! and (mean of last quarter - mean of first quarter) < 4 MiB.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const KEY: &str = "hmg-test-key-0123456789abcdef";

fn envu(k: &str, d: u64) -> u64 {
    std::env::var(k)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(d)
}

fn real() -> bool {
    std::env::var("GALION_REAL").is_ok_and(|v| v == "1")
}

fn zagreb_now() -> String {
    // date(1) prints the box-local Europe/Zagreb time with its offset.
    Command::new("date")
        .arg("+%Y-%m-%d %H:%M:%S %:z")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

struct Proc(Child);
impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn rss_kb(pid: u32) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn threads(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Threads:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn fds(pid: u32) -> u64 {
    std::fs::read_dir(format!("/proc/{pid}/fd"))
        .map(|d| d.count() as u64)
        .unwrap_or(0)
}

/// One HTTP/1.0 request; returns (status, body).
fn http(port: u16, method: &str, path: &str, extra: &[(&str, &str)], body: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, String::new());
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let mut req = format!(
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    if s.write_all(req.as_bytes()).is_err() {
        return (0, String::new());
    }
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = out
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .to_string();
    (code, body)
}

fn chat(port: u16, text: &str, sid: Option<&str>) -> (u16, String) {
    let body = serde_json::json!({"model": "hermes-agent", "messages": [{"role": "user", "content": text}]}).to_string();
    match sid {
        Some(s) => http(
            port,
            "POST",
            "/v1/chat/completions",
            &[("X-Hermes-Session-Id", s)],
            &body,
        ),
        None => http(port, "POST", "/v1/chat/completions", &[], &body),
    }
}

/// Stub upstream: answers every POST with a short OpenAI SSE stream.
fn stub_upstream(port: u16, stop: Arc<AtomicBool>, hits: Arc<AtomicU64>) {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind stub upstream");
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            match l.accept() {
                Ok((s, _)) => {
                    let hits = hits.clone();
                    std::thread::spawn(move || serve_stub(s, &hits));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    });
}

fn serve_stub(mut s: TcpStream, hits: &AtomicU64) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; len.min(16 << 20)];
    let _ = r.read_exact(&mut body);
    hits.fetch_add(1, Ordering::SeqCst);
    let chunks = [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":"stub "}}]}"#,
        r#"{"choices":[{"index":0,"delta":{"content":"answer ok"}}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":40,"completion_tokens":3,"total_tokens":43,"cost":0}}"#,
    ];
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
    );
    for c in chunks {
        out.push_str("data: ");
        out.push_str(c);
        out.push_str("\n\n");
    }
    out.push_str("data: [DONE]\n\n");
    let _ = s.write_all(out.as_bytes());
}

fn gateway_bin() -> PathBuf {
    std::env::var("HMG_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_hermes-gateway")))
}

fn spawn_gateway(work: &std::path::Path, port: u16, mock: bool, upstream: u16) -> Proc {
    let home = work.join(format!("home-{port}"));
    std::fs::create_dir_all(&home).unwrap();
    let mut c = Command::new(gateway_bin());
    if mock {
        c.arg("--mock");
    }
    c.env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("API_SERVER_KEY", KEY)
        .env("HERMES_HOME", &home)
        .env("HERMES_RS_HOST", "127.0.0.1")
        .env("HERMES_RS_PORT", port.to_string())
        .env("HERMES_RS_MCP", "0")
        .env("HERMES_RS_DAILY_CALL_CAP", "1000000000")
        .env("HERMES_RS_SESSION_USD_CAP", "0")
        .env("HERMES_RS_DAILY_USD_CAP", "0")
        // never a paid provider: loopback stub only
        .env("HERMES_RS_PROVIDER_LOOPBACK_ONLY", "1")
        .env(
            "OPENROUTER_BASE_URL",
            format!("http://127.0.0.1:{upstream}/v1"),
        )
        .stdout(Stdio::null())
        .stderr(
            std::fs::File::create(work.join(format!("gw-{port}.log")))
                .map(Stdio::from)
                .unwrap_or(Stdio::null()),
        );
    Proc(c.spawn().expect("spawn hermes-gateway"))
}

fn wait_up(port: u16) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) {
        if http(port, "GET", "/health", &[], "").0 == 200 {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("gateway on {port} did not come up");
}

#[derive(Default)]
struct Counts {
    sent: AtomicU64,
    ok: AtomicU64,
    c502: AtomicU64,
    c429: AtomicU64,
    other: AtomicU64,
}

impl Counts {
    fn add(&self, code: u16) {
        self.sent.fetch_add(1, Ordering::Relaxed);
        match code {
            200 | 202 => &self.ok,
            502 => &self.c502,
            429 => &self.c429,
            _ => &self.other,
        }
        .fetch_add(1, Ordering::Relaxed);
    }
    fn line(&self) -> String {
        format!(
            "sent {} ok {} 502 {} 429 {} other {}",
            self.sent.load(Ordering::Relaxed),
            self.ok.load(Ordering::Relaxed),
            self.c502.load(Ordering::Relaxed),
            self.c429.load(Ordering::Relaxed),
            self.other.load(Ordering::Relaxed)
        )
    }
}

/// Paced load: `rps` operations per second spread over 3 threads.
fn load(
    port: u16,
    mock: bool,
    rps: u64,
    stop: Arc<AtomicBool>,
    counts: Arc<Counts>,
) -> Vec<std::thread::JoinHandle<()>> {
    let per = Duration::from_micros(3_000_000 / rps.max(1));
    (0..3u64)
        .map(|t| {
            let stop = stop.clone();
            let counts = counts.clone();
            std::thread::spawn(move || {
                let mut i: u64 = t;
                let mut next = Instant::now();
                while !stop.load(Ordering::SeqCst) {
                    i += 3;
                    let code = if mock && i % 4 == 0 {
                        chat(port, &format!("mock-fail {i}"), None).0
                    } else if i % 8 == 1 {
                        let b = serde_json::json!({"input": format!("run {i}")}).to_string();
                        http(port, "POST", "/v1/runs", &[], &b).0
                    } else if i % 16 == 5 {
                        // a short multi-turn session (stored transcript re-read each turn)
                        let sid = format!("hmg-{port}-{i}");
                        let mut c = 0;
                        for k in 0..6 {
                            c = chat(port, &format!("turn {k} of {i}"), Some(&sid)).0;
                        }
                        c
                    } else if i % 32 == 7 {
                        let a = http(port, "GET", "/health", &[], "").0;
                        let b = http(port, "GET", "/v1/models", &[], "").0;
                        if a == 200 {
                            b
                        } else {
                            a
                        }
                    } else {
                        chat(port, &format!("hi {i}"), None).0
                    };
                    counts.add(code);
                    next += per;
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
            })
        })
        .collect()
}

/// Least-squares slope (kB per minute) of (seconds, kB) points.
fn slope_kb_per_min(p: &[(f64, f64)]) -> f64 {
    let n = p.len() as f64;
    if n < 2.0 {
        return 0.0;
    }
    let mx = p.iter().map(|x| x.0).sum::<f64>() / n;
    let my = p.iter().map(|x| x.1).sum::<f64>() / n;
    let num: f64 = p.iter().map(|x| (x.0 - mx) * (x.1 - my)).sum();
    let den: f64 = p.iter().map(|x| (x.0 - mx).powi(2)).sum();
    if den == 0.0 {
        0.0
    } else {
        num / den * 60.0
    }
}

fn mean(p: &[(f64, f64)]) -> f64 {
    p.iter().map(|x| x.1).sum::<f64>() / (p.len().max(1) as f64)
}

#[test]
fn hermes_gateway_rss_flat_under_steady_load() {
    if !real() {
        eprintln!("skipped: set GALION_REAL=1 (real binary soak)");
        return;
    }
    let base = envu("HMG_PORT_BASE", 20680) as u16;
    let (p_mock, p_stub, p_up) = (base, base + 1, base + 2);
    let soak = Duration::from_secs(envu("HMG_SOAK_MIN", 20) * 60);
    let warm = Duration::from_secs(envu("HMG_WARM_S", 120));
    let rps = envu("HMG_RPS", 30);
    let expect_growth = std::env::var("HMG_EXPECT_GROWTH").is_ok_and(|v| v == "1");
    let only = std::env::var("HMG_ONLY").unwrap_or_default();
    let work = std::env::temp_dir().join(format!("hmg-{}-{}", std::process::id(), base));
    std::fs::create_dir_all(&work).unwrap();
    println!(
        "-- start {} bin {} soak {:?} warm {:?} rps {} ports {}-{}",
        zagreb_now(),
        gateway_bin().display(),
        soak,
        warm,
        rps,
        p_mock,
        p_up
    );

    let stop_up = Arc::new(AtomicBool::new(false));
    let hits = Arc::new(AtomicU64::new(0));
    stub_upstream(p_up, stop_up.clone(), hits.clone());

    let mut gws: Vec<(&str, u16, bool, Proc)> = Vec::new();
    if only != "stub" {
        gws.push((
            "mock",
            p_mock,
            true,
            spawn_gateway(&work, p_mock, true, p_up),
        ));
    }
    if only != "mock" {
        gws.push((
            "stub",
            p_stub,
            false,
            spawn_gateway(&work, p_stub, false, p_up),
        ));
    }
    for g in &gws {
        wait_up(g.1);
    }
    // sanity: the production path really answers from the stub upstream
    if only != "mock" {
        let (c, b) = chat(p_stub, "sanity", None);
        assert_eq!(c, 200, "stub gateway chat: {b}");
        assert!(b.contains("stub answer ok"), "stub reply not relayed: {b}");
    }

    let stop = Arc::new(AtomicBool::new(false));
    let mut joins = Vec::new();
    let counts: Vec<Arc<Counts>> = gws.iter().map(|_| Arc::new(Counts::default())).collect();
    for (g, c) in gws.iter().zip(&counts) {
        joins.extend(load(g.1, g.2, rps, stop.clone(), c.clone()));
    }

    let t0 = Instant::now();
    let mut samples: Vec<Vec<(f64, f64)>> = vec![Vec::new(); gws.len()];
    let start_res: Vec<(u64, u64, u64)> = gws
        .iter()
        .map(|g| {
            (
                rss_kb(g.3 .0.id()).unwrap_or(0),
                threads(g.3 .0.id()),
                fds(g.3 .0.id()),
            )
        })
        .collect();
    let mut warm_res = start_res.clone();
    let mut warmed = false;
    while t0.elapsed() < soak {
        std::thread::sleep(Duration::from_secs(15));
        let el = t0.elapsed().as_secs_f64();
        let mut line = format!("  t={:>5.0}s", el);
        for (k, g) in gws.iter().enumerate() {
            let pid = g.3 .0.id();
            let r = rss_kb(pid).expect("gateway died");
            if t0.elapsed() >= warm {
                samples[k].push((el, r as f64));
            }
            line.push_str(&format!(
                " | {} rss {} kB thr {} fd {} [{}]",
                g.0,
                r,
                threads(pid),
                fds(pid),
                counts[k].line()
            ));
        }
        if !warmed && t0.elapsed() >= warm {
            warmed = true;
            warm_res = gws
                .iter()
                .map(|g| {
                    (
                        rss_kb(g.3 .0.id()).unwrap_or(0),
                        threads(g.3 .0.id()),
                        fds(g.3 .0.id()),
                    )
                })
                .collect();
        }
        println!("{line}");
    }
    stop.store(true, Ordering::SeqCst);
    for j in joins {
        let _ = j.join();
    }

    let mut fails = Vec::new();
    for (k, g) in gws.iter().enumerate() {
        let pid = g.3 .0.id();
        let s = &samples[k];
        let q = (s.len() / 4).max(1);
        let slope = slope_kb_per_min(s);
        let delta = mean(&s[s.len() - q..]) - mean(&s[..q]);
        let end = (rss_kb(pid).unwrap_or(0), threads(pid), fds(pid));
        println!(
            "-- {} :{} start rss {} kB thr {} fd {} | warm {} kB thr {} fd {} | end {} kB thr {} fd {} | slope {:.1} kB/min | last-first quarter {:+.0} kB | {} | samples {}",
            g.0, g.1, start_res[k].0, start_res[k].1, start_res[k].2,
            warm_res[k].0, warm_res[k].1, warm_res[k].2,
            end.0, end.1, end.2, slope, delta, counts[k].line(), s.len()
        );
        let sent = counts[k].sent.load(Ordering::Relaxed);
        let other = counts[k].other.load(Ordering::Relaxed);
        if sent == 0 || other * 100 > sent {
            fails.push(format!("{}: bad replies ({})", g.0, counts[k].line()));
        }
        let flat = slope < 256.0 && delta < 4096.0;
        if expect_growth && g.2 {
            if flat {
                fails.push(format!(
                    "{}: positive control expected growth, got slope {slope:.1} kB/min",
                    g.0
                ));
            }
        } else if !flat {
            fails.push(format!(
                "{}: RSS not flat: slope {slope:.1} kB/min (limit 256), last-first quarter {delta:+.0} kB (limit 4096)",
                g.0
            ));
        }
    }
    if only != "mock" {
        println!("-- stub upstream hits {}", hits.load(Ordering::SeqCst));
    }
    println!("-- end {}", zagreb_now());
    drop(gws);
    stop_up.store(true, Ordering::SeqCst);
    let _ = std::fs::remove_dir_all(&work);
    assert!(fails.is_empty(), "{fails:#?}");
}
