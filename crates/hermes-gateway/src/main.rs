// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! hermes-gateway: the Rust Hermes API server.
//!
//!   hermes-gateway [--mock] [--check-config]
//!
//! Config: $HERMES_HOME/config.yaml (the Python Hermes file, subset), then
//! env (HERMES_RS_HOST, HERMES_RS_PORT, HERMES_RS_STATE_DB, ...). The API key
//! is read from $API_SERVER_KEY only. Memory: `memory.provider:
//! memory_tencentdb` in config.yaml (or HERMES_RS_MEMORY=1/0) turns on the
//! memory gateway client (MEMORY_TENCENTDB_GATEWAY_HOST/PORT, key from
//! MEMORY_TENCENTDB_GATEWAY_API_KEY or TDAI_GATEWAY_API_KEY). `--mock` uses the offline mock
//! provider (no network, no spend) for shadow smoke tests.

use hermes_rs::api::Gateway;
use hermes_rs::config::Config;
use hermes_rs::provider::{CurlProvider, MockProvider, Provider};
use hermes_rs::session::SessionStore;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{ClockTool, Registry};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// One line about the memory provider; names the key env var, never the value.
fn memory_summary(cfg: &Config) -> String {
    match &cfg.memory {
        None => "off".into(),
        Some(m) => format!(
            "{} {} key_env={} recall_timeout_ms={} capture_timeout_ms={} idle_end_secs={}",
            hermes_rs::memory::PROVIDER_NAME,
            m.base_url(),
            hermes_rs::memory::resolve_key(&env)
                .map(|(_, n)| n)
                .unwrap_or("none"),
            m.recall_timeout.as_millis(),
            m.capture_timeout.as_millis(),
            m.idle_end_secs
        ),
    }
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
            "home={} bind={}:{} default_model={} routes={:?} aliases={:?} delegation(max_children={}, depth={}) max_concurrent_runs={} context_length={} mcp={:?} cron={} memory={}",
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
            memory_summary(&cfg)
        );
        return;
    }
    let api_key = cfg.api_key(&env);
    let cfg = Arc::new(cfg);
    let provider: Arc<dyn Provider> = if mock {
        Arc::new(MockProvider::echo())
    } else {
        let mut p = CurlProvider::new(Arc::new(|k: &str| std::env::var(k).ok()));
        p.max_time_s = cfg.limits.provider_max_time_s;
        Arc::new(p)
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
    if let Some(mcfg) = cfg.memory.clone() {
        let mem = hermes_rs::memory::MemoryProvider::new(mcfg, &env);
        let probe = match mem.probe() {
            Ok(()) => "health 200".to_string(),
            Err(e) => format!("not reachable yet ({e}); calls stay non-fatal"),
        };
        log(&format!("memory: {} ({probe})", memory_summary(&cfg)));
        mem.spawn_reaper();
        gw.memory = Some(mem);
    }
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
    hermes_rs::limits::shutdown::install_handlers();
    let l = &cfg.limits;
    let bound = match hermes_rs::api::spawn(gw.clone(), &addr) {
        Ok(a) => {
            log(&format!(
                "listening on {a} mock={mock} loopback={} max_conns={} max_runs={} queue={}x{}ms run_timeout={}s header_timeout={}s grace={}s metrics_open={}",
                cfg.is_loopback(),
                l.max_conns,
                cfg.max_concurrent_runs,
                l.queue_max,
                l.queue_wait.as_millis(),
                l.run_timeout.as_secs(),
                l.header_timeout.as_secs(),
                l.shutdown_grace.as_secs(),
                l.metrics_open
            ));
            a
        }
        Err(e) => {
            log(&format!("bind {addr}: {e}"));
            std::process::exit(1);
        }
    };
    while !hermes_rs::limits::shutdown::requested() {
        std::thread::sleep(Duration::from_millis(200));
    }
    drain(&gw, bound, cfg.limits.shutdown_grace);
    std::process::exit(0);
}

/// Graceful shutdown (SIGTERM/SIGINT): stop accepting (the accept loop drops
/// the listener), let in-flight requests and runs finish for up to `grace`,
/// cancel what is left, end open memory sessions, exit 0.
fn drain(gw: &Arc<Gateway>, bound: std::net::SocketAddr, grace: Duration) {
    use std::sync::atomic::Ordering;
    let m = hermes_rs::limits::metrics();
    let t0 = Instant::now();
    log(&format!(
        "shutdown requested: draining (conns={} runs={} grace={}s)",
        m.conns_in_flight.load(Ordering::SeqCst),
        gw.active.load(Ordering::SeqCst),
        grace.as_secs()
    ));
    // Wake the blocking accept() so it sees the flag and closes the listener.
    let _ = std::net::TcpStream::connect_timeout(&bound, Duration::from_secs(1));
    let busy =
        || m.conns_in_flight.load(Ordering::SeqCst) > 0 || gw.active.load(Ordering::SeqCst) > 0;
    while busy() && t0.elapsed() < grace {
        std::thread::sleep(Duration::from_millis(100));
    }
    let cancelled = if busy() {
        hermes_rs::limits::watchdog::fire_all()
    } else {
        0
    };
    if cancelled > 0 {
        // Give cancelled runs a moment to persist their partial turn.
        let t1 = Instant::now();
        while gw.active.load(Ordering::SeqCst) > 0 && t1.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    if let Some(mem) = &gw.memory {
        let n = mem.shutdown();
        log(&format!("memory: session/end sent for {n} open sessions"));
    }
    log(&format!(
        "drained in {} ms: conns_left={} runs_left={} cancelled_runs={cancelled}",
        t0.elapsed().as_millis(),
        m.conns_in_flight.load(Ordering::SeqCst),
        gw.active.load(Ordering::SeqCst)
    ));
}
