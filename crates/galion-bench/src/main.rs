// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! galion-bench: closed-loop HTTP/1.1 load generator (std only).
//!
//!   galion-bench [-c CONC] [-n TOTAL] [-m METHOD] [-H 'Name: value']...
//!                [--body JSON | --body-file PATH] [--auth-env VAR] http://host:port/path
//!
//! `--auth-env VAR` sends `Authorization: Bearer $VAR`; the value is read from
//! the environment and never printed. Plain http only (loopback services).
//! Prints one JSON line: ok, errors, status counts, rps, p50/p90/p99/max ms.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Target {
    addr: std::net::SocketAddr,
    host: String,
    path: String,
}

fn parse_url(u: &str) -> Result<Target, String> {
    let rest = u.strip_prefix("http://").ok_or("only http:// URLs")?;
    let (hp, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let hp_full = if hp.contains(':') {
        hp.to_string()
    } else {
        format!("{hp}:80")
    };
    let addr = hp_full
        .to_socket_addrs()
        .map_err(|e| format!("resolve {hp}: {e}"))?
        .next()
        .ok_or("no address")?;
    Ok(Target {
        addr,
        host: hp.to_string(),
        path: path.to_string(),
    })
}

/// One request; returns the HTTP status (0 on transport error).
fn one(t: &Target, raw: &[u8], timeout: Duration) -> u16 {
    let Ok(mut s) = TcpStream::connect_timeout(&t.addr, timeout) else {
        return 0;
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let _ = s.set_nodelay(true);
    if s.write_all(raw).is_err() {
        return 0;
    }
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];
    let mut want: Option<usize> = None;
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if want.is_none() {
                    if let Some(end) = find(&buf, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                        want = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .map(|l| end + 4 + l);
                    }
                }
                if want.is_some_and(|w| buf.len() >= w) {
                    break;
                }
            }
            Err(_) => return 0,
        }
    }
    let line = String::from_utf8_lossy(&buf[..buf.len().min(32)]).to_string();
    line.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

/// Claim one of the remaining requests (CAS loop; false when none are left).
fn take_one(left: &AtomicUsize) -> bool {
    let mut v = left.load(Ordering::SeqCst);
    while v > 0 {
        match left.compare_exchange(v, v - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(cur) => v = cur,
        }
    }
    false
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn pct(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut conc, mut total, mut method) = (8usize, 1000usize, String::from("GET"));
    let (mut headers, mut body, mut url, mut timeout_ms) =
        (Vec::<String>::new(), Vec::<u8>::new(), None, 10_000u64);
    let mut i = 0;
    let next = |i: &mut usize| -> String {
        *i += 1;
        args.get(*i).cloned().unwrap_or_else(|| {
            eprintln!("missing value for {}", args[*i - 1]);
            std::process::exit(64)
        })
    };
    while i < args.len() {
        match args[i].as_str() {
            "-c" => conc = next(&mut i).parse().unwrap_or(8),
            "-n" => total = next(&mut i).parse().unwrap_or(1000),
            "-m" => method = next(&mut i).to_ascii_uppercase(),
            "-H" => headers.push(next(&mut i)),
            "-t" => timeout_ms = next(&mut i).parse().unwrap_or(10_000),
            "--body" => body = next(&mut i).into_bytes(),
            "--body-file" => {
                let p = next(&mut i);
                body = std::fs::read(&p).unwrap_or_else(|e| {
                    eprintln!("read {p}: {e}");
                    std::process::exit(66)
                })
            }
            "--auth-env" => {
                let v = next(&mut i);
                match std::env::var(&v) {
                    Ok(k) if !k.trim().is_empty() => {
                        headers.push(format!("Authorization: Bearer {}", k.trim()))
                    }
                    _ => {
                        eprintln!("env {v} is empty");
                        std::process::exit(64)
                    }
                }
            }
            "-h" | "--help" => {
                println!("usage: galion-bench [-c CONC] [-n TOTAL] [-m METHOD] [-t TIMEOUT_MS] [-H 'K: V'] [--body S|--body-file P] [--auth-env VAR] http://host:port/path");
                return;
            }
            u => url = Some(u.to_string()),
        }
        i += 1;
    }
    let Some(url) = url else {
        eprintln!("missing URL");
        std::process::exit(64)
    };
    let t = match parse_url(&url) {
        Ok(t) => Arc::new(t),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(64)
        }
    };
    let mut raw = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: galion-bench\r\n",
        t.path, t.host
    );
    for h in &headers {
        raw.push_str(h);
        raw.push_str("\r\n");
    }
    if !body.is_empty() || method == "POST" {
        raw.push_str("Content-Type: application/json\r\n");
        raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    raw.push_str("\r\n");
    let mut raw = raw.into_bytes();
    raw.extend_from_slice(&body);
    let raw = Arc::new(raw);
    let left = Arc::new(AtomicUsize::new(total));
    let lat = Arc::new(Mutex::new(Vec::with_capacity(total)));
    let codes = Arc::new(Mutex::new(std::collections::BTreeMap::<u16, usize>::new()));
    let timeout = Duration::from_millis(timeout_ms);
    let start = Instant::now();
    let workers: Vec<_> = (0..conc.max(1))
        .map(|_| {
            let (t, raw, left, lat, codes) = (
                t.clone(),
                raw.clone(),
                left.clone(),
                lat.clone(),
                codes.clone(),
            );
            std::thread::spawn(move || {
                let mut mine = Vec::new();
                let mut mc = std::collections::BTreeMap::<u16, usize>::new();
                while take_one(&left) {
                    let t0 = Instant::now();
                    let code = one(&t, &raw, timeout);
                    mine.push(t0.elapsed().as_secs_f64() * 1000.0);
                    *mc.entry(code).or_default() += 1;
                }
                lat.lock().unwrap().extend(mine);
                let mut c = codes.lock().unwrap();
                for (k, v) in mc {
                    *c.entry(k).or_default() += v;
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    let secs = start.elapsed().as_secs_f64();
    let mut l = lat.lock().unwrap().clone();
    l.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let codes = codes.lock().unwrap();
    let ok: usize = codes
        .iter()
        .filter(|(k, _)| (200..300).contains(*k))
        .map(|(_, v)| v)
        .sum();
    let status: Vec<String> = codes.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    println!(
        "{{\"url_path\":\"{}\",\"method\":\"{method}\",\"concurrency\":{conc},\"requests\":{},\"ok\":{ok},\"status\":{{{}}},\"secs\":{:.3},\"rps\":{:.1},\"p50_ms\":{:.3},\"p90_ms\":{:.3},\"p99_ms\":{:.3},\"max_ms\":{:.3}}}",
        t.path.replace('"', ""),
        l.len(),
        status.join(","),
        secs,
        l.len() as f64 / secs.max(1e-9),
        pct(&l, 50.0),
        pct(&l, 90.0),
        pct(&l, 99.0),
        l.last().copied().unwrap_or(0.0)
    );
}
