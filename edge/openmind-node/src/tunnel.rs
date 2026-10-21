// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Outbound tunnel to Hermes after enrollment (docs/omnode-enroll.md).
//!
//! The device dials out and keeps one HTTPS response open
//! (`GET /openmind/node/tunnel`, Bearer credential, server-sent events).
//! Nothing here listens: Hermes reaches the device only through frames on
//! this connection, and the device answers with ordinary outbound POSTs.
//!
//! Frames from Hermes: `hello` (session + challenge), `ready`, `challenge`,
//! `ping`, `bye`. Every challenge is answered with an Ed25519 signature over
//! `omnode-tunnel-v1|device_id|session|id|challenge|enroll_url`, so Hermes
//! knows each answer comes from the enrolled key (the bearer token alone is
//! not enough). The private key never leaves the device.

use crate::enroll::{self, Net};
use crate::identity::Identity;
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, Instant};

/// Why [`run`] returned.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// `--secs` elapsed (tests, or a supervised one-shot run).
    Deadline,
    /// Hermes refused the credential (revoked / expired): pair again.
    Refused(String),
}

pub struct Opts {
    pub net: Net,
    /// Stop after this long (None = until killed).
    pub secs: Option<u64>,
    /// Print every ping, not only state changes.
    pub verbose: bool,
}

fn say(msg: &str) {
    println!("{} tunnel: {msg}", crate::zagreb::now_rfc3339());
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

fn read_cred(dir: &Path) -> Result<(String, String), String> {
    let p = enroll::credential_file(dir);
    let t = std::fs::read_to_string(&p).map_err(|e| {
        format!(
            "no credential at {} ({e}); run `omnode pair URL` first",
            p.display()
        )
    })?;
    let v: Value = serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))?;
    let url = v["url"]
        .as_str()
        .ok_or("credential has no url")?
        .to_string();
    let token = v["token"]
        .as_str()
        .ok_or("credential has no token")?
        .to_string();
    Ok((url, token))
}

enum Session {
    /// Hermes said bye (reason); reconnect.
    Bye(String),
    /// The connection dropped or went idle; reconnect with backoff.
    Lost(String),
    Deadline,
    Refused(String),
}

