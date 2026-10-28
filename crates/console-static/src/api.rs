// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! In-process API handlers (`--local-api`, implied by `--dist`).
//!
//! Gate (same order as the former Node srvx phone middleware, `start.ts`):
//! 1. non-console Host on `/api/*` -> 401 `{"error":"phone required"}`;
//! 2. loopback Host from a non-loopback peer -> 401 (hardening over Node);
//! 3. `/api/v1/admin` -> admin-device pairing ([`crate::admin`]);
//! 4. loopback Host -> allowed; any other console Host (cloudflared:
//!    console.galion.app, ...) needs a valid `galion_admin` cookie
//!    (`requireAdmin`), else 401. Without `--admin-store` it is always 401.
//!
//! Routes answered here: health, ping (Rust identity), the board catalog
//! ([`crate::board`]: services, domains, repos, catalog, ledger, docs,
//! protocol, protocol/snapshot, markers GET/POST, boot POST), admin, and the
//! session phone-gate. Everything else returns `None` (proxy or 404).
//! `/api/v1/health` keeps its honest Rust identity (not `galion-control`).

use crate::admin::{AdminStore, Pair};
use crate::board::Board;
use crate::http::RequestHead;
use console_ui::gate;
use console_ui::json::{self, Value};

/// Honest process identity for `/api/v1/health` (not the live Node board).
pub const SERVICE: &str = "galion-console-static";
/// Crate/semver string embedded in the health JSON.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Largest request body read for local POST routes.
pub const MAX_API_BODY: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiReply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    /// `Set-Cookie` value (admin pairing only).
    pub set_cookie: Option<String>,
}

/// Shared state the handlers need.
pub struct ApiCtx<'a> {
    pub board: &'a Board,
    pub admin: Option<&'a AdminStore>,
    pub extra_hosts: &'a [String],
    /// `--arena-root`: answer `GET /api/arena` from these files ([`crate::arena`]).
    pub arena: Option<&'a std::path::Path>,
}

/// One request as the handlers see it.
pub struct ApiRequest<'a> {
    pub method: &'a str,
    /// Sanitized path (query allowed; it is cut here).
    pub path: &'a str,
    pub head: &'a RequestHead,
    pub peer_loopback: bool,
    /// Request body (only read for [`wants_body`] routes).
    pub body: &'a [u8],
}

fn json_reply(status: u16, body: String) -> ApiReply {
    ApiReply {
        status,
        content_type: "application/json",
        body,
        set_cookie: None,
    }
}

