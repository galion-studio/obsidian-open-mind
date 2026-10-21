// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! End-to-end tests for the console-port-3 rooms (`/instances`, `/keys`,
//! `/markers`) with `--ui rust`: the real server, a stateful fake console API
//! (key store and marker ring in memory, shapes from the live API on
//! 2026-10-21), raw HTTP/1.1 over loopback (ephemeral ports).
//!
//! Covers: GET-only plain views; the no-JS mint / revoke / ping form posts
//! and exactly what reaches the upstream; CSRF refusals with nothing sent;
//! unknown actions and bad ids with nothing sent; the fresh key shown once
//! and never again; the 401 "phone required" list; a slow laptop endpoint
//! (call budget) and a dead upstream; the pairing gate.

use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const HEALTH: &str = r#"{"status":"healthy","version":"3.8.3-board","target":{"pod":"ip50"}}"#;
const INSTANCES: &str = r#"{"gpu":null,"nodes":[{"id":"phone","name":"This phone","role":"Screen","place":"Your hand","detail":"Opens the console. Runs no model.","state":"online","mark":"online"},{"id":"pod","name":"RunPod","role":"Machine","place":"RTX 2000 Ada","detail":"GPU unread","state":"online","mark":"online"},{"id":"laptop","name":"GALION-STUDIO","role":"Open model","place":"RTX 4060 Laptop · 8 GB","detail":"Windows 11 · 64 GB RAM · private tunnel only","state":"offline","mark":"offline"}],"parts":[{"id":"front","name":"Front","where":"This pod","bind":":8080","note":"The console.","state":"online"},{"id":"hermes","name":"Hermes","where":"This pod","bind":"127.0.0.1:8642","note":"The mind.","state":"online"},{"id":"voice","name":"Voice <x>","where":"This pod","bind":"127.0.0.1:8002","note":"Inside One.","state":"offline"}],"link":{"public":false,"pubkey":"ssh-ed25519 AAAAtest galion-laptop","tunnel":"ssh -N -R 127.0.0.1:11434:127.0.0.1:11434"}}"#;
const LAPTOP: &str = r#"{"connected":true,"jobs":[{"id":"model-pull","name":"huihui_ai/qwen3.5-abliterated:9b","detail":"100 MB / 200 MB on the laptop","pct":50}],"lines":["copying installer 1 / 2","live powershell -NoProfile"]}"#;

/// The fake upstream's memory.
#[derive(Default)]
struct Fake {
    /// "METHOD path cookie body" per request.
    log: Vec<String>,
    /// (id, name, scopes, revoked)
    keys: Vec<(String, String, Vec<String>, bool)>,
    markers: Vec<String>,
    seq: u32,
    /// `/api/v1/laptop` sleeps this long (call-budget test).
    laptop_delay: Duration,
}

type Shared = Arc<Mutex<Fake>>;

fn fake_api(fake: Shared) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let f = Arc::clone(&fake);
            thread::spawn(move || answer(s, f));
        }
    });
    addr
}

fn field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let k = format!("\"{key}\":\"");
    let i = body.find(&k)? + k.len();
    body[i..].split('"').next()
}

