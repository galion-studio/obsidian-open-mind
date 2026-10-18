// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! Routing and auth, mirroring `gateway/server.ts#handleRequest`.

use crate::engine::Engine;
use crate::http::{Request, Response};
use crate::offload::{error, request_id, Offload};
use serde_json::{json, Value};

/// The 15 `/v2/*` routes of the gateway's v2 router (not used by Hermes).
const V2_ROUTES: [&str; 15] = [
    "/v2/conversation/add",
    "/v2/conversation/query",
    "/v2/conversation/search",
    "/v2/conversation/delete",
    "/v2/atomic/update",
    "/v2/atomic/query",
    "/v2/atomic/search",
    "/v2/atomic/delete",
    "/v2/scenario/ls",
    "/v2/scenario/read",
    "/v2/scenario/write",
    "/v2/scenario/rm",
    "/v2/core/read",
    "/v2/core/write",
    "/v2/pipeline/status",
];

pub struct App {
    pub engine: Engine,
    pub offload: Offload,
}

fn safe_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = (a.len() ^ b.len()) as u8 | u8::from(a.len() != b.len());
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

enum Auth {
    Ok,
    Missing,
    Invalid,
}

fn resp(status: u16, v: Value) -> Response {
    Response {
        status,
        body: v.to_string(),
    }
}

impl App {
    pub fn new(engine: Engine) -> App {
        let offload = Offload::new(&engine.cfg.data_dir, engine.cfg.read_only);
        App { engine, offload }
    }

    fn verify(&self, req: &Request) -> Auth {
        let Some(expected) = &self.engine.cfg.api_key else {
            return Auth::Ok;
        };
        match req
            .header("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
        {
            None => Auth::Missing,
            Some(t) if t.trim().is_empty() => Auth::Invalid,
            Some(t) if safe_eq(t.trim(), expected) => Auth::Ok,
            Some(_) => Auth::Invalid,
        }
    }

    pub fn handle(&self, req: Request) -> Response {
        let (method, path) = (req.method.as_str(), req.path.as_str());
        if method == "OPTIONS" {
            return Response {
                status: 204,
                body: String::new(),
            };
        }
        let body = || -> Result<Value, Response> {
            if req.body.iter().all(u8::is_ascii_whitespace) {
                return Ok(json!({}));
            }
            serde_json::from_slice(&req.body)
                .map_err(|_| resp(400, json!({ "error": "Invalid JSON body" })))
        };

        if method == "POST" && path == "/v2/instance/destroy" {
            if let Some(r) = self.v1_auth_fail(&req) {
                return r;
            }
            return resp(
                501,
                json!({ "error": "Not implemented in openmind-memory: POST /v2/instance/destroy" }),
            );
        }
        if path.starts_with("/v2/") {
            let rid = request_id();
            match self.verify(&req) {
                Auth::Ok => {}
                Auth::Missing => {
                    return resp(401, error(401, "Unauthorized: missing Bearer token", &rid))
                }
                Auth::Invalid => return resp(401, error(401, "Unauthorized: invalid token", &rid)),
            }
            let is_offload = path.starts_with("/v2/offload/");
            if is_offload || (method == "POST" && V2_ROUTES.contains(&path)) {
                // parseV2Auth: a non-empty Bearer and x-tdai-service-id are always required.
                let bearer = req
                    .header("authorization")
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .map(str::trim)
                    .unwrap_or("");
                if bearer.is_empty() {
                    return resp(
                        401,
                        error(
                            401,
                            "Missing or invalid Authorization header. Expected: Bearer {api_key}",
                            &rid,
                        ),
                    );
                }
                if req
                    .header("x-tdai-service-id")
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    return resp(401, error(401, "Missing x-tdai-service-id header", &rid));
                }
            }
            if is_offload {
                let norm = if path.len() > 1 {
                    path.trim_end_matches('/')
                } else {
                    path
                };
                let b = match body() {
                    Ok(b) => b,
                    Err(r) => return r,
                };
                let (s, v) = match (method, norm) {
                    ("POST", "/v2/offload/ingest") => self.offload.ingest(&b, &rid),
                    ("POST", "/v2/offload/query-mmd") => self.offload.query_mmd(&b, &rid),
                    ("POST", "/v2/offload/read-ref") => self.offload.read_ref(&b, &rid),
                    ("POST", "/v2/offload/compact") => (
                        501,
                        error(
                            501,
                            "Not implemented in openmind-memory: /v2/offload/compact",
                            &rid,
                        ),
                    ),
                    _ => (0, Value::Null),
                };
                if s != 0 {
                    return resp(s, v);
                }
            } else if method == "POST" && V2_ROUTES.contains(&path) {
                return resp(
                    501,
                    error(
                        501,
                        &format!("Not implemented in openmind-memory: {path}"),
                        &rid,
                    ),
                );
            }
        }

        if method == "GET" && path == "/health" {
            return resp(200, self.engine.health());
        }
        if let Some(r) = self.v1_auth_fail(&req) {
            return r;
        }
        let route = |f: &dyn Fn(&Value) -> (u16, Value)| match body() {
            Ok(b) => {
                let (s, v) = f(&b);
                resp(s, v)
            }
            Err(r) => r,
        };
        match (method, path) {
            ("POST", "/recall") => route(&|b| self.engine.recall(b)),
            ("POST", "/capture") => route(&|b| self.engine.capture(b)),
            ("POST", "/search/memories") => route(&|b| self.engine.search_memories(b)),
            ("POST", "/search/conversations") => route(&|b| self.engine.search_conversations(b)),
            ("POST", "/session/end") => route(&|b| self.engine.session_end(b)),
            // growth additions (new paths only; existing routes unchanged)
            ("POST", "/capture/batch") => route(&|b| self.engine.capture_batch(b)),
            ("POST", "/conversations/list") => route(&|b| self.engine.list_conversations(b)),
            ("POST", "/memories/list") => route(&|b| self.engine.list_memories(b)),
            ("GET", "/stats") => resp(200, self.engine.stats_json()),
            ("POST", "/seed") => resp(
                501,
                json!({ "error": "Not implemented in openmind-memory: POST /seed" }),
            ),
            _ => resp(
                404,
                json!({ "error": format!("Not found: {method} {path}") }),
            ),
        }
    }

    fn v1_auth_fail(&self, req: &Request) -> Option<Response> {
        match self.verify(req) {
            Auth::Ok => None,
            Auth::Missing => Some(resp(
                401,
                json!({ "error": "Unauthorized: missing Bearer token" }),
            )),
            Auth::Invalid => Some(resp(401, json!({ "error": "Unauthorized: invalid token" }))),
        }
    }
}
