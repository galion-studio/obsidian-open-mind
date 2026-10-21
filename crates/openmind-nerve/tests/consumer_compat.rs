// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Nerve T3.6 (wave-4 slot 180): consumer compatibility. Every in-repo
//! consumer of the nerve's endpoints is replayed against openmind-nerve's
//! real HTTP server (a real loopback socket, in-process), using the
//! consumer's OWN client code and contract tables where it has them:
//!
//! | consumer | what it calls | how it is checked here |
//! |---|---|---|
//! | galion-realtest live probe (`live::VPS`) | `GET /healthz` Exact("ok"), `GET /` Contains("galion-nerve") | its table rows + `live::judge` |
//! | galion-realtest `scenarios::nerve` | `/healthz`, `POST /signal` 202, `GET /signals`, bad signal 400 | its `http::get` / `http::post_json` |
//! | nerve-client (hermes-rs, hermes-gateway, openmind-memory) | `POST /signal` HTTP/1.0 | its `Client` + `body()` |
//! | gaia-live `nerve` (gaia heal-tick) | `POST /event`, `GET /events?n=`, `GET /health` | its `post` / `tail` / `healthy` |
//! | galion-deploy health table (F4) | `GET /healthz` | its `health_table` / `rollback_health` |
//! | deploy/nerve/install.sh, nerve-capture-vps.sh, replay | `/healthz`, `/state`, `/self-eval(.md)`, `/`, `/health` | status + content type |
//! | gaia nerve-restarts (swarm/gaia-loop-decisions b40208a) | `GET /state` components[].restarts_1h, reflex.budgets[unit].live_1h | shape of `/state` |
//!
//! Breaks found are listed in docs/nerve-consumers.md (B1-B6).

use galion_realtest::live::{self, Body, Got};
use openmind_nerve::engine::Shared;
use openmind_nerve::server::{serve, EventLog};
use openmind_nerve::Writer;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LIVE_STATE: &str = include_str!("fixtures/consumer/state-live-trimmed.json");

struct Nerve {
    addr: SocketAddr,
    dir: Option<PathBuf>,
}

