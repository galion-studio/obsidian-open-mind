// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tuner process. Bind and root come from [`tuner::resolve`]. Connections go
//! through the shared galion-common HTTP server (head deadline, size caps,
//! bounded threads); the reply bytes still come from [`tuner::respond`].

#![forbid(unsafe_code)]

use galion_common::http;
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

fn main() -> ExitCode {
    galion_common::build_info::exit_if_version("tuner", env!("CARGO_PKG_VERSION"));
    let settings = match tuner::resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("tuner: {err}");
            return ExitCode::from(1);
        }
    };
    let listener = match TcpListener::bind(&settings.bind) {
        Ok(l) => l,
        Err(err) => {
            eprintln!("tuner: bind {}: {err}", settings.bind);
            return ExitCode::from(1);
        }
    };
    eprintln!("tuner {} serving {}", settings.bind, settings.root);
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
        http::Response::raw(tuner::respond(&req.raw_head, &root).unwrap_or_default())
    };
    match http::serve(listener, limits, Arc::new(handler)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("tuner: serve: {err}");
            ExitCode::from(1)
        }
    }
}
