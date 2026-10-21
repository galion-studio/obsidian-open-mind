// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `hermes-gateway enroll ...`: the owner's approval commands for device
//! enrollment (docs/omnode-enroll.md). They talk to the RUNNING gateway on
//! loopback with the gateway key from `$API_SERVER_KEY` (never printed, never
//! on argv), so the one process that owns the store stays its only writer.
//!
//! ```text
//! hermes-gateway enroll pending                 # codes + fingerprints waiting
//! hermes-gateway enroll approve K7QX-M2PA [--scopes cpu,sandbox]
//! hermes-gateway enroll reject  K7QX-M2PA
//! hermes-gateway enroll devices                 # enrolled devices + tunnel state
//! hermes-gateway enroll ping    omn-0123456789ab  # signed round trip over the tunnel
//! hermes-gateway enroll revoke  omn-0123456789ab
//! ```

use hermes_rs::config::Config;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub const USAGE: &str = "usage: hermes-gateway enroll pending | devices | approve CODE [--scopes a,b] | reject CODE | ping DEVICE_ID | revoke DEVICE_ID";

/// Loopback address of the running gateway (a wildcard bind is reached on 127.0.0.1).
fn target(cfg: &Config) -> String {
    let host = match cfg.host.as_str() {
        "0.0.0.0" | "" => "127.0.0.1",
        "::" => "::1",
        h => h,
    };
    if host.contains(':') {
        format!("[{host}]:{}", cfg.port)
    } else {
        format!("{host}:{}", cfg.port)
    }
}

fn call(
    cfg: &Config,
    key: &str,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<(u16, Value), String> {
    let addr = target(cfg);
    let mut s = TcpStream::connect(&addr)
        .map_err(|e| format!("connect {addr}: {e} (is hermes-gateway running?)"))?;
    // ping waits up to 10 s for the node.
    s.set_read_timeout(Some(Duration::from_secs(20))).ok();
    s.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let b = body.map(|v| v.to_string()).unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAuthorization: Bearer {key}\r\nAccept: application/json\r\n"
    );
    if body.is_some() {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())
        .and_then(|_| s.write_all(b.as_bytes()))
        .map_err(|e| format!("send: {e}"))?;
    let mut raw = Vec::new();
    s.take(1 << 20)
        .read_to_end(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let text = String::from_utf8_lossy(&raw);
    let (h, body) = text
        .split_once("\r\n\r\n")
        .ok_or("malformed HTTP response")?;
    let code = h
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("malformed status line")?;
    let v = serde_json::from_str(body)
        .unwrap_or_else(|_| json!({"raw": body.chars().take(300).collect::<String>()}));
    Ok((code, v))
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Null => "-".into(),
        o => o.to_string(),
    }
}

/// Human table for `pending`.
fn print_pending(v: &Value) {
    let list = v["pending"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("no device is waiting for approval");
    }
    for p in &list {
        println!(
            "CODE {}  {}  {}  {} {} cores {} MB  caps {}  waiting {} s  (times out {})",
            s(&p["code"]),
            s(&p["device_id"]),
            s(&p["fingerprint"]),
            s(&p["claimed_platform"]),
            s(&p["cores"]),
            s(&p["ram_mb"]),
            p["capabilities"]
                .as_array()
                .map(|a| a.iter().map(s).collect::<Vec<_>>().join(","))
                .unwrap_or_default(),
            s(&p["waiting_secs"]),
            s(&p["expires"]),
        );
    }
    for o in v["recent"].as_array().cloned().unwrap_or_default() {
        println!(
            "recent: {} {} at {}",
            s(&o["device_id"]),
            s(&o["status"]),
            s(&o["at"])
        );
    }
    if !list.is_empty() {
        println!("compare CODE and fingerprint with the device screen, then: hermes-gateway enroll approve CODE (or reject CODE)");
    }
}

/// Run one owner command; returns the process exit code.
pub fn run(cfg: &Config, key: Option<String>, args: &[String]) -> i32 {
    let Some(key) = key.filter(|k| k.len() >= 16) else {
        eprintln!(
            "hermes-gateway enroll: the gateway key (${}) is not set; owner commands need it",
            cfg.api_key_env
        );
        return 2;
    };
    let arg = |i: usize| args.get(i).map(String::as_str);
    let r = match (arg(0), arg(1)) {
        (Some("pending"), None) => call(cfg, &key, "GET", "/openmind/enroll/pending", None),
        (Some("devices"), None) => call(cfg, &key, "GET", "/openmind/enroll/devices", None),
        (Some("approve"), Some(code)) => {
            let mut b = json!({"code": code});
            match (arg(2), arg(3)) {
                (None, None) => {}
                (Some("--scopes"), Some(list)) if args.len() == 4 => {
                    b["scopes"] = json!(list
                        .split(',')
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>());
                }
                _ => {
                    eprintln!("{USAGE}");
                    return 2;
                }
            }
            call(cfg, &key, "POST", "/openmind/enroll/approve", Some(&b))
        }
        (Some("reject"), Some(code)) if args.len() == 2 => call(
            cfg,
            &key,
            "POST",
            "/openmind/enroll/reject",
            Some(&json!({"code": code})),
        ),
        (Some(cmd @ ("ping" | "revoke")), Some(dev)) if args.len() == 2 => call(
            cfg,
            &key,
            "POST",
            &format!("/openmind/enroll/{cmd}"),
            Some(&json!({"device_id": dev})),
        ),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match r {
        Ok((code, v)) => {
            if arg(0) == Some("pending") && code == 200 {
                print_pending(&v);
            } else {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            }
            if (200..300).contains(&code) {
                0
            } else {
                eprintln!("hermes-gateway enroll: HTTP {code}");
                1
            }
        }
        Err(e) => {
            eprintln!("hermes-gateway enroll: {e}");
            1
        }
    }
}
