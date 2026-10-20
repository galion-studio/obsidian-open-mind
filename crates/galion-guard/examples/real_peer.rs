// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Stand-in "sshd" for the real-life test (scripts/real/guard-rust.sh):
//! accepts TCP connections and answers with an SSH banner, so a client can
//! tell "connected" from "rejected by the ban".
//! usage: real_peer <bind-addr:port>

use std::io::Write;
use std::net::TcpListener;

fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:19381".into());
    let l = TcpListener::bind(&addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    eprintln!("real_peer listening on {addr}");
    for mut s in l.incoming().flatten() {
        let peer = s.peer_addr().map(|p| p.to_string()).unwrap_or_default();
        let _ = s.write_all(format!("SSH-2.0-galion-real-peer hello {peer}\r\n").as_bytes());
    }
}
