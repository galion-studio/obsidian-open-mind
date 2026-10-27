// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! `galion-admin-console` CLI.
//!
//! ```text
//! galion-admin-console serve --bind 127.0.0.1:PORT --state DIR [--nerve 127.0.0.1:8670]
//!     [--console 127.0.0.1:8080] [--console-host console.galion.app]
//!     [--origin https://console.galion.app] [--insecure-cookie]
//! galion-admin-console issue-code --state DIR [--ttl SECONDS]
//! galion-admin-console devices --state DIR
//! galion-admin-console revoke --state DIR NODE
//! galion-admin-console audit-verify --state DIR
//! ```
//! Exit codes: 0 ok, 1 runtime error, 2 usage / refused config, 4 audit chain broken.

use galion_admin_console::audit::{self, Verify};
use galion_admin_console::pairing::{Store, DEFAULT_TTL};
use galion_admin_console::server::Config;
use galion_common::http::{self, Limits};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::exit;
use std::sync::Arc;

fn usage(msg: &str) -> ! {
    eprintln!("galion-admin-console: {msg}");
    eprintln!(
        "usage: galion-admin-console serve|issue-code|devices|revoke|audit-verify --state DIR ..."
    );
    exit(2)
}

fn loopback(name: &str, v: &str) -> SocketAddr {
    match v.parse::<SocketAddr>() {
        Ok(a) if a.ip().is_loopback() => a,
        Ok(_) => usage(&format!(
            "{name} must be a loopback address (got a non-loopback one)"
        )),
        Err(_) => usage(&format!("{name}: want IP:PORT")),
    }
}

struct Args {
    cmd: String,
    state: Option<PathBuf>,
    bind: Option<SocketAddr>,
    nerve: Option<SocketAddr>,
    console: Option<SocketAddr>,
    console_host: String,
    origin: Option<String>,
    insecure_cookie: bool,
    ttl: i64,
    rest: Vec<String>,
}

fn parse() -> Args {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_else(|| usage("missing command"));
    let mut a = Args {
        cmd,
        state: None,
        bind: None,
        nerve: None,
        console: None,
        console_host: "console.galion.app".into(),
        origin: None,
        insecure_cookie: false,
        ttl: DEFAULT_TTL,
        rest: vec![],
    };
    while let Some(k) = it.next() {
        let mut val = || {
            it.next()
                .unwrap_or_else(|| usage(&format!("{k} needs a value")))
        };
        match k.as_str() {
            "--state" => a.state = Some(val().into()),
            "--bind" => a.bind = Some(loopback("--bind", &val())),
            "--nerve" => a.nerve = Some(loopback("--nerve", &val())),
            "--console" => a.console = Some(loopback("--console", &val())),
            "--console-host" => a.console_host = val(),
            "--origin" => {
                let o = val();
                if !(o.starts_with("https://") || o.starts_with("http://127.0.0.1:"))
                    || o.ends_with('/')
                {
                    usage("--origin must be https://HOST (or http://127.0.0.1:PORT for tests), no trailing slash");
                }
                a.origin = Some(o)
            }
            "--insecure-cookie" => a.insecure_cookie = true,
            "--ttl" => {
                a.ttl = val()
                    .parse()
                    .ok()
                    .filter(|t| (30..=3600).contains(t))
                    .unwrap_or_else(|| usage("--ttl must be 30..=3600 seconds"))
            }
            s if s.starts_with("--") => usage(&format!("unknown flag {s}")),
            s => a.rest.push(s.into()),
        }
    }
    a
}

fn main() {
    galion_common::build_info::exit_if_version("galion-admin-console", env!("CARGO_PKG_VERSION"));
    let a = parse();
    let state = a
        .state
        .clone()
        .unwrap_or_else(|| usage("--state DIR is required"));
    let store = Store::open(&state).unwrap_or_else(|e| {
        eprintln!("galion-admin-console: state dir: {e}");
        exit(2)
    });
    match a.cmd.as_str() {
        "serve" => {
            let bind = a
                .bind
                .unwrap_or_else(|| usage("serve needs --bind 127.0.0.1:PORT"));
            if a.insecure_cookie
                && a.origin
                    .as_deref()
                    .is_some_and(|o| o.starts_with("https://"))
            {
                usage("--insecure-cookie is only for http://127.0.0.1 test origins");
            }
            let listener = TcpListener::bind(bind).unwrap_or_else(|e| {
                eprintln!("galion-admin-console: bind {bind}: {e}");
                exit(1)
            });
            let cfg = Arc::new(Config {
                store,
                nerve: a.nerve,
                console: a.console,
                console_host: a.console_host,
                origin: a.origin,
                secure_cookie: !a.insecure_cookie,
            });
            let _ = audit::append(&cfg.store.audit_path(), "serve.start", "-", "listening");
            eprintln!("galion-admin-console: listening on {bind}");
            let limits = Limits {
                max_body_bytes: 4096,
                max_connections: 32,
                ..Limits::default()
            };
            let h = cfg.clone();
            if let Err(e) = http::serve(listener, limits, Arc::new(move |r| h.handle(r))) {
                eprintln!("galion-admin-console: serve: {e}");
                exit(1)
            }
        }
        "issue-code" => match store.issue_code(a.ttl, galion_admin_console::now_secs()) {
            Ok(code) => {
                let _ = audit::append(&store.audit_path(), "pair.code", "-", "issued");
                // Printed once for the operator; only its hash is stored.
                println!("{code}");
                eprintln!("valid {} s, single use", a.ttl);
            }
            Err(e) => {
                eprintln!("galion-admin-console: issue-code: {e}");
                exit(1)
            }
        },
        "devices" => {
            for d in store.list().unwrap_or_else(|e| {
                eprintln!("galion-admin-console: devices: {e}");
                exit(1)
            }) {
                println!(
                    "{}\t{}\t{}\t{}",
                    d.node,
                    galion_common::time::from_unix(d.paired),
                    if d.revoked { "revoked" } else { "active" },
                    d.label
                );
            }
        }
        "revoke" => {
            let node = a.rest.first().unwrap_or_else(|| usage("revoke needs NODE"));
            match store.revoke(node) {
                Ok(true) => {
                    let _ = audit::append(&store.audit_path(), "revoke", node, "revoked");
                    println!("revoked {node}")
                }
                Ok(false) => {
                    eprintln!("no active device {node}");
                    exit(1)
                }
                Err(e) => {
                    eprintln!("galion-admin-console: revoke: {e}");
                    exit(1)
                }
            }
        }
        "audit-verify" => match audit::verify(&store.audit_path()) {
            Ok(Verify::Ok { records, head }) => println!("ok records={records} head={head}"),
            Ok(Verify::Broken { line, why }) => {
                println!("BROKEN line={line} why={why}");
                exit(4)
            }
            Err(e) => {
                eprintln!("galion-admin-console: audit: {e}");
                exit(1)
            }
        },
        other => usage(&format!("unknown command {other}")),
    }
}
