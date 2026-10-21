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
//! (scan, heal with the PIN header). `/context` and `/hermes` are GET only;
//! a POST to `/hermes` is 405 (writes held).

use crate::client::{self, Forward};
use crate::json::{self, Value};
use crate::layout::{self, Origin};
use crate::pages::{context, gateway, guardian, hermes};
use crate::{html, Reply, Request, Ui};

/// The four routes.
pub const ROUTES: &[&str] = &["/context", "/gateway", "/guardian", "/hermes"];

/// Routes that may take a form POST.
pub const POST_ROUTES: &[&str] = &["/gateway", "/guardian"];

/// Ported rooms that the pre-built SPA keeps while a fallback is configured.
///
/// Empty since console-route-31 (wave 4 slot 170): `/hermes` is Rust-first
/// even with `--root`, read-only (no JavaScript, CSP unchanged; asking,
/// runs, mic and the guard switches held). Kept as a list so a room can be
/// handed back to the SPA with a one-line change.
pub const SPA_FIRST: &[&str] = &[];

/// The 31 page routes of the live console source tree
/// (`artifacts/console-ui/live-src-20261020-2125/src/routes`): the 29
/// `_console/*.tsx` files (`index` is `/`, `md.$name` is checked as
/// `/md/user`) plus `status.tsx` and `soon.tsx`, alphabetical by file.
pub const LIVE_SRC_ROUTES: &[&str] = &[
    "/agents",
    "/analytics",
    "/arena",
    "/bank",
    "/cache",
    "/chill",
    "/cloudflare",
    "/console",
    "/context",
    "/data",
    "/docs",
    "/domains",
    "/gateway",
    "/guardian",
    "/hermes",
    "/",
    "/instances",
    "/jupyter",
    "/keys",
    "/markers",
    "/md/user",
    "/memory",
    "/mesh",
    "/node",
    "/pack",
    "/repos",
    "/security",
    "/services",
    "/session",
    "/soon",
    "/status",
];

impl Ui {
    /// Live-source routes that are **not** Rust-first: with an SPA fallback
    /// configured (`spa_fallback = true`) the SPA serves them; without one
    /// they get the "not in Rust yet" page.
    pub fn spa_fallbacks(&self, spa_fallback: bool) -> Vec<&'static str> {
        LIVE_SRC_ROUTES
            .iter()
            .copied()
            .filter(|p| !self.owns(p, spa_fallback))
            .collect()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UiConfig, PORTED};

    fn ui() -> Ui {
        Ui::new(UiConfig {
            upstream: "127.0.0.1:9".parse().unwrap(),
            pulse: None,
            extra_hosts: vec![],
            desk: None,
        })
    }

    #[test]
    fn live_src_is_31_unique_routes() {
        assert_eq!(LIVE_SRC_ROUTES.len(), 31);
        let mut s: Vec<&str> = LIVE_SRC_ROUTES.to_vec();
        s.sort_unstable();
        s.dedup();
        assert_eq!(s.len(), 31);
        for p in PORTED {
            assert!(LIVE_SRC_ROUTES.contains(p), "{p} is not a live-src route");
        }
    }

    #[test]
    fn hermes_is_rust_first_with_and_without_spa() {
        assert!(SPA_FIRST.is_empty());
        let ui = ui();
        for spa in [true, false] {
            for p in ROUTES {
                assert!(ui.owns(p, spa), "{p} spa={spa}");
            }
            assert!(ui.owns("/hermes", spa));
        }
    }

    #[test]
    fn fallbacks_on_this_base() {
        let ui = ui();
        // console-port-2 0916684: 17 PORTED + `/` are Rust-first; the other
        // 13 live-src routes are still the SPA's (ported on console-port-1,
        // -3, -4, -5; merged in console-port-merge).
        let spa = ui.spa_fallbacks(true);
        assert_eq!(
            spa,
            [
                "/analytics",
                "/arena",
                "/bank",
                "/cache",
                "/instances",
                "/keys",
                "/markers",
                "/md/user",
                "/memory",
                "/mesh",
                "/node",
                "/security",
                "/session",
            ]
        );
        assert_eq!(spa.len() + PORTED.len() + 1, 31);
        assert!(!spa.contains(&"/hermes"));
    }
}
