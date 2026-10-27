// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
// Source: base.git ws/foundation e43d7ab crates/galion-health/src/main.rs (landed onto main 13a296c by swarm/health-land).
//! galion-health: std-only loopback status page.
//! Env: GALION_HEALTH_ADDR (default 127.0.0.1:8660), GALION_HEALTH_CHECKS (default list below),
//! GALION_LOG_FORMAT=json|kv (default kv). Never reads secrets; probes only GET <path> on loopback.
//! Check spec: name|unit|port|path  (unit "-" = no systemd unit, port 0 = no probe), ';'-separated.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_CHECKS: &str = "openmind-memory|openmind-memory.service|8420|/health;\
hermes-rs|hermes-rs.service|8642|/health;\
link-watch-rs|link-watch-rs.service|0|-;\
galion-desk|galion-desk.service|8646|/health;\
tuner|tuner.service|8650|/health;\
tuner-quick|tuner-quick.service|0|-;\
one|-|8091|/health;\
console|console.service|8080|/;\
cloudflared|cloudflared.service|20241|/ready;\
journald|systemd-journald.service|0|-;\
ssh|ssh.service|22|-;\
galion-backup.timer|galion-backup.timer|0|-";

struct Check {
    name: String,
    unit: String,
    port: u16,
    path: String,
}
#[derive(Clone)]
struct Res {
    name: String,
    unit: String,
    unit_state: String,
    port: u16,
    http: String,
    ms: u128,
    ok: bool,
}

/// Probe cache: (last run, results, checked_at stamp, failing names).
type Cache = (Option<Instant>, Vec<Res>, String, Vec<String>);

