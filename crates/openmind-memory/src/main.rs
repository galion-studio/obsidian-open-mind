// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
use openmind_memory::{api::App, config::Config, engine::Engine, http, log};
use std::net::TcpListener;
use std::sync::Arc;

const EARLY_USAGE: &str = "usage: openmind-memory                      (serve; config from env)\n       openmind-memory snapshot <src.db> <dst.db>\n       openmind-memory parity <http://a> <http://b> <queries.txt> [session_key]\n";

/// `--check`: resolve the env config; opens no DB, never binds. The API key
/// is reported as set or not, never printed.
fn check() -> Result<String, String> {
    use galion_common::flags::{bind_addr, line, path_exists};
    let cfg = Config::from_env(&|k| std::env::var(k).ok());
    let mut r = String::new();
    bind_addr(&mut r, "bind", &format!("{}:{}", cfg.host, cfg.port))?;
    let ok = path_exists(&mut r, "data_dir", &cfg.data_dir, true);
    let db = cfg.db_path();
    line(
        &mut r,
        "db",
        format!(
            "{} ({})",
            db.display(),
            if db.is_file() { "exists" } else { "absent" }
        ),
    );
    line(&mut r, "read_only", cfg.read_only);
    line(&mut r, "loopback", cfg.is_loopback());
    line(
        &mut r,
        "api_key",
        if cfg.api_key.is_some() {
            "set"
        } else {
            "not set (auth disabled)"
        },
    );
    if ok {
        Ok(r)
    } else {
        Err(format!("{r}data_dir is not a directory"))
    }
}

fn main() {
    galion_common::flags::early(
        "openmind-memory",
        env!("CARGO_PKG_VERSION"),
        EARLY_USAGE,
        check,
    );
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
                "usage: openmind-memory                      (serve; config from env)\n       openmind-memory snapshot <src.db> <dst.db>\n       openmind-memory parity <http://a> <http://b> <queries.txt> [session_key]"
            );
            Some(64)
        }
    }
}
