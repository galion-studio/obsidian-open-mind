// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--phone` (console-phone-home, 2026-10-31 CET +01:00) end to end over
//! loopback: fake hermes-rs, openmind-nerve and gaia-cortex (ephemeral
//! ports), a temp OpenMind status dir, temp systemd unit dir and a fake
//! signed self-change entry (galion-owner-key testkit). Covers the pairing
//! gate (401 before any data), the page, the status JSON (whitelisted spend,
//! no session names), dead probes, and a full passkey set-up + approve on
//! the console origin through the console's own routes.

use console_static::admin::sha_hex;
use console_static::config::Config;
use console_static::routes::Routes;
use console_static::server::{self, State};
use console_static::site::Site;
use galion_owner_key as ok;
use galion_owner_key::testkit::{fake_entry, SoftPasskey};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// Paired-device cookie token for the test admin store (not a real secret).
const TOKEN: &str = "paired-phone-token-for-tests";
const ORIGIN: &str = "https://console.galion.app";
const HOST: &str = "console.galion.app";
const ID: &str = "sm-20261031-phone";

fn tmp(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "phone-home-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn now() -> i64 {
    console_static::phone::now_secs()
}

/// A fake JSON service: path -> (status, body).
fn fake(routes: Vec<(&'static str, u16, String)>) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let routes = routes.clone();
            thread::spawn(move || {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                let line = String::from_utf8_lossy(&got)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let path = line.split(' ').nth(1).unwrap_or("").to_string();
                let (code, body) = routes
                    .iter()
                    .find(|(p, _, _)| *p == path)
                    .map(|(_, c, b)| (*c, b.clone()))
                    .unwrap_or((404, r#"{"ok":false,"error":"not found"}"#.into()));
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            });
        }
    });
    a
}

/// A loopback port with nothing listening.
fn dead() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    drop(l);
    a
}

struct Box {
    addr: SocketAddr,
    root: PathBuf,
}

struct Svc {
    hermes: SocketAddr,
    nerve: SocketAddr,
    cortex: SocketAddr,
}