fn answer(mut s: TcpStream, fake: Shared) {
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head, body) = loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..p]).to_string();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            while buf.len() < p + 4 + len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            break (head, String::from_utf8_lossy(&buf[p + 4..]).to_string());
        }
    };
    let first = head.lines().next().unwrap_or("").to_string();
    let cookie = head
        .lines()
        .find_map(|l| l.strip_prefix("Cookie: "))
        .unwrap_or("-")
        .to_string();
    let method = first.split(' ').next().unwrap_or("").to_string();
    let path = first.split(' ').nth(1).unwrap_or("").to_string();
    let admin = cookie.contains("galion_admin=good");
    let delay = {
        let mut f = fake.lock().unwrap();
        f.log.push(format!("{method} {path} {cookie} {body}"));
        f.laptop_delay
    };
    let (code, out): (u16, String) = match (method.as_str(), path.as_str()) {
        ("GET", "/api/v1/admin") => (200, format!("{{\"ok\":true,\"you\":{admin}}}")),
        ("GET", "/api/v1/health") => (200, HEALTH.into()),
        ("GET", "/api/v1/instances") => (200, INSTANCES.into()),
        ("GET", "/api/v1/laptop") => {
            thread::sleep(delay);
            (200, LAPTOP.into())
        }
        ("GET" | "POST", "/api/v1/keys") if !admin => {
            (401, "{\"error\":\"phone required\"}".into())
        }
        ("GET", "/api/v1/keys") => {
            let f = fake.lock().unwrap();
            let rows: Vec<String> = f
                .keys
                .iter()
                .rev()
                .map(|(id, name, scopes, rev)| {
                    format!(
                        "{{\"id\":\"{id}\",\"name\":\"{name}\",\"prefix\":\"gal_sk_{id}\",\"scopes\":[{}],\"created_at\":\"2026-10-21T03:40:00.000Z\",\"revoked_at\":{}}}", // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
                        scopes.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(","),
                        if *rev { "\"2026-10-21T03:41:00.000Z\"" } else { "null" } // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
                    )
                })
                .collect();
            (
                200,
                format!("{{\"ok\":true,\"keys\":[{}]}}", rows.join(",")),
            )
        }
        ("POST", "/api/v1/keys") if body.contains("\"action\":\"revoke\"") => {
            let id = field(&body, "id").unwrap_or("").to_string();
            let mut f = fake.lock().unwrap();
            let hit = f.keys.iter_mut().find(|k| k.0 == id && !k.3);
            let ok = hit.map(|k| k.3 = true).is_some();
            (200, format!("{{\"ok\":{ok}}}"))
        }
        ("POST", "/api/v1/keys") => {
            let mut f = fake.lock().unwrap();
            let id = format!("{:08x}", 0xa0 + f.keys.len());
            let name = field(&body, "name").unwrap_or("key").to_string();
            let scopes: Vec<String> = ["chat", "tools", "files", "admin"]
                .iter()
                .filter(|s| body.contains(&format!("\"{s}\"")))
                .map(|s| s.to_string())
                .collect();
            f.keys.push((id.clone(), name.clone(), scopes, false));
            (200, format!("{{\"id\":\"{id}\",\"name\":\"{name}\",\"prefix\":\"gal_sk_{id}\",\"scopes\":[\"chat\"],\"key\":\"gal_sk_SECRET{id}xyz\"}}"))
        }
        ("GET", "/api/v1/markers") => {
            let f = fake.lock().unwrap();
            (
                200,
                format!(
                    "{{\"count\":{},\"cap\":42,\"items\":[{}]}}",
                    f.markers.len(),
                    f.markers.join(",")
                ),
            )
        }
        ("POST", "/api/v1/markers") => {
            let mut f = fake.lock().unwrap();
            f.seq += 1;
            let seq = 41 + f.seq;
            let m = format!("{{\"seq\":{seq},\"t\":1792552685000,\"zone\":\"active\",\"kind\":\"event\",\"source\":\"desk\",\"note\":\"manual ping\"}}");
            f.markers.push(m.clone());
            (201, m)
        }
        _ => (404, "nope".into()),
    };
    let resp = format!(
        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
        out.len()
    );
    let _ = s.write_all(resp.as_bytes());
}

fn start_with(fake: Shared, upstream: Option<SocketAddr>) -> SocketAddr {
    let api = upstream.unwrap_or_else(|| fake_api(Arc::clone(&fake)));
    let args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--upstream".into(),
        api.to_string(),
        "--quiet".into(),
    ];
    let cfg = Config::from_args(args).unwrap();
    let routes = Routes::parse("page /\n").unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let state = Arc::new(State::new(Site::default(), routes, cfg));
    thread::spawn(move || server::serve(l, state));
    addr
}

