// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! gaia-cortex
//!
//!   gaia-cortex [serve] [--config PATH]   loopback API + event ingest (default 127.0.0.1:8674)
//!   gaia-cortex replay FILE [--config PATH]  decide on an NDJSON event file, print decisions + stats
//!   gaia-cortex config [--config PATH]    print the effective config (no secrets: only key_env NAME)
//!   gaia-cortex hermes-dispatch           drain the Hermes handoff inbox once (systemd .path unit)
//!   gaia-cortex --version
//!
//! Env: CORTEX_CONFIG, CORTEX_LISTEN, CORTEX_SOCKET, CORTEX_DATA_DIR, CORTEX_NERVE_URL,
//! CORTEX_PULL_EVERY_S (0 = off), CORTEX_ACTIONS (dry-run|live), CORTEX_COALESCE_MS,
//! CORTEX_OUTBOX, CORTEX_INCIDENT_QUIET_S (one-shot incidents, default 1800), CORTEX_LLM_ENABLED (default off),
//! CORTEX_HERMES_HANDOFF (default off), CORTEX_HERMES_INBOX, CORTEX_HERMES_PER_HOUR (max 6),
//! CORTEX_HERMES_EXCLUDE_UNITS (comma list of unit globs never handed to Hermes, default none),
//! CORTEX_REPEAT_POLL_S (default 30, 0 = off: re-read the nerve ring for repeats it folded into
//! watched failure events).

use gaia_cortex::config::Config;
use gaia_cortex::engine::{now_secs, run_decider};
use gaia_cortex::server::api_limits;
use galion_common::http::serve;
use nerve_cortex_contract::Event;
use std::net::TcpListener;
use std::process::ExitCode;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const USAGE: &str = "usage: gaia-cortex [serve | replay FILE | config | hermes-dispatch] [--config PATH] | --version";

fn opt(args: &[String], k: &str) -> Option<String> {
    args.iter()
        .position(|a| a == k)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("{}", gaia_cortex::build_info::version_line());
        return ExitCode::SUCCESS;
    }
    let cmd = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .unwrap_or("serve");
    if cmd == "hermes-dispatch" {
        return hermes_dispatch();
    }
    let cfg = match Config::load(opt(&args, "--config").as_deref(), &|k| {
        std::env::var(k).ok()
    }) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gaia-cortex: {e}");
            return ExitCode::from(2);
        }
    };
    match cmd {
        "config" => {
            println!("{}", serde_json::to_string_pretty(&cfg).unwrap_or_default());
            ExitCode::SUCCESS
        }
        "replay" => {
            let Some(f) = args.get(1).filter(|a| !a.starts_with("--")) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let evs = match gaia_cortex::read_events(std::path::Path::new(f)) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("gaia-cortex: {e}");
                    return ExitCode::from(2);
                }
            };
            let _ = std::fs::create_dir_all(&cfg.data_dir);
            let mut cx = gaia_cortex::build(cfg);
            let d = cx.process(evs, now_secs());
            println!(
                "{}",
                serde_json::json!({"decisions": d, "stats": cx.stats_json()})
            );
            ExitCode::SUCCESS
        }
        "serve" => serve_main(cfg),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Drain the cortex -> Hermes inbox once (started by a systemd .path unit).
fn hermes_dispatch() -> ExitCode {
    use gaia_cortex::dispatch::{run, CurlHttp, DispatchCfg};
    let get = |k: &str| std::env::var(k).ok();
    let cfg = match DispatchCfg::from_env(&get) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("gaia-cortex hermes-dispatch: {e}");
            return ExitCode::from(2);
        }
    };
    let http = CurlHttp {
        tmp_dir: std::env::temp_dir(),
    };
    let r = run(&cfg, &http, &get, &|| gaia_cortex::engine::ts(now_secs()));
    println!("{r}");
    ExitCode::SUCCESS
}

