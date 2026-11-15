// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
use openmind_memory::{api::App, config::Config, engine::Engine, http, log};
use std::net::TcpListener;
use std::sync::Arc;

fn main() {
    galion_common::build_info::exit_if_version("openmind-memory", env!("CARGO_PKG_VERSION"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = subcommand(&args) {
        std::process::exit(code);
    }
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
    // NERVE push (no-op unless NERVE_URL is set; never blocks: bounded queue,
    // drop-on-full counted): started, heartbeat, 5xx errors, slow requests,
    // connection-cap hits (http.rs).
    nerve_client::event(
        "openmind-memory",
        "started",
        &format!("listening on {addr}"),
    );
    nerve_client::start_heartbeat("openmind-memory", 60);
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
        Arc::new(move |req: http::Request| {
            let what = format!(
                "{} {}",
                req.method,
                req.path.split('?').next().unwrap_or("")
            );
            let t0 = std::time::Instant::now();
            let resp = app.handle(req);
            nerve_client::observe_request("openmind-memory", &what, resp.status, t0.elapsed());
            resp
        }),
    );
}

fn subcommand(args: &[String]) -> Option<i32> {
    use openmind_memory::tools;
    use std::path::Path;
    let key = std::env::var("OPENMIND_MEMORY_API_KEY")
        .or_else(|_| std::env::var("TDAI_GATEWAY_API_KEY"))
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    match args.first().map(String::as_str) {
        None => None,
        Some("--version" | "-V") => {
            println!(
                "{}",
                galion_common::build_info::version_line(
                    "openmind-memory",
                    env!("CARGO_PKG_VERSION")
                )
            );
            Some(0)
        }
        Some("snapshot") if args.len() == 3 => Some(
            match tools::snapshot(Path::new(&args[1]), Path::new(&args[2])) {
                Ok(()) => 0,
                Err(e) => {
                    log::warn(&format!("snapshot failed: {e}"));
                    1
                }
            },
        ),
        Some("schema") if args.len() == 2 => Some(
            match openmind_memory::store::Store::schema_info(Path::new(&args[1])) {
                Ok(i) => {
                    println!("user_version={}", i.user_version);
                    println!("binary_schema_version={}", i.binary_version);
                    println!(
                        "state={}",
                        if i.user_version > i.binary_version {
                            "newer-than-binary (no DDL will run)"
                        } else if i.user_version == i.binary_version {
                            "current"
                        } else {
                            "will-migrate-on-writable-open"
                        }
                    );
                    println!("l0_conversations.timestamp={}", i.l0_timestamp);
                    println!("fts_v2={}", i.fts_v2);
                    println!(
                        "rows l0={} l0_fts={} l1={} l1_fts={}",
                        i.l0_rows, i.l0_fts_rows, i.l1_rows, i.l1_fts_rows
                    );
                    match &i.schema_migrations {
                        None => println!("schema_migrations=absent"),
                        Some(rows) => {
                            for (v, n) in rows {
                                println!("schema_migrations {v} {n}");
                            }
                        }
                    }
                    println!("tables={}", i.tables.join(","));
                    0
                }
                Err(e) => {
                    log::warn(&format!("schema failed: {e}"));
                    1
                }
            },
        ),
        Some("freshness") if (2..=4).contains(&args.len()) => {
            Some(openmind_memory::freshness::run(&args[1..]))
        }
        Some("parity") if args.len() >= 4 => {
            let session = args.get(4).map(String::as_str).unwrap_or("parity-session");
            Some(
                match tools::parity(
                    &args[1],
                    &args[2],
                    Path::new(&args[3]),
                    session,
                    key.as_deref(),
                ) {
                    Ok(true) => 0,
                    Ok(false) => 2,
                    Err(e) => {
                        log::warn(&format!("parity failed: {e}"));
                        1
                    }
                },
            )
        }
        Some(_) => {
            eprintln!(
                "usage: openmind-memory                      (serve; config from env)\n       openmind-memory snapshot <src.db> <dst.db>\n       openmind-memory schema <db>              (read-only schema/version report)\n       openmind-memory parity <http://a> <http://b> <queries.txt> [session_key]\n       openmind-memory freshness <data_dir> [instance_id] [max_age_hours]  (read-only; exit 3 = stale)"
            );
            Some(64)
        }
    }
}
