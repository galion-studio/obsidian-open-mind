// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Route OpenAI-compatible requests to approved LAN providers, with the
//! `openmind-provider` fallback order GPU -> LAN -> CPU.
//!
//! The chain comes from [`openmind_provider::route::chain`] over the
//! registry rows that are approved (pending and denied rows are never in it)
//! plus operator-configured `--gpu` / `--cpu` rows. A LAN row is in the chain
//! only after `join` recorded it up. The registry is re-read for every
//! request, so `approve`, `deny` and `join` take effect on the next request
//! without a restart.
//!
//! The router never changes approval or health: it is a read-only consumer
//! of the registry. When the first entry cannot be reached (connect error,
//! timeout, or a 5xx) it tries the next one for that request and reports
//! every attempt. Only `http://` endpoints on loopback, private or
//! link-local addresses are dialed. No payload is logged.

use crate::admit::{endpoint_addr, is_local};
use openmind_provider::route::{chain, Approval, Capability, Health, Kind, ProviderRow};
use openmind_provider::Registry;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Largest request or upstream response body accepted (bytes).
pub const MAX_BODY: usize = 4 * 1024 * 1024;
/// Most connections handled at once by `serve-route`.
pub const MAX_CONNS: usize = 32;

/// Where the router reads rows from and what it adds itself.
#[derive(Debug, Clone)]
pub struct Router {
    pub registry: PathBuf,
    /// Operator-configured rows (`--gpu`, `--cpu`). Always approved.
    pub extra: Vec<ProviderRow>,
    pub timeout: Duration,
}

/// One dial of one provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attempt {
    pub id: String,
    pub kind: &'static str,
    pub endpoint: String,
    /// `ok`, `http-<code>` (passed to the client), `upstream-<code>` (5xx,
    /// next tried), `connect-failed`, `io-error`, `not-local`, `not-http`.
    pub outcome: String,
}

/// What [`Router::forward`] got back.
#[derive(Debug, Clone)]
pub struct Forwarded {
    pub provider: Option<String>,
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    pub attempts: Vec<Attempt>,
}

pub fn kind_str(kind: Kind) -> &'static str {
    match kind {
        Kind::Gpu => "gpu",
        Kind::Lan => "lan",
        Kind::CpuLocal => "cpu_local",
        Kind::Remote => "remote",
    }
}

/// An operator row for `--gpu URL` / `--cpu URL`. The URL must be
/// `http://IP:PORT` on a loopback, private or link-local address.
pub fn operator_row(id: &str, kind: Kind, url: &str) -> Result<ProviderRow, String> {
    let addr = http_addr(url).ok_or_else(|| format!("{url}: want http://IP:PORT"))?;
    if !is_local(addr.ip()) {
        return Err(format!(
            "{url}: not a loopback, private or link-local address"
        ));
    }
    Ok(ProviderRow {
        id: id.into(),
        kind,
        endpoint: format!("http://{addr}"),
        approval: Approval::Approved,
        health: Health::Unknown,
        capabilities: vec![Capability::Llm],
    })
}

fn http_addr(endpoint: &str) -> Option<SocketAddr> {
    let rest = endpoint.strip_prefix("http://")?;
    if rest.split('/').next()?.is_empty() {
        return None;
    }
    endpoint_addr(endpoint)
}

impl Router {
    /// The current chain for `need`. Reads the registry; dials nothing.
    pub fn chain(&self, need: Capability) -> Result<Vec<ProviderRow>, String> {
        let registry = Registry::load(&self.registry).map_err(|e| e.to_string())?;
        Ok(chain(need, &registry, &self.extra))
    }

