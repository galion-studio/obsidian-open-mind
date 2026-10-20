// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Hear, reply, and warm shapes against a fake listener on 127.0.0.1:18002.
//! Never 8002, 8642, or 8643. The listener is stopped before the test returns.

use one_talk::{
    b64_encode, chat_content, empty_reply, fault_reply, hear_exchange, reply_exchange,
    voice_setting, warm_exchange, HEAR_PATH, REPLY_PATH, SYNTH_PATH,
};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const SPARE: u16 = 18002;

struct Hit {
    path: String,
    content_type: String,
    authorization: String,
    header: String,
    body: Vec<u8>,
}

struct Stop {
    port: u16,
    join: Option<JoinHandle<()>>,
}

impl Drop for Stop {
    fn drop(&mut self) {
        if let Ok(mut sock) = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)) {
            let _ = sock.set_write_timeout(Some(Duration::from_secs(2)));
            let _ = sock.write_all(
                b"POST /stop HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[test]
fn spare_listener_shapes() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, SPARE))
        .expect("127.0.0.1:18002 must be free; refusing to pick another port");
    let hits = Arc::new(Mutex::new(Vec::<Hit>::new()));
    let recorded = Arc::clone(&hits);
    let join = thread::spawn(move || serve(listener, recorded));
    let _stop = Stop {
        port: SPARE,
        join: Some(join),
    };
    let addr = listener_addr();
    let timeout = Duration::from_secs(2);

    let heard = hear_exchange(addr, b"RIFFtest", timeout).unwrap();
    assert_eq!(heard, json!({"say": "Hello there. Next bit."}));

    let reply = reply_exchange(
        addr,
        "good evening",
        "Stay brief.",
        "fixture-not-a-secret",
        timeout,
    )
    .unwrap();
    assert_eq!(
        reply,
        json!({
            "heard": "good evening",
            "say": "Yes, I am here.",
            "audio": b64_encode(b"FAKEWAV")
        })
    );

    let quiet = reply_exchange(
        addr,
        "please-fail-mouth",
        "",
        "fixture-not-a-secret",
        timeout,
    )
    .unwrap();
    assert_eq!(
        quiet,
        json!({"heard": "please-fail-mouth", "say": "boom", "audio": ""})
    );

    let down = reply_exchange(
        addr,
        "please-fail-mind",
        "",
        "fixture-not-a-secret",
        timeout,
    )
    .unwrap();
    assert_eq!(down, fault_reply());

    let skipped = reply_exchange(addr, "", "Stay brief.", "fixture-not-a-secret", timeout).unwrap();
    assert_eq!(skipped, empty_reply());

    let warm = warm_exchange(addr, timeout).unwrap();
    assert_eq!(warm, 200);

    let hits = hits.lock().unwrap();
    let paths: Vec<&str> = hits.iter().map(|hit| hit.path.as_str()).collect();
    assert_eq!(
        paths,
        vec![HEAR_PATH, REPLY_PATH, SYNTH_PATH, REPLY_PATH, SYNTH_PATH, REPLY_PATH, SYNTH_PATH,]
    );

    let hear = &hits[0];
    assert_eq!(hear.content_type, "multipart/form-data; boundary=onebound");
    assert!(hear.authorization.is_empty());
    let hear_body = String::from_utf8_lossy(&hear.body);
    assert!(hear_body.contains("filename=\"speech.wav\""));
    assert!(hear_body.contains("name=\"language\""));
    assert!(hear_body.contains("name=\"fast\""));
    assert!(hear.body.windows(8).any(|window| window == b"RIFFtest"));
    assert!(hear.body.ends_with(b"--onebound--\r\n"));
    assert!(!hear.header.to_ascii_lowercase().contains("cookie"));
    assert!(!hear.header.contains("X-Hermes"));

    let chat = &hits[1];
    assert_eq!(chat.content_type, "application/json");
    assert_eq!(chat.authorization, "Bearer fixture-not-a-secret");
    assert!(!chat.header.contains("X-Hermes"));
    let chat_body: Value = serde_json::from_slice(&chat.body).unwrap();
    assert_eq!(chat_body["model"], "hermes-agent");
    assert_eq!(chat_body["stream"], false);
    assert_eq!(chat_body["max_tokens"], 80);
    assert_eq!(
        chat_body["messages"][0]["content"],
        voice_setting("Stay brief.")
    );
    assert_eq!(chat_body["messages"][1]["content"], "good evening");
    assert!(chat_content(&chat.body).is_some());

    let synth = &hits[2];
    assert_eq!(synth.authorization, "");
    let synth_body: Value = serde_json::from_slice(&synth.body).unwrap();
    assert_eq!(
        synth_body,
        json!({"text": "Yes, I am here.", "voice": "samantha"})
    );

    let failed_mouth = &hits[4];
    let failed_body: Value = serde_json::from_slice(&failed_mouth.body).unwrap();
    assert_eq!(failed_body["text"], "boom");
    assert_eq!(failed_body["voice"], "samantha");

    let warm_hit = &hits[6];
    assert_eq!(warm_hit.path, SYNTH_PATH);
    assert_eq!(warm_hit.content_type, "application/json");
    assert!(warm_hit.authorization.is_empty());
    let warm_body: Value = serde_json::from_slice(&warm_hit.body).unwrap();
    assert_eq!(warm_body, json!({"text": "Yes.", "voice": "samantha"}));
    let raw = String::from_utf8(warm_hit.body.clone()).unwrap();
    assert!(!raw.contains("admin-device"));
    assert!(!raw.contains("state.db"));
    assert!(!raw.contains("ONE_COOKIE"));
}

