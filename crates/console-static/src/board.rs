// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Board catalog API (`--local-api`): Rust port of the read-only catalog
//! routes the former Node srvx board (`galion-control` 3.8.3-board,
//! console-ide `src/lib/catalog.ts`, `ledger.ts`, `api-catalog.ts`,
//! `protocol.ts`, `markers.ts`) answered on `/api/v1/*`.
//!
//! Same payload shapes and key order as the TypeScript, so the Rust UI pages
//! (`console-ui`) that GET these paths from their upstream render again.
//! The data is a JSON copy of `fleet.ts` ([`FLEET_JSON`]); the machine target
//! (addresses, ssh keys) is deliberately not part of it.

use console_ui::json::{self, Value};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// `fleet.ts` + `api-catalog.ts` data (no machine target).
pub const FLEET_JSON: &str = include_str!("../board/fleet-board.json");
/// Marker ring size (`markers.ts` MAX).
pub const MARKER_CAP: usize = 42;
/// Longest `source` / `note` accepted on POST /api/v1/markers (chars).
pub const MARKER_TEXT_MAX: usize = 200;
const MARKER_KINDS: &[&str] = &["heartbeat", "boot", "hold", "origin", "dns", "event"];

fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}

fn n(v: usize) -> Value {
    Value::Num(v as f64)
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub seq: u64,
    pub t: u64,
    pub zone: String,
    pub kind: String,
    pub source: String,
    pub note: String,
}

impl Marker {
    fn to_value(&self) -> Value {
        obj(vec![
            ("seq", Value::Num(self.seq as f64)),
            ("t", Value::Num(self.t as f64)),
            ("zone", s(&self.zone)),
            ("kind", s(&self.kind)),
            ("source", s(&self.source)),
            ("note", s(&self.note)),
        ])
    }
}

struct Ring {
    seq: u64,
    items: VecDeque<Marker>,
}

/// Parsed fleet data plus the in-memory marker ring.
pub struct Board {
    fleet: Value,
    ring: Mutex<Ring>,
}

impl Default for Board {
    fn default() -> Self {
        Board::new()
    }
}

impl Board {
    /// Parse the embedded fleet data. Panics only if the embedded file is
    /// broken, which the unit tests catch at build time.
    pub fn new() -> Board {
        let fleet = json::parse(FLEET_JSON.as_bytes()).expect("embedded fleet-board.json");
        Board {
            fleet,
            ring: Mutex::new(Ring {
                seq: 41,
                items: VecDeque::new(),
            }),
        }
    }

    fn arr(&self, key: &str) -> &[Value] {
        self.fleet.get(key).as_array()
    }

    fn version(&self) -> &str {
        self.fleet.get("fleetVersion").as_str().unwrap_or("")
    }

    fn project(&self) -> Value {
        self.fleet.get("project").clone()
    }

    fn owner(&self) -> &str {
        self.fleet.get("githubOwner").as_str().unwrap_or("")
    }

    fn count_where(list: &[Value], key: &str, want: &str) -> usize {
        list.iter()
            .filter(|v| v.get(key).as_str() == Some(want))
            .count()
    }

    /// `domainsPayload()`.
    pub fn domains(&self) -> Value {
        let d = self.arr("domains");
        obj(vec![
            ("count", n(d.len())),
            ("items", Value::Arr(d.to_vec())),
        ])
    }

    /// `servicesPayload()`: items get `zone` (heavy -> chill, else active).
    pub fn services(&self) -> Value {
        let sv = self.arr("services");
        let items = sv
            .iter()
            .map(|x| {
                let mut x = x.clone();
                let zone = if x.get("weight").as_str() == Some("heavy") {
                    "chill"
                } else {
                    "active"
                };
                if let Value::Obj(m) = &mut x {
                    m.push(("zone".into(), s(zone)));
                }
                x
            })
            .collect();
        obj(vec![
            ("count", n(sv.len())),
            ("active", n(Self::count_where(sv, "weight", "light"))),
            (
                "chill",
                n(Self::count_where(sv, "weight", "heavy") + self.arr("chillExtra").len()),
            ),
            ("items", Value::Arr(items)),
        ])
    }

    /// `reposPayload()`.
    pub fn repos(&self) -> Value {
        let r = self.arr("repos");
        obj(vec![
            ("owner", s(self.owner())),
            ("public", n(Self::count_where(r, "visibility", "public"))),
            (
                "privateVisible",
                n(Self::count_where(r, "visibility", "private")),
            ),
            ("items", Value::Arr(r.to_vec())),
        ])
    }