fn serve_main(cfg: Config) -> ExitCode {
    if let Err(e) = std::fs::create_dir_all(&cfg.data_dir) {
        eprintln!("gaia-cortex: data dir {}: {e}", cfg.data_dir.display());
        return ExitCode::from(2);
    }
    let listener = match TcpListener::bind(&cfg.listen) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gaia-cortex: bind {}: {e}", cfg.listen);
            return ExitCode::from(2);
        }
    };
    let coalesce = Duration::from_millis(cfg.coalesce_ms);
    let freeze_on = cfg.freeze.enabled;
    if freeze_on {
        let f = &cfg.freeze;
        eprintln!(
            "gaia-cortex: freeze watch ON: probe every {}s (timeout {}s), {} consecutive failures over >= {}s, cooldown {}s, {} freeze restart(s)/unit/h",
            f.probe_s, f.timeout_s, f.fails, f.min_s, f.cooldown_s, f.per_unit_hour
        );
        if !f.skip.is_empty() {
            eprintln!("gaia-cortex: freeze watch skips {}", f.skip.join(", "));
        }
    }
    if let Some(p) = &cfg.phone {
        eprintln!(
            "gaia-cortex: failed-heal phone alerts ON via loopback relay {} (urgent, max {}/h)",
            p.path, p.per_hour
        );
    }
    let (socket, pull_every, nerve_url, stream, repeat_poll, flap_window) = (
        cfg.socket.clone(),
        cfg.pull_every_s,
        cfg.nerve_url.clone(),
        cfg.nerve_stream,
        cfg.repeat_poll_s,
        cfg.flap_window_s,
    );
    eprintln!(
        "gaia-cortex: listening {} mode {} action-mode {} llm {} (feature {}) nerve {} stream {} pull {}s repeat-poll {}s",
        cfg.listen,
        if cfg.dry_run() { "dry-run" } else { "live" },
        cfg.action_mode.as_str(),
        if cfg.llm.enabled {
            "ENABLED"
        } else {
            "disabled"
        },
        if gaia_cortex::llm::COMPILED {
            "compiled"
        } else {
            "not compiled"
        },
        nerve_url.as_deref().unwrap_or("none"),
        stream,
        pull_every,
        repeat_poll
    );
    let cx = Arc::new(Mutex::new(gaia_cortex::build(cfg)));
    let (tx, rx) = channel::<Vec<Event>>();
    {
        let cx = cx.clone();
        std::thread::spawn(move || run_decider(rx, cx, coalesce));
    }
    #[cfg(unix)]
    if let Some(p) = socket {
        let tx = tx.clone();
        std::thread::spawn(move || unix_socket(p, tx));
    }
    #[cfg(not(unix))]
    let _ = socket;
    if stream {
        if let Some(addr) = nerve_url
            .as_deref()
            .and_then(gaia_cortex::config::nerve_cortex_addr)
        {
            if repeat_poll > 0 {
                let w = Arc::new(gaia_cortex::repeat::RepeatWatch::new(flap_window));
                cx.lock().unwrap_or_else(|e| e.into_inner()).repeats = Some(w.clone());
                let tx = tx.clone();
                std::thread::spawn(move || {
                    gaia_cortex::repeat::run_poller(
                        w,
                        addr,
                        Duration::from_secs(repeat_poll),
                        &|v| tx.send(v).is_ok(),
                    )
                });
            }
            let (tx, cx) = (tx.clone(), cx.clone());
            std::thread::spawn(move || subscribe(addr, tx, cx));
        }
    }
    if freeze_on {
        match nerve_url
            .as_deref()
            .and_then(gaia_cortex::config::nerve_cortex_addr)
        {
            Some(addr) => {
                let (tx, cx) = (tx.clone(), cx.clone());
                std::thread::spawn(move || gaia_cortex::engine::run_freeze_watch(cx, tx, addr));
            }
            None => eprintln!("gaia-cortex: freeze watch needs nerve_url; not started"),
        }
    }
    if pull_every > 0 {
        if let Some(n) = nerve_url
            .as_deref()
            .and_then(gaia_cortex::nerve::HttpNerve::new)
        {
            let tx = tx.clone();
            let n = n.with_contact(cx.lock().unwrap_or_else(|e| e.into_inner()).contact.clone());
            std::thread::spawn(move || pull_loop(n, tx, Duration::from_secs(pull_every)));
        }
    }
    let api = gaia_cortex::server::Api::new(cx, tx);
    let handler =
        Arc::new(move |r: &galion_common::http::Request| gaia_cortex::server::handle(r, &api));
    match serve(listener, api_limits(), handler) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gaia-cortex: {e}");
            ExitCode::FAILURE
        }
    }
}

/// NDJSON over a unix socket: one connection per producer, one event per line.
#[cfg(unix)]
fn unix_socket(path: std::path::PathBuf, tx: Sender<Vec<Event>>) {
    use std::io::BufRead;
    use std::os::unix::net::UnixListener;
    let _ = std::fs::remove_file(&path);
    let l = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("gaia-cortex: socket {}: {e}", path.display());
            return;
        }
    };
    for s in l.incoming().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(s)
                .lines()
                .map_while(Result::ok)
                .take(1_000_000)
            {
                let evs: Vec<Event> = nerve_cortex_contract::parse_events(&line)
                    .into_iter()
                    .filter_map(Result::ok)
                    .collect();
                if !evs.is_empty() && tx.send(evs).is_err() {
                    break;
                }
            }
        });
    }
}

