// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tuner process. Bind and root come from [`tuner::resolve`].
//!
//! With no arguments this is the live `/opt/galion/tuner` server: same env,
//! same log line, same bytes on the wire. `--version`, `--help` and `--check`
//! exit without binding (see [`tuner::cli`]).

#![forbid(unsafe_code)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use tuner::cli::{self, Action};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = match cli::parse_args(&args) {
        Ok(action) => action,
        Err(err) => {
            eprintln!("tuner: {err}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match action {
        Action::Version => {
            println!("{}", cli::version_line());
            return ExitCode::SUCCESS;
        }
        Action::Help => {
            print!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Action::Check | Action::Serve => {}
    }
    let settings = match tuner::resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("tuner: {err}");
            return ExitCode::from(1);
        }
    };
    if action == Action::Check {
        return match cli::check_root(&settings.root) {
            Ok(report) => {
                print!("bind {}\n{report}", settings.bind);
                ExitCode::SUCCESS
            }
            Err(report) => {
                print!("bind {}\n{report}", settings.bind);
                ExitCode::from(1)
            }
        };
    }
    let listener = match TcpListener::bind(&settings.bind) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("tuner: bind {}: {err}", settings.bind);
            return ExitCode::from(1);
        }
    };
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
