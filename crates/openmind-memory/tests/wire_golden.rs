// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Wire golden for `tools::post_json` (the parity client, now on
//! galion-common's client): exact request bytes and the outcome against odd
//! servers. Valid cases are asserted; every case is printed (`--nocapture`)
//! so runs before and after a change can be diffed.

use openmind_memory::tools::post_json;
use serde_json::json;

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

#[test]
fn post_json_wire_golden() {
    let body = json!({"query": "sail", "limit": 5});
    let (addr, rx) = capture(vec![ok_json(r#"{"results":"r","total":1}"#)]);
    let base = format!("http://{addr}/");
    let out = post_json(
        &base,
        "/search/memories",
        &body,
        Some("fixture-not-a-secret"),
    )
    .unwrap();
    assert_eq!(out, (200, r#"{"results":"r","total":1}"#.to_string()));
    let p = body.to_string();
    let want = format!(
        "POST /search/memories HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthorization: Bearer fixture-not-a-secret\r\nConnection: close\r\n\r\n{p}",
        p.len()
    );
    assert_eq!(drain(&rx), vec![want.into_bytes()]);

    let cases: Vec<(&str, Reply)> = vec![
        ("200", ok_json(r#"{"context":"c"}"#)),
        ("chunked", chunked_json(r#"{"context":"c"}"#)),
        (
            "401",
            raw(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\n\r\n{}"),
        ),
        (
            "no content-length, close",
            raw(b"HTTP/1.1 200 OK\r\n\r\n{\"a\":1}"),
        ),
        ("garbage", raw(b"NOPE\r\n\r\n")),
        ("status line only", raw(b"HTTP/1.1 503 Busy\r\n")),
        ("empty", raw(b"")),
        (
            "complete reply, socket held open 2 s",
            Reply::Hold(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}".to_vec(),
                Duration::from_secs(2),
            ),
        ),
    ];
    for (case, reply) in cases {
        let (addr, rx) = capture(vec![reply]);
        let out = post_json(
            &format!("http://{addr}"),
            "/recall",
            &json!({"query": "q"}),
            None,
        );
        report(case, &drain(&rx), &format!("{out:?}"));
    }
    let out = post_json("https://127.0.0.1:1", "/recall", &json!({}), None);
    println!("OUT  https base {out:?}");
    assert!(out.is_err());
}