    /// `catalogPayload()` (api-catalog.ts).
    pub fn catalog(&self) -> Value {
        let a = self.arr("apiCatalog");
        obj(vec![
            ("version", s("3.0.0-cpu")),
            ("live", n(Self::count_where(a, "zone", "live"))),
            ("held", n(Self::count_where(a, "zone", "held"))),
            ("items", Value::Arr(a.to_vec())),
        ])
    }

    /// `bootPayload()` (POST /api/v1/boot; nothing is started, as in Node).
    pub fn boot(&self) -> Value {
        let sv = self.arr("services");
        let ids = |f: &dyn Fn(&Value) -> bool| -> Vec<Value> {
            sv.iter()
                .filter(|x| f(x))
                .map(|x| x.get("id").clone())
                .collect()
        };
        let started = ids(&|x| {
            x.get("weight").as_str() == Some("light") && x.get("defaultOn").as_bool() == Some(true)
        });
        let mut chill = ids(&|x| x.get("weight").as_str() == Some("heavy"));
        chill.extend(self.arr("chillExtra").iter().map(|x| x.get("id").clone()));
        obj(vec![
            ("ok", Value::Bool(true)),
            ("mode", s("light")),
            ("zone", s("active")),
            ("project", self.project()),
            ("started", Value::Arr(started)),
            ("chill", Value::Arr(chill)),
        ])
    }

    /// `ledgerPayload()`.
    pub fn ledger(&self) -> Value {
        let apis = self.arr("apiCatalog");
        let principles = [
            "CPU only. No VRAM. No models.",
            "ZeroAI: no login, no visible AI, no collection.",
            "Active zone is catalog + console. Fridge holds GPU and Next apps.",
            "Edge is Cloudflare Tunnel to origin :8080.",
        ];
        obj(vec![
            ("version", s(self.version())),
            ("project", self.project()),
            (
                "principles",
                Value::Arr(principles.iter().map(|p| s(p)).collect()),
            ),
            ("domains", Value::Arr(self.arr("domains").to_vec())),
            ("services", Value::Arr(self.arr("services").to_vec())),
            ("fridge", Value::Arr(self.arr("chillExtra").to_vec())),
            ("repos", Value::Arr(self.arr("repos").to_vec())),
            ("apis", Value::Arr(apis.to_vec())),
            (
                "counts",
                obj(vec![
                    ("domains", n(self.arr("domains").len())),
                    ("services", n(self.arr("services").len())),
                    ("apis", n(apis.len())),
                    ("liveApis", n(Self::count_where(apis, "zone", "live"))),
                    ("heldApis", n(Self::count_where(apis, "zone", "held"))),
                    ("repos", n(self.arr("repos").len())),
                ]),
            ),
        ])
    }

    /// `docsPayload()` (OpenAPI-ish index of the light catalog).
    pub fn docs(&self) -> Value {
        let get = |summary: &str| obj(vec![("get", obj(vec![("summary", s(summary))]))]);
        let post = |summary: &str| obj(vec![("post", obj(vec![("summary", s(summary))]))]);
        obj(vec![
            ("openapi", s("3.0.3")),
            (
                "info",
                obj(vec![
                    ("title", s("Galion light catalog")),
                    ("version", s(self.version())),
                    ("project", self.project()),
                ]),
            ),
            (
                "servers",
                Value::Arr(vec![obj(vec![
                    ("url", s("/")),
                    ("description", s("This origin")),
                ])]),
            ),
            (
                "paths",
                obj(vec![
                    ("/api/v1/health", get("Origin health")),
                    ("/api/v1/domains", get("Domain map")),
                    ("/api/v1/services", get("Active and chill services")),
                    ("/api/v1/repos", get("GitHub inventory")),
                    (
                        "/api/v1/markers",
                        obj(vec![
                            ("get", obj(vec![("summary", s("Stored markers (cap 42)"))])),
                            ("post", obj(vec![("summary", s("Ingest a marker"))])),
                        ]),
                    ),
                    ("/api/v1/markers/stream", get("SSE heartbeats")),
                    ("/api/v1/cloudflare", get("DNS cutover plan")),
                    ("/api/v1/boot", post("Boot the active zone")),
                    ("/api/v1/docs", get("This document")),
                    ("/api/v1/protocol", get("Galion Light Protocol spec")),
                    ("/api/v1/ping", get("Ping public edge (detects 1033)")),
                    ("/api/v1/ledger", get("In-console paper trail")),
                    ("/api/v1/command", post("Talk to the origin")),
                ]),
            ),
        ])
    }

