// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tuner process. Bind and root come from [`tuner::resolve`].

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    let settings = match tuner::resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("tuner: {err}");
            return ExitCode::from(1);
        }
    };
    let listener = TcpListener::bind(&settings.bind).expect("bind");
    eprintln!("tuner {} serving {}", settings.bind, settings.root);
    for stream in listener.incoming().flatten() {
        let root = settings.root.clone();
        thread::spawn(move || handle(stream, &root));
    }
    ExitCode::SUCCESS
}

fn handle(mut stream: TcpStream, root: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    if let Some(out) = tuner::respond(&buf[..n], root) {
        let _ = stream.write_all(&out);
    }
}
