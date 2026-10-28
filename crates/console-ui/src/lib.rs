// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Server-rendered Rust UI for the Galion console.
//!
//! Replaces the React/TanStack/Tailwind browser UI page by page with HTML
//! rendered on the server from the console API, a stylesheet generated from
//! Rust ([`css`]) and **no JavaScript**: menus are `<details>`, actions are
//! `<form method="post">`, the theme is a cookie.
//!
//! [`Ui::handle`] is transport-free: the HTTP front (`console-static`) parses
//! the request and writes the [`Reply`]. Data comes from the API upstream
//! (`/api/v1/*`, today the Node console moved to 8088) and the desk pulse
//! (`127.0.0.1:8091/api/v1/awake`) through [`client`].
//!
//! std only, no dependencies, `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

pub mod client;
pub mod css;
pub mod gate;
pub mod html;
pub mod icons;
pub mod json;
pub mod layout;
pub mod overview;
pub mod pages;
pub mod port2;
pub mod port3;
pub mod port4;
pub mod port5;
pub mod rooms;

use client::Forward;
use gate::Decision;
use json::Value;
use layout::{Frame, Origin};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Pages rendered in Rust, in port order.
pub const PORTED: &[&str] = &[
    "/status",
    "/services",
    "/agents",
    "/domains",
    "/repos",
    "/docs",
    "/cloudflare",
    "/pack",
    "/chill",
    "/jupyter",
    "/data",
    "/soon",
    "/console",
    // console-port-1 (wave 4 slot 135): the first fifth, alphabetical, of the
    // 17 routes still in TypeScript at 669dc18.
    "/analytics",
    "/arena",
    "/bank",
    "/cache",
    // console-port-2 (wave 4 slot 136): 2nd fifth, alphabetical, of the 17
    // routes still in TypeScript at 669dc18.
    "/context",
    "/gateway",
    "/guardian",
    "/hermes",
    // console-port-3 (wave 4 slot 137): 3rd fifth, alphabetical, of the 17
    // routes still in TypeScript at 669dc18.
    "/instances",
    "/keys",
    "/markers",
    // console-port-4 (wave 4 slot 138): 4th fifth, alphabetical, of the 17
    // routes still in TypeScript at 669dc18 (`/md/user` stands for `/md/$name`).
    "/md/user",
    "/memory",
    "/mesh",
    // console-port-5 (wave 4 slot 139): last fifth, alphabetical, of the 17
    // routes still in TypeScript at 669dc18.
    "/node",
    "/security",
    "/session",
];

/// Ported pages the pre-built SPA keeps while a fallback is configured.
/// Empty since wave 4 slot 299: every ported room is Rust-first. `/hermes`
/// went Rust-first in slot 254; `/arena` and `/bank` followed in slot 299
/// (console V2 R2): `/arena` is a read-only board (Breed/Stop/Promote stay
/// held until Slavik approves the spend) and `/bank` renders the `BankSnap`
/// from `/api/v1/bank` or explains why there is none. Neither page needs
/// browser JS, so the SPA keeps no route. Kept as a (now empty) list so a
/// future room can opt back in without an API change.
pub const SPA_FIRST: &[&str] = &[];

/// Largest form body accepted (pairing code or a command name).
pub const MAX_FORM: usize = 4096;

/// How long a shared /api/v1/health answer is reused for the header
/// origin-dot (and for rooms that only need that payload). Single-flight
/// under [`Ui::cached_health`].
pub const HEALTH_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct UiConfig {
    /// Console API (loopback).
    pub upstream: SocketAddr,
    /// Desk pulse (`/api/v1/awake`), loopback; `None` disables it.
    pub pulse: Option<SocketAddr>,
    /// Extra hosts treated as console hosts (the machine's public IP).
    pub extra_hosts: Vec<String>,
    /// galion-desk (`GET /desk`, live on 127.0.0.1:8646), loopback; `None`
    /// leaves `/cache` on its empty state and shows `/context` as "not
    /// answering".
    pub desk: Option<SocketAddr>,
    /// Host files read by `/memory` and `/md/*` (console-port-4).
    pub files: port4::HostFiles,
}

