// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
//! `galion-mouth-stub`: tiny honest :8002 stand-in.
//!
//! usage: galion-mouth-stub [--bind ADDR]
//!
//! * `--bind` / `GALION_MOUTH_STUB_BIND` / `ONE_MOUTH_BIND` (default 127.0.0.1:8002)
//!
//! GET /health and POST /synthesize answer 503 with reason "voice not deployed".
//! No keep-warm loop, no model load, no paid API. Extends the galion-one mouth
//! contract (slot 259 honesty) without pulling in the full One process.

#[path = "../mouth_stub.rs"]
mod mouth_stub;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "galion-mouth-stub [--bind ADDR]\n\
             Honest mouth stub: GET /health and POST /synthesize return 503\n\
             with reason \"voice not deployed\". No keep-warm, no models.\n\
             Bind: --bind, else GALION_MOUTH_STUB_BIND, else ONE_MOUTH_BIND,\n\
             else {}.",
            mouth_stub::DEFAULT_BIND
        );
        return;
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        eprintln!("galion-mouth-stub {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let bind = mouth_stub::resolve_bind(&args);
    if let Err(err) = mouth_stub::serve(&bind) {
        eprintln!("mouth-stub: bind/serve failed on {bind}: {err}");
        std::process::exit(1);
    }
}
