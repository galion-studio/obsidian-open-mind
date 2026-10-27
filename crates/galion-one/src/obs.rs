// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Per-request nerve observation (wave-4 slot 327, nerve-wire-gaps-2).
//!
//! galion-one answers each connection on its own thread and every response
//! head goes through `write_response` or `begin_audio`. Those record the
//! status and the time to the response head in a thread-local; the accept
//! loop then calls [`finish`], which hands it to
//! `nerve_client::observe_request` (5xx -> `http_5xx`, slow -> `slow_request`).
//! Time-to-head, not total time, so a long streamed voice reply is not a
//! "slow request". No-op cost when `NERVE_URL` is unset is one atomic load
//! inside nerve-client plus a thread-local write.

use std::cell::RefCell;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Req {
    pub what: String,
    pub status: u16,
    pub head_after: Option<Duration>,
    started: Option<Instant>,
}

thread_local! {
    static REQ: RefCell<Req> = const {
        RefCell::new(Req { what: String::new(), status: 0, head_after: None, started: None })
    };
}

/// Start observing one request on this thread.
pub fn begin() {
    REQ.with(|r| {
        *r.borrow_mut() = Req {
            what: String::new(),
            status: 0,
            head_after: None,
            started: Some(Instant::now()),
        }
    });
}

/// Name the request once the request line is parsed (`GET /path`, no query).
pub fn what(method: &str, path: &str) {
    let path: String = path
        .split('?')
        .next()
        .unwrap_or("/")
        .chars()
        .take(80)
        .collect();
    let method: String = method.chars().take(16).collect();
    REQ.with(|r| r.borrow_mut().what = format!("{method} {path}"));
}

/// A response head with `status` is being written. The first call wins.
pub fn status(code: u16) {
    REQ.with(|r| {
        let mut r = r.borrow_mut();
        if r.status == 0 {
            r.status = code;
            r.head_after = r.started.map(|t| t.elapsed());
        }
    });
}

/// Take this thread's record (and reset it).
pub fn take() -> Req {
    REQ.with(|r| {
        std::mem::replace(
            &mut *r.borrow_mut(),
            Req {
                what: String::new(),
                status: 0,
                head_after: None,
                started: None,
            },
        )
    })
}

/// Push the finished request to the nerve as `source`. Requests that never
/// got a response head (client hung up, oversized header) are not reported.
pub fn finish(source: &str) -> Option<Req> {
    let r = take();
    if r.status == 0 {
        return None;
    }
    let what = if r.what.is_empty() {
        "?"
    } else {
        r.what.as_str()
    };
    nerve_client::observe_request(source, what, r.status, r.head_after.unwrap_or_default());
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_first_status_and_time_to_head() {
        begin();
        what("GET", "/api/v1/awake?x=1");
        std::thread::sleep(Duration::from_millis(5));
        status(503);
        status(200);
        let r = take();
        assert_eq!(r.what, "GET /api/v1/awake");
        assert_eq!(r.status, 503);
        assert!(r.head_after.unwrap() >= Duration::from_millis(5));
        // reset after take
        assert_eq!(take().status, 0);
    }

    #[test]
    fn no_head_is_not_reported() {
        begin();
        what("GET", "/");
        assert!(finish("galion-one-test").is_none());
    }

    #[test]
    fn threads_do_not_share_records() {
        begin();
        status(404);
        let other = std::thread::spawn(|| {
            begin();
            take().status
        })
        .join()
        .unwrap();
        assert_eq!(other, 0);
        assert_eq!(take().status, 404);
    }

    #[test]
    fn long_names_are_capped() {
        begin();
        what("GETGETGETGET", &"/a".repeat(100));
        let r = take();
        assert_eq!(r.what.len(), 12 + 1 + 80);
    }
}
