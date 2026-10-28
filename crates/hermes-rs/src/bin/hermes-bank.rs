// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `hermes-bank`: build the console `/bank` snapshot (`BankSnap` JSON) from
//! saved provider replies and local spend files, print it (`report`) or
//! serve it read-only on a loopback port (`serve`, GET /api/v1/bank).
//!
//! It makes no network call and reads no key file. Provider replies are
//! files (the saved body of OpenRouter `GET /api/v1/key` / `/credits`);
//! whether a live reader may fetch them is owner decision D-351-2.
//!
//! ```text
//! hermes-bank report|serve
//!   [--or-key-json F] [--or-key-status N] [--or-credits-json F] [--or-credits-status N]
//!   [--or-missing-key] [--river-status N] [--survival F]
//!   [--state-db F]... [--spend-json F] [--now RFC3339]
//!   serve: --bind 127.0.0.1:PORT [--refresh-s N]
//! ```

use hermes_rs::bank::{snapshot, Inputs, Reply, SpendRow};
use hermes_rs::http::{self, Request, Response};
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default, Clone)]
struct Args {
    cmd: String,
    or_key: Option<PathBuf>,
    or_key_status: Option<u16>,
    or_credits: Option<PathBuf>,
    or_credits_status: Option<u16>,
    or_missing: bool,
    river_status: Option<u16>,
    survival: Option<PathBuf>,
    dbs: Vec<PathBuf>,
    spend_json: Option<PathBuf>,
    now: Option<i64>,
    bind: Option<SocketAddr>,
    refresh_s: u64,
}

fn usage() -> ! {
    eprintln!("usage: hermes-bank report|serve [--or-key-json F] [--or-key-status N] [--or-credits-json F] [--or-credits-status N] [--or-missing-key] [--river-status N] [--survival F] [--state-db F]... [--spend-json F] [--now RFC3339] [--bind 127.0.0.1:PORT] [--refresh-s N]");
    std::process::exit(2)
}

fn parse() -> Args {
    let mut a = Args {
        refresh_s: 300,
        ..Default::default()
    };
    let mut it = std::env::args().skip(1);
    a.cmd = it.next().unwrap_or_default();
    if a.cmd == "--version" {
        println!("hermes-bank {}", hermes_rs::VERSION);
        std::process::exit(0);
    }
    if a.cmd != "report" && a.cmd != "serve" {
        usage();
    }
    while let Some(k) = it.next() {
        let mut v = || it.next().unwrap_or_else(|| usage());
        match k.as_str() {
            "--or-key-json" => a.or_key = Some(v().into()),
            "--or-key-status" => a.or_key_status = v().parse().ok().or_else(|| usage()),
            "--or-credits-json" => a.or_credits = Some(v().into()),
            "--or-credits-status" => a.or_credits_status = v().parse().ok().or_else(|| usage()),
            "--or-missing-key" => a.or_missing = true,
            "--river-status" => a.river_status = v().parse().ok().or_else(|| usage()),
            "--survival" => a.survival = Some(v().into()),
            "--state-db" => a.dbs.push(v().into()),
            "--spend-json" => a.spend_json = Some(v().into()),
            "--now" => {
                a.now = Some(hermes_rs::time::parse_rfc3339(&v()).unwrap_or_else(|| usage()))
            }
            "--bind" => a.bind = Some(v().parse().unwrap_or_else(|_| usage())),
            "--refresh-s" => a.refresh_s = v().parse().unwrap_or_else(|_| usage()),
            _ => usage(),
        }
    }
    if a.cmd == "serve" {
        match a.bind {
            Some(b) if b.ip().is_loopback() => {}
            Some(_) => {
                eprintln!("hermes-bank: --bind must be a loopback address");
                std::process::exit(2)
            }
            None => usage(),
        }
        if a.refresh_s == 0 {
            usage();
        }
    }
    a
}

/// JSON file -> Value; missing/unreadable/invalid = None (no OS text).
fn read_json(p: &Option<PathBuf>) -> Option<Value> {
    let p = p.as_ref()?;
    let t = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()
}

