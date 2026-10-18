// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// tuner server. See lib.rs for behaviour and config.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

fn main() {
    let addr = tuner::addr();
    let root = tuner::root();
    let listener = TcpListener::bind(&addr).expect("bind");
    eprintln!("tuner {} serving {}", addr, root);
    for stream in listener.incoming().flatten() {
        let root = root.clone();
        thread::spawn(move || handle(stream, &root));
    }
}

fn handle(mut s: TcpStream, root: &str) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).unwrap_or(0);
    if let Some(out) = tuner::respond(&buf[..n], root) {
        let _ = s.write_all(&out);
    }
}
