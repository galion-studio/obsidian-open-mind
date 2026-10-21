// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! galion-desk binary: resolve settings, bind, serve. See the library docs
//! (`galion_desk::config`) for every environment variable.

use std::net::TcpListener;

const EARLY_USAGE: &str = "galion-desk: operator desk on DESK_BIND (default 127.0.0.1:8646). GET /desk, POST /desk, GET /health.\n";

/// `--check`: resolve the settings exactly as serving would; never binds.
fn check() -> Result<String, String> {
    use galion_common::flags::{bind_addr, line, path_exists};
    let s =
        galion_desk::config::resolve(&|key| std::env::var(key).ok()).map_err(|e| e.to_string())?;
    let mut r = String::new();
    bind_addr(&mut r, "bind", &s.bind)?;
    line(&mut r, "ollama", &s.ollama);
    path_exists(&mut r, "state_dir", &s.state_dir, true);
    path_exists(&mut r, "one_dir", &s.one_dir, true);
    path_exists(&mut r, "hermes_dir", &s.hermes_dir, true);
    path_exists(&mut r, "hermes_bin", &s.hermes_bin, false);
    Ok(r)
}

fn main() {
    galion_common::flags::early("galion-desk", env!("CARGO_PKG_VERSION"), EARLY_USAGE, check);
    if std::env::args()
        .nth(1)
        .is_some_and(|a| a == "--help" || a == "-h")
    {
        println!("galion-desk: operator desk on DESK_BIND (default 127.0.0.1:8646). GET /desk, POST /desk, GET /health.");
        return;
    }
    let settings = match galion_desk::config::resolve(&|key| std::env::var(key).ok()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("galion-desk: {err}");
            std::process::exit(1);
        }
    };
    let bind = settings.bind.clone();
    let listener = match TcpListener::bind(&bind) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("galion-desk: bind {bind}: {err}");
            std::process::exit(1);
        }
    };
    eprintln!("galion-desk {bind}");
    galion_desk::Desk::new(settings).serve(listener);
}
