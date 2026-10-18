// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! galion-origin: env ORIGIN_ADDR (default 127.0.0.1:8655), ORIGIN_ROOT,
//! ORIGIN_GRINDER_ROOT, ORIGIN_CONSOLE_URL. One thread per connection, at
//! most 32 at once.

use galion_origin::{handle, Config, VERSION};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

const MAX_CONN: usize = 32;

fn main() {
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
    let live = Arc::new(AtomicUsize::new(0));
    for s in l.incoming().flatten() {
        if live.load(Ordering::SeqCst) >= MAX_CONN {
            drop(s);
            continue;
        }
        live.fetch_add(1, Ordering::SeqCst);
        let (cfg, live2) = (cfg.clone(), live.clone());
        let spawned = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                handle(&cfg, s, started);
                live2.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            live.fetch_sub(1, Ordering::SeqCst);
            eprintln!("galion-origin: thread spawn failed");
        }
    }
}