fn log(level: &str, msg: &str) {
    let ts = zagreb_time::now_rfc3339();
    if std::env::var("GALION_LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false)
    {
        eprintln!(
            "{{\"ts\":\"{}\",\"level\":\"{}\",\"svc\":\"galion-health\",\"msg\":\"{}\"}}",
            ts,
            level,
            esc(msg)
        );
    } else {
        eprintln!(
            "ts={} level={} svc=galion-health msg=\"{}\"",
            ts,
            level,
            msg.replace('"', "'")
        );
    }
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '<' => o.push_str("\\u003c"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}
fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse(spec: &str) -> Vec<Check> {
    spec.split(';')
        .filter_map(|e| {
            let p: Vec<&str> = e.trim().split('|').collect();
            if p.len() != 4 {
                return None;
            }
            Some(Check {
                name: p[0].into(),
                unit: p[1].into(),
                port: p[2].parse().unwrap_or(0),
                path: p[3].into(),
            })
        })
        .collect()
}

fn unit_state(unit: &str) -> String {
    if unit == "-" {
        return "n/a".into();
    }
    Command::new("systemctl")
        .args(["is-active", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn probe(port: u16, path: &str) -> (String, u128) {
    let t = Instant::now();
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut s = match TcpStream::connect_timeout(&addr, Duration::from_millis(1500)) {
        Ok(s) => s,
        Err(_) => return ("down".into(), t.elapsed().as_millis()),
    };
    if path == "-" {
        return ("tcp-open".into(), t.elapsed().as_millis());
    }
    let _ = s.set_read_timeout(Some(Duration::from_millis(2500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(1500)));
    let req = format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nUser-Agent: galion-health\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return ("write-err".into(), t.elapsed().as_millis());
    }
    let mut buf = [0u8; 64];
    let n = s.read(&mut buf).unwrap_or(0);
    let line = String::from_utf8_lossy(&buf[..n]);
    let code = line
        .split_whitespace()
        .nth(1)
        .unwrap_or("no-reply")
        .to_string();
    (code, t.elapsed().as_millis())
}

fn run(checks: &[Check]) -> Vec<Res> {
    checks
        .iter()
        .map(|c| {
            let us = unit_state(&c.unit);
            let (http, ms) = if c.port == 0 {
                ("-".to_string(), 0)
            } else {
                probe(c.port, &c.path)
            };
            let unit_ok = c.unit == "-" || us == "active";
            let http_ok =
                c.port == 0 || http == "tcp-open" || http.starts_with('2') || http.starts_with('3');
            Res {
                name: c.name.clone(),
                unit: c.unit.clone(),
                unit_state: us,
                port: c.port,
                http,
                ms,
                ok: unit_ok && http_ok,
            }
        })
        .collect()
}

fn to_json(rs: &[Res], at: &str) -> String {
    let ok = rs.iter().all(|r| r.ok);
    let items: Vec<String> = rs.iter().map(|r| format!(
        "{{\"name\":\"{}\",\"unit\":\"{}\",\"unit_state\":\"{}\",\"port\":{},\"http\":\"{}\",\"ms\":{},\"ok\":{}}}",
        esc(&r.name), esc(&r.unit), esc(&r.unit_state), r.port, esc(&r.http), r.ms, r.ok)).collect();
    format!(
        "{{\"ok\":{},\"checked_at\":\"{}\",\"services\":[{}]}}",
        ok,
        at,
        items.join(",")
    )
}

fn to_html(rs: &[Res], at: &str) -> String {
    let mut rows = String::new();
    for r in rs {
        rows.push_str(&format!("<tr class=\"{}\"><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{} ms</td></tr>",
            if r.ok { "ok" } else { "bad" }, html(&r.name), html(&r.unit), html(&r.unit_state),
            if r.port == 0 { "-".into() } else { r.port.to_string() }, html(&r.http), r.ms));
    }
    let up = rs.iter().filter(|r| r.ok).count();
    format!("<!doctype html><html><head><meta charset=utf-8><meta http-equiv=refresh content=15><title>galion-health</title>\
<style>body{{font:14px system-ui;background:#111;color:#ddd;margin:2em}}td,th{{padding:4px 10px;text-align:left}}\
tr.ok td:first-child{{color:#4c4}}tr.bad td{{color:#f55}}</style></head><body><h2>Galion health: {up}/{} ok</h2>\
<p>checked {at} (Europe/Zagreb) &middot; <a href=/status.json style=color:#8af>json</a></p>\
<table><tr><th>service</th><th>unit</th><th>state</th><th>port</th><th>http</th><th>latency</th></tr>{rows}</table></body></html>", rs.len())
}

fn main() {
    let addr = std::env::var("GALION_HEALTH_ADDR").unwrap_or_else(|_| "127.0.0.1:8660".into());
    let checks =
        parse(&std::env::var("GALION_HEALTH_CHECKS").unwrap_or_else(|_| DEFAULT_CHECKS.into()));
    if std::env::args().nth(1).as_deref() == Some("--once") {
        let rs = run(&checks);
        println!("{}", to_json(&rs, &zagreb_time::now_rfc3339()));
        std::process::exit(if rs.iter().all(|r| r.ok) { 0 } else { 1 });
    }
    let listener = TcpListener::bind(&addr).unwrap_or_else(|e| {
        log("error", &format!("bind {addr}: {e}"));
        std::process::exit(1)
    });
    log(
        "info",
        &format!("listening on {addr} checks={}", checks.len()),
    );
    // NERVE push (no-op unless NERVE_URL is set): started + heartbeat.
    nerve_client::event("galion-health", "started", &format!("listening on {addr}"));
    nerve_client::start_heartbeat("galion-health", 60);
    // Cache results for 5 s so page reloads never fan out into probe storms.
    let cache: Arc<Mutex<Cache>> = Arc::new(Mutex::new((None, vec![], String::new(), vec![])));
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        let t0 = Instant::now();
        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
        let mut buf = [0u8; 1024];
        let n = s.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.0
            .map(|t| t.elapsed() > Duration::from_secs(5))
            .unwrap_or(true)
        {
            let rs = run(&checks);
            let bad: Vec<String> = rs
                .iter()
                .filter(|r| !r.ok)
                .map(|r| r.name.clone())
                .collect();
            if bad != c.3 {
                log(
                    if bad.is_empty() { "info" } else { "warn" },
                    &format!("failing=[{}]", bad.join(",")),
                );
            }
            *c = (Some(Instant::now()), rs, zagreb_time::now_rfc3339(), bad);
        }
        let all_ok = c.1.iter().all(|r| r.ok);
        let (status, ctype, body) = match path.as_str() {
            "/status.json" | "/json" => ("200 OK", "application/json", to_json(&c.1, &c.2)),
            "/healthz" => (
                if all_ok {
                    "200 OK"
                } else {
                    "503 Service Unavailable"
                },
                "text/plain",
                if all_ok {
                    "ok\n".into()
                } else {
                    format!("failing: {}\n", c.3.join(","))
                },
            ),
            "/" => ("200 OK", "text/html; charset=utf-8", to_html(&c.1, &c.2)),
            _ => ("404 Not Found", "text/plain", "not found\n".into()),
        };
        drop(c);
        let status_code: u16 = if status.starts_with("200") {
            200
        } else if status.starts_with("503") {
            503
        } else if status.starts_with("404") {
            404
        } else {
            500
        };
        nerve_client::observe_request(
            "galion-health",
            &format!("GET {path}"),
            status_code,
            t0.elapsed(),
        );
        let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn res(name: &str, ok: bool) -> Res {
        Res {
            name: name.into(),
            unit: "-".into(),
            unit_state: "n/a".into(),
            port: 0,
            http: "-".into(),
            ms: 0,
            ok,
        }
    }

    #[test]
    fn default_checks_parse_to_twelve() {
        let c = parse(DEFAULT_CHECKS);
        assert_eq!(c.len(), 12, "live 8660 logs checks=12");
        assert_eq!(c[0].name, "openmind-memory");
        assert_eq!(c[0].port, 8420);
        assert_eq!(c[10].name, "ssh");
        assert_eq!(c[10].path, "-");
        assert_eq!(c[11].unit, "galion-backup.timer");
    }

    #[test]
    fn parse_skips_malformed_and_bad_ports() {
        let c = parse("a|-|x|/;bad;b|u.service|80|/h; ");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].port, 0);
        assert_eq!(c[1].unit, "u.service");
    }

    #[test]
    fn esc_handles_quotes_controls_and_lt() {
        assert_eq!(esc("a\"b\\c<\n"), "a\\\"b\\\\c\\u003c\\u000a");
        assert_eq!(html("<a&b>"), "&lt;a&amp;b&gt;");
    }

    #[test]
    fn json_shape_matches_live_key_set() {
        let j = to_json(
            &[res("x", true), res("y<", false)],
            "2026-10-20T22:52:05+02:00",
        );
        assert!(j.starts_with("{\"ok\":false,\"checked_at\":\"2026-10-20T22:52:05+02:00\",\"services\":[{\"name\":\"x\",\"unit\":\"-\",\"unit_state\":\"n/a\",\"port\":0,\"http\":\"-\",\"ms\":0,\"ok\":true}"));
        assert!(j.contains("\"name\":\"y\\u003c\""));
    }

    #[test]
    fn html_counts_ok_rows() {
        let h = to_html(&[res("x", true), res("y", false)], "t");
        assert!(h.contains("Galion health: 1/2 ok"));
        assert!(h.contains("<tr class=\"bad\"><td>y</td>"));
    }

    #[test]
    fn port_zero_and_no_unit_is_ok() {
        let r = run(&parse("n|-|0|-"));
        assert!(r[0].ok);
        assert_eq!(r[0].unit_state, "n/a");
        assert_eq!(r[0].http, "-");
    }
}
