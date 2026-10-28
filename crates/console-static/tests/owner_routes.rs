// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--owner-routes` (wave 4 slot 395, D-384-1 option A prep): in-process
//! end-to-end over loopback with fake galion-one and hermes-rs owners
//! (ephemeral ports). The live-data proof is the seven console real tests
//! with `CONSOLE_REAL_API=local` (docs/real-tests/console-rust-routes-a.md).

use console_static::config::Config;
use console_static::owners;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use galion_realtest::console_routes::{self as map, Owner as MapOwner};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

type Seen = Arc<Mutex<Vec<String>>>;

/// Fake owner: records each request head, answers `status` with a JSON body
/// naming `who` and the path it got.
fn fake_owner(who: &'static str, status: u16, seen: Seen) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let seen = Arc::clone(&seen);
            thread::spawn(move || {
                let mut s = s;
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                let path = head.split(' ').nth(1).unwrap_or("").to_string();
                seen.lock().unwrap().push(head);
                let body = format!(r#"{{"who":"{who}","path":"{path}"}}"#);
                let _ = write!(
                    s,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    addr
}

/// Empty arena root under the temp dir (the reader answers the script
/// defaults for an empty folder).
fn arena_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("owner-routes-arena-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn start(extra: &[String]) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let mut args: Vec<String> = ["--ui", "rust", "--local-api", "--quiet", "--bind"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(addr.to_string());
    args.extend_from_slice(extra);
    // Slot 413 (F-395-2): --owner-routes needs an arena reader; give it an
    // empty throwaway root unless the test brings its own.
    if extra.iter().any(|a| a == "--owner-routes") && !extra.iter().any(|a| a == "--arena-root") {
        args.push("--arena-root".into());
        args.push(arena_dir().display().to_string());
    }
    let cfg = Config::from_args(args).unwrap();
    let state = Arc::new(State::new(
        Site::default(),
        Routes::parse("page /").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    addr
}

struct Resp {
    code: u16,
    body: String,
}

fn req(addr: SocketAddr, method: &str, path: &str, host: &str, extra: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    Resp {
        code: head
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0),
        body: body.to_string(),
    }
}

fn get(addr: SocketAddr, path: &str) -> Resp {
    req(addr, "GET", path, "127.0.0.1", "")
}

#[test]
fn owner_rows_forward_rewritten_without_credentials() {
    let seen_p: Seen = Arc::default();
    let seen_h: Seen = Arc::default();
    let pulse = fake_owner("one", 401, Arc::clone(&seen_p));
    let hermes = fake_owner("hermes", 200, Arc::clone(&seen_h));
    let a = start(&[
        "--owner-routes".into(),
        "--pulse".into(),
        pulse.to_string(),
        "--hermes".into(),
        hermes.to_string(),
    ]);
    let r = req(
        a,
        "GET",
        "/api/v1/analytics?board=x",
        "127.0.0.1",
        "Cookie: galion_admin=secret-ish\r\nAuthorization: Bearer nope\r\n",
    );
    assert_eq!(r.code, 401, "owner status passed through");
    assert!(r.body.contains(r#""who":"one""#), "{}", r.body);
    assert!(r.body.contains(r#""path":"/api/v1/analytics?board=x""#));
    let r = get(a, "/api/v1/awake");
    assert!(r.body.contains(r#""who":"one""#));
    let r = get(a, "/api/hermes/health");
    assert_eq!(r.code, 200);
    assert!(
        r.body.contains(r#""who":"hermes","path":"/health""#),
        "{}",
        r.body
    );
    let heads = seen_p.lock().unwrap().clone();
    assert_eq!(heads.len(), 2);
    for h in &heads {
        let l = h.to_ascii_lowercase();
        assert!(!l.contains("\r\ncookie:"), "cookie sent on: {h}");
        assert!(
            !l.contains("\r\nauthorization:"),
            "authorization sent on: {h}"
        );
    }
    // POST to an owner row is refused here, never sent on.
    let r = req(
        a,
        "POST",
        "/api/v1/analytics",
        "127.0.0.1",
        "Content-Length: 0\r\n",
    );
    assert_eq!(r.code, 405);
    assert_eq!(seen_p.lock().unwrap().len(), 2);
    // D-384-3: models/history not proxied (bearer key server-side is held).
    assert_eq!(get(a, "/api/hermes/history").code, 404);
    assert_eq!(get(a, "/api/hermes/models").code, 404);
    assert_eq!(seen_h.lock().unwrap().len(), 1);
}

#[test]
fn retired_rows_answer_410_json() {
    let a = start(&["--owner-routes".into()]);
    for p in owners::RETIRED {
        let r = get(a, p);
        assert_eq!(r.code, 410, "{p}");
        let v = console_ui::json::parse(r.body.as_bytes()).unwrap();
        assert_eq!(v.get("error").as_str(), Some("retired"), "{p}");
        assert_eq!(v.get("route").as_str(), Some(*p));
        let h = req(a, "HEAD", p, "127.0.0.1", "");
        assert_eq!(h.code, 410);
        assert!(h.body.is_empty(), "HEAD body {p}");
    }
    // Owner configured nowhere: owner rows stay 404 (honest, like live).
    assert_eq!(get(a, "/api/v1/analytics").code, 404);
    // Console-owned and board rows unchanged.
    assert_eq!(get(a, "/api/v1/health").code, 200);
    assert_eq!(get(a, "/api/v1/markers").code, 200);
    assert_eq!(get(a, "/api/v1/session").code, 401);
}

#[test]
fn gate_runs_before_owner_and_retired_rows() {
    let seen: Seen = Arc::default();
    let pulse = fake_owner("one", 200, Arc::clone(&seen));
    let a = start(&["--owner-routes".into(), "--pulse".into(), pulse.to_string()]);
    for p in ["/api/v1/analytics", "/api/mesh"] {
        let r = req(a, "GET", p, "evil.example", "");
        assert_eq!(r.code, 401, "{p}: foreign host");
        let r = req(a, "GET", p, "console.galion.app", "");
        assert_eq!(r.code, 401, "{p}: remote console host without admin");
    }
    assert!(seen.lock().unwrap().is_empty(), "nothing reached the owner");
}

#[test]
fn without_flag_live_behaviour_is_kept() {
    let seen: Seen = Arc::default();
    let pulse = fake_owner("one", 200, Arc::clone(&seen));
    let a = start(&["--pulse".into(), pulse.to_string()]);
    assert_eq!(get(a, "/api/v1/analytics").code, 404);
    assert_eq!(get(a, "/api/mesh").code, 404);
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn flags_validate() {
    let base = |x: &[&str]| {
        let mut v: Vec<String> = ["--ui", "rust", "--upstream", "127.0.0.1:9"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        v.extend(x.iter().map(|s| s.to_string()));
        Config::from_args(v)
    };
    assert!(base(&["--owner-routes"]).is_err(), "needs --local-api");
    assert!(
        base(&["--local-api", "--hermes", "127.0.0.1:1"]).is_err(),
        "needs --owner-routes"
    );
    assert!(base(&["--local-api", "--owner-routes", "--hermes", "10.0.0.1:1"]).is_err());
    let c = base(&[
        "--local-api",
        "--owner-routes",
        "--hermes",
        "127.0.0.1:1",
        "--arena-root",
        "/workspace/arena",
    ])
    .unwrap();
    assert!(c.owner_routes && c.hermes.is_some());
    assert!(!base(&["--local-api"]).unwrap().owner_routes);
}

/// F-395-2 (slot 413): --owner-routes refuses to start when nothing answers
/// /api/arena; an --arena-root or an arena --data-path satisfies it.
#[test]
fn owner_routes_refuse_without_arena_reader() {
    let base = |x: &[&str]| {
        let mut v: Vec<String> = ["--ui", "rust", "--local-api", "--bind", "127.0.0.1:22839"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        v.extend(x.iter().map(|s| s.to_string()));
        Config::from_args(v)
    };
    let e = base(&["--owner-routes"]).unwrap_err().to_string();
    assert!(e.contains("--owner-routes needs --arena-root"), "{e}");
    let e = base(&[
        "--owner-routes",
        "--data-upstream",
        "127.0.0.1:1",
        "--data-path",
        "/api/v1/bank",
    ])
    .unwrap_err()
    .to_string();
    assert!(e.contains("/api/arena would 404"), "{e}");
    assert!(base(&["--owner-routes", "--arena-root", "/workspace/arena"]).is_ok());
    let c = base(&["--owner-routes", "--data-upstream", "127.0.0.1:1"]).unwrap();
    assert_eq!(
        c.data_paths,
        vec!["/api/arena"],
        "F-395-3 default drops bank"
    );
    // Without --owner-routes nothing changes (live argv has no flag).
    assert!(base(&[]).is_ok());
    let c = base(&["--data-upstream", "127.0.0.1:1"]).unwrap();
    assert_eq!(c.data_paths, vec!["/api/arena", "/api/v1/bank"]);
}

/// F-395-3 (slot 413): with --owner-routes the retired /api/v1/bank 410 wins
/// over the default data proxy; only an explicit --data-path sends it on.
#[test]
fn retired_bank_410_wins_over_default_data_proxy() {
    let seen: Seen = Arc::default();
    let data = fake_owner("data", 200, Arc::clone(&seen));
    let a = start(&[
        "--owner-routes".into(),
        "--data-upstream".into(),
        data.to_string(),
    ]);
    let r = get(a, "/api/v1/bank");
    assert_eq!(r.code, 410, "{}", r.body);
    assert!(r.body.contains(r#""route":"/api/v1/bank""#), "{}", r.body);
    // Arena: in-process reader (empty root -> script defaults), not proxied.
    let r = get(a, "/api/arena");
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains(r#""ok":true"#), "{}", r.body);
    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing reached data upstream"
    );

    // Owner opt-in: explicit --data-path /api/v1/bank forwards.
    let b = start(&[
        "--owner-routes".into(),
        "--data-upstream".into(),
        data.to_string(),
        "--data-path".into(),
        "/api/v1/bank".into(),
    ]);
    let r = get(b, "/api/v1/bank");
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains(r#""who":"data""#), "{}", r.body);
    assert_eq!(seen.lock().unwrap().len(), 1);

    // No --owner-routes (live argv): default data proxy still has bank.
    let c = start(&["--data-upstream".into(), data.to_string()]);
    let r = get(c, "/api/v1/bank");
    assert!(r.body.contains(r#""who":"data""#), "{}", r.body);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

/// The console lists and the slot 384 route MAP agree: every retired MAP row
/// is answered 410, every owner row forwards to the mapped owner path, and
/// nothing is retired here that the MAP gives an owner.
#[test]
fn lists_match_console_routes_map() {
    let mut retired: Vec<&str> = map::MAP
        .iter()
        .filter(|r| r.owner == MapOwner::Retired)
        .map(|r| r.node)
        .collect();
    retired.sort();
    let mut ours: Vec<&str> = owners::RETIRED.to_vec();
    ours.sort();
    assert_eq!(ours, retired);
    for r in owners::OWNER_ROUTES {
        let m = map::lookup(r.node).unwrap_or_else(|| panic!("{} not in MAP", r.node));
        let want = match r.owner {
            owners::Owner::Pulse => MapOwner::One,
            owners::Owner::Hermes => MapOwner::HermesRs,
        };
        assert_eq!(m.owner, want, "{}", r.node);
        assert_eq!(m.rust, r.target, "{}", r.node);
    }
    // Every One/HermesRs MAP row is either forwarded or held on purpose.
    for m in map::MAP
        .iter()
        .filter(|m| matches!(m.owner, MapOwner::One | MapOwner::HermesRs))
    {
        let held = ["/api/hermes/models", "/api/hermes/history"].contains(&m.node);
        assert!(
            owners::owner_route(m.node).is_some() != held,
            "{}: forwarded xor held (D-384-3)",
            m.node
        );
    }
}