/// Optional pull adapter (off by default). Remembers fingerprints so the
/// re-read tail is not re-sent; the first read only primes that memory, so
/// events from before the cortex started are not re-decided. Zero model
/// calls; one loopback GET per tick.
fn pull_loop(mut n: gaia_cortex::nerve::HttpNerve, tx: Sender<Vec<Event>>, every: Duration) {
    use gaia_cortex::nerve::NerveLink;
    use std::collections::{HashSet, VecDeque};
    let mut seen: HashSet<u64> = HashSet::new();
    let mut order: VecDeque<u64> = VecDeque::new();
    let mut first = true;
    loop {
        if let Ok(evs) = n.pull_events(200) {
            let mut fresh = vec![];
            for e in evs {
                let fp = e.fingerprint();
                if seen.insert(fp) {
                    order.push_back(fp);
                    if order.len() > 4096 {
                        if let Some(o) = order.pop_front() {
                            seen.remove(&o);
                        }
                    }
                    fresh.push(e);
                }
            }
            if !first && !fresh.is_empty() && tx.send(fresh).is_err() {
                return;
            }
            first = false;
        }
        std::thread::sleep(every);
    }
}

/// Subscribe to the nerve's NDJSON stream. Blocks in `read()` while the
/// nerve is quiet (zero work); reconnects with backoff, resuming from the
/// last seq. A nerve without `/events/stream` (polling mode, 404) is
/// retried every 10 min only.
///
/// On every (re)connect the nerve instance is checked first (`GET /stats`:
/// `started`, `seq`). A restarted nerve numbers its events from 1 again, so
/// the old cursor would skip them and the old seq fingerprints would drop
/// them as duplicates: the cursor goes back to 0 and the engine forgets the
/// old fingerprints (T5.1 chaos S8).
fn subscribe(
    addr: std::net::SocketAddr,
    tx: Sender<Vec<Event>>,
    cx: Arc<Mutex<gaia_cortex::engine::Cortex>>,
) {
    use gaia_cortex::nerve::{
        nerve_incidents, nerve_instance, resume_since, stream_once_seen, StreamEnd,
    };
    let contact = cx.lock().unwrap_or_else(|e| e.into_inner()).contact.clone();
    contact.set_stream(gaia_cortex::contact::Stream::Down);
    let mut since: Option<u64> = None;
    let mut instance: Option<String> = None;
    let mut backoff = Duration::from_secs(1);
    loop {
        let now = nerve_instance(addr, Duration::from_secs(3));
        if now.is_some() {
            contact.ok();
        }
        let (s, restarted) = resume_since(
            since,
            instance.as_deref(),
            now.as_ref().map(|(st, q)| (st.as_str(), *q)),
        );
        if restarted {
            eprintln!(
                "gaia-cortex: nerve restarted (was {instance:?} at seq {since:?}, now {now:?}); resuming from its first event"
            );
            let mut g = cx.lock().unwrap_or_else(|e| e.into_inner());
            g.nerve_restarted();
            g.publish();
        }
        since = s;
        // First connect: start at the nerve's current seq (read before the
        // incident snapshot below), so an incident opened between the
        // snapshot and the stream is replayed instead of falling in between.
        if since.is_none() {
            since = now.as_ref().map(|(_, q)| *q);
        }
        if let Some((st, _)) = now {
            instance = Some(st);
        }
        // Backfill: open nerve incidents the cortex has not seen (opened
        // before it was listening), and resolve the ones the nerve dropped
        // while the stream was down (slot 237).
        if let Some(snap) = nerve_incidents(addr, Duration::from_secs(3)) {
            contact.ok();
            let evs = cx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .backfill(&snap, now_secs());
            if !evs.is_empty() {
                eprintln!(
                    "gaia-cortex: nerve incident backfill: {} event(s) from GET /incidents",
                    evs.len()
                );
                if tx.send(evs).is_err() {
                    return;
                }
            }
        }
        let started = std::time::Instant::now();
        let mut got = 0u64;
        let end = stream_once_seen(
            addr,
            &mut since,
            Duration::from_secs(11 * 60),
            Some(&contact),
            &mut |e| {
                got += 1;
                tx.send(vec![e]).is_ok()
            },
        );
        match &end {
            StreamEnd::Stopped => {}
            // The nerve answered, it just has no stream (polling mode).
            StreamEnd::Status(404) | StreamEnd::Status(405) => contact.ok(),
            other => contact.fail(&format!("stream ended: {other:?}")),
        }
        let wait = match end {
            StreamEnd::Stopped => return,
            StreamEnd::Status(404) | StreamEnd::Status(405) => {
                eprintln!(
                    "gaia-cortex: nerve has no /events/stream (polling nerve?); retry in 600 s"
                );
                Duration::from_secs(600)
            }
            other => {
                if got > 0 || started.elapsed() > Duration::from_secs(60) {
                    backoff = Duration::from_secs(1);
                }
                eprintln!("gaia-cortex: nerve stream ended ({other:?}) after {got} lines; reconnect in {}s (since {since:?})", backoff.as_secs());
                let w = backoff;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                w
            }
        };
        std::thread::sleep(wait);
    }
}