fn reply(file: &Option<PathBuf>, status: Option<u16>) -> Option<Reply> {
    match (file, status) {
        (None, None) => None,
        (f, s) => Some(Reply::Http(s.unwrap_or(200), read_json(f))),
    }
}

fn spend_sources(dbs: &[PathBuf]) -> Vec<Vec<SpendRow>> {
    #[cfg(feature = "sqlite")]
    {
        dbs.iter()
            .map(|p| match hermes_rs::bank::read_spend_db(p) {
                Ok(rows) => rows,
                Err(why) => {
                    eprintln!("hermes-bank: spend db skipped ({why})");
                    Vec::new()
                }
            })
            .collect()
    }
    #[cfg(not(feature = "sqlite"))]
    {
        if !dbs.is_empty() {
            eprintln!("hermes-bank: built without sqlite; --state-db ignored");
        }
        Vec::new()
    }
}

fn build(a: &Args) -> Value {
    let now = a.now.unwrap_or_else(hermes_rs::time::now_secs);
    let key = if a.or_missing {
        Reply::MissingKey
    } else {
        reply(&a.or_key, a.or_key_status).unwrap_or(Reply::NotAsked)
    };
    let inp = Inputs {
        openrouter_key: key,
        openrouter_credits: reply(&a.or_credits, a.or_credits_status),
        river: a
            .river_status
            .map_or(Reply::NotAsked, |s| Reply::Http(s, None)),
        survival: read_json(&a.survival),
        spend_sources: spend_sources(&a.dbs),
        ledger: read_json(&a.spend_json),
        now,
    };
    snapshot(&inp)
}

fn main() {
    let a = parse();
    if a.cmd == "report" {
        println!("{}", build(&a));
        return;
    }
    let bind = a.bind.unwrap_or_else(|| usage());
    let listener = TcpListener::bind(bind).unwrap_or_else(|_| {
        eprintln!("hermes-bank: cannot bind {bind}");
        std::process::exit(1)
    });
    // snapshot first, then serve: never a half-built answer
    let snap: Arc<Mutex<(Value, std::time::Instant)>> =
        Arc::new(Mutex::new((build(&a), std::time::Instant::now())));
    {
        let snap = snap.clone();
        let a = a.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(a.refresh_s));
            let v = build(&a);
            *snap.lock().unwrap_or_else(|e| e.into_inner()) = (v, std::time::Instant::now());
        });
    }
    let refresh = a.refresh_s;
    let handler: http::Handler = Arc::new(move |req: Request| -> Response {
        // Never look at or log Cookie / Authorization: the reader needs neither.
        let path = req.path.split('?').next().unwrap_or("");
        match (req.method.as_str(), path) {
            ("GET" | "HEAD", "/api/v1/bank") => {
                let (mut v, at) = snap.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let age = at.elapsed().as_secs();
                if let Some(o) = v.as_object_mut() {
                    o.insert("ageS".into(), json!(age));
                    o.insert("stale".into(), json!(age > 3 * refresh));
                    o.insert(
                        "source".into(),
                        json!("hermes-bank (files only, no provider calls)"),
                    );
                }
                let r = Response::json(200, &v).with_header("Cache-Control", "no-store");
                if req.method == "HEAD" {
                    if let Response::Full {
                        status,
                        content_type,
                        headers,
                        ..
                    } = r
                    {
                        return Response::Full {
                            status,
                            content_type,
                            headers,
                            body: Vec::new(),
                        };
                    }
                }
                r
            }
            (_, "/api/v1/bank") => {
                http::error(405, "method not allowed", "invalid_request_error", None)
                    .with_header("Allow", "GET, HEAD")
            }
            ("GET", "/health") => {
                Response::json(200, &json!({"ok": true, "service": "hermes-bank"}))
            }
            _ => http::error(404, "not found", "invalid_request_error", None),
        }
    });
    eprintln!("hermes-bank: serving GET /api/v1/bank on {bind}");
    let _ = http::serve(listener, handler, 4);
}
