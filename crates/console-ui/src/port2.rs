// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! console-port-2 (wave 4 slot 136): the second fifth, alphabetical, of the
//! 17 console routes still in TypeScript at 669dc18: `/context`,
//! `/gateway`, `/guardian`, `/hermes`. Page bodies are in
//! `pages::{context, gateway, guardian, hermes}`; this module does the
//! upstream calls.
//!
//! State-changing upstream calls happen only from same-origin-checked form
//! posts, with method/path/body fixed in the binary:
//! `/gateway` (the two POST endpoints of `API_ENDPOINTS`) and `/guardian`
//! (scan, heal with the PIN header). `/context` and `/hermes` are GET only.

use crate::client::{self, Forward};
use crate::json::{self, Value};
use crate::layout::{self, Origin};
use crate::pages::{context, gateway, guardian, hermes};
use crate::{html, Reply, Request, Ui};

/// The four routes.
pub const ROUTES: &[&str] = &["/context", "/gateway", "/guardian", "/hermes"];

/// Routes that may take a form POST.
pub const POST_ROUTES: &[&str] = &["/gateway", "/guardian"];

fn title(label: &str) -> String {
    format!("Galion Console · {label}")
}

impl Ui {
    pub(crate) fn port2_room(&self, req: &Request<'_>) -> Reply {
        match req.path {
            "/context" => self.p2_context(req),
            "/gateway" => self.p2_gateway(req),
            "/guardian" => self.p2_guardian(req),
            _ => self.p2_hermes(req),
        }
    }

    /// galion-desk `GET /desk`.
    fn p2_desk(&self) -> Option<Value> {
        let addr = self.cfg.desk?;
        let fwd = Forward {
            host: addr.to_string(),
            ..Forward::default()
        };
        self.get_json(addr, "/desk", &fwd)
    }

