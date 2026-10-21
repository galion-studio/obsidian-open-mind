// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! How a bench tool adapter reaches its backend: real loopback HTTP to the
//! sibling's server binary, or the sibling's own in-process router (unit
//! tests). Both speak the same `(method, path, body) -> (status, body)`.
//!
//! The HTTP transport refuses any host that is not loopback before a socket
//! is opened, so no adapter can reach a live bank, broker or supplier.

use bench_banking::client::is_loopback_host;
use bench_banking::http::{parse_url, send, Url};
use std::sync::Mutex;

pub trait Transport: Send + Sync {
    /// One request. `Err` only for transport failures (unreachable, I/O);
    /// HTTP error statuses come back as `Ok((status, body))`.
    fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String>;
    /// Short label for reports (`http://127.0.0.1:19601`, `in-process`).
    fn label(&self) -> String;
}

/// Loopback-only HTTP/1.1 (std only, `Connection: close`).
pub struct HttpTransport {
    url: Url,
    base: String,
}

impl HttpTransport {
    pub fn new(base: &str) -> Result<Self, String> {
        let url = parse_url(base)?;
        if url.scheme != "http" || !is_loopback_host(&url.host) {
            return Err(format!(
                "bench backends must be loopback http (127.0.0.1 / ::1 / localhost), got {base}"
            ));
        }
        Ok(HttpTransport {
            url,
            base: base.trim_end_matches('/').to_string(),
        })
    }
}

impl Transport for HttpTransport {
    fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
        let mut h = vec![("Accept", "application/json")];
        if body.is_some() {
            h.push(("Content-Type", "application/json"));
        }
        let r = send(&self.url, method, path, &h, body.map(str::as_bytes))
            .map_err(|e| format!("backend unreachable at {}: {e}", self.base))?;
        Ok((r.status, String::from_utf8_lossy(&r.body).into_owned()))
    }
    fn label(&self) -> String {
        self.base.clone()
    }
}

/// `galion_wallstreet::http::route` on an in-process paper broker.
pub struct WallStreetInProcess(pub Mutex<galion_wallstreet::broker::PaperBroker>);

impl WallStreetInProcess {
    pub fn fixture() -> Self {
        WallStreetInProcess(Mutex::new(galion_wallstreet::broker::PaperBroker::fixture()))
    }
}

impl Transport for WallStreetInProcess {
    fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
        let r = galion_wallstreet::http::route(&self.0, method, path, body.unwrap_or(""));
        Ok((r.status, r.body))
    }
    fn label(&self) -> String {
        "in-process galion-wallstreet".into()
    }
}

/// `galion_vending::http::Server::handle` on an in-process sim.
pub struct VendingInProcess(pub Mutex<galion_vending::http::Server>);

impl VendingInProcess {
    pub fn new(seed: u64, days: u32, log: Option<std::path::PathBuf>) -> std::io::Result<Self> {
        Ok(VendingInProcess(Mutex::new(
            galion_vending::http::Server::new(seed, days, log)?,
        )))
    }
}

impl Transport for VendingInProcess {
    fn send(&self, method: &str, path: &str, body: Option<&str>) -> Result<(u16, String), String> {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let (code, v, _stop) = s.handle(method, path, body.unwrap_or("").as_bytes());
        Ok((code, v.to_string()))
    }
    fn label(&self) -> String {
        "in-process galion-vending".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_refuses_non_loopback() {
        for u in [
            "https://api.revolut.com",
            "http://10.0.0.5:8080",
            "http://example.com",
            "https://127.0.0.1:443",
        ] {
            assert!(HttpTransport::new(u).is_err(), "{u}");
        }
        assert!(HttpTransport::new("http://127.0.0.1:19601").is_ok());
    }
}
