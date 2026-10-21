// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-origin: env ORIGIN_ADDR (default 127.0.0.1:8655), ORIGIN_ROOT,
//! ORIGIN_GRINDER_ROOT, ORIGIN_CONSOLE_URL. One thread per connection, at
//! most 32 at once (served by the shared galion-common HTTP server).

use galion_common::http;
use galion_origin::{limits, render, Config, VERSION};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Instant;

/// `--check`: resolve the config and check its roots; never binds.
fn check() -> Result<String, String> {
    use galion_common::flags::{bind_addr, line, path_exists};
    let cfg = Config::from_env();
    let mut r = String::new();
    bind_addr(&mut r, "bind", &cfg.addr)?;
    let ok = path_exists(&mut r, "root", &cfg.root, true);
    path_exists(&mut r, "grinder_root", &cfg.grinder_root, true);
    line(
        &mut r,
        "console_url",
        cfg.console_url.as_deref().unwrap_or("none (splash)"),
    );
    if ok {
        Ok(r)
    } else {
        Err(format!("{r}ORIGIN_ROOT is not a directory"))
    }
}

fn main() {
    galion_common::flags::early(
        "galion-origin",
        env!("CARGO_PKG_VERSION"),
        "usage: galion-origin
serves the galion-light origin pages on ORIGIN_ADDR (default 127.0.0.1:8655)
env: ORIGIN_ADDR, ORIGIN_ROOT, ORIGIN_GRINDER_ROOT, ORIGIN_CONSOLE_URL
",
        check,
    );
    let cfg = Arc::new(Config::from_env());
    let l = match TcpListener::bind(&cfg.addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("galion-origin: bind {}: {e}", cfg.addr);
            std::process::exit(1);
        }
    };
    println!("galion-origin {VERSION} on {}", cfg.addr);
    let started = Instant::now();
    let handler = move |req: &http::Request| http::Response::raw(render(&cfg, req, started));
    if let Err(e) = http::serve(l, limits(), Arc::new(handler)) {
        eprintln!("galion-origin: serve: {e}");
        std::process::exit(1);
    }
}