    /// Send `method path body` to the first reachable row of the chain.
    pub fn forward(
        &self,
        need: Capability,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<Forwarded, String> {
        let rows = self.chain(need)?;
        let mut attempts = Vec::new();
        for row in rows {
            let mut attempt = Attempt {
                id: row.id.clone(),
                kind: kind_str(row.kind),
                endpoint: row.endpoint.clone(),
                outcome: String::new(),
            };
            let Some(addr) = http_addr(&row.endpoint) else {
                attempt.outcome = "not-http".into();
                attempts.push(attempt);
                continue;
            };
            if !is_local(addr.ip()) {
                attempt.outcome = "not-local".into();
                attempts.push(attempt);
                continue;
            }
            match send(addr, method, path, body, self.timeout) {
                Err(Dial::Connect) => attempt.outcome = "connect-failed".into(),
                Err(Dial::Io) => attempt.outcome = "io-error".into(),
                Ok(resp) if resp.status >= 500 => {
                    attempt.outcome = format!("upstream-{}", resp.status);
                }
                Ok(resp) => {
                    attempt.outcome = if resp.status < 300 {
                        "ok".into()
                    } else {
                        format!("http-{}", resp.status)
                    };
                    attempts.push(attempt);
                    return Ok(Forwarded {
                        provider: Some(row.id),
                        status: resp.status,
                        content_type: resp.content_type,
                        body: resp.body,
                        attempts,
                    });
                }
            }
            attempts.push(attempt);
        }
        let body = json!({"error": {
            "message": "no approved, healthy provider answered (GPU -> LAN -> CPU)",
            "type": "no_provider",
            "attempts": attempts,
        }});
        Ok(Forwarded {
            provider: None,
            status: 503,
            content_type: "application/json".into(),
            body: serde_json::to_vec(&body).unwrap_or_default(),
            attempts,
        })
    }
}

#[derive(Debug)]
enum Dial {
    Connect,
    Io,
}

struct Upstream {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<Upstream, Dial> {
    let mut conn = TcpStream::connect_timeout(&addr, timeout).map_err(|_| Dial::Connect)?;
    let io = |_| Dial::Io;
    conn.set_read_timeout(Some(timeout)).map_err(io)?;
    conn.set_write_timeout(Some(timeout)).map_err(io)?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    conn.write_all(head.as_bytes()).map_err(io)?;
    conn.write_all(body).map_err(io)?;
    let mut reader = BufReader::new(conn);
    let (status, headers) = read_head(&mut reader).map_err(io)?;
    let status: u16 = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or(Dial::Io)?;
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    let content_type = header("content-type")
        .unwrap_or("application/json")
        .to_string();
    let body = if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        read_chunked(&mut reader).map_err(io)?
    } else if let Some(len) = header("content-length").and_then(|v| v.trim().parse::<usize>().ok())
    {
        if len > MAX_BODY {
            return Err(Dial::Io);
        }
        let mut buf = vec![0; len];
        reader.read_exact(&mut buf).map_err(io)?;
        buf
    } else {
        let mut buf = Vec::new();
        reader
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(io)?;
        if buf.len() > MAX_BODY {
            return Err(Dial::Io);
        }
        buf
    };
    Ok(Upstream {
        status,
        content_type,
        body,
    })
}

type Head = (String, Vec<(String, String)>);

/// Read a request or status line plus headers (at most 64 headers, 8 KiB each).
fn read_head(reader: &mut impl BufRead) -> io::Result<Head> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut first = String::new();
    if reader.take(8192).read_line(&mut first)? == 0 {
        return Err(bad("empty"));
    }
    let first = first.trim_end().to_string();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        if reader.take(8192).read_line(&mut line)? == 0 {
            return Err(bad("eof in headers"));
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if headers.len() >= 64 {
            return Err(bad("too many headers"));
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok((first, headers))
}

fn read_chunked(reader: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    let mut out = Vec::new();
    loop {
        let mut line = String::new();
        reader.take(1024).read_line(&mut line)?;
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| bad("bad chunk size"))?;
        if size == 0 {
            return Ok(out);
        }
        if out.len() + size > MAX_BODY {
            return Err(bad("body too large"));
        }
        let start = out.len();
        out.resize(start + size, 0);
        reader.read_exact(&mut out[start..])?;
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
    }
}

/// `serve-route` limits.
#[derive(Debug, Clone)]
pub struct ServeOpts {
    pub bind: SocketAddr,
    /// Stop after this long (None: until killed).
    pub duration: Option<Duration>,
}

/// Counters reported when `serve-route` stops.
#[derive(Debug, Default, Serialize)]
pub struct ServeStats {
    pub requests: u64,
    pub routed: u64,
    pub fallbacks: u64,
    pub no_provider: u64,
}

/// Bind `opts.bind` and serve until `opts.duration` passes.
///
/// Endpoints: `GET /healthz`, `GET /v1/route` (the chain, nothing dialed),
/// `GET /v1/models`, `POST /v1/chat/completions`, `POST /v1/completions`,
/// `POST /v1/embeddings`. Answers carry `X-Galion-Provider` and
/// `X-Galion-Attempts`. One line per request goes to stderr with a Zagreb
/// timestamp: provider ids and outcomes, never the payload.
pub fn serve(router: Router, opts: &ServeOpts) -> io::Result<ServeStats> {
    let listener = TcpListener::bind(opts.bind)?;
    let local = listener.local_addr()?;
    eprintln!(
        "{} serve-route listening on {local} registry {} extra [{}]",
        zagreb_time::now_rfc3339(),
        router.registry.display(),
        router
            .extra
            .iter()
            .map(|r| format!("{}={}", r.id, r.endpoint))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(d) = opts.duration {
        let stop = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(d);
            stop.store(true, Ordering::SeqCst);
            // Wake the blocking accept.
            let _ = TcpStream::connect_timeout(&local, Duration::from_millis(200));
        });
    }
    let router = Arc::new(router);
    let conns = Arc::new(AtomicUsize::new(0));
    let counters: Arc<[AtomicU64; 4]> = Arc::new(Default::default());
    for conn in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut conn) = conn else { continue };
        if conns.load(Ordering::SeqCst) >= MAX_CONNS {
            let _ = respond(
                &mut conn,
                503,
                "application/json",
                br#"{"error":"busy"}"#,
                &[],
            );
            continue;
        }
        conns.fetch_add(1, Ordering::SeqCst);
        let (router, conns, counters) = (router.clone(), conns.clone(), counters.clone());
        std::thread::spawn(move || {
            let _ = handle(&router, &mut conn, &counters);
            conns.fetch_sub(1, Ordering::SeqCst);
        });
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while conns.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(ServeStats {
        requests: counters[0].load(Ordering::SeqCst),
        routed: counters[1].load(Ordering::SeqCst),
        fallbacks: counters[2].load(Ordering::SeqCst),
        no_provider: counters[3].load(Ordering::SeqCst),
    })
}

