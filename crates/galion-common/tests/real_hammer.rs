// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Real-life hammer against running services (gated: `GALION_REAL=1`).
//!
//! `GALION_REAL_TARGETS` is a comma list of `name=host:port`, e.g.
//! `tuner=127.0.0.1:19160,origin=127.0.0.1:19161`. `real_common.rs` (tuner,
//! galion-origin) and `real_migrate.rs` (hermes, memory, desk, room) build the
//! release binaries, start them, call [`hammer`] and tear down.
//! Each target gets: sequential and concurrent GETs (latency), malformed
//! requests, oversized heads/bodies, a slowloris wave larger than the
//! connection cap, and a final health check. Output is a plain report.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn targets() -> Option<Vec<(String, SocketAddr)>> {
    if std::env::var("GALION_REAL").ok().as_deref() != Some("1") {
        return None;
    }
    let list = std::env::var("GALION_REAL_TARGETS").ok()?;
    Some(
        list.split(',')
            .filter_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                Some((k.to_string(), v.parse().ok()?))
            })
            .collect(),
    )
}

/// Send raw bytes, read the reply (or nothing). Returns (status, elapsed).
fn raw(addr: SocketAddr, bytes: &[u8], timeout: Duration) -> (Option<u16>, Duration) {
    let t = Instant::now();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, timeout) else {
        return (None, t.elapsed());
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let _ = s.write_all(bytes);
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let status = std::str::from_utf8(out.get(9..12).unwrap_or(&[]))
        .ok()
        .and_then(|c| c.parse().ok());
    (status, t.elapsed())
}

fn get(addr: SocketAddr, path: &str) -> (Option<u16>, Duration) {
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    raw(addr, req.as_bytes(), Duration::from_secs(5))
}

fn pct(v: &mut [Duration], p: f64) -> f64 {
    v.sort();
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v.get(i).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

fn latency(name: &str, label: &str, mut v: Vec<Duration>, ok: usize) {
    let n = v.len();
    println!(
        "{name} {label}: n={n} ok={ok} p50={:.2}ms p99={:.2}ms max={:.2}ms",
        pct(&mut v, 0.5),
        pct(&mut v, 0.99),
        pct(&mut v, 1.0)
    );
}

fn malformed() -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = [
        &b"\r\n\r\n"[..],
        b"GARBAGE\r\n\r\n",
        b"GET\r\n\r\n",
        b"GET /  HTTP/1.1\r\n\r\n",
        b"GET / HTTP/9.9\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost : x\r\n\r\n",
        b"GET / HTTP/1.1\r\nX: a\r\n folded\r\n\r\n",
        b"GET / HTTP/1.1\r\nX: a\x00b\r\n\r\n",
        b"GET /\x7f HTTP/1.1\r\n\r\n",
        b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab",
        b"POST / HTTP/1.1\r\nContent-Length: -5\r\n\r\n",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        b"POST / HTTP/1.1\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n",
        b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03",
        b"GET / HTTP/1.1\r\nHost: x\r\n",
        b"",
    ]
    .iter()
    .map(|b| b.to_vec())
    .collect();
    let mut x = 0x2545_f491_4f6c_dd1du64;
    for i in 0..400 {
        let len = (i * 7) % 600;
        v.push(
            (0..len)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect(),
        );
    }
    v
}