fn start() -> (SocketAddr, Shared) {
    let fake: Shared = Arc::new(Mutex::new(Fake::default()));
    (start_with(Arc::clone(&fake), None), fake)
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

fn send(addr: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
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
        head: head.to_string(),
        body: body.to_string(),
    }
}

fn get(addr: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn post(addr: SocketAddr, path: &str, host: &str, extra: &str, body: &str) -> Resp {
    send(
        addr,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

const SAME: &str = "Sec-Fetch-Site: same-origin\r\nCookie: galion_admin=good; other=1\r\n";

fn posts(f: &Shared) -> Vec<String> {
    f.lock()
        .unwrap()
        .log
        .iter()
        .filter(|l| l.starts_with("POST"))
        .cloned()
        .collect()
}

#[test]
fn plain_views_are_get_only_and_scriptless() {
    let (a, f) = start();
    for p in ["/instances", "/keys", "/markers"] {
        let r = get(a, p, "127.0.0.1", "");
        assert_eq!(r.code, 200, "{p}: {}", r.head);
        assert!(
            r.head
                .contains("Content-Security-Policy: default-src 'none'"),
            "{p}"
        );
        assert!(r.head.contains("Cache-Control: no-store"), "{p}");
        assert!(!r.body.contains("<script"), "{p} has a script");
        assert!(
            r.body.contains("aria-current=\"page\"") || r.body.contains(p),
            "{p}"
        );
        let h = get(a, p, "127.0.0.1", "");
        assert_eq!(h.code, 200);
    }
    assert!(
        posts(&f).is_empty(),
        "a GET sent a POST upstream: {:?}",
        posts(&f)
    );
    for m in ["PUT", "DELETE", "PATCH"] {
        let r = send(a, &format!("{m} /keys HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
        assert_eq!(r.code, 405, "{m}");
    }
    assert!(posts(&f).is_empty());
}

#[test]
fn instances_room_content() {
    let (a, _) = start();
    let r = get(a, "/instances", "127.0.0.1", "");
    let b = &r.body;
    assert!(b.contains("Phone, pod, laptop"));
    assert_eq!(
        b.matches("<li class=\"p3-card\">").count(),
        3 + 1 + 3,
        "nodes + job + parts"
    );
    assert!(b.contains("RTX 4060 Laptop · 8 GB"));
    assert!(b.contains("Voice &lt;x&gt;"));
    assert!(b.contains("2 of 3 online"));
    assert!(b.contains("<span class=\"badge online\">linked</span>"));
    assert!(b.contains("value=\"50\""));
    assert!(b.contains("copying installer 1 / 2\nlive powershell -NoProfile"));
    assert!(b.contains("ssh-ed25519 AAAAtest galion-laptop"));
    assert!(!b.contains("<form"), "GET-only room has no form");
}

#[test]
fn instances_with_slow_laptop_and_dead_upstream() {
    let (a, f) = start();
    f.lock().unwrap().laptop_delay = Duration::from_secs(6);
    let t = Instant::now();
    let r = get(a, "/instances", "127.0.0.1", "");
    let took = t.elapsed();
    assert_eq!(r.code, 200);
    assert!(took < Duration::from_secs(6), "budget held: {took:?}");
    assert!(r
        .body
        .contains("The laptop status did not load: the console API did not answer."));
    assert!(r.body.contains("GALION-STUDIO"), "machines still shown");
    // Upstream gone: every room still renders and says so.
    let dead = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let a = start_with(Arc::new(Mutex::new(Fake::default())), Some(dead));
    for (p, what) in [
        ("/instances", "The machine list did not load"),
        ("/keys", "The key list did not load"),
        ("/markers", "The marker ring did not load"),
    ] {
        let r = get(a, p, "127.0.0.1", "");
        assert_eq!(r.code, 200, "{p}");
        assert!(r.body.contains(what), "{p}");
        assert!(
            r.body.contains("origin offline") || r.body.contains("offline"),
            "{p}"
        );
    }
}

#[test]
fn keys_mint_once_revoke_and_list() {
    let (a, f) = start();
    // Unpaired (no cookie) on loopback: the page says why the list is empty.
    let r = get(a, "/keys", "127.0.0.1", "");
    assert!(
        r.body
            .contains("The key list did not load: HTTP 401 (phone required)."),
        "{}",
        r.body
    );
    assert!(!r.body.contains("No keys."));
    // Paired: empty list.
    let r = get(a, "/keys", "127.0.0.1", "Cookie: galion_admin=good\r\n");
    assert!(r.body.contains("No keys."));
    // Mint with junk scopes, a control character and a long name.
    let long = "n".repeat(80);
    let r = post(
        a,
        "/keys",
        "127.0.0.1",
        SAME,
        &format!(
            "op=mint&name=+ph%0Done+{long}&scope=admin&scope=chat&scope=root&scope=chat&extra=1"
        ),
    );
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.body.contains("Copy it now. It will not be shown again."));
    assert!(r.body.contains("gal_sk_SECRET000000a0xyz"));
    let p = posts(&f);
    assert_eq!(p.len(), 1, "{p:?}");
    let want_name: String = format!("phone {long}").chars().take(64).collect();
    assert_eq!(
        p[0],
        format!("POST /api/v1/keys galion_admin=good {{\"name\":\"{want_name}\",\"scopes\":[\"chat\",\"admin\"]}}"),
        "only the admin cookie forwarded, clean body"
    );
    // The key is shown once: a reload (GET) does not have it.
    let r = get(a, "/keys", "127.0.0.1", "Cookie: galion_admin=good\r\n");
    assert!(!r.body.contains("SECRET"));
    assert!(r
        .body
        .contains("gal_sk_000000a0… · chat admin · 2026-10-21 05:40 CEST"));
    assert!(r.body.contains("name=\"id\" value=\"000000a0\""));
    // Revoke it.
    let r = post(a, "/keys", "127.0.0.1", SAME, "op=revoke&id=000000a0");
    assert_eq!(r.code, 200);
    assert!(r.body.contains(">revoked</p>"));
    assert!(r.body.contains("· revoked"));
    assert!(
        !r.body.contains("value=\"revoke\""),
        "no button on a revoked key"
    );
    assert_eq!(
        posts(&f)[1],
        "POST /api/v1/keys galion_admin=good {\"action\":\"revoke\",\"id\":\"000000a0\"}"
    );
    // Revoking again: upstream ok:false.
    let r = post(a, "/keys", "127.0.0.1", SAME, "op=revoke&id=000000a0");
    assert!(r.body.contains("Nothing revoked"));
    assert_eq!(posts(&f).len(), 3);
}

#[test]
fn keys_refusals_send_nothing() {
    let (a, f) = start();
    let cookie = "Cookie: galion_admin=good\r\n";
    // CSRF: no proof, cross-site, foreign Origin.
    for extra in [
        cookie.to_string(),
        format!("Sec-Fetch-Site: cross-site\r\n{cookie}"),
        format!("Origin: https://evil.example\r\n{cookie}"),
    ] {
        let r = post(
            a,
            "/keys",
            "127.0.0.1",
            &extra,
            "op=mint&name=x&scope=admin",
        );
        assert_eq!(r.code, 403, "{extra}");
    }
    // Same-origin by Origin header is accepted.
    let r = post(
        a,
        "/keys",
        "127.0.0.1",
        &format!("Origin: http://127.0.0.1\r\n{cookie}"),
        "op=revoke&id=ffffffff",
    );
    assert_eq!(r.code, 200);
    assert_eq!(posts(&f).len(), 1);
    // Bad ids and unknown actions (incl. the API's own "check").
    for body in [
        "op=revoke&id=",
        "op=revoke&id=..%2Fx",
        "op=revoke&id=abc%22%3E",
        "op=check&key=gal_sk_x",
        "op=delete&id=aa",
        "",
    ] {
        let r = post(a, "/keys", "127.0.0.1", SAME, body);
        assert_eq!(r.code, 400, "{body}");
        assert!(r.body.contains("Nothing sent."), "{body}");
    }
    assert_eq!(posts(&f).len(), 1, "{:?}", posts(&f));
    // Upstream refuses an unpaired mint.
    let r = post(
        a,
        "/keys",
        "127.0.0.1",
        "Sec-Fetch-Site: same-origin\r\n",
        "op=mint&name=x",
    );
    assert!(
        r.body.contains("Mint refused: HTTP 401 (phone required)."),
        "{}",
        r.body
    );
    assert!(!r.body.contains("Copy it now"));
}

#[test]
fn markers_ping_fixed_body() {
    let (a, f) = start();
    let r = get(a, "/markers", "127.0.0.1", "");
    assert!(r.body.contains("Ring of 42 · 0 stored"));
    assert!(r.body.contains("No stored events yet."));
    for extra in [
        "",
        "Sec-Fetch-Site: cross-site\r\n",
        "Origin: https://evil.example\r\n",
    ] {
        assert_eq!(post(a, "/markers", "127.0.0.1", extra, "op=ping").code, 403);
    }
    let r = post(
        a,
        "/markers",
        "127.0.0.1",
        "Sec-Fetch-Site: same-origin\r\n",
        "op=other",
    );
    assert_eq!(r.code, 400);
    assert!(posts(&f).is_empty());
    // Extra fields never reach the upstream body.
    let r = post(
        a,
        "/markers",
        "127.0.0.1",
        "Sec-Fetch-Site: same-origin\r\n",
        "op=ping&note=evil&zone=chill&kind=dns",
    );
    assert_eq!(r.code, 200);
    assert!(r.body.contains("Ping stored as #42."));
    let r = post(
        a,
        "/markers",
        "127.0.0.1",
        "Sec-Fetch-Site: same-origin\r\n",
        "op=ping",
    );
    assert!(r.body.contains("Ping stored as #43."));
    assert!(r.body.contains("Ring of 42 · 2 stored"));
    assert!(
        r.body.find(">43<").unwrap() < r.body.find(">42<").unwrap(),
        "newest first"
    );
    assert!(r.body.contains("05:18:05 CEST"));
    for p in posts(&f) {
        assert!(
            p.ends_with(" {\"zone\":\"active\",\"kind\":\"event\",\"source\":\"desk\",\"note\":\"manual ping\"}"),
            "{p}"
        );
        assert!(!p.contains("evil"));
    }
}

#[test]
fn gate_blocks_unpaired_public_host() {
    let (a, f) = start();
    for p in ["/instances", "/keys", "/markers"] {
        let r = get(a, p, "console.galion.app", "");
        assert_eq!(r.code, 200, "{p}");
        assert!(r.body.contains("action=\"/_ui/pair"), "{p}: pairing page");
        assert!(!r.body.contains("GALION-STUDIO"));
        let r = get(a, p, "galion.app", "");
        assert!(
            !r.body.contains("p3-"),
            "{p}: Soon page on a non-console host"
        );
    }
    // Unpaired POSTs do nothing upstream even with same-origin proof.
    let r = post(
        a,
        "/keys",
        "console.galion.app",
        "Sec-Fetch-Site: same-origin\r\n",
        "op=mint&name=x",
    );
    assert!(r.body.contains("action=\"/_ui/pair"));
    let r = post(
        a,
        "/markers",
        "console.galion.app",
        "Sec-Fetch-Site: same-origin\r\n",
        "op=ping",
    );
    assert!(r.body.contains("action=\"/_ui/pair"));
    assert!(posts(&f).is_empty(), "{:?}", posts(&f));
    // Paired public host works.
    let r = get(
        a,
        "/keys",
        "console.galion.app",
        "Cookie: galion_admin=good\r\n",
    );
    assert!(r.body.contains("No keys."));
}
