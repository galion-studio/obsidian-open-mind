// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Process entry shared by `galion-console-static` and `galion-console-serve`.

use crate::config::{ArgsError, Config};
use crate::routes::Routes;
use crate::server::{self, State};
use crate::site::Site;
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::Arc;

fn gen_routes(args: &[String]) -> ExitCode {
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
    match crate::routes::from_route_tree(&text, &redirects) {
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

/// CLI entry used by both binaries.
pub fn run() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("gen-routes") {
        return gen_routes(&argv[1..]);
    }
    let cfg = match Config::from_args(argv) {
        Ok(c) => c,
        Err(ArgsError::Help) => {
            print!("{}", crate::config::USAGE);
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
    let routes = if let Some(text) = &cfg.routes_text {
        match Routes::parse(text) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("galion-console-static: dist default routes: {e}");
                return ExitCode::from(1);
            }
        }
    } else {
        match &cfg.routes {
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
        }
    };
    let listener = match TcpListener::bind(cfg.bind) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("galion-console-static: bind {}: {e}", cfg.bind);
            return ExitCode::from(1);
        }
    };
    eprintln!(
        "galion-console-static: listening {} ui {} dist {} local_api {} root {} files {} skipped {} pages {} upstream {} prefixes {:?}",
        cfg.bind,
        if cfg.rust_ui { "rust" } else { "spa" },
        cfg.dist_mode,
        cfg.local_api,
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
    if let Some(a) = &cfg.arena_root {
        if !a.is_dir() {
            // Not fatal (a late mount must not crash-loop the unit): the
            // reader answers the script defaults until the folder appears.
            eprintln!(
                "galion-console-static: warning: --arena-root {} is not a directory; /api/arena shows defaults",
                a.display()
            );
        }
    }
    eprintln!(
        "galion-console-static: api owner_routes {} pulse {} hermes {} arena_root {} data_upstream {} data_paths {:?}",
        cfg.owner_routes,
        addr_or_none(cfg.pulse),
        addr_or_none(cfg.hermes),
        cfg.arena_root
            .as_ref()
            .map(|r| r.display().to_string())
            .unwrap_or_else(|| "none".into()),
        addr_or_none(cfg.data_upstream),
        cfg.data_paths
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

fn addr_or_none(a: Option<std::net::SocketAddr>) -> String {
    a.map(|a| a.to_string()).unwrap_or_else(|| "none".into())
}
