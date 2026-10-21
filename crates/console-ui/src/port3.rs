// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! console-port-3 (wave 4 slot 137): the third fifth, alphabetical, of the
//! 17 console routes still in TypeScript at 669dc18: `/instances`, `/keys`,
//! `/markers`. Page bodies are in `pages::{instances, keys, markers}`; this
//! module does the upstream calls.
//!
//! State-changing upstream calls happen only from same-origin-checked form
//! posts, with method and path fixed in the binary and the JSON body built
//! here from checked fields: `/keys` (mint with a cleaned name and known
//! scopes, revoke with a hex id) and `/markers` (Ping, fixed body).
//! `/instances` is GET only.

use crate::client::{self, Forward};
use crate::json::{self, Value};
use crate::layout::{self, Origin};
use crate::pages::{instances, keys, markers};
use crate::{html, Reply, Request, Ui};

/// The three routes.
pub const ROUTES: &[&str] = &["/instances", "/keys", "/markers"];

/// Routes that may take a form POST.
pub const POST_ROUTES: &[&str] = &["/keys", "/markers"];

/// A GET answer for a page: the JSON, or `(status, error)` when it failed
/// (`None` status = no answer / not JSON).
pub type Fetched = Result<Value, (Option<u16>, String)>;

fn title(label: &str) -> String {
    format!("Galion Console · {label}")
}

fn error_of(body: &[u8]) -> String {
    json::parse(body)
        .ok()
        .and_then(|v| v.get("error").as_str().map(str::to_string))
        .unwrap_or_default()
}

fn as_ref(f: &Fetched) -> Result<&Value, (Option<u16>, String)> {
    match f {
        Ok(v) => Ok(v),
        Err((c, e)) => Err((*c, e.clone())),
    }
}

impl Ui {
    pub(crate) fn port3_room(&self, req: &Request<'_>) -> Reply {
        match req.path {
            "/instances" => self.p3_instances(req),
            "/keys" => self.p3_keys(req),
            _ => self.p3_markers(req),
        }
    }

    /// GET an API path, keeping why it failed.
    fn p3_fetch(&self, path: &str, fwd: &Forward) -> Fetched {
        match client::call(self.cfg.upstream, "GET", path, fwd, None) {
            Ok(r) if r.status == 200 => json::parse(&r.body).map_err(|_| (None, String::new())),
            Ok(r) => Err((Some(r.status), error_of(&r.body))),
            Err(_) => Err((None, String::new())),
        }
    }

    fn p3_post(&self, path: &str, fwd: &Forward, body: &str) -> Option<(u16, Option<Value>)> {
        client::call(
            self.cfg.upstream,
            "POST",
            path,
            fwd,
            Some(("application/json", body.as_bytes())),
        )
        .ok()
        .map(|r| (r.status, json::parse(&r.body).ok()))
    }

