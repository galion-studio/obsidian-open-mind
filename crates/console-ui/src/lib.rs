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
pub mod pages;
pub mod rooms;

use client::Forward;
use gate::Decision;
use json::Value;
use layout::{Frame, Origin};
use std::net::SocketAddr;

/// Pages rendered in Rust, in port order.
pub const PORTED: &[&str] = &["/status", "/services", "/agents"];

/// Largest form body accepted (pairing code or a command name).
pub const MAX_FORM: usize = 4096;

#[derive(Debug, Clone)]
pub struct UiConfig {
    /// Console API (loopback).
    pub upstream: SocketAddr,
    /// Desk pulse (`/api/v1/awake`), loopback; `None` disables it.
    pub pulse: Option<SocketAddr>,
    /// Extra hosts treated as console hosts (the machine's public IP).
    pub extra_hosts: Vec<String>,
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
        Ui {
            cfg,
            css_href: format!("/_ui/console.{h:016x}.css"),
            css_etag: format!("\"{h:016x}\""),
            css,
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
            || PORTED.contains(&path)
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
            light: gate::cookie(req.cookie, "galion_theme").as_deref() == Some("light"),
            path: req.path,
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
            p if p == self.css_href => return self.stylesheet(req),
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
        let post_ok = req.path == "/agents" && req.method == "POST";
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
            "/soon" => {
                let f = self.frame(req, "Soon");
                Reply::html(200, layout::document(&f, "", &pages::gate::soon()))
            }
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

    fn stylesheet(&self, req: &Request<'_>) -> Reply {
        let _ = req;
        Reply {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "text/css; charset=utf-8".into()),
                (
                    "Cache-Control".into(),
                    "public, max-age=31536000, immutable".into(),
                ),
                ("ETag".into(), self.css_etag.clone()),
            ],
            body: self.css.as_bytes().to_vec(),
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
        let f = self.frame(req, "Admin");
        if code.is_empty() {
            return Reply::html(
                200,
                layout::document(&f, "", &pages::gate::pair(&back, Some("Enter the code."))),
            );
        }
        let body = json::obj(&[("code", Value::Str(code))]).to_json();
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

    fn origin(&self, fwd: &Forward) -> Origin {
        match self.get_json(self.cfg.upstream, "/api/v1/health", fwd) {
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
            let h = s.spawn(|| self.get_json(self.cfg.upstream, "/api/v1/health", &fwd));
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
        let health = self.get_json(self.cfg.upstream, "/api/v1/health", &fwd);
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
    fn back_targets_are_ours() {
        assert_eq!(safe_back("/agents"), "/agents");
        assert_eq!(safe_back("https://evil.example"), "/console");
        assert_eq!(safe_back("//evil.example"), "/console");
        assert_eq!(safe_back("/api/v1/admin"), "/console");
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
            light: false,
            path: "/agents",
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
        let mut missing = Vec::new();
        for cap in all.split("class=\"").skip(1) {
            let list = cap.split('"').next().unwrap_or("");
            for c in list.split_whitespace() {
                if !defined.iter().any(|d| d == c)
                    && ![
                        "primary", "ghost", "online", "offline", "held", "i-open", "i-close",
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
