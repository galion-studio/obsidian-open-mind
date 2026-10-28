// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The health probe: what `curl -s -o /dev/null -m 3 -w '%{http_code}' URL` did in the
//! bash version, done in-process for `http://` URLs (every URL in the health table is
//! plain HTTP on loopback). Other schemes still go to curl. 0 means "no answer",
//! like curl's `000`.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// (host:port, path) of an `http://` URL.
pub fn split_http(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (auth, path) = match rest.find(['/', '?']) {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    let path = path.split('#').next().unwrap_or("/").to_string();
    if auth.is_empty() || auth.contains('@') {
        return None;
    }
    let hostport = if auth.starts_with('[') {
        if auth.contains("]:") {
            auth.to_string()
        } else {
            format!("{auth}:80")
        }
    } else if auth.contains(':') {
        auth.to_string()
    } else {
        format!("{auth}:80")
    };
    Some((hostport, path))
}

fn http_status(hostport: &str, path: &str, limit: Duration) -> u16 {
    let t0 = Instant::now();
    let left = |t0: Instant| {
        limit
            .saturating_sub(t0.elapsed())
            .max(Duration::from_millis(1))
    };
    let Ok(addrs) = hostport.to_socket_addrs() else {
        return 0;
    };
    let host = hostport.rsplit_once(':').map(|h| h.0).unwrap_or(hostport);
    for a in addrs {
        let Ok(mut s) = TcpStream::connect_timeout(&a, left(t0)) else {
            continue;
        };
        let _ = s.set_write_timeout(Some(left(t0)));
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: galion-deploy\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        if s.write_all(req.as_bytes()).is_err() {
            return 0;
        }
        let mut head = Vec::new();
        let mut b = [0u8; 256];
        while !head.windows(2).any(|w| w == b"\r\n") && head.len() < 4096 {
            if t0.elapsed() >= limit {
                return 0;
            }
            let _ = s.set_read_timeout(Some(left(t0)));
            match s.read(&mut b) {
                Ok(0) => break,
                Ok(n) => head.extend_from_slice(&b[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return 0,
            }
        }
        let line = String::from_utf8_lossy(&head);
        let mut it = line.split_whitespace();
        return match (it.next(), it.next()) {
            (Some(v), Some(c)) if v.starts_with("HTTP/") && c.len() == 3 => c.parse().unwrap_or(0),
            _ => 0,
        };
    }
    0
}

/// HTTP status of a GET, within `limit` (connect + send + status line), 0 on failure.
pub fn status(url: &str, limit: Duration) -> u16 {
    if let Some((hp, path)) = split_http(url) {
        return http_status(&hp, &path, limit);
    }
    let out = Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-m"])
        .arg(limit.as_secs().max(1).to_string())
        .args(["-w", "%{http_code}", url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn split() {
        assert_eq!(
            split_http("http://127.0.0.1:8650/health"),
            Some(("127.0.0.1:8650".into(), "/health".into()))
        );
        assert_eq!(
            split_http("http://localhost"),
            Some(("localhost:80".into(), "/".into()))
        );
        assert_eq!(
            split_http("http://h:1?x=1"),
            Some(("h:1".into(), "/?x=1".into()))
        );
        assert_eq!(
            split_http("http://[::1]:9/a#f"),
            Some(("[::1]:9".into(), "/a".into()))
        );
        assert_eq!(split_http("https://x/"), None);
        assert_eq!(split_http("http://u@x/"), None);
    }

    #[test]
    fn statuses() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let th = std::thread::spawn(move || {
            for (i, c) in l.incoming().take(3).enumerate() {
                let mut c = c.unwrap();
                let mut b = [0u8; 512];
                let _ = c.read(&mut b);
                match i {
                    0 => {
                        let _ = c.write_all(b"HTTP/1.1 204 No Content\r\n\r\n");
                    }
                    1 => {
                        let _ = c.write_all(b"HTTP/1.0 503 Busy\r\n\r\n");
                    }
                    _ => {
                        let _ = c.write_all(b"garbage\r\n");
                    }
                }
            }
        });
        let u = format!("http://127.0.0.1:{port}/health");
        assert_eq!(status(&u, Duration::from_secs(3)), 204);
        assert_eq!(status(&u, Duration::from_secs(3)), 503);
        assert_eq!(status(&u, Duration::from_secs(3)), 0);
        th.join().unwrap();
        // nothing listening
        let free = TcpListener::bind("127.0.0.1:0").unwrap();
        let p2 = free.local_addr().unwrap().port();
        drop(free);
        assert_eq!(
            status(&format!("http://127.0.0.1:{p2}/"), Duration::from_secs(1)),
            0
        );
    }

    #[test]
    fn silent_server_times_out() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = Instant::now();
        assert_eq!(
            status(
                &format!("http://127.0.0.1:{port}/"),
                Duration::from_millis(300)
            ),
            0
        );
        assert!(t.elapsed() < Duration::from_secs(2));
        drop(l);
    }
}
