// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tuner process. Bind and root come from [`tuner::resolve`]. Connections go
//! through the shared galion-common HTTP server (head deadline, size caps,
//! bounded threads); the reply bytes still come from [`tuner::respond`].
//!
//! With no arguments this is the live `/opt/galion/tuner` server: same env,
//! same log line, same bytes on the wire. `--version`, `--help` and `--check`
//! exit without binding (see [`tuner::cli`]).

#![forbid(unsafe_code)]

use galion_common::http;
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use tuner::cli::{self, Action};

fn main() -> ExitCode {
    galion_common::build_info::exit_if_version("tuner", env!("CARGO_PKG_VERSION"));
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
        Ok(l) => l,
        Err(err) => {
            eprintln!("tuner: bind {}: {err}", settings.bind);
            return ExitCode::from(1);
        }
    };
    eprintln!("tuner {} serving {}", settings.bind, settings.root);
    // NERVE push (no-op unless NERVE_URL is set): started + heartbeat.
    nerve_client::event(
        "tuner",
        "started",
        &format!("listening on {}", settings.bind),
    );
    nerve_client::start_heartbeat("tuner", 60);
    let limits = http::Limits {
        // The old server read one 4 KiB buffer with a 5 s read timeout.
        max_head_bytes: 4096,
        // Bodies are ignored, but a POST must still reach `respond` (405).
        max_body_bytes: 64 * 1024,
        head_timeout: Duration::from_secs(5),
        body_timeout: Duration::from_secs(5),
        max_connections: 64,
        thread_stack: 128 * 1024,
        ..http::Limits::default()
    };
    let root = settings.root;
    let handler = move |req: &http::Request| {
        let t0 = std::time::Instant::now();
        let bytes = tuner::respond(&req.raw_head, &root).unwrap_or_default();
        let status = if bytes.starts_with(b"HTTP/1.1 200") {
            200
        } else if bytes.starts_with(b"HTTP/1.1 404") {
            404
        } else if bytes.starts_with(b"HTTP/1.1 405") {
            405
        } else {
            500
        };
        let what = format!("{} {}", req.method(), req.path());
        nerve_client::observe_request("tuner", &what, status, t0.elapsed());
        http::Response::raw(bytes)
    };
    match http::serve(listener, limits, Arc::new(handler)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("tuner: serve: {err}");
            ExitCode::from(1)
        }
    }
}