    /// `protocolSpec()` (GLP v1).
    pub fn protocol(&self) -> Value {
        let light = ["health", "services", "domains", "boot", "markers"];
        let held = ["guardian", "workforce", "neural", "voice"];
        let list = |a: &[&str]| Value::Arr(a.iter().map(|x| s(x)).collect());
        obj(vec![
            ("name", s("glp")),
            ("version", n(1)),
            ("transport", list(&["http", "sse"])),
            ("auth", s("none")),
            ("zeroai", Value::Bool(true)),
            ("models", Value::Bool(false)),
            (
                "channels",
                obj(vec![("light", list(&light)), ("held", list(&held))]),
            ),
            (
                "envelope",
                obj(vec![
                    ("v", s("1")),
                    ("seq", s("uint, starts 42")),
                    ("ts", s("unix ms")),
                    ("channel", s(&light.join(" | "))),
                    ("type", s("snapshot | event | heartbeat | subscribe")),
                    ("data", s("object")),
                ]),
            ),
            (
                "routes",
                obj(vec![
                    ("spec", s("GET /api/v1/protocol")),
                    ("snapshot", s("GET /api/v1/protocol/snapshot")),
                    ("stream", s("GET /api/v1/markers/stream")),
                    ("health", s("GET /api/v1/health")),
                    ("services", s("GET /api/v1/services")),
                    ("domains", s("GET /api/v1/domains")),
                ]),
            ),
        ])
    }

    /// GET /api/v1/protocol/snapshot. `health` is this process's honest
    /// health document (not the Node board payload).
    pub fn snapshot(&self, health: Value) -> Value {
        let seq = self
            .ring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .back()
            .map(|m| m.seq)
            .unwrap_or(42);
        let ts = now_ms();
        let frame = |channel: &str, data: Value| {
            obj(vec![
                ("v", n(1)),
                ("seq", Value::Num(seq as f64)),
                ("ts", Value::Num(ts as f64)),
                ("channel", s(channel)),
                ("type", s("snapshot")),
                ("data", data),
            ])
        };
        obj(vec![
            ("v", n(1)),
            ("ts", Value::Num(ts as f64)),
            (
                "frames",
                Value::Arr(vec![
                    frame("health", health),
                    frame("services", self.services()),
                    frame("domains", self.domains()),
                    frame("boot", self.boot()),
                    frame("markers", self.markers()),
                ]),
            ),
        ])
    }

    /// `markersPayload()`.
    pub fn markers(&self) -> Value {
        let g = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        let items: Vec<Value> = g.items.iter().map(Marker::to_value).collect();
        obj(vec![
            ("count", n(items.len())),
            ("cap", n(MARKER_CAP)),
            ("items", Value::Arr(items)),
        ])
    }

    /// `emitMarker()` for POST /api/v1/markers. Heartbeats are numbered but
    /// not stored (as in markers.ts). Unlike the TypeScript, unknown kinds
    /// become `event` and long text is cut, so the ring stays small.
    pub fn emit(&self, body: &Value) -> Marker {
        let text = |key: &str, dflt: &str| -> String {
            let v = body.get(key).as_str().unwrap_or(dflt);
            let v: String = v
                .chars()
                .filter(|c| !c.is_control())
                .take(MARKER_TEXT_MAX)
                .collect();
            if v.is_empty() {
                dflt.to_string()
            } else {
                v
            }
        };
        let zone = if body.get("zone").as_str() == Some("chill") {
            "chill"
        } else {
            "active"
        };
        let kind = body
            .get("kind")
            .as_str()
            .filter(|k| MARKER_KINDS.contains(k))
            .unwrap_or("event");
        let mut g = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        g.seq += 1;
        let m = Marker {
            seq: g.seq,
            t: now_ms(),
            zone: zone.into(),
            kind: kind.into(),
            source: text("source", "api"),
            note: text("note", "ingest"),
        };
        if m.kind != "heartbeat" {
            g.items.push_back(m.clone());
            while g.items.len() > MARKER_CAP {
                g.items.pop_front();
            }
        }
        m
    }