impl Drop for Nerve {
    fn drop(&mut self) {
        if let Some(d) = &self.dir {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

/// openmind-nerve's real server on 127.0.0.1:0. `state`: None = before the
/// first cycle (Shared::default, state null). `log`: enable POST /event.
fn start(state: Option<&str>, log: bool, tag: &str) -> Nerve {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let sh = Arc::new(Mutex::new(Shared::default()));
    if let Some(s) = state {
        sh.lock().unwrap().state = serde_json::from_str(s).unwrap();
    }
    let (ev, dir) = if log {
        let dir = std::env::temp_dir().join(format!(
            "nerve-t36-{tag}-{}-{}",
            std::process::id(),
            addr.port()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        let ev = EventLog {
            writer: Writer::open(&path, 1 << 20, 2).unwrap(),
            path,
            appended: 0,
        };
        (Some(Arc::new(Mutex::new(ev))), Some(dir))
    } else {
        (None, None)
    };
    let sh2 = sh.clone();
    std::thread::spawn(move || serve(l, sh2, ev));
    Nerve { addr, dir }
}

/// Raw GET -> galion-realtest's `Got` (status, content type, body).
fn got(addr: SocketAddr, path: &str) -> Got {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    live::parse_curl(&raw)
}

fn path_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    rest.find('/').map(|i| &rest[i..]).unwrap_or("/")
}

fn nerve_rows() -> Vec<&'static live::Probe> {
    live::VPS
        .iter()
        .filter(|p| p.service == "galion-nerve")
        .collect()
}

#[test]
fn realtest_live_probe_rows_pass_before_and_after_the_first_cycle() {
    let rows = nerve_rows();
    assert!(
        rows.len() >= 2,
        "galion-realtest live::VPS lost its galion-nerve rows"
    );
    assert!(rows
        .iter()
        .any(|p| matches!(p.body, Body::Contains("galion-nerve"))));
    for (label, st) in [
        ("before first cycle", None),
        ("live-shaped state", Some(LIVE_STATE)),
    ] {
        let n = start(st, false, "live");
        for p in &rows {
            assert!(p.url.starts_with("http://127.0.0.1:8670/"), "{}", p.url);
            let g = got(n.addr, path_of(p.url));
            let r = live::judge(p, &g);
            assert!(r.is_ok(), "{label}: {} {} -> {r:?}", p.name, p.url);
        }
    }
}

#[test]
fn realtest_scenario_nerve_ingest_contract() {
    use galion_realtest::http;
    let n = start(None, false, "scn");
    let a = n.addr.to_string();
    assert_eq!(http::get(&a, "/healthz").unwrap().status, 200);
    let r = http::post_json(
        &a,
        "/signal",
        &[],
        "{\"source\":\"realtest\",\"kind\":\"event\",\"name\":\"realtest-probe\",\"value\":1}",
    )
    .unwrap();
    assert_eq!(r.status, 202, "{}", r.text());
    let t = http::get(&a, "/signals").unwrap().text();
    assert!(t.contains("realtest"), "{t}");
    let bad = http::post_json(&a, "/signal", &[], "{\"kind\":\"nope\"}").unwrap();
    assert_eq!(bad.status, 400);
}

fn wait<F: Fn() -> bool>(f: F) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn nerve_client_bodies_of_every_in_repo_caller_are_accepted() {
    let n = start(None, false, "client");
    let c = nerve_client::Client::start(n.addr, 64).unwrap();
    // The (source, kind, name) triples hermes-rs, hermes-gateway (via
    // hermes-rs) and openmind-memory push: started, heartbeat, metric,
    // http_5xx, slow_request, spend_cap / conn_cap.
    let bodies = [
        nerve_client::body(
            "hermes-rs",
            "event",
            "started",
            None,
            "listening on 127.0.0.1:8642",
        ),
        nerve_client::body(
            "hermes-rs",
            "heartbeat",
            "rss_kb",
            Some(9000.0),
            "queued=1 sent=0 dropped=0 failed=0 rejected=0",
        ),
        nerve_client::body("hermes-rs", "metric", "nerve_client_dropped", Some(0.0), ""),
        nerve_client::body(
            "hermes-rs",
            "error",
            "http_5xx",
            None,
            "POST /v1/chat/completions -> 502",
        ),
        nerve_client::body(
            "hermes-rs",
            "event",
            "slow_request",
            Some(1500.0),
            "GET / -> 200 in 1500 ms",
        ),
        nerve_client::body(
            "hermes-rs",
            "event",
            "spend_cap",
            Some(1.0),
            "daily cap \"x\"\nhit",
        ),
        nerve_client::body("openmind-memory", "event", "started", None, "listening"),
        nerve_client::body(
            "openmind-memory",
            "event",
            "conn_cap",
            Some(3.0),
            "at cap (+2 more since the last report)",
        ),
    ];
    for b in &bodies {
        assert!(c.push(b.clone()));
    }
    assert!(
        wait(|| c.counters.snapshot().sent == bodies.len() as u64),
        "{:?}",
        c.counters.snapshot()
    );
    let s = c.counters.snapshot();
    assert_eq!((s.rejected, s.failed, s.dropped), (0, 0, 0));
    let sig: Value = serde_json::from_str(&got(n.addr, "/signals").body).unwrap();
    let names: Vec<&str> = sig["sources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["source"].as_str())
        .collect();
    assert_eq!(names, ["hermes-rs", "openmind-memory"]);
}

#[test]
fn nerve_client_sees_the_per_source_rate_limit_as_rejected() {
    // B4: the nerve answers 429 above 120 signals/min/source. The client used
    // to count those as `sent`; it now counts them as `rejected`.
    let n = start(None, false, "rate");
    let c = nerve_client::Client::start(n.addr, 256).unwrap();
    for i in 0..130 {
        assert!(c.push(nerve_client::body(
            "hermes-rs",
            "error",
            "http_5xx",
            None,
            &format!("r{i}")
        )));
    }
    assert!(wait(|| {
        let s = c.counters.snapshot();
        s.sent + s.rejected + s.failed == 130
    }));
    let s = c.counters.snapshot();
    assert_eq!(s.failed, 0, "{s:?}");
    assert!(s.sent <= 120 && s.rejected >= 10, "{s:?}");
    let sig: Value = serde_json::from_str(&got(n.addr, "/signals").body).unwrap();
    let dropped =
        sig["sources"][0]["dropped"].as_u64().unwrap_or(0) + sig["rejected"].as_u64().unwrap_or(0);
    assert_eq!(dropped, s.rejected, "nerve and client agree: {sig}");
}

#[test]
fn gaia_live_client_against_openmind_nerve() {
    use gaia_live::nerve;
    let n = start(Some(LIVE_STATE), true, "gaia");
    assert!(nerve::healthy(n.addr), "GET /health must carry status ok");
    nerve::post(
        n.addr,
        "gaia-live",
        "heal.restart",
        "info",
        serde_json::json!({"service": "dummy", "port": 20501}),
    )
    .unwrap();
    let t = nerve::tail(n.addr, 5).unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0]["source"], "gaia-live");
    assert_eq!(t[0]["data"]["port"], 20501);
    assert!(t[0]["ts"].as_str().unwrap().contains("+0"), "Zagreb offset");
    // A torn last line (crash mid-append) or a foreign JSONL line must not
    // turn the tail into HTTP 500 (B5).
    let p = n.dir.as_ref().unwrap().join("events.jsonl");
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(b"{\"ts\":\"2026-10-21T08:00:00+02:00\",\"source\":\"guardian\",\"kind\":\"probe\",\"name\":\"one\",\"ok\":false,\"detail\":\"x\"}\n{\"ts\":\"2026-10-2").unwrap();
    drop(f);
    let t = nerve::tail(n.addr, 5).unwrap();
    assert_eq!(t.len(), 1, "foreign + torn lines skipped");
}

#[test]
fn writer_reopen_heals_a_torn_tail() {
    // B5b: after a crash mid-append, the next process's first event used to be
    // glued onto the torn line (both unreadable).
    let dir = std::env::temp_dir().join(format!("nerve-t36-torn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("events.jsonl");
    std::fs::write(&p, "{\"ts\":\"2026-10-2").unwrap();
    let mut w = Writer::open(&p, 1 << 20, 2).unwrap();
    w.append(&openmind_nerve::Event::new("gaia-live", "after-crash", "info", "{}").unwrap())
        .unwrap();
    let (ev, skipped) = openmind_nerve::read_lossy(&p).unwrap();
    assert_eq!((ev.len(), skipped), (1, 1));
    assert_eq!(ev[0].kind(), "after-crash");
    // A clean file is left alone (no blank line added on reopen).
    let before = std::fs::read(&p).unwrap();
    drop(w);
    let _w = Writer::open(&p, 1 << 20, 2).unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gaia_live_without_event_log_is_a_404_break() {
    // B2: the unit file runs `galion-nerve serve` with no --log /
    // NERVE_EVENT_LOG, so gaia-live's POST /event gets 404 there.
    use gaia_live::nerve;
    let n = start(None, false, "nolog");
    assert!(nerve::healthy(n.addr));
    let e = nerve::post(n.addr, "gaia-live", "k", "info", serde_json::json!({})).unwrap_err();
    assert!(e.contains("HTTP 404"), "{e}");
    assert!(nerve::tail(n.addr, 5).unwrap().is_empty());
}

#[test]
fn galion_deploy_health_paths() {
    use galion_deploy::deploy::{health_table, rollback_health};
    let n = start(None, false, "deploy");
    let url = health_table("galion-nerve").unwrap();
    assert_eq!(url, "http://127.0.0.1:8670/healthz");
    assert_eq!(got(n.addr, path_of(url)).status, Some(200));
    // F4: a deploy judged on /health judges the rollback (old nerve, /health
    // 404 on the live build) on /healthz.
    let rb = rollback_health("galion-nerve", "http://127.0.0.1:8670/health");
    assert_eq!(path_of(&rb), "/healthz");
    // openmind-nerve itself answers /health (the live build does not).
    assert_eq!(got(n.addr, "/health").status, Some(200));
}

#[test]
fn capture_install_and_replay_routes() {
    let n = start(Some(LIVE_STATE), false, "routes");
    for (path, ctype) in [
        ("/healthz", "text/plain"),
        ("/", "text/html; charset=utf-8"),
        ("/health", "application/json"),
        ("/state", "application/json"),
        ("/state.json", "application/json"),
        ("/self-eval", "application/json"),
        ("/self-eval.md", "text/markdown; charset=utf-8"),
        ("/alerts", "application/json"),
        ("/audit", "application/json"),
        ("/signals", "application/json"),
        ("/incidents", "application/json"),
        ("/cortex", "application/json"),
        ("/ddos", "application/json"),
        ("/api/v1/state", "application/json"),
    ] {
        let g = got(n.addr, path);
        assert_eq!(
            (g.status, g.content_type.as_str()),
            (Some(200), ctype),
            "{path}"
        );
    }
    assert_eq!(
        got(n.addr, "/signal").status,
        Some(405),
        "live answers 405 too"
    );
    assert_eq!(got(n.addr, "/nope").status, Some(404));
    let st: Value = serde_json::from_str(&got(n.addr, "/state").body).unwrap();
    assert_eq!(st["cycle"], 7004);
}

#[test]
fn gaia_restart_counter_contract_on_state() {
    // swarm/gaia-loop-decisions b40208a reads components[].restarts_1h and
    // reflex.budgets[unit].live_1h, failing closed when /state is not an
    // object with a components list.
    let n = start(Some(LIVE_STATE), false, "gaia-st");
    let st: Value = serde_json::from_str(&got(n.addr, "/state").body).unwrap();
    let comps = st["components"].as_array().expect("components list");
    for c in comps {
        assert!(c["name"].is_string());
        assert!(c["restarts_1h"].is_u64(), "{c}");
    }
    let b = st["reflex"]["budgets"].as_object().expect("reflex.budgets");
    for (u, v) in b {
        assert!(u.ends_with(".service"));
        assert!(v["live_1h"].is_u64(), "{u}: {v}");
    }
    // Before the first cycle /state is `null`: no components list, so the
    // gaia reader fails closed (B3; owner decision D1 whether to answer 503).
    let n0 = start(None, false, "gaia-null");
    let g = got(n0.addr, "/state");
    assert_eq!((g.status, g.body.as_str()), (Some(200), "null"));
}

#[test]
fn guardian_tick_writer_lines_are_what_events_and_galion_one_read() {
    // guardian-tick heal writes <state>/nerve.jsonl with openmind_nerve::Writer;
    // galion-one shows its last non-empty line raw; read_all/read_lossy parse it.
    let dir = std::env::temp_dir().join(format!("nerve-t36-gt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("nerve.jsonl");
    let mut w = Writer::open(&p, 1 << 20, 3).unwrap();
    let e = openmind_nerve::Event::new(
        "guardian-tick",
        "heal-cap-hit",
        "warn",
        "{\"service\":\"tuner\"}",
    )
    .unwrap();
    w.append(&e).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    let last = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap();
    let v: Value = serde_json::from_str(last).unwrap();
    assert_eq!(
        (v["source"].as_str(), v["kind"].as_str()),
        (Some("guardian-tick"), Some("heal-cap-hit"))
    );
    assert_eq!(openmind_nerve::read_all(&p).unwrap().len(), 1);
    // B6: openmind-survival's nerve_line format is NOT a nerve event (no
    // severity/data): strict read_all fails, read_lossy skips it.
    std::fs::write(&p, format!("{text}{{\"ts\":\"t\",\"source\":\"guardian\",\"kind\":\"probe\",\"name\":\"one\",\"ok\":true,\"detail\":\"\"}}\n")).unwrap();
    assert!(openmind_nerve::read_all(&p).is_err());
    assert_eq!(openmind_nerve::read_lossy(&p).unwrap().1, 1);
    let _ = std::fs::remove_dir_all(&dir);
}