fn live_fakes() -> Svc {
    let hermes = fake(vec![(
        "/health",
        200,
        r#"{"status":"ok","platform":"hermes","version":"0.4.0","implementation":"hermes-rs","implementation_version":"0.4.0"}"#.into(),
    )]);
    let nerve = fake(vec![
        ("/health", 200, r#"{"status":"ok","service":"openmind-nerve","system":"degrading","score":91,"cycle":112,"state_age_s":574,"state_fresh":true}"#.into()),
        ("/api/v1/state", 200, r#"{"components":[{"name":"hermes-rs","status":"degrading"},{"name":"console","status":"healthy"},{"name":"galion-one","status":"healthy"}]}"#.into()),
    ]);
    let cortex = fake(vec![
        ("/health", 200, r#"{"ok":true,"service":"gaia-cortex","mode":"live","commit":"4eaedeadd2fd8ed4d3121e152af1587a479f87c4","incidents_open":1,"escalations_open":2}"#.into()),
        ("/escalations", 200, r#"{"open":[{"id":"esc-1","incident":"unit:a.service","reason":"a.service failed twice; not on the restart list","handoff":"handed-to-hermes","lead_event":{"t":1793258469,"unit":"a.service","severity":"low"}},{"id":"esc-2","incident":"unit:b.service","reason":"b.service memory climbing","lead_event":{"t":1793278972,"unit":"b.service","severity":"high"}}],"total":2,"closed_total":0}"#.into()),
        ("/approvals", 200, r#"{"pending":[{"id":"ap-7","ts":1793279000,"unit":"b.service","action":"restart","rule":"mem-high","reason":"memory over 90%","expires":1793282600,"status":"pending"}],"action_mode":"ask"}"#.into()),
    ]);
    Svc {
        hermes,
        nerve,
        cortex,
    }
}

fn write_status(dir: &Path) {
    std::fs::write(
        dir.join("spend.json"),
        r#"{"snapshot_of":"spend","written":"2026-10-31T09:33:40+01:00","data":{"ledger_total_usd":8.5991,"fresh_usd":5.1501,"baseline_usd":3.449,"limit_usd":10.0,"stop_line_usd":9.5,"left_to_stop_line_usd":4.3499,"left_to_limit_usd":4.8499,"state":"ok","day":"2026-10-31","day_usd":0.0229,"day_calls":3,"sessions":41,"top_sessions":[{"session":"tg:private-chat-name","usd":1.2}],"ledger_updated":"2026-10-31T09:30:00+01:00","source":"ledger"}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("backup.json"),
        r#"{"snapshot_of":"backup","written":"2026-10-31T09:33:40+01:00","data":{"state":"ok","problems":[],"archives":12,"newest_age_h":2.2,"recent":[],"offsite_copies":12,"note":"x"}}"#,
    )
    .unwrap();
}

fn start(svc: &Svc, extra: &[&str]) -> Box {
    let root = tmp("box");
    let store = root.join("admin-device.json");
    std::fs::write(
        &store,
        format!(r#"{{"tokenHashes":["{}"]}}"#, sha_hex(TOKEN)),
    )
    .unwrap();
    let status = root.join("status");
    std::fs::create_dir_all(&status).unwrap();
    write_status(&status);
    let units = root.join("units");
    let dropd = units.join("hermes-rs.service.d");
    std::fs::create_dir_all(&dropd).unwrap();
    std::fs::write(
        units.join("hermes-rs.service"),
        "[Service]\nExecStart=/opt/galion/rust/bin/hermes-gateway\n",
    )
    .unwrap();
    std::fs::write(
        dropd.join("99i-self-modify.conf"),
        "[Service]\nExecStart=\nExecStart=/opt/galion/rust/bin/hermes-gateway-6da98fdc\n",
    )
    .unwrap();
    fake_entry(&root.join("queue"), &root.join("keys"), ID, now()).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let p = |x: PathBuf| x.display().to_string();
    let mut args: Vec<String> = vec![
        "--ui".into(),
        "rust".into(),
        "--local-api".into(),
        "--quiet".into(),
        "--bind".into(),
        addr.to_string(),
        "--admin-store".into(),
        p(store),
        "--phone".into(),
        "--hermes".into(),
        svc.hermes.to_string(),
        "--nerve".into(),
        svc.nerve.to_string(),
        "--cortex".into(),
        svc.cortex.to_string(),
        "--status-dir".into(),
        p(status),
        "--unit-dir".into(),
        p(units),
        "--approval-queue".into(),
        p(root.join("queue")),
        "--approval-state".into(),
        p(root.join("state")),
        "--owner-pub".into(),
        p(root.join("keys/owner.pub")),
        "--sandbox-pub".into(),
        p(root.join("keys/sandbox.pub")),
        "--stack-pub".into(),
        p(root.join("keys/stack.pub")),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    let cfg = Config::from_args(args).unwrap();
    let state = std::sync::Arc::new(State::new(
        Site::default(),
        Routes::parse("page /").unwrap(),
        cfg,
    ));
    thread::spawn(move || server::serve(l, state));
    Box { addr, root }
}

struct Resp {
    code: u16,
    head: String,
    body: String,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

fn send(a: SocketAddr, raw: &str) -> Resp {
    let mut s = TcpStream::connect(a).unwrap();
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

fn cookie() -> String {
    format!("Cookie: galion_admin={TOKEN}\r\n")
}

fn get(a: SocketAddr, path: &str, host: &str, extra: &str) -> Resp {
    send(
        a,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n"),
    )
}

fn post(a: SocketAddr, path: &str, body: &Value, origin: &str) -> Resp {
    let b = body.to_string();
    send(
        a,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {HOST}\r\n{}Origin: {origin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
            cookie(),
            b.len()
        ),
    )
}

#[test]
fn gate_comes_first() {
    let b = start(&live_fakes(), &[]);
    for p in [
        "/api/v1/phone/status",
        "/api/approvals",
        "/api/approvals/count",
        "/api/owner-key",
    ] {
        let r = get(b.addr, p, HOST, "");
        assert_eq!(r.code, 401, "{p} unpaired: {}", r.body);
        assert!(!r.body.contains("fresh_usd") && !r.body.contains(ID), "{p}");
        let r = get(b.addr, p, HOST, "Cookie: galion_admin=wrong\r\n");
        assert_eq!(r.code, 401, "{p} bad cookie");
        let r = get(b.addr, p, "evil.example", &cookie());
        assert_eq!(r.code, 401, "{p} foreign host");
    }
    let r = post(
        b.addr,
        "/api/approvals/challenge",
        &json!({"id": ID}),
        ORIGIN,
    );
    assert_ne!(r.code, 401, "paired POST passes the gate: {}", r.body);
    let r = send(
        b.addr,
        "POST /api/approvals/challenge HTTP/1.1\r\nHost: console.galion.app\r\nOrigin: https://console.galion.app\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
    );
    assert_eq!(r.code, 401, "unpaired POST: {}", r.body);
}

#[test]
fn page_is_the_approvals_screen_with_tiles() {
    let b = start(&live_fakes(), &[]);
    let r = get(b.addr, "/phone", HOST, "");
    assert_eq!(r.code, 200);
    assert!(
        r.head
            .contains("Content-Security-Policy: default-src 'none'"),
        "{}",
        r.head
    );
    assert!(r.head.contains("frame-ancestors 'none'"));
    for want in [
        "Today at a glance",
        "id=\"ph-spend-v\"",
        "id=\"ap-list\"",
        "Full console",
        "/api/v1/phone/status",
        "/api/approvals",
    ] {
        assert!(r.body.contains(want), "{want}");
    }
    assert!(!r.body.contains("5.1501"), "page holds no data");
    let r = get(b.addr, "/phone", "evil.example", "");
    assert_ne!(r.code, 200, "not served to foreign hosts");
}

#[test]
fn status_reads_every_source() {
    let b = start(&live_fakes(), &[]);
    let r = get(b.addr, "/api/v1/phone/status", HOST, &cookie());
    assert_eq!(r.code, 200, "{}", r.body);
    assert!(r.head.contains("Cache-Control: no-store"));
    let v = r.json();
    assert_eq!(v["hermes"]["up"], true, "{v}");
    assert_eq!(v["hermes"]["build"], "hermes-gateway-6da98fdc");
    assert_eq!(v["hermes"]["commit"], "6da98fdc");
    assert_eq!(v["hermes"]["nerve"], "degrading");
    let s = &v["spend"];
    assert_eq!(s["ok"], true, "{s}");
    assert_eq!(s["fresh_usd"], 5.1501);
    assert_eq!(s["limit_usd"], 10.0);
    assert_eq!(s["stop_line_usd"], 9.5);
    assert_eq!(s["baseline_usd"], 3.449);
    assert_eq!(s["level"], "ok");
    assert_eq!(s["stale"], false);
    let w = &v["watchdog"];
    assert_eq!(w["ok"], true, "{w}");
    assert_eq!(w["awaiting"], 3);
    assert_eq!(w["actions_pending"], 1);
    assert_eq!(w["items"][0]["kind"], "action");
    assert_eq!(w["items"][1]["id"], "esc-2", "newest escalation next");
    let h = v["health"].as_array().unwrap();
    let names: Vec<&str> = h.iter().map(|x| x["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["cortex", "nerve", "console"]);
    assert_eq!(h[0]["ok"], true);
    assert_eq!(h[1]["status"], "up");
    assert!(
        h[1]["detail"]
            .as_str()
            .unwrap()
            .starts_with("system degrading"),
        "{}",
        h[1]
    );
    assert_eq!(h[2]["detail"], "nerve sees: healthy");
    assert_eq!(v["backup"]["archives"], 12);
    assert!(
        v["approvals"]["ready"].is_u64(),
        "no owner key yet, so nothing is ready to sign: {v}"
    );
    for leak in [
        "private-chat-name",
        "top_sessions",
        "ledger_total",
        "sessions\"",
    ] {
        assert!(!r.body.contains(leak), "{leak} leaked: {}", r.body);
    }
    let r = send(
        b.addr,
        &format!("POST /api/v1/phone/status HTTP/1.1\r\nHost: {HOST}\r\n{}Content-Length: 0\r\nConnection: close\r\n\r\n", cookie()),
    );
    assert_eq!(r.code, 405);
}

#[test]
fn dead_sources_are_reported_not_fatal() {
    let svc = Svc {
        hermes: dead(),
        nerve: dead(),
        cortex: dead(),
    };
    let b = start(&svc, &[]);
    std::fs::remove_file(b.root.join("status/spend.json")).unwrap();
    let r = get(b.addr, "/api/v1/phone/status", HOST, &cookie());
    assert_eq!(r.code, 200, "{}", r.body);
    let v = r.json();
    assert_eq!(v["hermes"]["up"], false);
    assert_eq!(v["hermes"]["error"], "not listening");
    assert_eq!(
        v["hermes"]["build"], "hermes-gateway-6da98fdc",
        "build id still from the unit file"
    );
    assert_eq!(v["spend"]["ok"], false);
    assert_eq!(v["spend"]["error"], "no snapshot yet");
    assert_eq!(v["watchdog"]["ok"], false);
    assert_eq!(v["health"][0]["status"], "down");
    assert_eq!(v["health"][1]["status"], "down");
    assert_eq!(v["health"][2]["detail"], "nerve view unavailable");
    assert!(
        !r.body.contains("127.0.0.1"),
        "no addresses in errors: {}",
        r.body
    );
}

#[test]
fn passkey_setup_and_approve_on_the_console_origin() {
    let b = start(&live_fakes(), &[]);
    let mut pk = SoftPasskey::new(9, "galion.app", ORIGIN);
    // Wrong origin is refused before anything is staged.
    let r = post(
        b.addr,
        "/api/owner-key/begin",
        &json!({"scheme": "webauthn-es256"}),
        "https://evil.example",
    );
    assert_eq!(r.code, 403, "{}", r.body);
    let r = post(
        b.addr,
        "/api/owner-key/begin",
        &json!({"scheme": "webauthn-es256"}),
        ORIGIN,
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let begin = r.json();
    let chal = ok::unb64url(begin["webauthn_challenge"].as_str().unwrap()).unwrap();
    let cd = pk.client_data("webauthn.create", &chal);
    let r = post(
        b.addr,
        "/api/owner-key/register",
        &json!({"nonce": begin["nonce"], "public_key_spki_hex": ok::hex(&pk.spki()),
            "credential_id": pk.credential_id, "client_data_json": ok::b64url(&cd)}),
        ORIGIN,
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let r = post(
        b.addr,
        "/api/owner-key/activate-challenge",
        &json!({}),
        ORIGIN,
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let ac = r.json();
    let a = &ac["activate"];
    let sig = pk.assert(
        a["domain"].as_str().unwrap(),
        a["body"].as_str().unwrap(),
        "",
    );
    let r = post(
        b.addr,
        "/api/owner-key/activate",
        &json!({"nonce": ac["nonce"], "confirmed": true, "activate": sig}),
        ORIGIN,
    );
    assert_eq!(r.code, 200, "{}", r.body);
    // Stand-in for the root helper.
    let req: Value = serde_json::from_slice(
        &std::fs::read(b.root.join("state/owner-key/install-request.json")).unwrap(),
    )
    .unwrap();
    ok::install(
        &req,
        &b.root.join("keys/owner.pub"),
        now(),
        &ok::InstallOpts {
            chown_root: false,
            dry_run: false,
        },
    )
    .unwrap();

    let r = get(b.addr, "/api/approvals", HOST, &cookie());
    assert_eq!(r.code, 200);
    let card = r.json()["items"][0].clone();
    assert_eq!(card["state"], "ready", "{card}");
    let r = post(
        b.addr,
        "/api/approvals/challenge",
        &json!({"id": ID}),
        ORIGIN,
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let ch = r.json();
    let assertion = pk.assert(
        ch["domain"].as_str().unwrap(),
        ch["body"].as_str().unwrap(),
        "",
    );
    let r = post(
        b.addr,
        "/api/approvals/approve",
        &json!({"nonce": ch["nonce"], "assertion": assertion}),
        ORIGIN,
    );
    assert_eq!(r.code, 200, "{}", r.body);
    let r = get(b.addr, "/api/v1/phone/status", HOST, &cookie());
    assert_eq!(
        r.json()["approvals"]["ready"],
        0,
        "approved card no longer waiting"
    );
    assert!(
        b.root.join(format!("queue/{ID}/owner")).is_dir(),
        "approval written next to the entry"
    );
}

#[test]
fn flags() {
    let base = ["--ui", "rust", "--quiet"];
    let mk = |extra: &[&str]| Config::from_args(base.iter().chain(extra).map(|s| s.to_string()));
    assert!(mk(&["--upstream", "127.0.0.1:9", "--phone"])
        .unwrap_err()
        .to_string()
        .contains("--phone needs --local-api"));
    assert!(mk(&["--local-api", "--nerve", "127.0.0.1:1"])
        .unwrap_err()
        .to_string()
        .contains("--nerve needs --phone"));
    assert!(mk(&["--local-api", "--phone", "--cortex", "10.0.0.9:1"]).is_err());
    assert!(mk(&[
        "--local-api",
        "--phone",
        "--webauthn-origin",
        "http://evil.example"
    ])
    .is_err());
    assert!(mk(&["--local-api", "--phone", "--status-dir", "rel/dir"]).is_err());
    let c = mk(&["--local-api", "--phone", "--hermes", "127.0.0.1:9"]).unwrap();
    let p = c.phone.unwrap();
    assert_eq!(p.hermes.port(), 9);
    assert_eq!(p.origins[0], ORIGIN);
    assert_eq!(p.rp_id, "galion.app");
    assert!(mk(&["--local-api"]).unwrap().phone.is_none());
}