/// One request, already parsed and path-sanitised by the front.
#[derive(Debug, Clone, Default)]
pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub host: &'a str,
    pub cookie: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub sec_fetch_site: Option<&'a str>,
    pub forwarded_proto: Option<&'a str>,
    pub peer_loopback: bool,
    pub client_ip: String,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// Extra headers (Content-Type and Cache-Control always present).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    fn html(status: u16, body: String) -> Reply {
        Reply {
            status,
            headers: vec![
                ("Content-Type".into(), "text/html; charset=utf-8".into()),
                ("Cache-Control".into(), "no-store".into()),
                (
                    "Content-Security-Policy".into(),
                    "default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self'; frame-ancestors 'none'; base-uri 'none'".into(),
                ),
                ("X-Frame-Options".into(), "DENY".into()),
            ],
            body: body.into_bytes(),
        }
    }

    fn redirect(status: u16, to: &str) -> Reply {
        Reply {
            status,
            headers: vec![
                ("Location".into(), to.into()),
                ("Content-Type".into(), "text/plain; charset=utf-8".into()),
                ("Cache-Control".into(), "no-store".into()),
            ],
            body: Vec::new(),
        }
    }

    fn text(status: u16, msg: &str) -> Reply {
        Reply {
            status,
            headers: vec![
                ("Content-Type".into(), "text/plain; charset=utf-8".into()),
                ("Cache-Control".into(), "no-store".into()),
            ],
            body: msg.as_bytes().to_vec(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub struct Ui {
    cfg: UiConfig,
    css: String,
    css_href: String,
    css_etag: String,
    overview_css: String,
    overview_css_href: String,
    overview_css_etag: String,
    sprite_href: String,
    sprite_etag: String,
    sprite: String,
    /// Shared /api/v1/health payload + stamp; see [`HEALTH_CACHE_TTL`].
    health: Mutex<HealthSlot>,
}

struct HealthSlot {
    at: Option<Instant>,
    value: Option<Value>,
}

/// The TanStack console answers a path it has no route for with
/// `500 {"error":"Only HTML requests are supported here"}` when the request
/// asks for JSON (live 2026-10-21); that is a 404 for our purposes.
fn no_route(status: u16, body: &[u8]) -> bool {
    status == 404
        || (status == 500
            && String::from_utf8_lossy(body).contains("Only HTML requests are supported here"))
}

/// `back=` targets must be one of our own page paths (no open redirect).
fn safe_back(back: &str) -> &str {
    let known = PORTED.contains(&back)
        || rooms::ROOMS.iter().any(|r| r.to == Some(back))
        || back == "/console";
    if known {
        back
    } else {
        "/console"
    }
}

impl Ui {
    pub fn new(cfg: UiConfig) -> Ui {
        let css = css::sheet();
        let h = css::fnv64(css.as_bytes());
        let overview_css = css::overview_sheet();
        let oh = css::fnv64(overview_css.as_bytes());
        Ui {
            cfg,
            css_href: format!("/_ui/console.{h:016x}.css"),
            css_etag: format!("\"{h:016x}\""),
            css,
            overview_css_href: format!("/_ui/overview.{oh:016x}.css"),
            overview_css_etag: format!("\"{oh:016x}\""),
            overview_css,
            sprite_href: icons::sprite_href().to_string(),
            sprite_etag: icons::sprite_etag().to_string(),
            sprite: icons::sprite_body().to_string(),
            health: Mutex::new(HealthSlot {
                at: None,
                value: None,
            }),
        }
    }

    pub fn css_href(&self) -> &str {
        &self.css_href
    }

    /// Whether this UI answers `path`. With an SPA fallback configured only
    /// the ported pages and `/_ui/*` are ours; without one, every console
    /// room is (unported rooms get a "not in Rust yet" page).
    pub fn owns(&self, path: &str, spa_fallback: bool) -> bool {
        path.starts_with("/_ui/")
            || (PORTED.contains(&path) && !(spa_fallback && SPA_FIRST.contains(&path)))
            || port4::owns(path)
            || path == "/"
            || (!spa_fallback && (rooms::exact(path).is_some() || path == "/soon"))
    }

    fn forward(&self, req: &Request<'_>) -> Forward {
        Forward {
            host: req.host.to_string(),
            cookie: gate::admin_cookie(req.cookie),
            forwarded_for: Some(req.client_ip.clone()).filter(|s| !s.is_empty()),
            forwarded_proto: req.forwarded_proto.map(str::to_string),
        }
    }

    fn get_json(&self, addr: SocketAddr, path: &str, fwd: &Forward) -> Option<Value> {
        let r = client::call(addr, "GET", path, fwd, None).ok()?;
        if r.status != 200 {
            return None;
        }
        json::parse(&r.body).ok()
    }

    fn frame<'a>(&'a self, req: &'a Request<'a>, title: &'a str) -> Frame<'a> {
        Frame {
            title,
            css_href: &self.css_href,
            extra_css_href: None,
            light: match gate::cookie(req.cookie, "galion_theme").as_deref() {
                Some("light") => Some(true),
                Some("dark") => Some(false),
                _ => None,
            },
            path: req.path,
            refresh_s: None,
        }
    }

    /// Same-origin check for state-changing form posts.
    fn same_origin(req: &Request<'_>) -> bool {
        match req.sec_fetch_site {
            Some("same-origin") => return true,
            Some(_) => return false,
            None => {}
        }
        match req.origin {
            Some(o) => {
                let host = o.split("://").nth(1).unwrap_or("");
                !host.is_empty() && host.eq_ignore_ascii_case(req.host)
            }
            None => false,
        }
    }

    /// Run the page gate. `Ok(())` means render; `Err(reply)` is the answer.
    fn gate(&self, req: &Request<'_>) -> Result<(), Reply> {
        match gate::decide(req.host, req.path, req.peer_loopback, &self.cfg.extra_hosts) {
            Decision::Allow => Ok(()),
            Decision::RedirectConsole => Err(Reply::redirect(302, "/console")),
            Decision::Soon { links } => {
                let f = self.frame(req, if links { "Galion developer" } else { "Soon" });
                let body = if links {
                    pages::gate::links()
                } else {
                    pages::gate::soon()
                };
                Err(Reply::html(200, layout::document(&f, "", &body)))
            }
            Decision::AskUpstream => {
                let fwd = self.forward(req);
                match client::call(self.cfg.upstream, "GET", "/api/v1/admin", &fwd, None) {
                    Ok(r) if r.status == 200 => {
                        let v = json::parse(&r.body).unwrap_or(Value::Null);
                        if v.get("you").as_bool() == Some(true) {
                            Ok(())
                        } else if let Some(dev) = gate::cookie(req.cookie, "galion_device")
                            .filter(|d| gate::device_token_ok(d))
                        {
                            // Android admin resume (SPA used localStorage + JS).
                            // Server-side POST keeps zero-JS and SameSite cookies.
                            match self.resume_device(req, &dev) {
                                Ok(reply) => Err(reply),
                                Err(clear) => {
                                    let f = self.frame(req, "Admin");
                                    let mut out = Reply::html(
                                        200,
                                        layout::document(
                                            &f,
                                            "",
                                            &pages::gate::pair(safe_back(req.path), None),
                                        ),
                                    );
                                    if clear {
                                        out.headers.push((
                                            "Set-Cookie".into(),
                                            "galion_device=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0"
                                                .into(),
                                        ));
                                    }
                                    Err(out)
                                }
                            }
                        } else {
                            let f = self.frame(req, "Admin");
                            Err(Reply::html(
                                200,
                                layout::document(
                                    &f,
                                    "",
                                    &pages::gate::pair(safe_back(req.path), None),
                                ),
                            ))
                        }
                    }
                    _ => {
                        let f = self.frame(req, "Admin");
                        Err(Reply::html(
                            503,
                            layout::document(&f, "", &pages::gate::unavailable()),
                        ))
                    }
                }
            }
        }
    }

    pub fn handle(&self, req: &Request<'_>) -> Reply {
        match req.path {
            p if p == self.css_href => return self.stylesheet(req, false),
            p if p == self.overview_css_href => return self.stylesheet(req, true),
            p if p == self.sprite_href => return self.sprite(req),
            "/_ui/favicon.svg" => {
                return Reply {
                    status: 200,
                    headers: vec![
                        ("Content-Type".into(), "image/svg+xml".into()),
                        ("Cache-Control".into(), "public, max-age=86400".into()),
                    ],
                    body: icons::mark("")
                        .replace(
                            "<svg class=\"\"",
                            "<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"#d4d4d4\"",
                        )
                        .into_bytes(),
                }
            }
            "/_ui/theme" => return self.theme(req),
            "/_ui/pair" => return self.pair(req),
            p if p.starts_with("/_ui/") => return Reply::text(404, "Not Found\n"),
            _ => {}
        }
        let post_ok = (req.path == "/agents"
            || port2::POST_ROUTES.contains(&req.path)
            || port3::POST_ROUTES.contains(&req.path)
            || port5::POST_ROUTES.contains(&req.path))
            && req.method == "POST";
        if req.method != "GET" && req.method != "HEAD" && !post_ok {
            let mut r = Reply::text(405, "Method Not Allowed\n");
            r.headers.push(("Allow".into(), "GET, HEAD".into()));
            return r;
        }
        if let Err(r) = self.gate(req) {
            return r;
        }
        match req.path {
            "/status" => self.status(req),
            "/services" => self.services(req),
            "/agents" => self.agents(req),
            "/console" => self.landing(req),
            "/soon" => {
                let f = self.frame(req, "Soon");
                let host = gate::normalize_host(req.host);
                Reply::html(
                    200,
                    layout::document(&f, "", &pages::gate::coming_soon(&host)),
                )
            }
            "/domains" | "/repos" | "/docs" | "/cloudflare" | "/pack" | "/chill" | "/jupyter"
            | "/data" => self.read_room(req),
            "/analytics" | "/arena" | "/bank" | "/cache" => self.port1_room(req),
            "/context" | "/gateway" | "/guardian" | "/hermes" => self.port2_room(req),
            "/instances" | "/keys" | "/markers" => self.port3_room(req),
            "/memory" | "/mesh" => self.port4_room(req),
            p if port4::owns(p) => self.port4_room(req),
            "/node" | "/security" | "/session" => self.port5_room(req),
            p => match rooms::exact(p) {
                Some(room) => {
                    let fwd = self.forward(req);
                    let origin = self.origin(&fwd);
                    let f = self.frame(req, "Galion Console");
                    Reply::html(
                        200,
                        layout::console(&f, origin, &pages::not_ported::body(room, PORTED)),
                    )
                }
                None => Reply::text(404, "Not Found\n"),
            },
        }
    }

    fn stylesheet(&self, req: &Request<'_>, overview: bool) -> Reply {
        let _ = req;
        let (etag, body) = if overview {
            (&self.overview_css_etag, self.overview_css.as_bytes())
        } else {
            (&self.css_etag, self.css.as_bytes())
        };
        Reply {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "text/css; charset=utf-8".into()),
                (
                    "Cache-Control".into(),
                    "public, max-age=31536000, immutable".into(),
                ),
                ("ETag".into(), etag.clone()),
            ],
            body: body.to_vec(),
        }
    }

    fn sprite(&self, req: &Request<'_>) -> Reply {
        let _ = req;
        Reply {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "image/svg+xml".into()),
                (
                    "Cache-Control".into(),
                    "public, max-age=31536000, immutable".into(),
                ),
                ("ETag".into(), self.sprite_etag.clone()),
            ],
            body: self.sprite.as_bytes().to_vec(),
        }
    }

    fn theme(&self, req: &Request<'_>) -> Reply {
        let q = html::form_pairs(req.query);
        let to = match html::form_get(&q, "to") {
            Some("light") => "light",
            _ => "dark",
        };
        let back = safe_back(html::form_get(&q, "back").unwrap_or("/console"));
        let mut r = Reply::redirect(303, back);
        r.headers.push((
            "Set-Cookie".into(),
            format!("galion_theme={to}; Path=/; Max-Age=31536000; SameSite=Lax"),
        ));
        r
    }

    /// Whether Set-Cookie should carry `Secure` (HTTPS console host or
    /// trusted loopback proxy that forwarded `https`).
    fn cookie_secure(req: &Request<'_>) -> bool {
        let host = gate::normalize_host(req.host);
        host.ends_with("galion.app")
            || host.ends_with("galion.studio")
            || req
                .forwarded_proto
                .map(|p| p.split(',').any(|x| x.trim() == "https"))
                .unwrap_or(false)
    }

    fn device_set_cookie(req: &Request<'_>, device: &str) -> String {
        let mut c =
            format!("galion_device={device}; HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000");
        if Self::cookie_secure(req) {
            c.push_str("; Secure");
        }
        c
    }

    /// POST `{device}` to `/api/v1/admin`. `Ok` = redirect with admin cookie.
    /// `Err(true)` = clear a refused device cookie; `Err(false)` = soft miss.
    fn resume_device(&self, req: &Request<'_>, device: &str) -> Result<Reply, bool> {
        let back = safe_back(req.path).to_string();
        let body = json::obj(&[("device", Value::Str(device.to_string()))]).to_json();
        let mut fwd = self.forward(req);
        fwd.cookie = None;
        match client::call(
            self.cfg.upstream,
            "POST",
            "/api/v1/admin",
            &fwd,
            Some(("application/json", body.as_bytes())),
        ) {
            Ok(r) if r.status == 200 => {
                let mut out = Reply::redirect(303, &back);
                for c in r.header_all("set-cookie") {
                    if c.starts_with("galion_admin=") {
                        out.headers.push(("Set-Cookie".into(), c.to_string()));
                    }
                }
                Ok(out)
            }
            Ok(r) if r.status == 401 || r.status == 429 => Err(true),
            _ => Err(false),
        }
    }

    fn pair(&self, req: &Request<'_>) -> Reply {
        let q = html::form_pairs(req.query);
        let back = safe_back(html::form_get(&q, "back").unwrap_or("/console")).to_string();
        if req.method != "POST" {
            return Reply::redirect(303, &back);
        }
        if !Self::same_origin(req) {
            return Reply::text(403, "Forbidden\n");
        }
        if !gate::is_console_host(req.host, &self.cfg.extra_hosts) {
            return Reply::text(404, "Not Found\n");
        }
        let form = html::form_pairs(&String::from_utf8_lossy(req.body));
        let code = html::form_get(&form, "code")
            .unwrap_or("")
            .trim()
            .to_string();
        let device = html::form_get(&form, "device")
            .unwrap_or("")
            .trim()
            .to_string();
        let f = self.frame(req, "Admin");
        let mut fields: Vec<(&str, Value)> = Vec::new();
        if !code.is_empty() {
            fields.push(("code", Value::Str(code)));
        }
        if gate::device_token_ok(&device) {
            fields.push(("device", Value::Str(device)));
        }
        if fields.is_empty() {
            return Reply::html(
                200,
                layout::document(&f, "", &pages::gate::pair(&back, Some("Enter the code."))),
            );
        }
        let body = json::obj(&fields).to_json();
        let mut fwd = self.forward(req);
        fwd.cookie = None;
        match client::call(
            self.cfg.upstream,
            "POST",
            "/api/v1/admin",
            &fwd,
            Some(("application/json", body.as_bytes())),
        ) {
            Ok(r) if r.status == 200 => {
                let mut out = Reply::redirect(303, &back);
                for c in r.header_all("set-cookie") {
                    if c.starts_with("galion_admin=") {
                        out.headers.push(("Set-Cookie".into(), c.to_string()));
                    }
                }
                // First pair returns a one-time device secret; keep it as an
                // HttpOnly cookie so Android can resume without JavaScript.
                if let Ok(v) = json::parse(&r.body) {
                    if let Some(dev) = v
                        .get("device")
                        .as_str()
                        .filter(|d| gate::device_token_ok(d))
                    {
                        out.headers
                            .push(("Set-Cookie".into(), Self::device_set_cookie(req, dev)));
                    }
                }
                out
            }
            Ok(r) if r.status == 429 => Reply::html(
                429,
                layout::document(
                    &f,
                    "",
                    &pages::gate::pair(&back, Some("Too many attempts. Wait 15 minutes.")),
                ),
            ),
            Ok(_) => Reply::html(
                401,
                layout::document(
                    &f,
                    "",
                    &pages::gate::pair(&back, Some("That code was refused.")),
                ),
            ),
            Err(_) => Reply::html(503, layout::document(&f, "", &pages::gate::unavailable())),
        }
    }

    /// Shared `/api/v1/health` lookup: one upstream GET at most every
    /// [`HEALTH_CACHE_TTL`] (single-flight under the mutex) so chrome pages
    /// do not re-hit the API on every render for the header origin-dot.
    fn cached_health(&self, fwd: &Forward) -> Option<Value> {
        {
            let g = self.health.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(at) = g.at {
                if at.elapsed() < HEALTH_CACHE_TTL {
                    return g.value.clone();
                }
            }
        }
        // Fetch while holding the lock so concurrent chrome renders share one
        // upstream round-trip (single-flight).
        let mut g = self.health.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(at) = g.at {
            if at.elapsed() < HEALTH_CACHE_TTL {
                return g.value.clone();
            }
        }
        let v = self.get_json(self.cfg.upstream, "/api/v1/health", fwd);
        g.at = Some(Instant::now());
        g.value = v.clone();
        v
    }

    fn origin(&self, fwd: &Forward) -> Origin {
        match self.cached_health(fwd) {
            Some(h) if h.get("status").as_str() == Some("healthy") => Origin::Up,
            _ => Origin::Down,
        }
    }

    fn pulse(&self) -> Option<Vec<pages::Pulse>> {
        let addr = self.cfg.pulse?;
        let fwd = Forward {
            host: addr.to_string(),
            ..Forward::default()
        };
        self.get_json(addr, "/api/v1/awake", &fwd)
            .map(|v| pages::pulses_of(&v))
    }

    fn status(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let (health, services, domains, pulse) = std::thread::scope(|s| {
            let h = s.spawn(|| self.cached_health(&fwd));
            let sv = s.spawn(|| self.get_json(self.cfg.upstream, "/api/v1/services", &fwd));
            let d = s.spawn(|| self.get_json(self.cfg.upstream, "/api/v1/domains", &fwd));
            let p = s.spawn(|| self.pulse());
            (
                h.join().ok().flatten(),
                sv.join().ok().flatten(),
                d.join().ok().flatten(),
                p.join().ok().flatten(),
            )
        });
        let healthy = health.map(|h| h.get("status").as_str() == Some("healthy"));
        let svcs = services
            .as_ref()
            .map(pages::services_of)
            .unwrap_or_default();
        let body = pages::status::body(
            healthy,
            &svcs,
            domains.as_ref().unwrap_or(&Value::Null),
            &pulse.unwrap_or_default(),
        );
        let f = self.frame(req, "Galion status");
        Reply::html(200, layout::document(&f, "", &body))
    }

    fn services(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let (origin, services, pulse) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let sv = s.spawn(|| self.get_json(self.cfg.upstream, "/api/v1/services", &fwd));
            let p = s.spawn(|| self.pulse());
            (
                o.join().unwrap_or(Origin::Down),
                sv.join().ok().flatten(),
                p.join().ok().flatten(),
            )
        });
        let svcs = services
            .as_ref()
            .map(pages::services_of)
            .unwrap_or_default();
        let pulse_ok = pulse.is_some();
        let body = pages::services::body(&svcs, &pulse.unwrap_or_default(), pulse_ok);
        let f = self.frame(req, "Galion Console · Services");
        Reply::html(200, layout::console(&f, origin, &body))
    }

    /// Read-only catalog rooms (`pages::catalog`): GET the API payloads the
    /// room needs plus `/api/v1/health` (origin dot), in parallel.
    fn read_room(&self, req: &Request<'_>) -> Reply {
        let apis: &[&str] = match req.path {
            "/domains" => &["/api/v1/domains", "/api/v1/services"],
            "/repos" => &["/api/v1/repos"],
            "/docs" => &["/api/v1/catalog"],
            "/cloudflare" => &["/api/v1/cloudflare"],
            "/pack" => &["/api/v1/pack"],
            "/chill" => &["/api/v1/ledger"],
            _ => &[],
        };
        let fwd = self.forward(req);
        let (health, got) = std::thread::scope(|s| {
            let h = s.spawn(|| self.cached_health(&fwd));
            let hs: Vec<_> = apis
                .iter()
                .map(|p| s.spawn(|| self.get_json(self.cfg.upstream, p, &fwd)))
                .collect();
            let got: Vec<Option<Value>> = hs.into_iter().map(|j| j.join().ok().flatten()).collect();
            (h.join().ok().flatten(), got)
        });
        let origin = match &health {
            Some(h) if h.get("status").as_str() == Some("healthy") => Origin::Up,
            _ => Origin::Down,
        };
        let a = got.first().and_then(Option::as_ref);
        let b = got.get(1).and_then(Option::as_ref);
        let body = match req.path {
            "/domains" => pages::catalog::domains(a, b),
            "/repos" => pages::catalog::repos(a),
            "/docs" => {
                let q = html::form_pairs(req.query);
                pages::catalog::docs(a, html::form_get(&q, "q").unwrap_or(""))
            }
            "/cloudflare" => pages::catalog::cloudflare(a, health.as_ref()),
            "/pack" => pages::catalog::pack(a),
            "/chill" => pages::catalog::chill(a),
            "/jupyter" => pages::catalog::jupyter(health.as_ref()),
            _ => pages::catalog::data(),
        };
        let title = format!(
            "Galion Console · {}",
            rooms::exact(req.path).map(|r| r.label).unwrap_or("Console")
        );
        let f = self.frame(req, &title);
        Reply::html(200, layout::console(&f, origin, &body))
    }

    /// The Desk landing: the whole-project overview ([`overview`]). Only the
    /// header dot needs the API; the overview itself is static facts.
    fn landing(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let origin = self.origin(&fwd);
        let body = pages::overview::body(PORTED.len() + 1);
        let mut f = self.frame(req, "Galion Studio · Project overview");
        f.extra_css_href = Some(&self.overview_css_href);
        Reply::html(200, layout::console(&f, origin, &body))
    }

    /// GET JSON with the reason when it fails: `Err(Some(status))` for a
    /// non-200 answer, `Err(None)` for no answer or a body that is not JSON.
    fn get_json_why(
        &self,
        addr: SocketAddr,
        path: &str,
        fwd: &Forward,
    ) -> Result<Value, Option<u16>> {
        let r = client::call(addr, "GET", path, fwd, None).map_err(|_| None)?;
        if r.status != 200 {
            return Err(Some(if no_route(r.status, &r.body) {
                404
            } else {
                r.status
            }));
        }
        json::parse(&r.body).map_err(|_| None)
    }

    /// galion-desk `GET /desk` (the `/cache` room's data).
    fn desk(&self) -> Option<Value> {
        let addr = self.cfg.desk?;
        let fwd = Forward {
            host: addr.to_string(),
            ..Forward::default()
        };
        self.get_json(addr, "/desk", &fwd)
    }

    /// console-port-1 rooms: `/analytics`, `/arena`, `/bank`, `/cache`
    /// (pages `analytics`, `arena`, `bank`, `cache`). GET only; the origin dot
    /// and the room's one data source are read in parallel.
    fn port1_room(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let up = self.cfg.upstream;
        let api = match req.path {
            "/analytics" => Some("/api/v1/analytics"),
            "/arena" => Some("/api/arena"),
            "/bank" => Some("/api/v1/bank"),
            _ => None,
        };
        let (origin, got, desk, surv) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let g = s.spawn(|| api.map(|p| self.get_json_why(up, p, &fwd)));
            // Survival card (slot 408): the staged console-static status
            // route, read in parallel with the bank snapshot.
            let sv = s.spawn(|| {
                (req.path == "/bank").then(|| self.get_json_why(up, "/api/v1/survival", &fwd))
            });
            let d = s.spawn(|| {
                if req.path == "/cache" {
                    self.desk()
                } else {
                    None
                }
            });
            (
                o.join().unwrap_or(Origin::Down),
                g.join().ok().flatten(),
                d.join().ok().flatten(),
                sv.join().ok().flatten(),
            )
        });
        let got = got.unwrap_or(Err(None));
        let body = match req.path {
            "/analytics" => {
                let q = html::form_pairs(req.query);
                let board = html::form_get(&q, "board").unwrap_or("");
                let miss = match &got {
                    Err(Some(c)) => pages::analytics::Miss::Status(*c),
                    _ => pages::analytics::Miss::Down,
                };
                pages::analytics::body(got.as_ref().map_err(|_| miss), board)
            }
            "/arena" => {
                let miss = match &got {
                    Err(Some(c)) => pages::arena::Miss::Status(*c),
                    _ => pages::arena::Miss::Down,
                };
                pages::arena::body(got.as_ref().map_err(|_| miss))
            }
            "/bank" => {
                let miss = match &got {
                    Err(Some(c)) => pages::bank::Miss::Status(*c),
                    _ => pages::bank::Miss::Down,
                };
                let card = match surv.as_ref() {
                    Some(Ok(v)) => pages::survival::card(Ok(v)),
                    Some(Err(Some(c))) => {
                        pages::survival::card(Err(pages::survival::Miss::Status(*c)))
                    }
                    _ => pages::survival::card(Err(pages::survival::Miss::Down)),
                };
                pages::bank::body_with(got.as_ref().map_err(|_| miss), &card)
            }
            _ => pages::cache::body(desk.as_ref()),
        };
        let title = format!(
            "Galion Console · {}",
            rooms::exact(req.path).map(|r| r.label).unwrap_or("Console")
        );
        let mut f = self.frame(req, &title);
        if req.path == "/bank" {
            f.refresh_s = Some(pages::bank::refresh_s(got.as_ref().ok()));
        }
        Reply::html(200, layout::console(&f, origin, &body))
    }

    fn agents(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let mut note = String::new();
        let mut status = 200;
        if req.method == "POST" {
            if !Self::same_origin(req) {
                return Reply::text(403, "Forbidden\n");
            }
            let form = html::form_pairs(&String::from_utf8_lossy(req.body));
            let cmd = html::form_get(&form, "cmd").unwrap_or("");
            if !pages::agents::allowed(cmd) {
                status = 400;
                note = "unknown command".into();
            } else {
                let body = json::obj(&[("cmd", Value::Str(cmd.to_string()))]).to_json();
                note = match client::call(
                    self.cfg.upstream,
                    "POST",
                    "/api/v1/command",
                    &fwd,
                    Some(("application/json", body.as_bytes())),
                ) {
                    Ok(r) => match json::parse(&r.body) {
                        Ok(v) => pages::agents::note_of(&v),
                        Err(_) => format!("HTTP {} (not JSON)", r.status),
                    },
                    Err(e) => format!("command failed: {e}"),
                };
            }
        }
        let health = self.cached_health(&fwd);
        let origin = match &health {
            Some(h) if h.get("status").as_str() == Some("healthy") => Origin::Up,
            _ => Origin::Down,
        };
        let body = pages::agents::body(health.as_ref(), &note);
        let f = self.frame(req, "Galion Console · Agents");
        Reply::html(status, layout::console(&f, origin, &body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tanstack_no_route_is_a_404() {
        assert!(no_route(404, b""));
        assert!(no_route(
            500,
            br#"{"error":"Only HTML requests are supported here"}"#
        ));
        assert!(!no_route(500, b"{\"error\":\"boom\"}"));
        assert!(!no_route(401, b""));
    }

    #[test]
    fn back_targets_are_ours() {
        assert_eq!(safe_back("/agents"), "/agents");
        assert_eq!(safe_back("https://evil.example"), "/console");
        assert_eq!(safe_back("//evil.example"), "/console");
        assert_eq!(safe_back("/api/v1/admin"), "/console");
    }

    #[test]
    fn device_cookie_marks_secure_on_galion_hosts() {
        let mut req = Request {
            host: "console.galion.app",
            ..Request::default()
        };
        assert!(Ui::cookie_secure(&req));
        assert!(Ui::device_set_cookie(&req, "abcdefghijklmnopqrstuv").contains("; Secure"));
        req.host = "127.0.0.1";
        assert!(!Ui::cookie_secure(&req));
        req.forwarded_proto = Some("https");
        assert!(Ui::cookie_secure(&req));
    }

    #[test]
    fn every_emitted_class_is_styled() {
        // Render each page with sample data and check class names exist in the sheet.
        let defined = css::defined_classes();
        let services = json::parse(br#"{"items":[{"id":"a","name":"A","kind":"api","weight":"light","summary":"s","hosts":["h"],"port":1,"defaultOn":true},{"id":"whisper","name":"W","kind":"held","weight":"heavy","summary":"s","hosts":[],"port":2,"defaultOn":false}]}"#).unwrap();
        let svcs = pages::services_of(&services);
        let domains =
            json::parse(br#"{"items":[{"host":"x","role":"r","serviceId":"a"}]}"#).unwrap();
        let health =
            json::parse(br#"{"status":"healthy","version":"1","target":{"pod":"ip50"}}"#).unwrap();
        let f = Frame {
            title: "t",
            css_href: "/c.css",
            extra_css_href: None,
            light: Some(false),
            path: "/agents",
            refresh_s: None,
        };
        let mut all = String::new();
        all.push_str(&layout::console(
            &f,
            Origin::Up,
            &pages::agents::body(Some(&health), ""),
        ));
        all.push_str(&pages::services::body(
            &svcs,
            &[pages::Pulse {
                id: "zz".into(),
                ok: false,
            }],
            true,
        ));
        all.push_str(&pages::status::body(Some(true), &svcs, &domains, &[]));
        all.push_str(&pages::gate::pair("/agents", Some("x")));
        all.push_str(&pages::gate::links());
        all.push_str(&pages::not_ported::body(&rooms::ROOMS[0], PORTED));
        let cat = json::parse(br#"{"owner":"o","public":1,"privateVisible":1,"items":[{"name":"a","visibility":"fork","empty":true,"weight":"heavy","role":"r","method":"GET","path":"/p","title":"t","source":"s","zone":"live","host":"h","brand":"b","serviceId":"a"},{"method":"POST","path":"/q","title":"t","source":"s","zone":"held"}],"steps":["one"],"tunnel":{"cname":"c"},"zones":[{"zone":"z","records":[{"type":"CNAME","name":"@","host":"z"}]}],"target":{"provider":"cpu","pod":"p","podId":"p"},"compose":"x","caddy":"y","services":[{"name":"n","weight":"heavy","summary":"s"}],"fridge":[{"name":"f","summary":"s"}]}"#).unwrap();
        all.push_str(&pages::catalog::domains(Some(&cat), Some(&services)));
        all.push_str(&pages::catalog::repos(Some(&cat)));
        all.push_str(&pages::catalog::docs(Some(&cat), "zz"));
        all.push_str(&pages::catalog::docs(Some(&cat), ""));
        all.push_str(&pages::catalog::cloudflare(Some(&cat), Some(&cat)));
        all.push_str(&pages::catalog::pack(Some(&cat)));
        all.push_str(&pages::catalog::chill(Some(&cat)));
        all.push_str(&pages::catalog::jupyter(Some(&cat)));
        all.push_str(&pages::catalog::data());
        all.push_str(&pages::catalog::repos(None));
        all.push_str(&pages::gate::coming_soon("galion.app"));
        all.push_str(&pages::overview::body(PORTED.len() + 1));
        let snap = json::parse(br#"{"health":{"score":1,"status":"ok"},"services":[{"id":"a","port":1,"ok":true,"ms":1}],"nerve":{"events":["e"]},"edge":[{"host":"h","code":200,"ms":1}],"defence":[{"service":"g","status":"ok","ms":1}],"cortex":{"cores":1},"agents":{"hermes":{"last":[1],"avg":1}},"systems":[{"id":"s","ok":true,"note":"n"}],"infrastructure":{"gpu":{"name":"g"}}}"#).unwrap();
        for b in pages::analytics::BOARDS {
            all.push_str(&pages::analytics::body(Ok(&snap), b));
        }
        all.push_str(&pages::analytics::body(
            Err(pages::analytics::Miss::Status(401)),
            "",
        ));
        let arena = json::parse(br#"{"spend":{"cap_usd":2,"spent_usd":0,"generations":0,"max_generations":3},"champion":{"id":"c"},"error":"x","rows":[{"id":"r","answer":"a","mutation":"m"}]}"#).unwrap();
        all.push_str(&pages::arena::body(Ok(&arena)));
        all.push_str(&pages::arena::body(Err(pages::arena::Miss::Down)));
        all.push_str(&pages::arena::body(Err(pages::arena::Miss::Status(404))));
        let bank =
            json::parse(br#"{"purses":[{"name":"n","usd":1,"note":"x"}],"note":"y"}"#).unwrap();
        all.push_str(&pages::bank::body(Ok(&bank)));
        let stale = json::parse(br#"{"asOf":"2026-10-27T18:19:00+01:00","purses":[{"name":"r","source":"survival","asOf":"2026-10-27T18:19:00+01:00"}]}"#).unwrap();
        all.push_str(&pages::bank::body_at(Ok(&stale), 1_793_156_400));
        all.push_str(&pages::bank::body(Err(pages::bank::Miss::Status(404))));
        // Survival card (slot 408): every state.
        let sv = json::parse(br#"{"exists":true,"asOf":"2026-10-27T18:19:50+01:00","stale":true,"writer":"/w","lastStart":"2026-10-28T04:00:00+01:00","lastExit":1,"runs":1,"failures":1}"#).unwrap();
        all.push_str(&pages::bank::body_with(
            Ok(&bank),
            &pages::survival::card(Ok(&sv)),
        ));
        all.push_str(&pages::survival::card(Err(pages::survival::Miss::Status(
            404,
        ))));
        all.push_str(&pages::survival::card(Err(pages::survival::Miss::Status(
            401,
        ))));
        all.push_str(&pages::survival::card(Err(pages::survival::Miss::Down)));
        let desk = json::parse(br#"{"mindpack":{"cache":{"read":1},"cached":[{"id":"c"}],"stores":[{"name":"s","n":1,"bytes":1}],"models":[{"name":"m"}],"recent":[{"name":"f","at":1,"bytes":1}]}}"#).unwrap();
        all.push_str(&pages::cache::body(Some(&desk)));
        all.push_str(&pages::cache::body(None));
        // console-port-5 rooms.
        all.push_str(&pages::node::body(Some(&health)));
        all.push_str(&pages::node::body(None));
        let docs = vec![
            pages::session::Doc {
                name: "a.md".into(),
                bytes: "1".into(),
            },
            pages::session::Doc {
                name: "<x>".into(),
                bytes: "2".into(),
            },
        ];
        let miss = pages::session::Miss::Status(401, "phone required".into());
        all.push_str(&pages::session::body(
            Ok(&docs),
            pages::session::Open::Doc("a.md", Ok("x")),
        ));
        all.push_str(&pages::session::body(
            Err(&miss),
            pages::session::Open::Refused,
        ));
        let guardian = json::parse(br#"{"status":"alert","score":1,"checks":[{"service":"a","status":"down"}],"lastScan":"2026-10-21T03:00:00Z"}"#).unwrap();
        let sec = json::parse(br#"{"denylist":true,"floor":["f"],"files":[{"path":"/a/b","mode":"644","ok":false}],"volume":[{"path":"/c","mode":"sealed","ok":true}]}"#).unwrap();
        let inc = json::parse(br#"[{"title":"t","severity":"high"}]"#).unwrap();
        all.push_str(&pages::security::body(&pages::security::View {
            guard: Ok(&sec),
            guardian: Ok(&guardian),
            incidents: Ok(&inc),
            note: "n",
        }));
        all.push_str(&pages::security::body(&pages::security::View {
            guard: Err(&miss),
            guardian: Err(&miss),
            incidents: Err(&miss),
            note: "",
        }));
        let none = Frame {
            light: None,
            ..f.clone()
        };
        all.push_str(&layout::console(&none, Origin::Down, ""));
        let mut missing = Vec::new();
        for cap in all.split("class=\"").skip(1) {
            let list = cap.split('"').next().unwrap_or("");
            for c in list.split_whitespace() {
                if !defined.iter().any(|d| d == c)
                    && ![
                        "primary", "ghost", "online", "offline", "held", "neutral", "fork",
                        "chill", "i-open", "i-close",
                    ]
                    .contains(&c)
                {
                    missing.push(c.to_string());
                }
            }
        }
        missing.sort();
        missing.dedup();
        // `room`-level and state classes are styled via compound selectors.
        assert!(missing.is_empty(), "unstyled classes: {missing:?}");
    }
}