    fn p2_context(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let (origin, desk) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let d = s.spawn(|| self.p2_desk());
            (o.join().unwrap_or(Origin::Down), d.join().ok().flatten())
        });
        let body = context::body(desk.as_ref());
        let t = title("Context");
        let f = self.frame(req, &t);
        Reply::html(200, layout::console(&f, origin, &body))
    }

    fn p2_gateway(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let q = html::form_pairs(req.query);
        let mut active = gateway::pick(html::form_get(&q, "ep")).unwrap_or(0);
        let mut sent = None;
        let mut note = String::new();
        let mut status = 200;
        if req.method == "POST" && !Self::same_origin(req) {
            return Reply::text(403, "Forbidden\n");
        }
        // The header dot is read while the endpoint is sent.
        let origin = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            if req.method == "POST" {
                let form = html::form_pairs(&String::from_utf8_lossy(req.body));
                match gateway::pick(html::form_get(&form, "ep")) {
                    Some(i) if gateway::ENDPOINTS[i].method == "POST" => {
                        active = i;
                        let e = gateway::ENDPOINTS[i];
                        let body = e.body.unwrap_or("{}");
                        sent = Some(
                            match client::call(
                                self.cfg.upstream,
                                "POST",
                                e.path,
                                &fwd,
                                Some(("application/json", body.as_bytes())),
                            ) {
                                Ok(r) => gateway::Sent::from_answer(r.status, &r.body),
                                Err(e) => gateway::Sent::failed(&e.to_string()),
                            },
                        );
                    }
                    _ => {
                        status = 400;
                        note = "Not a POST endpoint of this page.".into();
                    }
                }
            } else if html::form_get(&q, "send") == Some("1") {
                let e = gateway::ENDPOINTS[active];
                if e.method == "GET" {
                    sent = Some(
                        match client::call(self.cfg.upstream, "GET", e.path, &fwd, None) {
                            Ok(r) => gateway::Sent::from_answer(r.status, &r.body),
                            Err(e) => gateway::Sent::failed(&e.to_string()),
                        },
                    );
                } else {
                    note = "POST endpoints are sent with the Send button (a form post).".into();
                }
            }
            o.join().unwrap_or(Origin::Down)
        });
        let body = gateway::body(active, sent.as_ref(), &note);
        let t = title("Gateway");
        let f = self.frame(req, &t);
        Reply::html(status, layout::console(&f, origin, &body))
    }

    fn p2_guardian_status(&self, fwd: &Forward) -> Result<Value, guardian::Miss> {
        match client::call(
            self.cfg.upstream,
            "GET",
            "/api/v3/guardian/status",
            fwd,
            None,
        ) {
            Ok(r) if r.status == 200 => json::parse(&r.body).map_err(|_| guardian::Miss::Down),
            Ok(r) => {
                let err = json::parse(&r.body)
                    .ok()
                    .and_then(|v| v.get("error").as_str().map(str::to_string))
                    .unwrap_or_default();
                Err(guardian::Miss::Status(r.status, err))
            }
            Err(_) => Err(guardian::Miss::Down),
        }
    }

    fn p2_guardian(&self, req: &Request<'_>) -> Reply {
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
            let call = |extra: &[(&str, &str)]| {
                client::call_with(
                    self.cfg.upstream,
                    "POST",
                    &format!("/api/v3/guardian/{act}"),
                    &fwd,
                    extra,
                    Some(("application/json", b"{}")),
                )
            };
            let result = match act {
                "scan" => Some(call(&[])),
                "heal" if guardian::pin_ok(pin) => Some(call(&[("X-Galion-Pin", pin)])),
                "heal" => {
                    note = "heal: enter the Defender PIN (nothing was sent).".into();
                    status = 400;
                    None
                }
                _ => {
                    note = "unknown action".into();
                    status = 400;
                    None
                }
            };
            if let Some(r) = result {
                note = match r {
                    Ok(r) => format!("{act}: HTTP {}", r.status),
                    Err(e) => format!("{act}: failed ({e})"),
                };
            }
        }
        let (origin, st) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let g = s.spawn(|| self.p2_guardian_status(&fwd));
            (
                o.join().unwrap_or(Origin::Down),
                g.join().unwrap_or(Err(guardian::Miss::Down)),
            )
        });
        let body = guardian::body(st.as_ref(), &note);
        let t = title("Guardian");
        let f = self.frame(req, &t);
        Reply::html(status, layout::console(&f, origin, &body))
    }

    fn p2_hermes(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let q = html::form_pairs(req.query);
        let id = html::form_get(&q, "id").filter(|s| !s.is_empty());
        let mut status = 200;
        let id = match id {
            Some(i) if hermes::id_ok(i) => Some(i),
            Some(_) => {
                status = 400;
                None
            }
            None => None,
        };
        let up = self.cfg.upstream;
        let (origin, got, thread) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let hs: Vec<_> = hermes::READS
                .iter()
                .map(|p| s.spawn(|| self.get_json(up, p, &fwd)))
                .collect();
            let t = s.spawn(|| id.and_then(|i| self.get_json(up, &hermes::thread_path(i), &fwd)));
            let got: Vec<Option<Value>> = hs.into_iter().map(|j| j.join().ok().flatten()).collect();
            (
                o.join().unwrap_or(Origin::Down),
                got,
                t.join().ok().flatten(),
            )
        });
        let g = |i: usize| got.get(i).and_then(Option::as_ref);
        let view = hermes::View {
            health: g(0),
            guard: g(1),
            modes: g(2),
            models: g(3),
            history: g(4),
            thread: id.map(|i| (i, thread.as_ref())),
        };
        let mut body = hermes::body(&view);
        if status == 400 {
            body = format!("<p class=\"note t-bad\">That thread id is not valid.</p>{body}");
        }
        let t = title("Hermes Research");
        let f = self.frame(req, &t);
        Reply::html(status, layout::console(&f, origin, &body))
    }
}
