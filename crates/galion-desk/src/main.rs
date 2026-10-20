// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! galion-desk binary: resolve settings, bind, serve. See the library docs
//! (`galion_desk::config`) for every environment variable.

use std::net::TcpListener;

fn main() {
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
