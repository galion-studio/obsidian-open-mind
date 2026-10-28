// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `Limits::evict_idle` (slot 236): a connection flood that never sends a
//! request must not lock real clients out for the head deadline.

use galion_common::http::{self, ClientRequest, Limits, Response};
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn server(evict: bool, max: usize, slow_ms: u64) -> (SocketAddr, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    let limits = Limits {
        max_connections: max,
        head_timeout: Duration::from_secs(5),
        evict_idle: evict,
        ..Limits::default()
    };
    std::thread::spawn(move || {
        http::serve(
            l,
            limits,
            Arc::new(move |req: &http::Request| {
                h.fetch_add(1, Ordering::SeqCst);
                if req.path() == "/slow" {
                    std::thread::sleep(Duration::from_millis(slow_ms));
                }
                Response::text(200, "ok")
            }),
        )
    });
    (addr, hits)
}

fn idle(addr: SocketAddr, n: usize) -> Vec<TcpStream> {
    let v: Vec<TcpStream> = (0..n).map(|_| TcpStream::connect(addr).unwrap()).collect();
    std::thread::sleep(Duration::from_millis(150)); // let the server accept them
    v
}

fn get(addr: SocketAddr, path: &str) -> (u16, Duration) {
    let t = Instant::now();
    let r = http::send(
        addr,
        &ClientRequest::get(path, "x"),
        Duration::from_secs(8),
        4096,
    )
    .map(|r| r.status)
    .unwrap_or(0);
    (r, t.elapsed())
}

#[test]
#[cfg_attr(miri, ignore = "binds a TCP socket; Miri isolation has no socket")]
fn without_eviction_idle_flood_gives_503() {
    let (addr, _) = server(false, 4, 0);
    let _flood = idle(addr, 4);
    assert_eq!(get(addr, "/x").0, 503);
}

#[test]
#[cfg_attr(miri, ignore = "binds a TCP socket; Miri isolation has no socket")]
fn eviction_serves_real_clients_through_an_idle_flood() {
    let (addr, hits) = server(true, 4, 0);
    let mut flood = idle(addr, 4);
    for _ in 0..20 {
        let (code, took) = get(addr, "/x");
        assert_eq!(code, 200);
        assert!(took < Duration::from_millis(500), "{took:?}");
    }
    assert_eq!(hits.load(Ordering::SeqCst), 20);
    // the evicted flood connections were closed by the server (EOF), not left hanging
    let mut closed = 0;
    for s in flood.iter_mut() {
        s.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut b = [0u8; 16];
        if matches!(s.read(&mut b), Ok(0) | Err(_)) {
            closed += 1;
        }
    }
    assert_eq!(closed, 4);
}

#[test]
#[cfg_attr(miri, ignore = "binds a TCP socket; Miri isolation has no socket")]
fn requests_in_the_handler_are_never_evicted() {
    // 4 slots, all busy in the handler (not reading): nothing to evict -> 503,
    // and the busy requests still complete.
    let (addr, _) = server(true, 4, 800);
    let busy: Vec<_> = (0..4)
        .map(|_| std::thread::spawn(move || get(addr, "/slow").0))
        .collect();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(get(addr, "/x").0, 503);
    for b in busy {
        assert_eq!(b.join().unwrap(), 200);
    }
    assert_eq!(get(addr, "/x").0, 200);
}

#[test]
#[cfg_attr(miri, ignore = "binds a TCP socket; Miri isolation has no socket")]
fn live_connections_stay_bounded_under_a_reconnecting_flood() {
    let (addr, _) = server(true, 4, 0);
    // 200 idle connects in a row: each evicts an older one; the server never
    // holds more than 2 x max at once and a real request still gets through.
    let mut held = Vec::new();
    for _ in 0..200 {
        if let Ok(s) = TcpStream::connect(addr) {
            held.push(s);
        }
    }
    std::thread::sleep(Duration::from_millis(200));
    let (code, took) = get(addr, "/x");
    assert_eq!(code, 200);
    assert!(took < Duration::from_millis(500), "{took:?}");
    // count server-side open: peers not at EOF
    let mut open = 0;
    for s in held.iter_mut() {
        s.set_nonblocking(true).unwrap();
        let mut b = [0u8; 16];
        match s.read(&mut b) {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => open += 1,
            _ => {}
        }
    }
    assert!(open <= 8, "{open} connections still open on the server");
}