    fn p3_instances(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let (origin, inst, laptop) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let i = s.spawn(|| self.p3_fetch("/api/v1/instances", &fwd));
            let l = s.spawn(|| self.p3_fetch("/api/v1/laptop", &fwd));
            let none = || Err((None, String::new()));
            (
                o.join().unwrap_or(Origin::Down),
                i.join().unwrap_or_else(|_| none()),
                l.join().unwrap_or_else(|_| none()),
            )
        });
        let body = instances::body(as_ref(&inst), as_ref(&laptop));
        let t = title("Instances");
        let f = self.frame(req, &t);
        Reply::html(200, layout::console(&f, origin, &body))
    }

    fn p3_keys(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let mut status = 200;
        let mut done = None;
        if req.method == "POST" {
            if !Self::same_origin(req) {
                return Reply::text(403, "Forbidden\n");
            }
            let form = html::form_pairs(&String::from_utf8_lossy(req.body));
            match html::form_get(&form, "op") {
                Some("mint") => {
                    let name = keys::clean_name(html::form_get(&form, "name").unwrap_or(""));
                    let scopes = keys::pick_scopes(
                        form.iter()
                            .filter(|(k, _)| k == "scope")
                            .map(|(_, v)| v.as_str()),
                    );
                    let body = json::obj(&[
                        ("name", Value::Str(name)),
                        (
                            "scopes",
                            Value::Arr(scopes.iter().map(|s| Value::Str(s.to_string())).collect()),
                        ),
                    ])
                    .to_json();
                    done = Some(match self.p3_post("/api/v1/keys", &fwd, &body) {
                        Some((code, Some(v))) => keys::minted(code, &v),
                        Some((code, None)) => {
                            keys::Done::Refused(format!("Mint refused: HTTP {code}."))
                        }
                        None => keys::Done::Refused(
                            "Mint failed: the console API did not answer.".into(),
                        ),
                    });
                }
                Some("revoke") => {
                    let id = html::form_get(&form, "id").unwrap_or("");
                    if keys::id_ok(id) {
                        let body = json::obj(&[
                            ("action", Value::Str("revoke".into())),
                            ("id", Value::Str(id.to_string())),
                        ])
                        .to_json();
                        done = Some(match self.p3_post("/api/v1/keys", &fwd, &body) {
                            Some((200, Some(v))) if v.get("ok").as_bool().is_some() => {
                                keys::Done::Revoked(v.get("ok").as_bool() == Some(true))
                            }
                            Some((code, v)) => {
                                let err = v
                                    .as_ref()
                                    .and_then(|v| v.get("error").as_str().map(str::to_string))
                                    .unwrap_or_default();
                                keys::Done::Refused(if err.is_empty() {
                                    format!("Revoke refused: HTTP {code}.")
                                } else {
                                    format!("Revoke refused: HTTP {code} ({err}).")
                                })
                            }
                            None => keys::Done::Refused(
                                "Revoke failed: the console API did not answer.".into(),
                            ),
                        });
                    } else {
                        status = 400;
                        done = Some(keys::Done::Refused("Not a key id. Nothing sent.".into()));
                    }
                }
                _ => {
                    status = 400;
                    done = Some(keys::Done::Refused("Unknown action. Nothing sent.".into()));
                }
            }
        }
        let (origin, list) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let l = s.spawn(|| self.p3_fetch("/api/v1/keys", &fwd));
            (
                o.join().unwrap_or(Origin::Down),
                l.join().unwrap_or_else(|_| Err((None, String::new()))),
            )
        });
        let body = keys::body(as_ref(&list), done.as_ref());
        let t = title("Keys");
        let f = self.frame(req, &t);
        Reply::html(status, layout::console(&f, origin, &body))
    }

    fn p3_markers(&self, req: &Request<'_>) -> Reply {
        let fwd = self.forward(req);
        let mut status = 200;
        let mut note = String::new();
        if req.method == "POST" {
            if !Self::same_origin(req) {
                return Reply::text(403, "Forbidden\n");
            }
            let form = html::form_pairs(&String::from_utf8_lossy(req.body));
            if html::form_get(&form, "op") == Some("ping") {
                note = match self.p3_post("/api/v1/markers", &fwd, markers::PING) {
                    Some((code, v)) => markers::ping_note(code, v.as_ref()),
                    None => "Ping failed: the console API did not answer.".into(),
                };
            } else {
                status = 400;
                note = "Unknown action. Nothing sent.".into();
            }
        }
        let (origin, ring) = std::thread::scope(|s| {
            let o = s.spawn(|| self.origin(&fwd));
            let r = s.spawn(|| self.p3_fetch("/api/v1/markers", &fwd));
            (
                o.join().unwrap_or(Origin::Down),
                r.join().unwrap_or_else(|_| Err((None, String::new()))),
            )
        });
        let body = markers::body(as_ref(&ring), &note);
        let t = title("Markers");
        let f = self.frame(req, &t);
        Reply::html(status, layout::console(&f, origin, &body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_rooms_and_ported() {
        for r in ROUTES {
            assert!(crate::rooms::exact(r).is_some(), "{r} is a console room");
            assert!(crate::PORTED.contains(r), "{r} in PORTED");
        }
        for r in POST_ROUTES {
            assert!(ROUTES.contains(r));
        }
        assert_eq!(error_of(br#"{"error":"phone required"}"#), "phone required");
        assert_eq!(error_of(b"nope"), "");
    }

    #[test]
    fn every_port3_class_is_styled() {
        let inst = json::parse(br#"{"nodes":[{"name":"n","state":"held","mark":"linked","detail":"d","role":"r","place":"p"}],"parts":[{"name":"a","state":"online","note":"x","where":"w","bind":"b"}],"link":{"pubkey":"k","tunnel":"t"}}"#).unwrap();
        let lap = json::parse(br#"{"connected":true,"jobs":[{"id":"j","name":"j","detail":"d","pct":5}],"lines":["l"]}"#).unwrap();
        let ks = json::parse(br#"{"keys":[{"id":"ab","name":"n","prefix":"gal_sk_x","scopes":["chat"],"created_at":"2026-10-21T03:00:00Z","revoked_at":null}]}"#).unwrap(); // repo-check: allow (upstream UTC input, shown on the Zagreb clock)
        let ring = json::parse(br#"{"cap":42,"items":[{"seq":1,"t":0,"zone":"chill","kind":"dns","source":"s","note":"n"}]}"#).unwrap();
        let mut all = String::new();
        all.push_str(&instances::body(Ok(&inst), Ok(&lap)));
        all.push_str(&instances::body(
            Err((Some(500), "e".into())),
            Err((None, String::new())),
        ));
        let fresh = keys::Done::Minted {
            name: "n".into(),
            key: "gal_sk_x".into(),
        };
        all.push_str(&keys::body(Ok(&ks), Some(&fresh)));
        all.push_str(&keys::body(
            Err((Some(401), "phone required".into())),
            Some(&keys::Done::Revoked(false)),
        ));
        all.push_str(&markers::body(Ok(&ring), "n"));
        all.push_str(&markers::body(Ok(&json::parse(b"{}").unwrap()), ""));
        let defined = crate::css::defined_classes();
        let mut missing = Vec::new();
        for cap in all.split("class=\"").skip(1) {
            for c in cap.split('"').next().unwrap_or("").split_whitespace() {
                let tone = [
                    "online", "offline", "held", "neutral", "chill", "primary", "ghost",
                ]
                .contains(&c);
                if !tone && !defined.iter().any(|d| d == c) {
                    missing.push(c.to_string());
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "unstyled classes: {missing:?}");
    }
}