fn handle(router: &Router, conn: &mut TcpStream, counters: &[AtomicU64; 4]) -> io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(10)))?;
    conn.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(conn.try_clone()?);
    let Ok((line, headers)) = read_head(&mut reader) else {
        return Ok(()); // the wake-up connect, or a client that sent nothing
    };
    counters[0].fetch_add(1, Ordering::SeqCst);
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let path = target.split('?').next().unwrap_or("").to_string();
    let len = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    if len > MAX_BODY {
        return respond(
            conn,
            413,
            "application/json",
            br#"{"error":"body too large"}"#,
            &[],
        );
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;

    let error = |msg: &str| serde_json::to_vec(&json!({"error": msg})).unwrap_or_default();
    match (method.as_str(), path.as_str()) {
        ("GET", "/healthz") => respond(conn, 200, "application/json", br#"{"status":"ok"}"#, &[]),
        ("GET", "/v1/route") => match router.chain(Capability::Llm) {
            Ok(rows) => {
                let v = json!({
                    "need": "llm",
                    "order": "gpu -> lan -> cpu_local (approved rows only; lan needs join up)",
                    "chain": rows.iter().map(|r| json!({
                        "id": r.id, "kind": kind_str(r.kind), "endpoint": r.endpoint,
                    })).collect::<Vec<_>>(),
                });
                respond(conn, 200, "application/json", v.to_string().as_bytes(), &[])
            }
            Err(e) => respond(conn, 500, "application/json", &error(&e), &[]),
        },
        ("GET", "/v1/models")
        | ("POST", "/v1/chat/completions" | "/v1/completions" | "/v1/embeddings") => {
            if method == "POST" {
                match serde_json::from_slice::<Value>(&body) {
                    Ok(v) if v.get("stream").and_then(Value::as_bool) == Some(true) => {
                        return respond(
                            conn,
                            400,
                            "application/json",
                            &error("stream=true is not supported by this router"),
                            &[],
                        );
                    }
                    Ok(_) => {}
                    Err(_) => {
                        return respond(
                            conn,
                            400,
                            "application/json",
                            &error("body is not JSON"),
                            &[],
                        );
                    }
                }
            }
            let fwd = match router.forward(Capability::Llm, &method, &path, &body) {
                Ok(f) => f,
                Err(e) => return respond(conn, 500, "application/json", &error(&e), &[]),
            };
            let tried: Vec<String> = fwd
                .attempts
                .iter()
                .map(|a| format!("{}:{}", a.id, a.outcome))
                .collect();
            eprintln!(
                "{} route {method} {path} -> {} status {} tried [{}]",
                zagreb_time::now_rfc3339(),
                fwd.provider.as_deref().unwrap_or("none"),
                fwd.status,
                tried.join(", ")
            );
            match &fwd.provider {
                Some(_) => {
                    counters[1].fetch_add(1, Ordering::SeqCst);
                    if fwd.attempts.len() > 1 {
                        counters[2].fetch_add(1, Ordering::SeqCst);
                    }
                }
                None => {
                    counters[3].fetch_add(1, Ordering::SeqCst);
                }
            }
            let provider = fwd.provider.clone().unwrap_or_else(|| "none".into());
            let attempts = tried.join(",");
            respond(
                conn,
                fwd.status,
                &fwd.content_type,
                &fwd.body,
                &[
                    ("X-Galion-Provider", &provider),
                    ("X-Galion-Attempts", &attempts),
                ],
            )
        }
        _ => respond(conn, 404, "application/json", &error("not found"), &[]),
    }
}

fn respond(
    conn: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    extra: &[(&str, &str)],
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra {
        let v: String = v.chars().filter(|c| !c.is_control()).take(512).collect();
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    conn.write_all(head.as_bytes())?;
    conn.write_all(body)?;
    conn.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_rows_must_be_local_http() {
        assert!(operator_row("cpu-local", Kind::CpuLocal, "http://127.0.0.1:19311").is_ok());
        assert!(operator_row("cpu-local", Kind::CpuLocal, "http://203.0.113.8:80").is_err());
        assert!(operator_row("cpu-local", Kind::CpuLocal, "tcp://127.0.0.1:1").is_err());
        assert!(operator_row("cpu-local", Kind::CpuLocal, "http://localhost:1").is_err());
        let r = operator_row("gpu-0", Kind::Gpu, "http://10.0.0.5:8080/v1").unwrap();
        assert_eq!(r.endpoint, "http://10.0.0.5:8080");
    }

    #[test]
    fn chunked_bodies_decode() {
        let mut r = io::Cursor::new(b"4\r\nWiki\r\n5;x=y\r\npedia\r\n0\r\n\r\n".to_vec());
        assert_eq!(read_chunked(&mut r).unwrap(), b"Wikipedia");
    }
}
