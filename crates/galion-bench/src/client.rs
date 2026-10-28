// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! A tiny std-only HTTP/1.1 client for tests and benchmark drivers. It talks
//! only to the `SocketAddr` you hand it (no DNS, no TLS, no proxies), which is
//! exactly what you want against a local mock.

use crate::http::read_response;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// A response from [`request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Headers, lower-cased names.
    pub headers: Vec<(String, String)>,
    /// Body as UTF-8 (lossy).
    pub body: String,
}

impl Response {
    /// First header with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Body parsed as JSON.
    pub fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_str(&self.body)
    }
}

/// Send one request (`target` is path + optional `?query`) and read the reply.
/// A `Content-Length` header is added automatically.
pub fn request(
    addr: SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> io::Result<Response> {
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut head = format!("{method} {target} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length")
            || k.contains(['\r', '\n'])
            || v.contains(['\r', '\n'])
        {
            continue;
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
    let mut w = &stream;
    w.write_all(head.as_bytes())?;
    w.write_all(body.as_bytes())?;
    w.flush()?;
    let (status, headers, body) = read_response(&stream)?;
    Ok(Response {
        status,
        headers,
        body,
    })
}

/// `GET target` with no headers.
pub fn get(addr: SocketAddr, target: &str) -> io::Result<Response> {
    request(addr, "GET", target, &[], "")
}

/// `POST target` with a JSON body.
pub fn post_json(addr: SocketAddr, target: &str, body: &serde_json::Value) -> io::Result<Response> {
    request(
        addr,
        "POST",
        target,
        &[("Content-Type", "application/json")],
        &body.to_string(),
    )
}
