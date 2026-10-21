// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Wire golden for the probes' HTTP client (now galion-common's): the exact
//! request bytes and the probe outcome for normal and odd servers. The valid
//! cases are asserted; every case is printed (`--nocapture`) so a run before
//! and after a change can be diffed line by line.

use openmind_survival::{check_http, http_get};
use std::time::Instant;

// ---- capture server (shared shape across the wire_golden tests) ----
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::mpsc;
use std::time::Duration;

/// How the fake server answers one connection.
#[derive(Clone)]
enum Reply {
    /// Write these bytes, then close.
    Bytes(Vec<u8>),
    /// Write these bytes, then hold the socket open for this long.
    Hold(Vec<u8>, Duration),
}

/// Serve `replies` (one per connection, in order) on a spare loopback port;
/// returns the address and a channel of the raw request bytes received.
fn capture(replies: Vec<Reply>) -> (SocketAddr, mpsc::Receiver<Vec<u8>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut s, _)) = l.accept() else { return };
            let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let head_end = loop {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i + 4);
                }
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            };
            if let Some(end) = head_end {
                let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                let cl = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                while buf.len() < end + cl {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
            }
            let _ = tx.send(buf);
            match reply {
                Reply::Bytes(b) => {
                    let _ = s.write_all(&b);
                }
                Reply::Hold(b, d) => {
                    let _ = s.write_all(&b);
                    std::thread::sleep(d);
                }
            }
        }
    });
    (addr, rx)
}

fn ok_json(body: &str) -> Reply {
    Reply::Bytes(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
    )
}

fn chunked_json(body: &str) -> Reply {
    let (a, b) = body.split_at(body.len() / 2);
    Reply::Bytes(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
            a.len(),
            b.len()
        )
        .into_bytes(),
    )
}

fn raw(b: &[u8]) -> Reply {
    Reply::Bytes(b.to_vec())
}

/// Printed per case so two runs (before and after a change) can be diffed.
fn report(case: &str, wire: &[Vec<u8>], out: &str) {
    for (i, w) in wire.iter().enumerate() {
        println!("WIRE {case} #{i} {:?}", String::from_utf8_lossy(w));
    }
    println!("OUT  {case} {out}");
}

fn drain(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    while let Ok(b) = rx.recv_timeout(Duration::from_millis(200)) {
        v.push(b);
    }
    v
}

fn check(case: &str, reply: Reply) -> String {
    let (addr, rx) = capture(vec![reply]);
    let t = Instant::now();
    let r = check_http(case, addr.port(), "/health");
    let slow = if t.elapsed() >= Duration::from_secs(1) {
        " (slow)"
    } else {
        ""
    };
    let out = format!("ok={} detail={:?}{slow}", r.ok, r.detail);
    report(&format!("check_http {case}"), &drain(&rx), &out);
    out
}

fn get(case: &str, reply: Reply) -> String {
    let (addr, rx) = capture(vec![reply]);
    let t = Instant::now();
    let r = http_get(addr, "/v1/health?x=1", Duration::from_millis(1500));
    let slow = if t.elapsed() >= Duration::from_secs(1) {
        " (slow)"
    } else {
        ""
    };
    let out = format!("{:?}{slow}", r.result);
    report(&format!("http_get {case}"), &drain(&rx), &out);
    out
}

#[test]
fn probe_client_wire_golden() {
    // Exact request bytes, unchanged since the hand-rolled client.
    let (addr, rx) = capture(vec![ok_json("{\"ok\":true}")]);
    assert_eq!(check_http("w", addr.port(), "/health").detail, "status 200");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    );
    let (addr, rx) = capture(vec![ok_json("{\"ok\":true}")]);
    let r = http_get(addr, "/v1/health", Duration::from_secs(2));
    assert_eq!(r.result, Ok((200, "{\"ok\":true}".to_string())));
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        format!("GET /v1/health HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n")
            .into_bytes()
    );

    let pad = "p".repeat(1000);
    let big = "b".repeat(10_000);
    let cases: Vec<(&str, Reply)> = vec![
        ("200", ok_json("{\"ok\":true}")),
        (
            "302",
            raw(b"HTTP/1.1 302 Found\r\nLocation: /x\r\nContent-Length: 0\r\n\r\n"),
        ),
        (
            "404",
            raw(b"HTTP/1.1 404 Not Found\r\nContent-Length: 3\r\n\r\nno\n"),
        ),
        (
            "500",
            raw(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"),
        ),
        (
            "1 KB header",
            raw(
                format!("HTTP/1.1 200 OK\r\nX-Pad: {pad}\r\nContent-Length: 2\r\n\r\nok")
                    .as_bytes(),
            ),
        ),
        ("chunked", chunked_json("{\"status\":\"ok\",\"n\":1}")),
        (
            "10 KB body",
            raw(format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{big}",
                big.len()
            )
            .as_bytes()),
        ),
        (
            "no reason",
            raw(b"HTTP/1.1 200\r\nContent-Length: 2\r\n\r\nok"),
        ),
        (
            "LF-only head",
            raw(b"HTTP/1.0 200 OK\nContent-Length: 2\n\nok"),
        ),
        (
            "status line only",
            raw(b"HTTP/1.0 503 Service Unavailable\r\n"),
        ),
        ("garbage", raw(b"NOPE\r\n\r\n")),
        ("empty", raw(b"")),
        (
            "complete but kept open",
            Reply::Hold(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
                Duration::from_secs(4),
            ),
        ),
        (
            "silent and kept open",
            Reply::Hold(Vec::new(), Duration::from_secs(4)),
        ),
    ];
    for (case, reply) in cases {
        check(case, reply.clone());
        get(case, reply);
    }
    // Connection refused: a port that was free a moment ago.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let r = check_http("refused", port, "/");
    println!("OUT  check_http refused ok={} detail={:?}", r.ok, r.detail);
    assert!(!r.ok && r.detail.starts_with("connect: "), "{}", r.detail);
}
