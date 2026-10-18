// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! hermes-gateway: the Rust Hermes API server.
//!
//!   hermes-gateway [--mock] [--check-config]
//!
//! Config: $HERMES_HOME/config.yaml (the Python Hermes file, subset), then
//! env (HERMES_RS_HOST, HERMES_RS_PORT, HERMES_RS_STATE_DB, ...). The API key
//! is read from $API_SERVER_KEY only. `--mock` uses the offline mock
//! provider (no network, no spend) for shadow smoke tests.

use hermes_rs::agent::Agent;
use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::provider::{CurlProvider, MockProvider, Provider};
use hermes_rs::session::SessionStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{ClockTool, Registry};
use std::sync::Arc;

fn log(msg: &str) {
    eprintln!(
        "{} hermes-gateway {msg}",
        hermes_rs::time::zagreb_rfc3339(hermes_rs::time::now_secs())
    );
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn open_store(cfg: &Config) -> Result<Arc<dyn SessionStore>, String> {
    let dir = cfg.state_db.with_extension("sessions");
    let primary: Box<dyn SessionStore> = Box::new(hermes_rs::session::JsonlStore::open(dir)?);
    Ok(Arc::new(hermes_rs::session::Layered {
        primary,
        legacy: None,
    }))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mock = args.iter().any(|a| a == "--mock");
    let cfg = match Config::load(&env) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("config error: {e}"));
            std::process::exit(2);
        }
    };
    if args.iter().any(|a| a == "--check-config") {
        println!(
            "home={} bind={}:{} default_model={} routes={:?} aliases={:?} delegation(max_children={}, depth={}) max_concurrent_runs={} context_length={} mcp={:?} cron={}",
            cfg.home.display(),
            cfg.host,
            cfg.port,
            cfg.default_model,
            cfg.model_routes.keys().collect::<Vec<_>>(),
            cfg.aliases.keys().collect::<Vec<_>>(),
            cfg.delegation.max_children,
            cfg.delegation.max_depth,
            cfg.max_concurrent_runs,
            cfg.context_length,
            cfg.mcp_servers.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            cfg.cron_enabled
        );
        return;
    }
    let api_key = cfg.api_key(&env);
    let cfg = Arc::new(cfg);
    let provider: Arc<dyn Provider> = if mock {
        Arc::new(MockProvider::echo())
    } else {
        Arc::new(CurlProvider::new(Arc::new(|k: &str| std::env::var(k).ok())))
    };
    let spend = Arc::new(SpendTracker::new(
        cfg.spend.clone(),
        Some(cfg.spend_file.clone()),
    ));
    let mut tools = Registry::new();
    tools.add(Arc::new(ClockTool));
    let agent = Agent {
        cfg: cfg.clone(),
        provider,
        spend,
        tools,
    };
    let store = match open_store(&cfg) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("session store error: {e}"));
            std::process::exit(2);
        }
    };
    let gw = Arc::new(Gateway::new(cfg.clone(), agent, store, api_key));
    if !gw.key_ok() {
        log("refusing to start: API_SERVER_KEY is missing or shorter than 16 chars");
        std::process::exit(2);
    }
    let addr = format!("{}:{}", cfg.host, cfg.port);
    match hermes_rs::api::spawn(gw, &addr) {
        Ok(a) => log(&format!(
            "listening on {a} mock={mock} loopback={}",
            cfg.is_loopback()
        )),
        Err(e) => {
            log(&format!("bind {addr}: {e}"));
            std::process::exit(1);
        }
    }
    loop {
        std::thread::park();
    }
}
