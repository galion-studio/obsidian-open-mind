// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The cortex's link to the nerve: subscribe to its event stream, ask it for
//! actions, hand it alerts.
//!
//! - `GET <nerve>/events/stream?since=SEQ` (event-mode nerve): one held-open
//!   NDJSON connection; the subscriber thread blocks in `read()` (zero idle
//!   work) and resumes from the last `seq` after a reconnect.
//! - `POST <nerve>/actions`: [`ActionRequest`]; the nerve's allowlist, caps
//!   and dry-run switches decide; its reply is read with
//!   [`ActionResult::from_nerve`] whatever the HTTP status (200/403/429/500).
//! - `POST <nerve>/event`: cortex alerts.
//!
//! Loopback http only. With no nerve configured, or a nerve without
//! `/actions` (404/405), a dry-run request is simulated locally and a live
//! one is refused: the cortex never runs a command.

use crate::config::nerve_cortex_addr;
use galion_common::http::{send, ClientRequest};
use nerve_cortex_contract::{
    parse_events, ActionRequest, ActionResult, ActionStatus, Event, NERVE_ACTIONS_PATH,
    NERVE_EVENTS_PATH, NERVE_EVENT_POST_PATH, NERVE_STREAM_PATH,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Where the action ended up and how.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub status: ActionStatus,
    /// `nerve` = the nerve answered; `local` = simulated/refused by the cortex.
    pub by: &'static str,
    pub reason: String,
}

pub trait NerveLink: Send {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome;
    /// Best effort; returns false when the nerve did not take it.
    fn post_event(&mut self, ev: &Event) -> bool;
    /// Pull adapter (polling nerve): the newest events (`GET /events?n=N`).
    fn pull_events(&mut self, n: usize) -> Result<Vec<Event>, String>;
}

fn local(req: &ActionRequest, why: &str) -> Outcome {
    if req.dry_run {
        Outcome {
            status: ActionStatus::Simulated,
            by: "local",
            reason: format!(
                "dry-run: would ask the nerve to restart {} ({why})",
                req.unit
            ),
        }
    } else {
        Outcome {
            status: ActionStatus::Refused,
            by: "local",
            reason: format!("live action needs the nerve /actions endpoint ({why}); the cortex never runs commands"),
        }
    }
}

/// No nerve configured.
pub struct NoNerve;

impl NerveLink for NoNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        local(req, "no nerve_url configured")
    }
    fn post_event(&mut self, _ev: &Event) -> bool {
        false
    }
    fn pull_events(&mut self, _n: usize) -> Result<Vec<Event>, String> {
        Ok(vec![])
    }
}

pub struct HttpNerve {
    pub addr: SocketAddr,
    pub host: String,
    pub timeout: Duration,
}

impl HttpNerve {
    pub fn new(url: &str) -> Option<Self> {
        let addr = nerve_cortex_addr(url)?;
        Some(HttpNerve {
            addr,
            host: addr.to_string(),
            timeout: Duration::from_millis(3000),
        })
    }

    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        let req = ClientRequest::post(path, &self.host, "application/json", body.as_bytes());
        send(self.addr, &req, self.timeout, 256 * 1024)
            .map(|r| (r.status, r.text()))
            .map_err(|e| format!("{e:?}"))
    }
}

