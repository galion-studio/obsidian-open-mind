// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! hermes-gateway: the Rust Hermes API server.
//!
//!   hermes-gateway [--mock] [--check-config] [--offhost-once] [--offhost-standby]
//!
//! Config: $HERMES_HOME/config.yaml (the Python Hermes file, subset), then
//! env (HERMES_RS_HOST, HERMES_RS_PORT, HERMES_RS_STATE_DB, ...). The API key
//! is read from $API_SERVER_KEY only. `--mock` uses the offline mock
//! provider (no network, no spend) for shadow smoke tests.
//!
//! Coding tools (feature `coding`, on by default at build time) stay off
//! unless HERMES_RS_CODING_WORKTREE is set; see docs/hermes-tools.md.
//! NERVE: with `NERVE_URL=http://127.0.0.1:8670` the gateway pushes started,
//! heartbeat, 5xx, slow-request (`NERVE_SLOW_MS`), spend-cap and conn-cap
//! events to the nerve (bounded queue, never blocks a request).
//!
//! Off-host continuity (`HERMES_OFFHOST`, default off) snapshots gateway
//! state, cron, spend, and sessions to a secondary path or URI and resumes
//! them when local files are missing or stale. `--offhost-standby` only
//! serves loopback health. It does not start cron or a model provider.

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

/// `--mock` test knobs (offline only, no network, no spend): a user message
/// containing `mock-slow` waits `HERMES_RS_MOCK_DELAY_MS` (default 1500)
/// before answering (slow-request tests); a user message containing
/// `mock-fail` makes the call fail (a real 502 path for error-push tests).
struct TestMock(MockProvider, u64);

impl Provider for TestMock {
    fn chat(
        &self,
        target: &hermes_rs::provider::Target,
        req: &hermes_rs::provider::ChatRequest,
        cancel: &std::sync::atomic::AtomicBool,
        sink: &mut dyn FnMut(hermes_rs::provider::StreamEvent),
    ) -> Result<hermes_rs::provider::ChatResult, String> {
        let user = req
            .messages
            .iter()
            .rev()
            .find(|m| m["role"] == "user")
            .and_then(|m| m["content"].as_str())
            .unwrap_or("");
        if user.contains("mock-slow") {
            std::thread::sleep(std::time::Duration::from_millis(self.1));
        }
        if user.contains("mock-fail") {
            return Err("mock provider failure (requested by test)".into());
        }
        self.0.chat(target, req, cancel, sink)
    }
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
    let mut off = match hermes_rs::offhost::OffhostCfg::load(&cfg, &env) {
        Ok(o) => o,
        Err(e) => {
            log(&format!("offhost config error: {}", hermes_rs::redact(&e)));
            std::process::exit(2);
        }
    };
    let once = args.iter().any(|a| a == "--offhost-once");
    let standby_flag = args.iter().any(|a| a == "--offhost-standby");
    if standby_flag {
        off.enabled = true;
        off.role = hermes_rs::offhost::Role::Standby;
    }
    if once {
        off.enabled = true;
    }
    if args.iter().any(|a| a == "--check-config") {
        println!(
            "home={} bind={}:{} default_model={} routes={:?} aliases={:?} delegation(max_children={}, depth={}) max_concurrent_runs={} context_length={} mcp={:?} cron={} offhost={} role={} resume={} interval_s={} stale_s={} external={} {}",
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
            cfg.cron_enabled,
            off.enabled,
            off.role,
            off.resume,
            off.interval_s,
            off.stale_after_s,
            off.external_state_uri().as_deref().unwrap_or("-"),
            off.seal_summary(hermes_rs::time::now_secs())
        );
        return;
    }
    if once && off.role == hermes_rs::offhost::Role::Standby {
        log("refusing: a standby does not export snapshots");
        std::process::exit(2);
    }
    if once {
        let report = off.beat_at(
            hermes_rs::time::now_secs(),
            hermes_rs::offhost::put_from_process,
        );
        println!("{}", hermes_rs::redact(&report.summary()));
        std::process::exit(if report.local_ok { 0 } else { 1 });
    }
    let off_status = if off.enabled {
        let now = hermes_rs::time::now_secs();
        let resumed = off.resume_at(now, hermes_rs::offhost::get_from_process);
        log(&hermes_rs::redact(&resumed.summary()));
        let status = std::sync::Arc::new(std::sync::Mutex::new(resumed.view()));
        if off.role == hermes_rs::offhost::Role::Standby {
            if !cfg.is_loopback() {
                log("refusing standby: bind is not loopback");
                std::process::exit(2);
            }
            log("offhost standby: no model provider, no cron, no spend");
            if let Err(e) = hermes_rs::offhost::spawn_periodic(off, status.clone()) {
                log(&format!("offhost loop: {}", hermes_rs::redact(&e)));
            }
            let addr = format!("{}:{}", cfg.host, cfg.port);
            match hermes_rs::offhost::spawn_standby(&addr, status) {
                Ok(a) => log(&format!("standby listening on {a}")),
                Err(e) => {
                    log(&format!("bind {addr}: {e}"));
                    std::process::exit(1);
                }
            }
            loop {
                std::thread::park();
            }
        }
        if let Err(e) = hermes_rs::offhost::spawn_periodic(off, status.clone()) {
            log(&format!("offhost loop: {}", hermes_rs::redact(&e)));
        }
        Some(status)
    } else {
        None
    };
    let api_key = cfg.api_key(&env);
    let cfg = Arc::new(cfg);
    let provider: Arc<dyn Provider> = if mock {
        let slow = env("HERMES_RS_MOCK_DELAY_MS")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1500);
        Arc::new(TestMock(MockProvider::echo(), slow))
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
    gw.offhost = off_status;
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
