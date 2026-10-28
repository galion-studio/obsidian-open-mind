// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Downtime probe: one `GET <path>` every `interval` (sleeping between tries, no
//! busy loop) until a stop file appears or `max` elapses. A try is OK on a 2xx
//! status line. Prints every outage window (consecutive failed tries) with Zagreb
//! timestamps, the failure kinds, and the longest gap between two OK answers.

use crate::zagreb;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Refused,
    Timeout,
    Http(u16),
    Other,
}

pub fn try_once(addr: SocketAddr, path: &str, timeout: Duration) -> Outcome {
    let mut s = match TcpStream::connect_timeout(&addr, timeout) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return Outcome::Refused,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => return Outcome::Timeout,
        Err(_) => return Outcome::Other,
    };
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(timeout));
    let req = format!("GET {path} HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return Outcome::Other;
    }
    let mut buf = [0u8; 64];
    let mut got = 0;
    while got < 12 {
        match s.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Outcome::Timeout
            }
            Err(_) => return Outcome::Other,
        }
    }
    let head = String::from_utf8_lossy(&buf[..got]);
    match head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
    {
        Some(c) if (200..300).contains(&c) => Outcome::Ok,
        Some(c) => Outcome::Http(c),
        None => Outcome::Other,
    }
}

#[derive(Default, Debug)]
pub struct Summary {
    pub tries: u64,
    pub ok: u64,
    pub windows: Vec<Window>,
    pub longest_ok_gap_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub first_fail_ms: i64,
    pub last_fail_ms: i64,
    pub next_ok_ms: Option<i64>,
    pub fails: u64,
    pub kinds: Vec<(Outcome, u64)>,
}

impl Window {
    fn bump(&mut self, o: Outcome) {
        match self.kinds.iter_mut().find(|(k, _)| *k == o) {
            Some((_, n)) => *n += 1,
            None => self.kinds.push((o, 1)),
        }
    }
}

pub fn run(
    addr: SocketAddr,
    path: &str,
    interval: Duration,
    stop: &Path,
    max: Duration,
) -> Summary {
    let t0 = Instant::now();
    let mut sum = Summary::default();
    let mut last_ok: Option<i64> = None;
    let mut cur: Option<Window> = None;
    while t0.elapsed() < max && !stop.exists() {
        let start = zagreb::now_ms();
        let o = try_once(addr, path, Duration::from_millis(500));
        sum.tries += 1;
        if o == Outcome::Ok {
            sum.ok += 1;
            if let Some(prev) = last_ok {
                sum.longest_ok_gap_ms = sum.longest_ok_gap_ms.max((start - prev) as u64);
            }
            last_ok = Some(start);
            if let Some(mut w) = cur.take() {
                w.next_ok_ms = Some(start);
                sum.windows.push(w);
            }
        } else {
            let w = cur.get_or_insert_with(|| Window {
                first_fail_ms: start,
                last_fail_ms: start,
                next_ok_ms: None,
                fails: 0,
                kinds: Vec::new(),
            });
            w.last_fail_ms = start;
            w.fails += 1;
            w.bump(o);
        }
        let spent = Duration::from_millis((zagreb::now_ms() - start).max(0) as u64);
        std::thread::sleep(interval.saturating_sub(spent));
    }
    if let Some(w) = cur.take() {
        sum.windows.push(w);
    }
    sum
}

pub fn report(addr: SocketAddr, path: &str, interval: Duration, s: &Summary) -> String {
    let mut out = format!(
        "probe http://{addr}{path} interval={}ms tries={} ok={} fail={} outages={}\n",
        interval.as_millis(),
        s.tries,
        s.ok,
        s.tries - s.ok,
        s.windows.len()
    );
    for w in &s.windows {
        let kinds: Vec<String> = w.kinds.iter().map(|(k, n)| format!("{k:?}x{n}")).collect();
        let span = w.last_fail_ms - w.first_fail_ms;
        match w.next_ok_ms {
            Some(ok) => out += &format!(
                "outage first_fail={} last_fail={} next_ok={} fail_span_ms={span} until_ok_ms={} tries={} kinds={}\n",
                zagreb::stamp_ms(w.first_fail_ms),
                zagreb::stamp_ms(w.last_fail_ms),
                zagreb::stamp_ms(ok),
                ok - w.first_fail_ms,
                w.fails,
                kinds.join(",")
            ),
            None => out += &format!(
                "outage first_fail={} last_fail={} next_ok=never fail_span_ms={span} tries={} kinds={}\n",
                zagreb::stamp_ms(w.first_fail_ms),
                zagreb::stamp_ms(w.last_fail_ms),
                w.fails,
                kinds.join(",")
            ),
        }
    }
    out += &format!("longest_gap_between_ok_ms={}\n", s.longest_ok_gap_ms);
    out
}