fn phone_required() -> ApiReply {
    json_reply(401, r#"{"error":"phone required"}"#.into())
}

fn err(status: u16, msg: &str) -> ApiReply {
    let mut b = String::from(r#"{"error":"#);
    json::write_str(&mut b, msg);
    b.push('}');
    json_reply(status, b)
}

/// Path without query; already sanitized.
fn api_path(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// POST routes whose body the server must read before calling
/// [`try_handle`].
pub fn wants_body(method: &str, path: &str) -> bool {
    method == "POST"
        && matches!(
            api_path(path),
            "/api/v1/admin" | "/api/v1/markers" | "/api/v1/boot"
        )
}

/// Honest `/api/v1/health` body for a console host.
pub fn health_body() -> String {
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

fn body_json(body: &[u8]) -> Value {
    match json::parse(body) {
        Ok(v @ Value::Obj(_)) => v,
        _ => Value::Obj(Vec::new()),
    }
}

fn admin_route(req: &ApiRequest<'_>, ctx: &ApiCtx<'_>, bare_host: &str) -> ApiReply {
    let Some(store) = ctx.admin else {
        return err(503, "admin store not configured");
    };
    match req.method {
        "GET" | "HEAD" => match store.session(req.head.header("cookie")) {
            Ok((paired, you)) => json_reply(
                200,
                format!(r#"{{"ok":true,"paired":{paired},"you":{you}}}"#),
            ),
            Err(_) => err(503, "admin store unreadable"),
        },
        "POST" => {
            let b = body_json(req.body);
            let code = b.get("code").as_str().unwrap_or("");
            let device = b.get("device").as_str().unwrap_or("");
            // Only a loopback peer (cloudflared, the Rust UI) may tell us
            // the scheme.
            let https = req.peer_loopback
                && req
                    .head
                    .header("x-forwarded-proto")
                    .is_some_and(|p| p.contains("https"));
            let secure = bare_host.ends_with("galion.app") || https;
            match store.pair(code, device, secure) {
                Pair::Ok { cookie, device } => {
                    let mut body = String::from(r#"{"ok":true,"you":true,"device":"#);
                    json::write_str(&mut body, &device);
                    body.push('}');
                    let mut r = json_reply(200, body);
                    r.set_cookie = Some(cookie);
                    r
                }
                Pair::Err { status, error } => err(status, error),
            }
        }
        _ => err(405, "method not allowed"),
    }
}

/// Try to answer a request from the local API. `None` = not ours (caller may
/// proxy or 404).
pub fn try_handle(req: &ApiRequest<'_>, ctx: &ApiCtx<'_>) -> Option<ApiReply> {
    let path = api_path(req.path);
    if !path.starts_with("/api/") {
        return None;
    }
    let host = req.head.header("host").unwrap_or("");
    if !gate::is_console_host(host, ctx.extra_hosts) {
        return Some(phone_required());
    }
    let bare = gate::normalize_host(host);
    let loopback_host = gate::is_loopback_host(&bare);
    if loopback_host && !req.peer_loopback {
        return Some(phone_required());
    }
    if path == "/api/v1/admin" {
        return Some(admin_route(req, ctx, &bare));
    }
    let cookie = req.head.header("cookie");
    let is_admin = || ctx.admin.is_some_and(|a| a.is_admin(cookie));
    if !loopback_host && !is_admin() {
        return Some(phone_required());
    }
    if let (Some(root), "/api/arena") = (ctx.arena, path) {
        return Some(match req.method {
            "GET" | "HEAD" => {
                let (status, body) = crate::arena::reply(root);
                json_reply(status, body)
            }
            _ => json_reply(405, crate::arena::HELD.into()),
        });
    }
    let b = ctx.board;
    let ok = |v: Value| Some(json_reply(200, v.to_json()));
    match (req.method, path) {
        ("GET" | "HEAD", "/api/v1/health") => Some(json_reply(200, health_body())),
        ("GET" | "HEAD", "/api/v1/ping") => Some(json_reply(
            200,
            format!(
                r#"{{"ok":true,"service":"{SERVICE}","version":"{VERSION}","implementation":"rust"}}"#
            ),
        )),
        ("GET" | "HEAD", "/api/v1/services") => ok(b.services()),
        ("GET" | "HEAD", "/api/v1/domains") => ok(b.domains()),
        ("GET" | "HEAD", "/api/v1/repos") => ok(b.repos()),
        ("GET" | "HEAD", "/api/v1/catalog") => ok(b.catalog()),
        ("GET" | "HEAD", "/api/v1/ledger") => ok(b.ledger()),
        ("GET" | "HEAD", "/api/v1/docs") => ok(b.docs()),
        ("GET" | "HEAD", "/api/v1/protocol") => ok(b.protocol()),
        ("GET" | "HEAD", "/api/v1/protocol/snapshot") => {
            let health = json::parse(health_body().as_bytes()).unwrap_or(Value::Null);
            ok(b.snapshot(health))
        }
        ("GET" | "HEAD", "/api/v1/markers") => ok(b.markers()),
        ("POST", "/api/v1/markers") => {
            let m = b.emit(&body_json(req.body));
            Some(json_reply(201, Board::marker_json(&m)))
        }
        ("POST", "/api/v1/boot") => ok(b.boot()),
        ("GET" | "HEAD", "/api/v1/session" | "/api/v1/session-pack") => {
            // requireAdmin applies on loopback too (as in Node). Session
            // documents themselves are not ported yet.
            if is_admin() {
                Some(json_reply(
                    501,
                    r#"{"ok":false,"error":"session API not implemented in console-static --local-api; set --upstream"}"#.into(),
                ))
            } else {
                Some(phone_required())
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::{sha_hex, AdminStore};
    use std::path::PathBuf;

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

    fn call(
        ctx: &ApiCtx<'_>,
        method: &str,
        path: &str,
        h: &RequestHead,
        peer_loopback: bool,
        body: &[u8],
    ) -> Option<ApiReply> {
        try_handle(
            &ApiRequest {
                method,
                path,
                head: h,
                peer_loopback,
                body,
            },
            ctx,
        )
    }

    fn get(h: &RequestHead, path: &str, loopback: bool) -> Option<ApiReply> {
        let board = Board::new();
        let ctx = ApiCtx {
            board: &board,
            admin: None,
            extra_hosts: &[],
            arena: None,
        };
        call(&ctx, "GET", path, h, loopback, b"")
    }

    #[test]
    fn health_console_host() {
        let h = head("127.0.0.1:21730", None);
        let r = get(&h, "/api/v1/health", true).unwrap();
        assert_eq!(r.status, 200);
        assert!(r.body.contains(r#""status":"healthy""#));
        assert!(r.body.contains(SERVICE));
        assert!(!r.body.contains("galion-control"));
        assert!(!r.body.contains("3.8.3-board"));
    }

    #[test]
    fn health_foreign_host_phone_gate() {
        let h = head("evil.example", None);
        let r = get(&h, "/api/v1/health", true).unwrap();
        assert_eq!(r.status, 401);
        assert_eq!(r.body, r#"{"error":"phone required"}"#);
    }

    #[test]
    fn remote_console_host_needs_admin() {
        let h = head("console.galion.app", None);
        for p in ["/api/v1/health", "/api/v1/services", "/api/v1/ledger"] {
            assert_eq!(get(&h, p, true).unwrap().status, 401, "{p}");
        }
        // admin without store configured -> 503 (UI shows "unavailable").
        assert_eq!(get(&h, "/api/v1/admin", true).unwrap().status, 503);
    }

    #[test]
    fn session_unpaired_and_cookie_not_trusted_without_store() {
        let h = head("127.0.0.1", None);
        assert_eq!(get(&h, "/api/v1/session", true).unwrap().status, 401);
        assert_eq!(get(&h, "/api/v1/session-pack", true).unwrap().status, 401);
        let h = head("127.0.0.1", Some("galion_admin=abc"));
        assert_eq!(get(&h, "/api/v1/session", true).unwrap().status, 401);
    }

    #[test]
    fn catalog_routes_on_loopback() {
        let h = head("127.0.0.1", None);
        for p in [
            "/api/v1/services",
            "/api/v1/domains",
            "/api/v1/repos",
            "/api/v1/catalog",
            "/api/v1/ledger",
            "/api/v1/docs",
            "/api/v1/protocol",
            "/api/v1/protocol/snapshot",
            "/api/v1/markers",
            "/api/v1/services?x=1",
        ] {
            let r = get(&h, p, true).unwrap();
            assert_eq!(r.status, 200, "{p}");
            assert!(json::parse(r.body.as_bytes()).is_ok(), "{p}");
        }
    }

    #[test]
    fn unknown_api_not_ours() {
        let h = head("127.0.0.1", None);
        assert!(get(&h, "/api/v1/keys", true).is_none());
        assert!(get(&h, "/api/hermes/health", true).is_none());
        assert!(get(&h, "/console", true).is_none());
    }

    #[test]
    fn loopback_host_needs_loopback_peer() {
        let h = head("127.0.0.1", None);
        assert_eq!(get(&h, "/api/v1/health", false).unwrap().status, 401);
        assert_eq!(get(&h, "/api/v1/services", false).unwrap().status, 401);
    }

    #[test]
    fn markers_post_and_boot() {
        let board = Board::new();
        let ctx = ApiCtx {
            board: &board,
            admin: None,
            extra_hosts: &[],
            arena: None,
        };
        let h = head("localhost", None);
        let r = call(
            &ctx,
            "POST",
            "/api/v1/markers",
            &h,
            true,
            br#"{"kind":"dns","source":"slot303","note":"probe"}"#,
        )
        .unwrap();
        assert_eq!(r.status, 201);
        let v = json::parse(r.body.as_bytes()).unwrap();
        assert_eq!(v.get("seq").as_f64(), Some(42.0));
        let g = call(&ctx, "GET", "/api/v1/markers", &h, true, b"").unwrap();
        assert!(g.body.contains("slot303"));
        let r = call(&ctx, "POST", "/api/v1/markers", &h, true, b"garbage").unwrap();
        assert_eq!(r.status, 201);
        let b = call(&ctx, "POST", "/api/v1/boot", &h, true, b"").unwrap();
        assert_eq!(b.status, 200);
        assert!(b.body.starts_with(r#"{"ok":true,"mode":"light""#));
        assert!(call(&ctx, "POST", "/api/v1/services", &h, true, b"").is_none());
        assert!(wants_body("POST", "/api/v1/admin"));
        assert!(!wants_body("GET", "/api/v1/admin"));
    }

    #[test]
    fn arena_route_local_reader_and_gate() {
        let dir = std::env::temp_dir().join(format!("console-api-arena-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("board.jsonl"), "{\"id\":\"g1-0\"}\n").unwrap();
        let board = Board::new();
        let ctx = ApiCtx {
            board: &board,
            admin: None,
            extra_hosts: &[],
            arena: Some(&dir),
        };
        let h = head("127.0.0.1", None);
        let r = call(&ctx, "GET", "/api/arena", &h, true, b"").unwrap();
        assert_eq!(r.status, 200);
        let v = json::parse(r.body.as_bytes()).unwrap();
        assert_eq!(v.get("ok").as_bool(), Some(true));
        assert_eq!(v.get("rows").as_array().len(), 1);
        assert_eq!(
            call(&ctx, "GET", "/api/arena?x=1", &h, true, b"")
                .unwrap()
                .status,
            200
        );
        for m in ["POST", "PUT", "DELETE"] {
            let r = call(
                &ctx,
                m,
                "/api/arena",
                &h,
                true,
                br#"{"action":"generation"}"#,
            )
            .unwrap();
            assert_eq!(r.status, 405, "{m}");
        }
        // Gate still first: foreign host, remote peer, unpaired remote host.
        assert_eq!(
            call(
                &ctx,
                "GET",
                "/api/arena",
                &head("evil.example", None),
                true,
                b""
            )
            .unwrap()
            .status,
            401
        );
        assert_eq!(
            call(&ctx, "GET", "/api/arena", &h, false, b"")
                .unwrap()
                .status,
            401
        );
        assert_eq!(
            call(
                &ctx,
                "GET",
                "/api/arena",
                &head("console.galion.app", None),
                true,
                b""
            )
            .unwrap()
            .status,
            401
        );
        // No arena root: not ours (404 or data upstream), bank never local.
        assert!(get(&h, "/api/arena", true).is_none());
        assert!(call(&ctx, "GET", "/api/v1/bank", &h, true, b"").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn admin_pairing_unlocks_remote_console_host() {
        let dir = std::env::temp_dir().join(format!("console-api-admin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path: PathBuf = dir.join("admin-device.json");
        std::fs::write(&path, format!("{{\"codeHash\":\"{}\"}}", sha_hex("PAIR42"))).unwrap();
        let store = AdminStore::new(path, None);
        let board = Board::new();
        let ctx = ApiCtx {
            board: &board,
            admin: Some(&store),
            extra_hosts: &[],
            arena: None,
        };
        let h = head("console.galion.app", None);
        let r = call(&ctx, "GET", "/api/v1/admin", &h, true, b"").unwrap();
        assert_eq!(r.body, r#"{"ok":true,"paired":false,"you":false}"#);
        let bad = call(
            &ctx,
            "POST",
            "/api/v1/admin",
            &h,
            true,
            br#"{"code":"nope"}"#,
        )
        .unwrap();
        assert_eq!(bad.status, 401);
        assert_eq!(bad.body, r#"{"error":"wrong code"}"#);
        let r = call(
            &ctx,
            "POST",
            "/api/v1/admin",
            &h,
            true,
            br#"{"code":"pair42"}"#,
        )
        .unwrap();
        assert_eq!(r.status, 200);
        let cookie = r.set_cookie.clone().unwrap();
        assert!(cookie.ends_with("; Secure"), "galion.app host -> Secure");
        let v = json::parse(r.body.as_bytes()).unwrap();
        assert_eq!(v.get("you").as_bool(), Some(true));
        assert_eq!(v.get("device").as_str().unwrap().len(), 43);
        let pair = cookie.split(';').next().unwrap().to_string();
        let h2 = head("console.galion.app", Some(&pair));
        let r = call(&ctx, "GET", "/api/v1/admin", &h2, true, b"").unwrap();
        assert_eq!(r.body, r#"{"ok":true,"paired":true,"you":true}"#);
        assert_eq!(
            call(&ctx, "GET", "/api/v1/services", &h2, true, b"")
                .unwrap()
                .status,
            200
        );
        assert_eq!(
            call(&ctx, "GET", "/api/v1/session", &h2, true, b"")
                .unwrap()
                .status,
            501
        );
        // Foreign host never reaches admin.
        let f = head("galion.app", Some(&pair));
        assert_eq!(
            call(&ctx, "GET", "/api/v1/admin", &f, true, b"")
                .unwrap()
                .status,
            401
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
