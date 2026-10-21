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
use galion_common::http::{send, ClientError, ClientRequest};
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
    /// `nerve` = the nerve answered; `local` = simulated/refused by the cortex
    /// (the request never reached the nerve); `unknown` = the request was
    /// sent but no reply came back (hung or slow nerve, T5.1 chaos S7): the
    /// nerve may still run it, so it counts toward the cap and is escalated.
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

/// The request was written to the nerve but its reply was lost (read
/// timeout, reset, bad reply). The nerve may still act on it later (seen in
/// the T5.1 hung-nerve run: the nerve logged the request after SIGCONT), so
/// this is never reported as "not run".
fn lost_reply(req: &ActionRequest, why: &str) -> Outcome {
    if req.dry_run {
        Outcome {
            status: ActionStatus::Simulated,
            by: "unknown",
            reason: format!(
                "dry-run: request to restart {} sent, no nerve reply ({why}); the nerve may still log it",
                req.unit
            ),
        }
    } else {
        Outcome {
            status: ActionStatus::Failed,
            by: "unknown",
            reason: format!(
                "restart {} sent, no nerve reply ({why}): outcome UNKNOWN, counted toward the cap, escalated, not retried",
                req.unit
            ),
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

    fn post(&self, path: &str, body: &str) -> Result<(u16, String), ClientError> {
        let req = ClientRequest::post(path, &self.host, "application/json", body.as_bytes());
        send(self.addr, &req, self.timeout, 256 * 1024).map(|r| (r.status, r.text()))
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
            // Not connected: the nerve never saw it.
            Err(e @ ClientError::Connect(_)) | Err(e @ ClientError::Invalid(_)) => {
                local(req, &format!("nerve unreachable: {e}"))
            }
            // Written (or partly written): the nerve may have it.
            Err(e) => lost_reply(req, &e.to_string()),
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

/// Which nerve process answers on `addr`: `(started, seq)` from the
/// event-mode nerve's `GET /stats`. `None` for a nerve without `/stats`
/// (polling mode) or when it does not answer in `timeout`.
///
/// Used on every stream (re)connect: the nerve's `seq` starts again at 1
/// after a restart, so resuming with the old `since` would silently skip
/// the new nerve's first events (T5.1 chaos S8).
pub fn nerve_instance(addr: SocketAddr, timeout: Duration) -> Option<(String, u64)> {
    let host = addr.to_string();
    let req = ClientRequest::get("/stats", &host);
    let r = send(addr, &req, timeout, 256 * 1024).ok()?;
    if r.status != 200 {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&r.text()).ok()?;
    let started = v.get("started")?.as_str()?.to_string();
    let seq = v.get("seq")?.as_u64()?;
    Some((started, seq))
}

/// Resume point after a (re)connect. `known` is the nerve instance seen on
/// the previous connection, `now` the one answering now. Returns the
/// `since` to use and whether the nerve restarted in between.
pub fn resume_since(
    since: Option<u64>,
    known: Option<&str>,
    now: Option<(&str, u64)>,
) -> (Option<u64>, bool) {
    let Some((started, seq)) = now else {
        return (since, false);
    };
    let restarted = known.is_some_and(|k| k != started) || since.is_some_and(|s| seq < s);
    if restarted {
        // Everything in the new nerve's ring is new to us.
        (Some(0), true)
    } else {
        (since, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_after_nerve_restart() {
        // first connect: nothing known
        assert_eq!(resume_since(None, None, Some(("t1", 5))), (None, false));
        // same nerve, further along
        assert_eq!(
            resume_since(Some(70), Some("t1"), Some(("t1", 75))),
            (Some(70), false)
        );
        // new nerve: seq went back
        assert_eq!(
            resume_since(Some(70), Some("t1"), Some(("t1", 12))),
            (Some(0), true)
        );
        // new nerve that already published more than we had seen
        assert_eq!(
            resume_since(Some(70), Some("t1"), Some(("t2", 90))),
            (Some(0), true)
        );
        // nerve without /stats: keep the cursor
        assert_eq!(resume_since(Some(70), Some("t1"), None), (Some(70), false));
    }

    fn req(dry: bool) -> ActionRequest {
        ActionRequest {
            v: nerve_cortex_contract::CONTRACT_VERSION,
            request_id: "dec-1-inc-1".into(),
            source: nerve_cortex_contract::CORTEX_SOURCE.into(),
            ts: "t".into(),
            incident: "inc-1".into(),
            action: nerve_cortex_contract::ActionKind::Restart,
            unit: "tuner.service".into(),
            dry_run: dry,
            rule: "R3-known-crash".into(),
            decided_by: nerve_cortex_contract::DecidedBy::Rules,
            reason: "x".into(),
        }
    }

    #[test]
    fn lost_reply_is_unknown_not_refused() {
        let o = lost_reply(&req(false), "read: timed out");
        assert_eq!(o.by, "unknown");
        assert_eq!(o.status, ActionStatus::Failed);
        assert!(o.reason.contains("UNKNOWN"));
        let o = lost_reply(&req(true), "read: timed out");
        assert_eq!((o.by, o.status), ("unknown", ActionStatus::Simulated));
        let o = local(&req(false), "connect refused");
        assert_eq!((o.by, o.status), ("local", ActionStatus::Refused));
    }

    #[test]
    fn hung_nerve_reply_is_unknown() {
        // A listener that accepts and never answers (a SIGSTOPped nerve).
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let hold = std::thread::spawn(move || {
            let s = l.accept();
            std::thread::sleep(Duration::from_millis(800));
            drop(s);
        });
        let mut n = HttpNerve::new(&format!("http://{addr}")).unwrap();
        n.timeout = Duration::from_millis(300);
        let o = n.request_action(&req(false));
        assert_eq!(o.by, "unknown", "{o:?}");
        assert_eq!(o.status, ActionStatus::Failed);
        let _ = hold.join();
        // nothing listening: never delivered
        let o = n.request_action(&req(false));
        assert_eq!(o.by, "local", "{o:?}");
    }
}