pub(crate) fn hammer(name: &str, addr: SocketAddr) {
    // 1. sequential
    let mut lat = Vec::new();
    let mut ok = 0;
    for _ in 0..2000 {
        let (st, d) = get(addr, "/health");
        ok += usize::from(st == Some(200));
        lat.push(d);
    }
    latency(name, "sequential GET /health", lat, ok);

    // 2. concurrent: 16 clients x 200
    let ok = Arc::new(AtomicUsize::new(0));
    let t0 = Instant::now();
    let hs: Vec<_> = (0..16)
        .map(|_| {
            let ok = ok.clone();
            std::thread::spawn(move || {
                let mut v = Vec::new();
                for _ in 0..200 {
                    let (st, d) = get(addr, "/health");
                    if st == Some(200) {
                        ok.fetch_add(1, Ordering::Relaxed);
                    }
                    v.push(d);
                }
                v
            })
        })
        .collect();
    let lat: Vec<Duration> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
    let wall = t0.elapsed().as_secs_f64();
    println!(
        "{name} concurrent: {:.0} req/s over {wall:.2}s",
        3200.0 / wall
    );
    latency(
        name,
        "concurrent 16x200 GET /health",
        lat,
        ok.load(Ordering::Relaxed),
    );

    // 3. malformed
    let mut codes = std::collections::BTreeMap::new();
    let cases = malformed();
    let t0 = Instant::now();
    for c in &cases {
        let (st, _) = raw(addr, c, Duration::from_secs(12));
        *codes
            .entry(st.map_or("closed".to_string(), |s| s.to_string()))
            .or_insert(0) += 1;
    }
    println!(
        "{name} malformed: {} requests in {:.2}s -> {codes:?}",
        cases.len(),
        t0.elapsed().as_secs_f64()
    );
    println!(
        "{name} after malformed: /health -> {:?}",
        get(addr, "/health").0
    );

    // 4. oversized
    let big_head = format!("GET / HTTP/1.1\r\nX-Big: {}\r\n\r\n", "a".repeat(100_000));
    let many: String = (0..1000).map(|i| format!("H{i}: v\r\n")).collect();
    let many = format!("GET / HTTP/1.1\r\n{many}\r\n");
    let cl_huge = b"POST /health HTTP/1.1\r\nContent-Length: 10000000000\r\n\r\n".to_vec();
    let mut body = b"POST /health HTTP/1.1\r\nContent-Length: 5000000\r\n\r\n".to_vec();
    body.extend(std::iter::repeat_n(b'x', 5_000_000));
    for (label, bytes) in [
        ("100 KB header", big_head.into_bytes()),
        ("1000 headers", many.into_bytes()),
        ("Content-Length 10 GB (no body)", cl_huge),
        ("5 MB body", body),
    ] {
        let (st, d) = raw(addr, &bytes, Duration::from_secs(12));
        println!(
            "{name} oversized {label}: {:?} in {:.1}ms",
            st,
            d.as_secs_f64() * 1000.0
        );
    }

    // 5. slowloris: 80 connections dripping one header byte every 500 ms.
    let t0 = Instant::now();
    let slow: Vec<_> = (0..80)
        .map(|_| {
            std::thread::spawn(move || {
                let Ok(mut s) = TcpStream::connect(addr) else {
                    return ("refused".to_string(), Duration::ZERO);
                };
                let start = Instant::now();
                let _ = s.set_read_timeout(Some(Duration::from_millis(50)));
                let drip =
                    b"GET /health HTTP/1.1\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
                let mut buf = [0u8; 256];
                for b in drip {
                    if s.write_all(&[*b]).is_err() {
                        return ("write cut".to_string(), start.elapsed());
                    }
                    match s.read(&mut buf) {
                        Ok(0) => return ("closed".to_string(), start.elapsed()),
                        Ok(n) => {
                            let code = String::from_utf8_lossy(&buf[..n.min(12)]).into_owned();
                            return (code, start.elapsed());
                        }
                        Err(_) => {}
                    }
                    std::thread::sleep(Duration::from_millis(450));
                }
                ("survived whole drip".to_string(), start.elapsed())
            })
        })
        .collect();
    // Legit traffic while the wave is on.
    std::thread::sleep(Duration::from_millis(500));
    let mut during = std::collections::BTreeMap::new();
    let mut first_ok = None;
    for i in 0..60 {
        let (st, _) = get(addr, "/health");
        if st == Some(200) && first_ok.is_none() {
            first_ok = Some(t0.elapsed());
        }
        *during.entry(format!("{st:?}")).or_insert(0) += 1;
        if i < 59 {
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    let mut outcome = std::collections::BTreeMap::new();
    let mut longest = Duration::ZERO;
    for h in slow {
        let (o, d) = h.join().unwrap();
        longest = longest.max(d);
        *outcome.entry(o).or_insert(0) += 1;
    }
    println!(
        "{name} slowloris x80: outcomes {outcome:?}, longest held {:.1}s",
        longest.as_secs_f64()
    );
    println!(
        "{name} legit GETs during slowloris (60 over 15s): {during:?}, first 200 at {:?}",
        first_ok.map(|d| format!("{:.1}s", d.as_secs_f64()))
    );
    println!("{name} final /health -> {:?}", get(addr, "/health").0);
}

#[test]
fn real_hammer() {
    let Some(targets) = targets() else {
        eprintln!("skipped: set GALION_REAL=1 and GALION_REAL_TARGETS");
        return;
    };
    for (name, addr) in targets {
        hammer(&name, addr);
    }
}
