// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// `GET /api/v1/services`: up/down and latency of the Rust services on this
// host. Ported from ws/ui f69a916 (the build that runs live on :8091), with
// two changes so the binary stays portable and spare-safe:
//   * no `systemctl` shell-out: link-watch-rs counts as up when a process
//     whose /proc comm is `link-watch` exists (std::fs only; on hosts without
//     /proc it reads as down);
//   * the hermes and memory probes dial `GALION_MIND` / `GALION_MEMORY`
//     (defaults 127.0.0.1:8642 / 127.0.0.1:8420, today's live ports), so a
//     spare One does not probe the live services. The reported `port` is the
//     port actually dialled.
// Response shape is byte-compatible with live:
// {"services":[{"id":"hermes-rs","port":8642,"ok":true,"ms":0},
//   {"id":"openmind-memory","port":8420,"ok":true,"ms":0},
//   {"id":"link-watch-rs","port":null,"ok":true,"ms":0}]}

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// GET /health; ok when the status line is 2xx. Returns (ok, elapsed ms).
pub fn probe_http(addr: SocketAddr) -> (bool, u128) {
    let start = Instant::now();
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else {
        return (false, 0);
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(800)));
    let req = format!("GET /health HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return (false, 0);
    }
    let mut head = [0u8; 16];
    let n = s.read(&mut head).unwrap_or(0);
    (status_ok(&head[..n]), start.elapsed().as_millis())
}

/// `HTTP/x.y 2..` status line.
pub fn status_ok(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let mut bits = text.split_whitespace();
    bits.next().map(|v| v.starts_with("HTTP/")).unwrap_or(false)
        && bits.next().map(|c| c.starts_with('2')).unwrap_or(false)
}

/// True when some process under `proc_root` has `comm` equal to `name`.
pub fn process_named(proc_root: &str, name: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(proc_root) else {
        return false;
    };
    for ent in rd.flatten() {
        let file_name = ent.file_name();
        let pid = file_name.to_string_lossy();
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(ent.path().join("comm")) {
            if comm.trim_end() == name {
                return true;
            }
        }
    }
    false
}

/// The JSON body. `mind` and `memory` are the dial targets.
pub fn services_json(mind: SocketAddr, memory: SocketAddr, link_watch_up: bool) -> String {
    let (h_ok, h_ms) = probe_http(mind);
    let (m_ok, m_ms) = probe_http(memory);
    format!(
        "{{\"services\":[{{\"id\":\"hermes-rs\",\"port\":{},\"ok\":{h_ok},\"ms\":{h_ms}}},{{\"id\":\"openmind-memory\",\"port\":{},\"ok\":{m_ok},\"ms\":{m_ms}}},{{\"id\":\"link-watch-rs\",\"port\":null,\"ok\":{link_watch_up},\"ms\":0}}]}}",
        mind.port(),
        memory.port()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn status_line_parsing() {
        assert!(status_ok(b"HTTP/1.1 200 OK\r\n"));
        assert!(status_ok(b"HTTP/1.0 204 No"));
        assert!(!status_ok(b"HTTP/1.1 503 x"));
        assert!(!status_ok(b""));
        assert!(!status_ok(b"garbage"));
    }

    #[test]
    fn closed_port_is_down_and_shape_matches_live() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        assert!(!probe_http(addr).0);
        let j = services_json(addr, addr, false);
        let want = format!(
            "{{\"services\":[{{\"id\":\"hermes-rs\",\"port\":{p},\"ok\":false,\"ms\":0}},{{\"id\":\"openmind-memory\",\"port\":{p},\"ok\":false,\"ms\":0}},{{\"id\":\"link-watch-rs\",\"port\":null,\"ok\":false,\"ms\":0}}]}}",
            p = addr.port()
        );
        assert_eq!(j, want);
    }

    #[test]
    fn process_named_reads_a_fixture_proc_tree() {
        let dir = std::env::temp_dir().join(format!("one-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("42")).unwrap();
        std::fs::create_dir_all(dir.join("self")).unwrap();
        std::fs::write(dir.join("42/comm"), "link-watch\n").unwrap();
        std::fs::write(dir.join("self/comm"), "other\n").unwrap();
        let root = dir.to_str().unwrap();
        assert!(process_named(root, "link-watch"));
        assert!(!process_named(root, "other"));
        assert!(!process_named(root, "hermes"));
        assert!(!process_named("/nonexistent-proc", "link-watch"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
