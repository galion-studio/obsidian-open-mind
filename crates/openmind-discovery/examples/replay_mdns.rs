// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Replay a cargo-fuzz `mdns_parse` corpus at a live `openmind-discovery
//! announce` responder, then check it still answers a real query.
//!
//! `replay_mdns ADDR DIR SERVICE_TYPE` sends every file in DIR as one UDP
//! datagram from an ephemeral port (so replies are legacy unicast, back to
//! us), counts the replies, then sends one well-formed PTR query for
//! SERVICE_TYPE and waits up to 3 s for an answer that parses. Prints one
//! JSON line; exit 1 when the final query gets no valid answer. Used by
//! scripts/real/fuzz-sast.sh.

use openmind_discovery::dns::{build_query, parse_message, TYPE_PTR};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(addr), Some(dir), Some(svc)) = (args.get(1), args.get(2), args.get(3)) else {
        eprintln!("usage: replay_mdns ADDR DIR SERVICE_TYPE");
        std::process::exit(2);
    };
    let addr: SocketAddr = addr.parse().expect("ADDR is host:port");
    let sock = UdpSocket::bind("127.0.0.1:0").expect("bind");
    sock.set_read_timeout(Some(Duration::from_millis(2)))
        .expect("timeout");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read DIR")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let mut buf = [0u8; 9000];
    let (mut sent, mut replies) = (0u32, 0u32);
    for path in &files {
        let data = std::fs::read(path).unwrap_or_default();
        if data.is_empty() || data.len() > 9000 {
            continue;
        }
        if sock.send_to(&data, addr).is_ok() {
            sent += 1;
        }
        while sock.recv_from(&mut buf).is_ok() {
            replies += 1;
        }
    }
    // The responder rate-limits replies to 20 per second: let the window
    // roll over before the final check.
    std::thread::sleep(Duration::from_millis(1100));
    while sock.recv_from(&mut buf).is_ok() {
        replies += 1;
    }
    sock.set_read_timeout(Some(Duration::from_millis(100)))
        .expect("timeout");
    let query = build_query(0x4242, &[(svc.as_str(), TYPE_PTR)], false);
    sock.send_to(&query, addr).expect("send query");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut answered = false;
    let mut answers = 0usize;
    while Instant::now() < deadline && !answered {
        if let Ok((len, _)) = sock.recv_from(&mut buf) {
            if let Ok(msg) = parse_message(&buf[..len]) {
                if msg.id == 0x4242 && msg.response && !msg.records.is_empty() {
                    answered = true;
                    answers = msg.records.len();
                }
            }
        }
    }
    println!(
        "{{\"files\":{},\"sent\":{sent},\"replies_during_replay\":{replies},\"final_query_answered\":{answered},\"final_records\":{answers}}}",
        files.len()
    );
    if !answered {
        std::process::exit(1);
    }
}
