// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! hermes-gateway: the Rust Hermes API server.
//!
//!   hermes-gateway [--mock] [--check-config]
//!
//! Config: $HERMES_HOME/config.yaml (the Python Hermes file, subset), then
//! env (HERMES_RS_HOST, HERMES_RS_PORT, HERMES_RS_STATE_DB, ...). The API key
//! is read from $API_SERVER_KEY only. `--mock` uses the offline mock
//! provider (no network, no spend) for shadow smoke tests.
//!
//! Coding tools (feature `coding`, on by default at build time) stay off
//! unless HERMES_RS_CODING_WORKTREE is set; see docs/hermes-tools.md.

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
    #[cfg(feature = "sqlite")]
    let primary: Box<dyn SessionStore> = Box::new(hermes_rs::session_sqlite::SqliteStore::open_rw(
        &cfg.state_db,
    )?);
    #[cfg(not(feature = "sqlite"))]
    let primary: Box<dyn SessionStore> = Box::new(hermes_rs::session::JsonlStore::open(
        cfg.state_db.with_extension("sessions"),
    )?);
    #[cfg(feature = "sqlite")]
    let legacy: Option<Box<dyn SessionStore>> = match &cfg.legacy_state_db {
        Some(p) => Some(Box::new(hermes_rs::session_sqlite::SqliteStore::open_ro(
            p,
        )?)),
        None => None,
    };
    #[cfg(not(feature = "sqlite"))]
    let legacy: Option<Box<dyn SessionStore>> = None;
    Ok(Arc::new(hermes_rs::session::Layered { primary, legacy }))
}

const USAGE: &str = "usage: hermes-gateway [--mock] [--check-config]\n  \
no arguments    load $HERMES_HOME/config.yaml + env, start the API server\n  \
--mock          offline mock provider (no network, no spend)\n  \
--check-config  load the config, print a summary, exit (does not bind)\n\
env: HERMES_HOME, HERMES_RS_HOST, HERMES_RS_PORT, HERMES_RS_STATE_DB, API_SERVER_KEY (never printed)\n";

fn summary(cfg: &Config) -> String {
    format!(
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
    )
}

/// `--check`: config + env only. No store is opened, no MCP server or coding
/// preflight is started, no provider is contacted, nothing binds. The API key
/// is reported as present/long enough, never printed.
fn check() -> Result<String, String> {
    let cfg = Config::load(&env).map_err(|e| format!("config error: {e}"))?;
    let mut r = summary(&cfg);
    r.push('\n');
    let home_ok = galion_common::flags::path_exists(&mut r, "home", &cfg.home, true);
    galion_common::flags::line(
        &mut r,
        "state_db",
        format!(
            "{} ({})",
            cfg.state_db.display(),
            if cfg.state_db.exists() {
                "exists"
            } else {
                "absent, would be created at start"
            }
        ),
    );
    let key_ok = cfg.api_key(&env).is_some_and(|k| k.len() >= 16);
    galion_common::flags::line(
        &mut r,
        "api_key",
        if key_ok {
            "set (>= 16 chars)"
        } else {
            "MISSING or shorter than 16 chars"
        },
    );
    galion_common::flags::bind_addr(&mut r, "bind", &format!("{}:{}", cfg.host, cfg.port))?;
    if !home_ok {
        return Err(format!(
            "{r}HERMES_HOME {} is not a directory",
            cfg.home.display()
        ));
    }
    if !key_ok {
        return Err(format!(
            "{r}API_SERVER_KEY missing or shorter than 16 chars (start would refuse)"
        ));
    }
    Ok(r)
}

fn main() {
    // First thing: --version/--help/--check never run the startup checks
    // (config, store, MCP discovery, coding preflight) and never bind.
    galion_common::flags::early("hermes-gateway", env!("CARGO_PKG_VERSION"), USAGE, check);
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
        println!("{}", summary(&cfg));
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
    if !cfg.mcp_servers.is_empty() {
        let (mcp_tools, errors) = hermes_rs::mcp::discover(&cfg.mcp_servers);
        for e in errors {
            log(&format!("mcp: {e}"));
        }
        log(&format!("mcp: {} tools", mcp_tools.len()));
        for t in mcp_tools {
            tools.add(t);
        }
    }
    #[cfg(feature = "coding")]
    match hermes_tools::CodingConfig::from_env(&env, &cfg.home) {
        Ok(None) => {}
        Ok(Some(mut cc)) => match hermes_tools::preflight(&mut cc, &cfg.spend) {
            Ok(notes) => {
                for n in notes {
                    log(&format!("coding: {n}"));
                }
                hermes_tools::register(&mut tools, cc, Some(spend.clone()));
                log(&format!("coding: tools on: {:?}", hermes_tools::TOOL_NAMES));
            }
            Err(e) => {
                log(&format!("coding: refusing to start: {e}"));
                std::process::exit(2);
            }
        },
        Err(e) => {
            log(&format!("coding: config error: {e}"));
            std::process::exit(2);
        }
    }
    let cron_agent = Arc::new(hermes_rs::agent::Agent {
        cfg: cfg.clone(),
        provider: provider.clone(),
        spend: spend.clone(),
        tools: tools.clone(),
    });
    let agent = hermes_rs::agent::boss(cfg.clone(), provider, spend, tools);
    let store = match open_store(&cfg) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("session store error: {e}"));
            std::process::exit(2);
        }
    };
    let mut gw = Gateway::new(cfg.clone(), agent, store, api_key);
    let sched = Arc::new(hermes_rs::cron::Scheduler::new(&cfg.home, Some(cron_agent)));
    gw.jobs = Some(sched.clone());
    let gw = Arc::new(gw);
    if cfg.cron_enabled {
        sched.spawn();
        log(&format!(
            "cron on: {} jobs, own state {}",
            sched.jobs().len(),
            cfg.cron_state_file.display()
        ));
    }
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
