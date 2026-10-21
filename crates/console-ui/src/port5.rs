// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! console-port-5 (wave 4 slot 139): the last fifth, alphabetical, of the
//! 17 console routes still in TypeScript at 669dc18: `/node`, `/security`,
//! `/session`. Page bodies are in `pages::{node, security, session}`; this
//! module does the upstream calls.
//!
//! State-changing upstream calls happen only from same-origin-checked form
//! posts to `/security`, with method/path/body fixed in the binary:
//! `POST /api/v3/guardian/scan {}` and `POST /api/v3/guardian/heal {}` with
//! the PIN as `X-Galion-Pin`. The Denylist switch (`POST
//! /api/hermes/guard`) is not ported (owner decision). `/node` and
//! `/session` are GET only.

use crate::client::{self, Forward};
use crate::json::{self, Value};
use crate::layout::{self, Origin};
use crate::pages::session::Miss;
use crate::pages::{node, security, session};
use crate::{html, Reply, Request, Ui};

/// The three routes.
pub const ROUTES: &[&str] = &["/node", "/security", "/session"];

/// Routes that may take a form POST.
pub const POST_ROUTES: &[&str] = &["/security"];

fn title(label: &str) -> String {
    format!("Galion Console · {label}")
}

impl Ui {
    pub(crate) fn port5_room(&self, req: &Request<'_>) -> Reply {
        match req.path {
            "/node" => self.p5_node(req),
            "/security" => self.p5_security(req),
            _ => self.p5_session(req),
        }
    }

    /// GET a JSON payload; a non-200 answer says its status and `error`.
    fn p5_json(&self, path: &str, fwd: &Forward) -> Result<Value, Miss> {
        match client::call(self.cfg.upstream, "GET", path, fwd, None) {
            Ok(r) if r.status == 200 => json::parse(&r.body).map_err(|_| Miss::Down),
            Ok(r) => Err(Miss::Status(r.status, p5_error(&r.body))),
            Err(_) => Err(Miss::Down),
        }
    }

    fn p5_node(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let health = self.get_json(self.cfg.upstream, "/api/v1/health", &fwd);
        let origin = match &health {
            Some(h) if h.get("status").as_str() == Some("healthy") => Origin::Up,
            _ => Origin::Down,
        };
        let body = node::body(health.as_ref());
        let t = title("Pod");
        let f = self.frame(req, &t);
        Reply::html(200, layout::console(&f, origin, &body))
    }

    fn p5_security(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let mut note = String::new();
        let mut status = 200;
        if req.method == "POST" {
            if !Self::same_origin(req) {
                return Reply::text(403, "Forbidden\n");
            }
            let form = html::form_pairs(&String::from_utf8_lossy(req.body));
            let act = html::form_get(&form, "act").unwrap_or("");
            let pin = html::form_get(&form, "pin").unwrap_or("").trim();
            let call = |path: &str, extra: &[(&str, &str)]| {
                client::call_with(
                    self.cfg.upstream,
                    "POST",
                    path,
                    &fwd,
                    extra,
                    Some(("application/json", b"{}")),
                )
            };
            let result = match act {
                "scan" => Some(("scan", call("/api/v3/guardian/scan", &[]))),
                "heal" if security::pin_ok(pin) => Some((
                    "heal",
                    call("/api/v3/guardian/heal", &[("X-Galion-Pin", pin)]),
                )),
                "heal" => {
                    note = "Heal: enter the Defender PIN (nothing was sent).".into();
                    status = 400;
                    None
                }
                _ => {
                    note = "Unknown action (nothing was sent).".into();
                    status = 400;
                    None
                }
            };
            if let Some((what, r)) = result {
                note = match r {
                    Ok(r) if (200..300).contains(&r.status) => format!("{what}: HTTP {}", r.status),
                    Ok(r) => {
                        let e = p5_error(&r.body);
                        if e.is_empty() {
                            format!("{what}: refused, HTTP {}", r.status)
                        } else {
                            format!("{what}: refused, HTTP {} ({e})", r.status)
                        }
                    }
                    Err(e) => format!("{what}: failed ({e})"),
                };
            }
        }
        let (origin, guard, guardian, incidents) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let a = s.spawn(|| self.p5_json("/api/v1/security", &fwd));
            let b = s.spawn(|| self.p5_json("/api/v3/guardian/status", &fwd));
            let c = s.spawn(|| self.p5_json("/api/v3/guardian/incidents", &fwd));
            (
                o.join().unwrap_or(Origin::Down),
                a.join().unwrap_or(Err(Miss::Down)),
                b.join().unwrap_or(Err(Miss::Down)),
                c.join().unwrap_or(Err(Miss::Down)),
            )
        });
        let view = security::View {
            guard: guard.as_ref(),
            guardian: guardian.as_ref(),
            incidents: incidents.as_ref(),
            note: &note,
        };
        let body = security::body(&view);
        let t = title("Defence");
        let f = self.frame(req, &t);
        Reply::html(status, layout::console(&f, origin, &body))
    }

    fn p5_session(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let q = html::form_pairs(req.query);
        let name = html::form_get(&q, "name").filter(|s| !s.is_empty());
        let refused = name.is_some_and(|n| !session::name_ok(n));
        let name = name.filter(|n| session::name_ok(n));
        let up = self.cfg.upstream;
        let (origin, list, doc) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let l = s.spawn(|| self.p5_json("/api/v1/session", &fwd));
            let d = s.spawn(|| {
                name.map(
                    |n| match client::call(up, "GET", &session::doc_path(n), &fwd, None) {
                        Ok(r) if r.status == 200 => {
                            Ok(String::from_utf8_lossy(&r.body).to_string())
                        }
                        Ok(r) => Err(Miss::Status(r.status, p5_error(&r.body))),
                        Err(_) => Err(Miss::Down),
                    },
                )
            });
            (
                o.join().unwrap_or(Origin::Down),
                l.join().unwrap_or(Err(Miss::Down)),
                d.join().ok().flatten(),
            )
        });
        let docs = list.as_ref().map(session::docs_of);
        let open = match (&doc, name) {
            (Some(d), Some(n)) => {
                session::Open::Doc(n, d.as_ref().map(String::as_str).map_err(Clone::clone))
            }
            _ if refused => session::Open::Refused,
            _ => session::Open::None,
        };
        let body = session::body(docs.as_ref().map(Vec::as_slice).map_err(|m| *m), open);
        let t = title("Session");
        let f = self.frame(req, &t);
        Reply::html(
            if refused { 400 } else { 200 },
            layout::console(&f, origin, &body),
        )
    }
}

/// The API's `{"error": "..."}` text, printable and short, or empty.
fn p5_error(body: &[u8]) -> String {
    json::parse(body)
        .ok()
        .and_then(|v| v.get("error").as_str().map(str::to_string))
        .map(|e| e.chars().filter(|c| !c.is_control()).take(120).collect())
        .unwrap_or_default()
}