/// Read a nerve `/actions` reply (any status) into an [`Outcome`].
pub fn read_action_reply(req: &ActionRequest, code: u16, body: &str) -> Outcome {
    if code == 404 || code == 405 {
        return local(req, "nerve has no /actions endpoint");
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    match ActionResult::from_nerve(&v) {
        Some(r) if r.request_id.is_empty() || r.request_id == req.request_id => {
            // A dry-run request must never come back executed.
            if req.dry_run && matches!(r.status, ActionStatus::Executed | ActionStatus::Failed) {
                return Outcome {
                    status: ActionStatus::Refused,
                    by: "nerve",
                    reason:
                        "nerve reported running a dry-run request: contract violation, escalated"
                            .into(),
                };
            }
            Outcome {
                status: r.status,
                by: "nerve",
                reason: r.reason,
            }
        }
        Some(_) => local(req, "nerve reply carries another request_id"),
        None if code == 400 || code == 403 => Outcome {
            status: ActionStatus::Refused,
            by: "nerve",
            reason: format!(
                "HTTP {code}: {}",
                body.chars().take(200).collect::<String>()
            ),
        },
        None => local(req, &format!("nerve answered HTTP {code} without a result")),
    }
}

impl NerveLink for HttpNerve {
    fn request_action(&mut self, req: &ActionRequest) -> Outcome {
        let body = serde_json::to_string(req).unwrap_or_default();
        match self.post(NERVE_ACTIONS_PATH, &body) {
            Ok((code, text)) => read_action_reply(req, code, &text),
            Err(e) => local(req, &format!("nerve unreachable: {e}")),
        }
    }

    fn post_event(&mut self, ev: &Event) -> bool {
        let body = serde_json::json!({"source": ev.source, "kind": ev.kind, "severity": ev.severity, "data": ev.data}).to_string();
        matches!(self.post(NERVE_EVENT_POST_PATH, &body), Ok((200..=299, _)))
    }

    fn pull_events(&mut self, n: usize) -> Result<Vec<Event>, String> {
        let path = format!("{NERVE_EVENTS_PATH}?n={n}");
        let req = ClientRequest::get(&path, &self.host);
        let r =
            send(self.addr, &req, self.timeout, 4 * 1024 * 1024).map_err(|e| format!("{e:?}"))?;
        if r.status != 200 {
            return Err(format!("GET {path}: HTTP {}", r.status));
        }
        Ok(parse_events(&r.text())
            .into_iter()
            .filter_map(Result::ok)
            .collect())
    }
}

/// Why a stream ended.
#[derive(Debug)]
pub enum StreamEnd {
    /// HTTP status other than 200 (e.g. 404 on a polling-mode nerve, 503 too many streams).
    Status(u16),
    Io(String),
    /// The consumer asked to stop.
    Stopped,
}

/// One stream connection: blocks reading NDJSON lines and hands each
/// parsed event to `on_event` (return false to stop). Updates `since` to
/// the highest `seq` seen. The nerve writes a keepalive every 5 min, so a
/// read idle for `idle_timeout` means a dead peer.
pub fn stream_once(
    addr: SocketAddr,
    since: &mut Option<u64>,
    idle_timeout: Duration,
    on_event: &mut dyn FnMut(Event) -> bool,
) -> StreamEnd {
    let mut s = match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
        Ok(s) => s,
        Err(e) => return StreamEnd::Io(format!("connect: {e}")),
    };
    let path = match since {
        Some(n) => format!("{NERVE_STREAM_PATH}?since={n}"),
        None => NERVE_STREAM_PATH.to_string(),
    };
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nAccept: application/x-ndjson\r\nConnection: close\r\n\r\n");
    let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
    if let Err(e) = s.write_all(req.as_bytes()) {
        return StreamEnd::Io(format!("write: {e}"));
    }
    let _ = s.set_read_timeout(Some(idle_timeout));
    let mut r = BufReader::new(s.take(u64::MAX));
    let mut line = String::new();
    // Status line + headers.
    if r.read_line(&mut line).is_err() {
        return StreamEnd::Io("no status line".into());
    }
    let code: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let mut chunked = false;
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) => return StreamEnd::Io("eof in headers".into()),
            Ok(_) => {
                let l = line.trim_end();
                if l.is_empty() {
                    break;
                }
                let low = l.to_ascii_lowercase();
                if low.starts_with("transfer-encoding:") && low.contains("chunked") {
                    chunked = true;
                }
            }
            Err(e) => return StreamEnd::Io(format!("headers: {e}")),
        }
    }
    if code != 200 {
        return StreamEnd::Status(code);
    }
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) => return StreamEnd::Io("eof".into()),
            Ok(_) => {
                let l = line.trim();
                // Chunk-size lines (hex) are skipped if a proxy chunked the stream.
                if l.is_empty() || (chunked && !l.starts_with('{')) {
                    continue;
                }
                for ev in parse_events(l).into_iter().flatten() {
                    if let Some(n) = ev.seq() {
                        if since.is_none_or(|s| n > s) {
                            *since = Some(n);
                        }
                    }
                    if !on_event(ev) {
                        return StreamEnd::Stopped;
                    }
                }
            }
            Err(e) => return StreamEnd::Io(format!("read: {e}")),
        }
    }
}
