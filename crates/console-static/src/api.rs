// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! In-process API handlers for console-dist cutover prep (`--local-api`).
//!
//! These mirror the *shapes* the live Node srvx console answers for a few
//! paths (health + unpaired session), without inventing production identity
//! claims (`galion-control` / `3.8.3-board` / machine target). Bare `/health`
//! stays absent (404 from the static router), matching live srvx.

use crate::http::RequestHead;
use console_ui::gate;

/// Honest process identity for `/api/v1/health` (not the live Node board).
pub const SERVICE: &str = "galion-console-static";
/// Crate/semver string embedded in the health JSON.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiReply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

fn json(status: u16, body: String) -> ApiReply {
    ApiReply {
        status,
        content_type: "application/json",
        body,
    }
}

fn phone_required() -> ApiReply {
    json(401, r#"{"error":"phone required"}"#.into())
}

/// Path without query; already sanitized.
fn api_path(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// Honest `/api/v1/health` body for a console host (loopback or listed).
pub fn health_body() -> String {
    // Keep keys the Rust UI and probes already read (`status`), plus a clear
    // identity so nobody mistakes this for the live Node board payload.
    format!(
        concat!(
            r#"{{"status":"healthy","service":"{service}","version":"{version}","#,
            r#""implementation":"rust","mode":"local-api","#,
            r#""note":"Box/local console-static --local-api. Not the live Node srvx board."}}"#
        ),
        service = SERVICE,
        version = VERSION
    )
}

/// Try to answer a request from the local API. `None` = not ours (caller may
/// proxy or 404).
pub fn try_handle(
    method: &str,
    path: &str,
    head: &RequestHead,
    peer_loopback: bool,
    extra_hosts: &[String],
) -> Option<ApiReply> {
    let path = api_path(path);
    if !path.starts_with("/api/") {
        return None;
    }

    let host = head.header("host").unwrap_or("");
    let console = gate::is_console_host(host, extra_hosts);
    // Live srvx: non-console Host on /api/* -> 401 phone required.
    if !console {
        return Some(phone_required());
    }
    // Live srvx trusts Host alone for loopback; we also require a loopback peer
    // when Host is loopback (same hardening as console-ui pages).
    let bare = gate::normalize_host(host);
    if gate::is_loopback_host(&bare) && !peer_loopback {
        return Some(phone_required());
    }

    match (method, path) {
        ("GET" | "HEAD", "/api/v1/health") => Some(json(200, health_body())),
        ("GET" | "HEAD", "/api/v1/session" | "/api/v1/session-pack") => {
            // Parity with unpaired srvx: need admin cookie. We do not invent
            // session documents here; paired callers should use --upstream.
            if gate::has_admin_cookie(head.header("cookie")) {
                Some(json(
                    501,
                    r#"{"ok":false,"error":"session API not implemented in console-static --local-api; set --upstream"}"#.into(),
                ))
            } else {
                Some(phone_required())
            }
        }
        ("GET" | "HEAD", "/api/v1/ping") => Some(json(
            200,
            format!(
                r#"{{"ok":true,"service":"{SERVICE}","version":"{VERSION}","implementation":"rust"}}"#
            ),
        )),
        // Other /api/* paths: leave to proxy (or 404 if no upstream).
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::RequestHead;

    fn head(host: &str, cookie: Option<&str>) -> RequestHead {
        let mut headers = vec![("Host".into(), host.into())];
        if let Some(c) = cookie {
            headers.push(("Cookie".into(), c.into()));
        }
        RequestHead {
            method: "GET".into(),
            target: "/api/v1/health".into(),
            http11: true,
            headers,
        }
    }

    #[test]
    fn health_console_host() {
        let h = head("127.0.0.1:21610", None);
        let r = try_handle("GET", "/api/v1/health", &h, true, &[]).unwrap();
        assert_eq!(r.status, 200);
        assert!(r.body.contains(r#""status":"healthy""#));
        assert!(r.body.contains(SERVICE));
        assert!(r.body.contains("rust"));
        assert!(!r.body.contains("galion-control"));
        assert!(!r.body.contains("3.8.3-board"));
    }

    #[test]
    fn health_foreign_host_phone_gate() {
        let h = head("evil.example", None);
        let r = try_handle("GET", "/api/v1/health", &h, true, &[]).unwrap();
        assert_eq!(r.status, 401);
        assert_eq!(r.body, r#"{"error":"phone required"}"#);
    }

    #[test]
    fn session_unpaired() {
        let h = head("127.0.0.1", None);
        let r = try_handle("GET", "/api/v1/session", &h, true, &[]).unwrap();
        assert_eq!(r.status, 401);
        let r2 = try_handle("GET", "/api/v1/session-pack", &h, true, &[]).unwrap();
        assert_eq!(r2.status, 401);
    }

    #[test]
    fn session_paired_not_invented() {
        let h = head("127.0.0.1", Some("galion_admin=abc"));
        let r = try_handle("GET", "/api/v1/session", &h, true, &[]).unwrap();
        assert_eq!(r.status, 501);
        assert!(r.body.contains("not implemented"));
    }

    #[test]
    fn unknown_api_not_ours() {
        let h = head("127.0.0.1", None);
        assert!(try_handle("GET", "/api/v1/catalog", &h, true, &[]).is_none());
        assert!(try_handle("GET", "/console", &h, true, &[]).is_none());
    }

    #[test]
    fn loopback_host_needs_loopback_peer() {
        let h = head("127.0.0.1", None);
        let r = try_handle("GET", "/api/v1/health", &h, false, &[]).unwrap();
        assert_eq!(r.status, 401);
    }
}
