// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
use openmind_memory::{api::App, config::Config, engine::Engine, http, log};
use std::net::TcpListener;
use std::sync::Arc;

fn main() {
    let cfg = Config::from_env(&|k| std::env::var(k).ok());
    if !cfg.is_loopback() {
        log::warn(&format!("listening on non-loopback host {}", cfg.host));
    }
    if cfg.api_key.is_none() {
        log::warn("auth disabled (no OPENMIND_MEMORY_API_KEY / TDAI_GATEWAY_API_KEY)");
    }
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let (max_body, max_conns) = (cfg.max_body, cfg.max_conns);
    log::info(&format!(
        "data_dir={} db={} read_only={} listening on {addr}",
        cfg.data_dir.display(),
        cfg.db_path().display(),
        cfg.read_only
    ));
    let app = Arc::new(App::new(Engine::new(cfg)));
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            log::warn(&format!("bind {addr} failed: {e}"));
            std::process::exit(1);
        }
    };
    http::serve(
        listener,
        max_body,
        max_conns,
        Arc::new(move |req| app.handle(req)),
    );
}
