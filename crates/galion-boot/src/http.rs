// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tiny HTTP/1.0 POST for `fire-loops` (loop-daemon.sh died when curl went
//! missing on 2026-10-10; this needs no curl). The socket work is the shared
//! galion-common client; the request bytes are unchanged.

use galion_common::http::{self, ClientRequest};
use std::net::SocketAddr;
use std::time::Duration;

pub fn post_json(
    addr: SocketAddr,
    path: &str,
    body: &str,
    timeout: Duration,
) -> Result<u16, String> {
    let req = ClientRequest {
        method: "POST",
        path,
        minor: 0,
        headers: vec![
            ("Host".into(), addr.to_string()),
            ("content-type".into(), "application/json".into()),
            ("Content-Length".into(), body.len().to_string()),
            ("Connection".into(), "close".into()),
        ],
        body: body.as_bytes(),
    };
    http::send(addr, &req, timeout, 64 * 1024)
        .map(|r| r.status)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn request_bytes_and_status_unchanged() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = vec![0u8; 512];
            let mut got = 0;
            while !buf[..got].ends_with(b"{\"a\":1}") {
                let n = s.read(&mut buf[got..]).unwrap();
                assert!(n > 0);
                got += n;
            }
            s.write_all(b"HTTP/1.0 202 Accepted\r\ncontent-length: 0\r\n\r\n")
                .unwrap();
            String::from_utf8(buf[..got].to_vec()).unwrap()
        });
        let code = super::post_json(addr, "/fire", "{\"a\":1}", Duration::from_secs(3)).unwrap();
        assert_eq!(code, 202);
        assert_eq!(
            t.join().unwrap(),
            format!("POST /fire HTTP/1.0\r\nHost: {addr}\r\ncontent-type: application/json\r\nContent-Length: 7\r\nConnection: close\r\n\r\n{{\"a\":1}}")
        );
        let dead = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let err = super::post_json(dead, "/x", "{}", Duration::from_secs(1)).unwrap_err();
        assert!(err.starts_with("connect: "), "{err}");
    }
}
