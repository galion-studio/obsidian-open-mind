// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Wire golden for one-talk's POST client (now galion-common's): the exact
//! request bytes for warm, hear and reply, and the outcome against odd
//! servers. Valid cases are asserted; every case is printed
//! (`--nocapture`) so runs before and after a change can be diffed.
//! Spare loopback ports only (never 8002, 8642, 8643).

use one_talk::{b64_encode, hear_exchange, reply_exchange, warm_exchange, SYNTH_PATH};

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

const CHAT: &str = r#"{"choices":[{"message":{"content":"Yes, I am here. More words."}}]}"#;

fn wav() -> Reply {
    Reply::Bytes(
        b"HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: 7\r\n\r\nFAKEWAV".to_vec(),
    )
}

#[test]
fn post_client_wire_golden() {
    let t = Duration::from_secs(2);
    // warm: exact bytes.
    let (addr, rx) = capture(vec![wav()]);
    assert_eq!(warm_exchange(addr, t).unwrap(), 200);
    let wire = drain(&rx);
    let body = one_talk::warm_request_body();
    let mut want = format!(
        "POST {SYNTH_PATH} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n",
        body.len()
    )
    .into_bytes();
    want.extend_from_slice(&body);
    assert_eq!(wire, vec![want]);
    report("warm", &wire, "200");

    // hear: multipart upload.
    let (addr, rx) = capture(vec![ok_json(r#"{"text":"Hello there."}"#)]);
    let out = hear_exchange(addr, b"RIFFtest", t).unwrap();
    let wire = drain(&rx);
    let (ct, body) = one_talk::hear_multipart(b"RIFFtest");
    let mut want = format!(
        "POST /v1/audio/transcriptions HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: {ct}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    want.extend_from_slice(&body);
    assert_eq!(wire, vec![want]);
    report("hear", &wire, &out.to_string());

    // reply: chat with the caller's bearer, then synthesize.
    let (addr, rx) = capture(vec![ok_json(CHAT), wav()]);
    let out = reply_exchange(
        addr,
        "good evening",
        "Stay brief.",
        "fixture-not-a-secret",
        t,
    )
    .unwrap();
    assert_eq!(out["audio"], b64_encode(b"FAKEWAV"));
    let wire = drain(&rx);
    assert_eq!(wire.len(), 2);
    let head = String::from_utf8_lossy(&wire[0]);
    assert!(head.starts_with(&format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: "
    )));
    assert!(head.contains(
        "\r\nContent-Type: application/json\r\nAuthorization: Bearer fixture-not-a-secret\r\n\r\n{"
    ));
    report("reply", &wire, &out.to_string());

    // Odd servers.
    let big = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: 9000000\r\n\r\n{}",
        "x".repeat(9_000_000)
    );
    let cases: Vec<(&str, Reply)> = vec![
        ("chunked", chunked_json(r#"{"ok":true}"#)),
        (
            "no content-length, close",
            raw(b"HTTP/1.1 200 OK\r\n\r\nFAKEWAV"),
        ),
        (
            "404",
            raw(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\n\r\nno"),
        ),
        ("garbage", raw(b"NOPE\r\n\r\n")),
        ("status line only", raw(b"HTTP/1.1 503 Busy\r\n")),
        ("empty", raw(b"")),
        (
            "silent and kept open",
            Reply::Hold(Vec::new(), Duration::from_secs(3)),
        ),
        (
            "complete but kept open",
            Reply::Hold(
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec(),
                Duration::from_secs(3),
            ),
        ),
        ("9 MB body", raw(big.as_bytes())),
    ];
    for (case, reply) in cases {
        let (addr, rx) = capture(vec![reply.clone()]);
        let started = std::time::Instant::now();
        let warm = warm_exchange(addr, Duration::from_millis(1500));
        let slow = if started.elapsed() >= Duration::from_secs(1) {
            " (slow)"
        } else {
            ""
        };
        let warm = match warm {
            Ok(c) => format!("Ok({c})"),
            Err(e) => format!("Err({:?})", e.kind()),
        };
        report(
            &format!("warm {case}"),
            &drain(&rx),
            &format!("{warm}{slow}"),
        );
        let (addr, rx) = capture(vec![reply]);
        let hear = hear_exchange(addr, b"RIFF", Duration::from_millis(1500))
            .map(|v| v.to_string())
            .map_err(|e| format!("{:?}", e.kind()));
        report(&format!("hear {case}"), &drain(&rx), &format!("{hear:?}"));
    }
}