fn listener_addr() -> std::net::SocketAddr {
    std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, SPARE))
}

fn serve(listener: TcpListener, hits: Arc<Mutex<Vec<Hit>>>) {
    loop {
        let Ok((mut sock, _)) = listener.accept() else {
            break;
        };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
        let Ok(hit) = read_hit(&mut sock) else {
            continue;
        };
        if hit.path == "/stop" {
            let _ = respond(&mut sock, 200, b"bye");
            break;
        }
        let (code, body) = route(&hit);
        if let Ok(mut guard) = hits.lock() {
            guard.push(hit);
        }
        let _ = respond(&mut sock, code, &body);
    }
}

fn route(hit: &Hit) -> (u16, Vec<u8>) {
    match hit.path.as_str() {
        HEAR_PATH => (200, br#"{"text":"  Hello there. Next bit.  "}"#.to_vec()),
        REPLY_PATH => {
            let value: Value = serde_json::from_slice(&hit.body).unwrap_or(Value::Null);
            let heard = value["messages"][1]["content"].as_str().unwrap_or("");
            if heard == "please-fail-mind" {
                (500, br#"{"error":"no"}"#.to_vec())
            } else if heard == "please-fail-mouth" {
                (
                    200,
                    br#"{"choices":[{"message":{"content":"boom"}}]}"#.to_vec(),
                )
            } else {
                (
                    200,
                    br#"{"choices":[{"message":{"content":"Yes, I am here. And more follows."}}]}"#
                        .to_vec(),
                )
            }
        }
        SYNTH_PATH => {
            let value: Value = serde_json::from_slice(&hit.body).unwrap_or(Value::Null);
            if value["text"] == "boom" {
                (500, br#"{"error":"no"}"#.to_vec())
            } else if value["text"] == "Yes." || value["text"] == "Yes, I am here." {
                (200, b"FAKEWAV".to_vec())
            } else {
                (404, br#"{"error":"no"}"#.to_vec())
            }
        }
        _ => (404, br#"{"error":"no"}"#.to_vec()),
    }
}

fn read_hit(sock: &mut TcpStream) -> std::io::Result<Hit> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        let n = sock.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|window| window == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&buf[..pos]).to_string();
            let len = content_length(&header).unwrap_or(0);
            let have = buf.len() - (pos + 4);
            if have >= len {
                let body = buf[pos + 4..pos + 4 + len].to_vec();
                return Ok(Hit {
                    path: request_path(&header),
                    content_type: header_value(&header, "content-type"),
                    authorization: header_value(&header, "authorization"),
                    header,
                    body,
                });
            }
        }
        if buf.len() > 2_000_000 {
            return Err(std::io::Error::other("request too large"));
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "short request",
    ))
}

fn request_path(header: &str) -> String {
    header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string()
}

fn header_value(header: &str, name: &str) -> String {
    for line in header.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.eq_ignore_ascii_case(name) {
            return value.trim().to_string();
        }
    }
    String::new()
}

fn content_length(header: &str) -> Option<usize> {
    let value = header_value(header, "content-length");
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

fn respond(sock: &mut TcpStream, code: u16, body: &[u8]) -> std::io::Result<()> {
    let reason = if code == 200 { "OK" } else { "Error" };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    sock.write_all(head.as_bytes())?;
    sock.write_all(body)?;
    sock.flush()
}