/// Keep a tunnel up: reconnect with backoff (1 s .. 60 s) until the deadline
/// or until Hermes refuses the credential.
pub fn run(dir: &Path, id: &Identity, o: &Opts) -> Result<End, String> {
    let (url, token) = read_cred(dir)?;
    let started = Instant::now();
    let deadline = o.secs.map(|s| started + Duration::from_secs(s));
    let mut backoff = 1u64;
    loop {
        let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if left.is_some_and(|l| l.is_zero()) {
            return Ok(End::Deadline);
        }
        match session(&url, &token, id, o, left) {
            Session::Deadline => return Ok(End::Deadline),
            Session::Refused(r) => {
                say(&format!("Hermes refused this device: {r}"));
                return Ok(End::Refused(r));
            }
            Session::Bye(r) => {
                say(&format!("Hermes closed the tunnel: {r}; reconnecting"));
                backoff = 1;
            }
            Session::Lost(r) => {
                say(&format!("connection lost: {r}; retry in {backoff} s"));
                std::thread::sleep(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(60);
            }
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Ok(End::Deadline);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn answer(
    url: &str,
    token: &str,
    id: &Identity,
    session: &str,
    cid: &str,
    challenge: &str,
    net: Net,
) -> Result<(), String> {
    if [session, cid, challenge]
        .iter()
        .any(|s| s.is_empty() || s.contains('|') || s.len() > 128)
    {
        return Err("malformed challenge from Hermes".into());
    }
    let sig = id.sign(
        enroll::tunnel_canonical(&id.state.device_id, session, cid, challenge, url).as_bytes(),
    );
    let body = json!({"session": session, "id": cid, "signature": sig});
    let (code, text) = enroll::http(
        "POST",
        &enroll::node_url(url, "tunnel/answer")?,
        Some(&body.to_string()),
        Some(token),
        net,
    )?;
    if code == 200 {
        Ok(())
    } else {
        Err(format!(
            "answer refused (HTTP {code}): {}",
            text.chars().take(200).collect::<String>()
        ))
    }
}

fn session(url: &str, token: &str, id: &Identity, o: &Opts, left: Option<Duration>) -> Session {
    let turl = match enroll::node_url(url, "tunnel") {
        Ok(u) => u,
        Err(e) => return Session::Refused(e),
    };
    // Idle limit: Hermes pings every ping_secs (default 15 s); 75 s without a
    // byte means the path is dead (also covers a silent NAT drop).
    let idle = Duration::from_secs(75);
    let (code, mut st) = match enroll::open_stream(&turl, token, o.net, idle, left) {
        Ok(x) => x,
        Err(e) => return Session::Lost(e),
    };
    match code {
        200 => {}
        401 | 403 => return Session::Refused(format!("HTTP {code}")),
        c => return Session::Lost(format!("HTTP {c}")),
    }
    let t0 = Instant::now();
    let mut ev = String::new();
    let mut data = String::new();
    let mut sess = String::new();
    loop {
        if left.is_some_and(|l| t0.elapsed() >= l) {
            return Session::Deadline;
        }
        let line = match st.line() {
            Ok(Some(l)) => l,
            Ok(None) => {
                return if left.is_some_and(|l| t0.elapsed() + Duration::from_millis(500) >= l) {
                    Session::Deadline
                } else {
                    Session::Lost("stream ended".into())
                }
            }
            Err(e) => {
                return if left.is_some_and(|l| t0.elapsed() + Duration::from_millis(500) >= l) {
                    Session::Deadline
                } else {
                    Session::Lost(format!("read: {e}"))
                }
            }
        };
        if let Some(x) = line.strip_prefix("event: ") {
            ev = x.to_string();
            continue;
        }
        if let Some(x) = line.strip_prefix("data: ") {
            data.push_str(x);
            continue;
        }
        if !line.is_empty() {
            continue; // comments / unknown fields are ignored (SSE rules)
        }
        if ev.is_empty() && data.is_empty() {
            continue;
        }
        let d: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
        let name = std::mem::take(&mut ev);
        data.clear();
        match name.as_str() {
            "hello" => {
                sess = d["session"].as_str().unwrap_or("").to_string();
                let ch = d["challenge"].as_str().unwrap_or("");
                if d["device_id"].as_str() != Some(id.state.device_id.as_str()) {
                    return Session::Refused("hello names another device".into());
                }
                say(&format!(
                    "connected to {turl} (session {sess}, ping every {} s); proving the device key",
                    d["ping_secs"]
                ));
                if let Err(e) = answer(url, token, id, &sess, "hello", ch, o.net) {
                    return Session::Lost(e);
                }
            }
            "ready" => say(&format!(
                "UP: Hermes verified the device key (session {sess}); outbound only, nothing listens on this device"
            )),
            "challenge" => {
                let cid = d["id"].as_str().unwrap_or("");
                let ch = d["challenge"].as_str().unwrap_or("");
                match answer(url, token, id, &sess, cid, ch, o.net) {
                    Ok(()) => say(&format!("answered challenge {cid} from Hermes (signed)")),
                    Err(e) => say(&format!("could not answer challenge {cid}: {e}")),
                }
            }
            "ping" => {
                if o.verbose {
                    say(&format!("ping {} at {}", d["n"], d["t"].as_str().unwrap_or("?")));
                }
            }
            "bye" => {
                let r = d["reason"].as_str().unwrap_or("no reason").to_string();
                if r.contains("revoked") {
                    return Session::Refused(r);
                }
                return Session::Bye(r);
            }
            _ => {}
        }
    }
}
