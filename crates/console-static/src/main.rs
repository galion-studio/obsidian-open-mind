// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `galion-console-static`: see the crate docs and `docs/console-static.md`.

#![forbid(unsafe_code)]

use console_static::config::{ArgsError, Config};
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;

fn gen_routes(args: &[String]) -> ExitCode {
    // gen-routes ROUTE_TREE [--redirect FROM TO]...
    let Some(file) = args.first() else {
        eprintln!(
            "usage: galion-console-static gen-routes ROUTE_TREE_GEN_TS [--redirect FROM TO]..."
        );
        return ExitCode::from(2);
    };
    let mut redirects = Vec::new();
    let mut rest = args[1..].iter();
    while let Some(a) = rest.next() {
        match (a.as_str(), rest.next(), rest.next()) {
            ("--redirect", Some(f), Some(t)) => redirects.push((f.clone(), t.clone())),
            _ => {
                eprintln!("gen-routes: expected --redirect FROM TO");
                return ExitCode::from(2);
            }
        }
    }
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("gen-routes: {file}: {e}");
            return ExitCode::from(1);
        }
    };
    match console_static::routes::from_route_tree(&text, &redirects) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("gen-routes: {e}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("gen-routes") {
        return gen_routes(&argv[1..]);
    }
    let cfg = match Config::from_args(argv) {
        Ok(c) => c,
        Err(ArgsError::Help) => {
            print!("{}", console_static::config::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("galion-console-static: {e}");
            return ExitCode::from(2);
        }
    };
    let site = match &cfg.root {
        None => Site::default(),
        Some(root) => match Site::load(root, &cfg.shell) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("galion-console-static: root {}: {e}", root.display());
                return ExitCode::from(1);
            }
        },
    };
    let routes = match &cfg.routes {
        Some(p) => match std::fs::read_to_string(p)
            .map_err(|e| e.to_string())
            .and_then(|t| Routes::parse(&t).map_err(|e| e.to_string()))
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("galion-console-static: routes {}: {e}", p.display());
                return ExitCode::from(1);
            }
        },
        None => Routes::parse("page /").expect("default routes"),
    };
    let listener = match TcpListener::bind(cfg.bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("galion-console-static: bind {}: {e}", cfg.bind);
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "galion-console-static: listening {} ui {} root {} files {} skipped {} pages {} upstream {} prefixes {:?}",
        cfg.bind,
        if cfg.rust_ui { "rust" } else { "spa" },
        cfg.root
            .as_ref()
            .map(|r| r.display().to_string())
            .unwrap_or_else(|| "none".into()),
        site.file_count(),
        site.skipped(),
        routes.page_count(),
        cfg.upstream.map(|u| u.to_string()).unwrap_or_else(|| "none".into()),
        cfg.proxy_prefixes
    );
    let state = Arc::new(State::new(site, routes, cfg));
    match server::serve(listener, state) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("galion-console-static: {e}");
            ExitCode::from(1)
        }
    }
}
