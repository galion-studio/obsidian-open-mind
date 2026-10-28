// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `GET /events/stream?since=SEQ&min=SEVERITY`: NDJSON event stream for the
//! cortex. One JSON object per line ([`super::NerveEvent`]), oldest first.
//! `since` replays the ring first (gap-free resume after a reconnect; if the
//! ring no longer holds `since+1`, a `stream.gap` line says so). The stream
//! thread sleeps on a condvar; when idle it writes one `stream.keepalive`
//! line every 5 minutes so dead peers are noticed. At most 4 streams.

use super::{Hub, NerveEvent, Severity};
use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

pub const MAX_STREAMS: usize = 4;
pub const KEEPALIVE: Duration = Duration::from_secs(300);

pub fn query_u64(q: &str, k: &str) -> Option<u64> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
        .and_then(|v| v.parse().ok())
}

pub fn query_str<'a>(q: &'a str, k: &str) -> Option<&'a str> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
}

/// Serve one stream connection until the peer goes away.
pub fn serve(mut s: TcpStream, hub: Arc<Hub>, query: &str) {
    {
        let mut g = hub.lock();
        if g.streams >= MAX_STREAMS {
            drop(g);
            let _ = s.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: 13\r\nConnection: close\r\n\r\ntoo many: 4\n\n");
            return;
        }
        g.streams += 1;
    }
    let min = query_str(query, "min")
        .and_then(Severity::parse)
        .unwrap_or(Severity::Info);
    let mut since = query_u64(query, "since").unwrap_or_else(|| hub.lock().seq);
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    let ok = s
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-store\r\nConnection: close\r\nX-Nerve-Event-Schema: 1\r\n\r\n")
        .is_ok();
    if ok {
        // gap check
        let first = hub.lock().ring.front().map(|e| e.seq);
        if let Some(f) = first {
            if since + 1 < f {
                let mut g = NerveEvent::new(
                    "nerve",
                    "stream.gap",
                    Severity::Info,
                    format!("events {}..{} no longer in the ring", since + 1, f - 1),
                );
                g.data = serde_json::json!({"from": since + 1, "to": f - 1});
                let _ = s.write_all(format!("{}\n", g.to_line()).as_bytes());
            }
        }
        'outer: loop {
            let batch = hub.since(since, 256);
            if let Some(l) = batch.last() {
                since = l.seq;
                let mut out = String::new();
                for e in batch.iter().filter(|e| e.severity >= min) {
                    out.push_str(&e.to_line());
                    out.push('\n');
                }
                if !out.is_empty() && (s.write_all(out.as_bytes()).is_err() || s.flush().is_err()) {
                    break;
                }
                continue;
            }
            // sleep until a publish or the keepalive deadline
            let g = hub.lock();
            if g.seq > since {
                continue;
            }
            let (g, to) = hub
                .cv
                .wait_timeout_while(g, KEEPALIVE, |h| h.seq <= since)
                .unwrap_or_else(|e| e.into_inner());
            drop(g);
            if to.timed_out() {
                let k = NerveEvent::new("nerve", "stream.keepalive", Severity::Info, "").to_line();
                if s.write_all(format!("{k}\n").as_bytes()).is_err() {
                    break 'outer;
                }
            }
        }
    }
    hub.lock().streams -= 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query() {
        assert_eq!(query_u64("since=12&min=high", "since"), Some(12));
        assert_eq!(query_str("since=12&min=high", "min"), Some("high"));
        assert_eq!(query_u64("", "since"), None);
    }
}