    /// JSON for one emitted marker (201 body).
    pub fn marker_json(m: &Marker) -> String {
        m.to_value().to_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_fleet_parses_with_expected_sizes() {
        let b = Board::new();
        assert_eq!(b.version(), "3.8.3-board");
        assert_eq!(b.arr("domains").len(), 15);
        assert_eq!(b.arr("services").len(), 16);
        assert_eq!(b.arr("chillExtra").len(), 3);
        assert_eq!(b.arr("repos").len(), 20);
        assert_eq!(b.arr("apiCatalog").len(), 37);
        // No machine target / addresses / keys in the embedded copy.
        for bad in ["sshHost", "publicIp", "ssh-ed25519", "authorized_keys"] {
            assert!(!FLEET_JSON.contains(bad), "{bad}");
        }
        assert!(!FLEET_JSON.bytes().any(|c| c == b'\''));
    }

    #[test]
    fn services_shape_and_zone() {
        let b = Board::new();
        let v = b.services();
        let keys: Vec<&str> = match &v {
            Value::Obj(m) => m.iter().map(|(k, _)| k.as_str()).collect(),
            _ => vec![],
        };
        assert_eq!(keys, ["count", "active", "chill", "items"]);
        let items = v.get("items").as_array();
        assert_eq!(items.len() as f64, v.get("count").as_f64().unwrap());
        for it in items {
            let z = it.get("zone").as_str().unwrap();
            let w = it.get("weight").as_str().unwrap();
            assert_eq!(z, if w == "heavy" { "chill" } else { "active" });
            if let Value::Obj(m) = it {
                assert_eq!(m.last().unwrap().0, "zone");
            }
        }
        let heavy = items
            .iter()
            .filter(|i| i.get("weight").as_str() == Some("heavy"))
            .count();
        assert_eq!(v.get("chill").as_f64().unwrap() as usize, heavy + 3);
    }

    #[test]
    fn counts_are_consistent() {
        let b = Board::new();
        let c = b.catalog();
        let total = c.get("items").as_array().len() as f64;
        assert_eq!(
            c.get("live").as_f64().unwrap() + c.get("held").as_f64().unwrap(),
            total
        );
        let l = b.ledger();
        assert_eq!(l.get("counts").get("apis").as_f64(), Some(total));
        assert_eq!(l.get("principles").as_array().len(), 4);
        let r = b.repos();
        assert_eq!(r.get("owner").as_str(), Some("galion-studio"));
        let d = b.domains();
        assert_eq!(d.get("count").as_f64(), Some(15.0));
        let boot = b.boot();
        assert!(boot
            .get("started")
            .as_array()
            .iter()
            .any(|x| x.as_str() == Some("console")));
        assert!(boot
            .get("chill")
            .as_array()
            .iter()
            .any(|x| x.as_str() == Some("helium-browser")));
    }

    #[test]
    fn markers_ring_seq_cap_and_heartbeat() {
        let b = Board::new();
        let first = b.emit(&Value::Null);
        assert_eq!(first.seq, 42);
        assert_eq!(first.kind, "event");
        assert_eq!(first.source, "api");
        assert_eq!(first.note, "ingest");
        let hb = b.emit(&json::parse(br#"{"kind":"heartbeat"}"#).unwrap());
        assert_eq!(hb.seq, 43);
        assert_eq!(b.markers().get("count").as_f64(), Some(1.0));
        for i in 0..60 {
            let body = format!(r#"{{"zone":"chill","kind":"dns","note":"n{i}"}}"#);
            b.emit(&json::parse(body.as_bytes()).unwrap());
        }
        let m = b.markers();
        assert_eq!(m.get("count").as_f64(), Some(42.0));
        assert_eq!(m.get("cap").as_f64(), Some(42.0));
        let items = m.get("items").as_array();
        assert_eq!(items.last().unwrap().get("note").as_str(), Some("n59"));
        assert_eq!(items[0].get("zone").as_str(), Some("chill"));
        let odd = b.emit(&json::parse(br#"{"kind":"rm -rf","source":42}"#).unwrap());
        assert_eq!(odd.kind, "event");
        assert_eq!(odd.source, "api");
        let long = "x".repeat(5000);
        let big = b.emit(&json::parse(format!(r#"{{"note":"{long}"}}"#).as_bytes()).unwrap());
        assert_eq!(big.note.len(), MARKER_TEXT_MAX);
    }

    #[test]
    fn snapshot_frames() {
        let b = Board::new();
        let v = b.snapshot(json::parse(br#"{"status":"healthy"}"#).unwrap());
        let frames = v.get("frames").as_array();
        let ch: Vec<&str> = frames
            .iter()
            .map(|f| f.get("channel").as_str().unwrap())
            .collect();
        assert_eq!(ch, ["health", "services", "domains", "boot", "markers"]);
        assert_eq!(frames[0].get("seq").as_f64(), Some(42.0));
        assert_eq!(
            frames[0].get("data").get("status").as_str(),
            Some("healthy")
        );
        let p = b.protocol();
        assert_eq!(
            p.get("envelope").get("channel").as_str(),
            Some("health | services | domains | boot | markers")
        );
        assert_eq!(
            b.docs().get("info").get("version").as_str(),
            Some("3.8.3-board")
        );
    }
}
