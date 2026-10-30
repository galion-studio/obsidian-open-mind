// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--hermes-api`: the Hermes research adapter (owner rulings t359u and
//! t360u, 2026-10-29 CET +01:00).
//!
//! The browser talks to `/api/hermes/*` on the console; this module turns
//! those calls into hermes-rs gateway calls on loopback (`--hermes`, live
//! 127.0.0.1:8642) and adds the gateway key (`API_SERVER_KEY`) on the
//! server. The key is read once at start from the environment
//! (`HERMES_API_KEY` / `API_SERVER_KEY`, the names the Node console used) or
//! from `--hermes-key-file` (the gateway's own env file); it is never put in
//! a reply, a log line or an error message.
//!
//! Every route here runs **after** the local API gate in
//! [`crate::api::try_handle`]: a non-console Host, a loopback Host from a
//! remote peer, or a console Host without a paired `galion_admin` cookie gets
//! `401 {"error":"phone required"}` before this module is reached.
//!
//! Routes (all JSON, errors are `{"ok":false,"error":..,"route":..}`):
//! * `GET health`: gateway `/health` + `/health/detailed`;
//! * `GET spend`, `GET guard`: the console's research spend guard (cap
//!   [`CAP_USD`], warning at [`WARN_USD`]) next to the gateway's own caps;
//!   guard toggles are not a gateway feature, so `POST guard` is 405;
//! * `GET models`, `GET providers`, `GET modes`, `GET skills`;
//! * `GET history` (gateway `/api/sessions`), `GET history?id=` (one thread);
//! * `POST ask` (alias `POST run`): spend check, then gateway `POST /v1/runs`;
//! * `GET runs/ID`, `GET runs/ID/events` (SSE, streamed), `POST runs/ID/stop`.
//!
//! Provider picker (t360u): Local laptop, RunPod, OpenRouter (default),
//! River AI. hermes-rs falls back to its OpenRouter settings for a provider
//! it does not know, so only providers listed in `--hermes-providers` are
//! ever sent; the rest show "not set up". The guard never calls a model; if
//! it ever needs one it is pinned to [`GUARD_PROVIDER`] (River AI), never
//! OpenRouter.

use console_ui::client::{self, Forward};
use console_ui::json::{self, obj, Value};
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The console's Hermes research spend cap (USD, all console-started
/// research sessions together).
pub const CAP_USD: f64 = 20.0;
/// Spend at which the page warns.
pub const WARN_USD: f64 = 16.0;
/// Session-key prefix of threads started from this page.
pub const SESSION_KEY_PREFIX: &str = "console:hermes:";
/// Provider the picker selects first.
pub const DEFAULT_PROVIDER: &str = "openrouter";
/// The guard stays on River AI (t360u): never OpenRouter.
pub const GUARD_PROVIDER: &str = "river";
/// Largest question accepted (characters).
pub const MAX_INPUT: usize = 12_000;
/// Threads listed.
pub const HISTORY_LIMIT: usize = 200;
/// Sessions read for the spend total.
pub const SPEND_SCAN: usize = 500;
/// Laptop TCP probe budget.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(700);

/// Research instructions, kept from the Node console (`startHermesRun`).
pub const INSTRUCTIONS: &str = "You are Hermes Research, a colleague, not a parent. Answer the machine the user asked about. A question about H200s or a future fleet is not a question about the laptop or this VPS. The laptop is offline. Do not describe it. Do not say the goal is not possible. Give the path that makes it real. Open with that path, in plain sentences, no headers and no lecture. Search first and use only figures a tool returned. Do not assign a task. Do not recite memory or file paths.";

/// One picker choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    /// Gateway provider name (`provider` in `POST /v1/runs`).
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
}

/// The four picker choices, in picker order (t360u).
pub const PROVIDERS: &[Provider] = &[
    Provider {
        id: "laptop",
        label: "Local laptop",
        note: "Mate's DeepSeek model on the laptop",
    },
    Provider {
        id: "runpod",
        label: "RunPod",
        note: "GPU pod",
    },
    Provider {
        id: "openrouter",
        label: "OpenRouter",
        note: "paid research model (gateway default)",
    },
    Provider {
        id: "river",
        label: "River AI",
        note: "River AI (also the guard's provider)",
    },
];

/// Research depth (gateway `model_options.reasoning.effort`).
pub const MODES: &[(&str, &str, &str)] = &[
    ("deep", "Deep research", "high"),
    ("quick", "Quick answer", "low"),
];

/// A usable key: 16+ characters and not a sealed placeholder (Node rule).
pub fn usable_key(raw: &str) -> Option<String> {
    let v = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if v.len() < 16 || v.to_ascii_lowercase().contains("sealed") || v.contains(char::is_whitespace)
    {
        return None;
    }
    Some(v.to_string())
}

/// `API_SERVER_KEY=` (or `HERMES_API_KEY=`) from env-file text.
pub fn key_from_env_text(text: &str) -> Option<String> {
    ["API_SERVER_KEY", "HERMES_API_KEY"]
        .iter()
        .find_map(|name| named_key_from_env_text(text, name))
}

/// `NAME=value` from env-file text, if the value is a usable key.
pub fn named_key_from_env_text(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        let l = l.strip_prefix("export ").unwrap_or(l);
        if let Some(v) = l.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
            if let Some(k) = usable_key(v) {
                return Some(k);
            }
        }
    }
    None
}

/// River AI key for the model list (`RIVER_API_KEY`): env first, then the
/// gateway env file. Never logged.
pub fn load_river_key(file: Option<&Path>) -> Option<String> {
    if let Some(k) = std::env::var("RIVER_API_KEY")
        .ok()
        .and_then(|v| usable_key(&v))
    {
        return Some(k);
    }
    let text = std::fs::read_to_string(file?).ok()?;
    named_key_from_env_text(&text, "RIVER_API_KEY")
}

/// Key lookup order: env `HERMES_API_KEY`, env `API_SERVER_KEY`, then the
/// key file. Never logged.
pub fn load_key(file: Option<&Path>) -> Option<String> {
    for name in ["HERMES_API_KEY", "API_SERVER_KEY"] {
        if let Some(k) = std::env::var(name).ok().and_then(|v| usable_key(&v)) {
            return Some(k);
        }
    }
    let text = std::fs::read_to_string(file?).ok()?;
    key_from_env_text(&text)
}

/// A reply from this module.
#[derive(Debug, Clone, PartialEq)]
pub struct Out {
    pub status: u16,
    pub body: String,
}

/// What the server should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Reply(Out),
    /// Stream this gateway path (SSE) to the client with the key added.
    Stream {
        target: String,
    },
}

/// The adapter. `Debug` never shows the key.
pub struct Hermes {
    pub addr: SocketAddr,
    key: Option<String>,
    pub providers: Vec<String>,
    pub laptop: Option<SocketAddr>,
    /// Provider model catalogs (t364u/t365u picker).
    pub catalogs: crate::hermes_models::Catalogs,
    /// Saved picker choices and favourites.
    pub prefs: crate::hermes_models::PrefsStore,
    /// The owner-only self-modify switch (t400u), read-only here.
    pub self_modify: std::path::PathBuf,
}

impl std::fmt::Debug for Hermes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hermes")
            .field("addr", &self.addr)
            .field("key", &self.key.as_ref().map(|_| "<set>"))
            .field("providers", &self.providers)
            .field("laptop", &self.laptop)
            .field("catalogs", &self.catalogs)
            .field("prefs", &self.prefs.path)
            .field("self_modify", &self.self_modify)
            .finish()
    }
}

/// Where the owner-only self-modify switch lives (galion-self-modify crate).
pub const SELF_MODIFY_PATH: &str = "/etc/galion/hermes-self-modify.json";

/// The self-modify switch as the page shows it. Read-only. Locked is the
/// default and the fail-safe: a missing, unreadable, malformed or unsigned
/// `allowed` record shows as locked. The console does not verify the owner
/// signature (it has no crypto); `galion-self-modify status` does, and an
/// `allowed` record shows as allowed only when it carries a signature.
pub fn self_modify_value(path: &Path) -> Value {
    let rec = std::fs::metadata(path)
        .ok()
        .filter(|m| m.len() <= 16 * 1024)
        .and_then(|_| std::fs::read(path).ok())
        .and_then(|b| json::parse(&b).ok());
    let get = |k: &str| {
        rec.as_ref()
            .and_then(|v| v.get(k).as_str().map(str::to_string))
            .unwrap_or_default()
    };
    let signed = get("signature").len() == 128 && get("key_id").len() == 16;
    let allowed = get("version") == "galion/self-modify/v1" && get("state") == "allowed" && signed;
    obj(&[
        ("ok", b(true)),
        ("state", s(if allowed { "allowed" } else { "locked" })),
        ("owner_only", b(true)),
        ("found", b(rec.is_some())),
        ("changed_by", s(&get("changed_by"))),
        ("changed_at", s(&get("changed_at"))),
        ("signed", b(signed)),
        (
            "tip",
            s(if allowed {
                "The owner (admin) allowed Hermes to change its own persona, config and code. The owner can lock it again."
            } else {
                "Hermes cannot change its own persona, config or code. Only the owner (admin) can allow it, with the owner key."
            }),
        ),
    ])
}

/// True for a path this module answers (query ignored).
pub fn owns(path: &str) -> bool {
    bare(path).starts_with("/api/hermes/")
}

/// Whether the server must read a body before [`Hermes::handle`].
pub fn wants_body(method: &str, path: &str) -> bool {
    method == "POST" && owns(path)
}

fn bare(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// A run id from the gateway (`run_` + hex) or a session id: 1..=128 of
/// `[A-Za-z0-9._:-]`.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}

fn n(v: f64) -> Value {
    Value::Num(if v.is_finite() { v } else { 0.0 })
}

fn b(v: bool) -> Value {
    Value::Bool(v)
}

/// Error body: `{"ok":false,"error":msg,"route":path}`.
pub fn error(status: u16, msg: &str, route: &str) -> Out {
    Out {
        status,
        body: obj(&[
            ("ok", b(false)),
            ("error", s(msg)),
            ("route", s(bare(route))),
        ])
        .to_json(),
    }
}

fn ok(v: Value) -> Out {
    Out {
        status: 200,
        body: v.to_json(),
    }
}

/// Merge `extra` into an object value.
fn with(mut v: Value, extra: &[(&str, Value)]) -> Value {
    if let Value::Obj(pairs) = &mut v {
        for (k, x) in extra {
            pairs.retain(|(pk, _)| pk != k);
            pairs.push((k.to_string(), x.clone()));
        }
    }
    v
}

/// Gateway failure classes.
#[derive(Debug, Clone, PartialEq)]
pub enum GwErr {
    NoKey,
    Timeout,
    Down,
    Bad,
}

impl GwErr {
    fn out(&self, route: &str) -> Out {
        match self {
            GwErr::NoKey => error(
                503,
                "the gateway key is not configured on the console server",
                route,
            ),
            GwErr::Timeout => error(504, "the Hermes gateway did not answer in time", route),
            GwErr::Down => error(502, "the Hermes gateway is not reachable", route),
            GwErr::Bad => error(502, "the Hermes gateway sent an unreadable answer", route),
        }
    }
}

/// The gateway's own error message (`{"error":{"message":..}}`), if any.
fn gw_message(v: &Value) -> Option<String> {
    let e = v.get("error");
    e.get("message")
        .as_str()
        .or_else(|| e.as_str())
        .map(|m| m.chars().take(300).collect())
}

/// Spend as the guard sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spend {
    /// Console research total (sessions keyed [`SESSION_KEY_PREFIX`]).
    pub spent_usd: f64,
    pub sessions: usize,
    /// Gateway `/api/spend` (day totals and its caps), as sent.
    pub gateway: Value,
}

impl Spend {
    pub fn level(&self) -> &'static str {
        level(self.spent_usd)
    }

    pub fn to_value(&self) -> Value {
        let g = &self.gateway;
        obj(&[
            ("ok", b(true)),
            ("cap_usd", n(CAP_USD)),
            ("warn_usd", n(WARN_USD)),
            ("spent_usd", n(self.spent_usd)),
            ("left_usd", n((CAP_USD - self.spent_usd).max(0.0))),
            ("level", s(self.level())),
            ("sessions", n(self.sessions as f64)),
            ("day", g.get("day").clone()),
            ("day_usd", n(g.get("day_usd").as_f64().unwrap_or(0.0))),
            ("day_calls", n(g.get("day_calls").as_f64().unwrap_or(0.0))),
            ("gateway_caps", g.get("caps").clone()),
        ])
    }
}

/// `ok` under the warning, `warn` from [`WARN_USD`], `blocked` at [`CAP_USD`].
pub fn level(spent: f64) -> &'static str {
    if spent >= CAP_USD {
        "blocked"
    } else if spent >= WARN_USD {
        "warn"
    } else {
        "ok"
    }
}

/// Sum of `estimated_cost_usd` over console research sessions.
pub fn console_spend(sessions: &Value) -> (f64, usize) {
    let mut total = 0.0;
    let mut k = 0;
    for r in sessions.get("data").as_array() {
        if r.get("session_key")
            .as_str()
            .is_some_and(|x| x.starts_with(SESSION_KEY_PREFIX))
        {
            let c = r.get("estimated_cost_usd").as_f64().unwrap_or(0.0);
            if c.is_finite() && c > 0.0 {
                total += c;
            }
            k += 1;
        }
    }
    (total, k)
}

/// Session keys galion-one (one.galion.app and its voice line) sends to the
/// gateway as `X-Hermes-Session-Key`. Read-only: the stored rows are not
/// rewritten; older rows carry the same key, so they are labelled too.
pub const GALION_ONE_KEYS: [&str; 2] = ["galion-one", "one-warm"];

/// Where a session came from, from its gateway `session_key`:
/// `"galion-one"`, `"console"` or `""` (anything else).
pub fn session_origin(key: &str) -> &'static str {
    if GALION_ONE_KEYS.contains(&key) || key.starts_with("galion-one:") {
        "galion-one"
    } else if key.starts_with(SESSION_KEY_PREFIX) {
        "console"
    } else {
        ""
    }
}

/// Gateway session row -> history item.
pub fn history_item(r: &Value) -> Value {
    let input = r.get("input_tokens").as_f64().unwrap_or(0.0);
    let output = r.get("output_tokens").as_f64().unwrap_or(0.0);
    let id = r.get("id").as_str().unwrap_or("");
    let title = r
        .get("title")
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(id);
    let at = r
        .get("ended_at")
        .as_f64()
        .filter(|x| *x > 0.0)
        .or_else(|| r.get("started_at").as_f64())
        .unwrap_or(0.0);
    obj(&[
        ("id", s(id)),
        ("title", s(title)),
        ("count", n(r.get("message_count").as_f64().unwrap_or(0.0))),
        ("at", n(at)),
        ("started_at", n(r.get("started_at").as_f64().unwrap_or(0.0))),
        ("source", s(r.get("source").as_str().unwrap_or("session"))),
        ("model", s(r.get("model").as_str().unwrap_or(""))),
        (
            "provider",
            s(r.get("billing_provider").as_str().unwrap_or("")),
        ),
        (
            "origin",
            s(session_origin(r.get("session_key").as_str().unwrap_or(""))),
        ),
        (
            "console",
            b(r.get("session_key")
                .as_str()
                .is_some_and(|x| x.starts_with(SESSION_KEY_PREFIX))),
        ),
        (
            "cost_usd",
            n(r.get("estimated_cost_usd").as_f64().unwrap_or(0.0)),
        ),
        (
            "tokens",
            obj(&[
                ("input", n(input)),
                ("output", n(output)),
                ("total", n(input + output)),
            ]),
        ),
    ])
}

fn content_text(c: &Value) -> String {
    match c {
        Value::Str(t) => t.clone(),
        Value::Arr(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Gateway messages -> thread items. Tool output is not sent to the page.
pub fn thread_items(msgs: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for m in msgs.get("data").as_array() {
        let role = m.get("role").as_str().unwrap_or("");
        match role {
            "user" | "assistant" => {
                let text = content_text(m.get("content"));
                for tc in m.get("tool_calls").as_array() {
                    let name = tc
                        .get("function")
                        .get("name")
                        .as_str()
                        .or_else(|| tc.get("name").as_str())
                        .unwrap_or("tool");
                    out.push(obj(&[("kind", s("tool")), ("name", s(name))]));
                }
                if !text.trim().is_empty() {
                    out.push(obj(&[("kind", s(role)), ("text", s(&text))]));
                }
            }
            "tool" => {
                let name = m.get("name").as_str().unwrap_or("tool");
                out.push(obj(&[
                    ("kind", s("tool")),
                    ("name", s(name)),
                    ("status", s("done")),
                ]));
            }
            _ => {}
        }
    }
    out
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A fresh console session key (not secret; it only groups a thread).
pub fn new_session_key() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let k = SEQ.fetch_add(1, Ordering::SeqCst);
    let mixed = crate::sha256::hex(format!("{t}:{k}:{}", std::process::id()).as_bytes());
    format!("{SESSION_KEY_PREFIX}{}", &mixed[..20])
}

/// A model id the page may send ([`crate::hermes_models::model_ok`]).
fn model_ok(m: &str) -> bool {
    crate::hermes_models::model_ok(m)
}

impl Hermes {
    pub fn new(
        addr: SocketAddr,
        key: Option<String>,
        providers: Vec<String>,
        laptop: Option<SocketAddr>,
    ) -> Hermes {
        Hermes {
            addr,
            key,
            providers,
            laptop,
            catalogs: crate::hermes_models::Catalogs::new(Default::default(), None),
            prefs: crate::hermes_models::PrefsStore::new(None),
            self_modify: SELF_MODIFY_PATH.into(),
        }
    }

    /// Picker setup: catalog sources, the River key (server-side only) and
    /// the prefs file.
    pub fn with_models(
        mut self,
        sources: crate::hermes_models::Sources,
        river_key: Option<String>,
        prefs: Option<std::path::PathBuf>,
    ) -> Hermes {
        self.catalogs = crate::hermes_models::Catalogs::new(sources, river_key);
        self.prefs = crate::hermes_models::PrefsStore::new(prefs);
        self
    }

    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// `Authorization` value for a streamed call. Server-side only.
    pub fn auth_header(&self) -> Option<String> {
        self.key.as_ref().map(|k| format!("Bearer {k}"))
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        session_key: Option<&str>,
    ) -> Result<(u16, Value), GwErr> {
        let auth = self.auth_header().ok_or(GwErr::NoKey)?;
        let fwd = Forward {
            host: self.addr.to_string(),
            ..Forward::default()
        };
        let mut extra: Vec<(&str, &str)> = vec![("Authorization", auth.as_str())];
        if let Some(k) = session_key {
            extra.push(("X-Hermes-Session-Key", k));
        }
        let r = client::call_with(
            self.addr,
            method,
            path,
            &fwd,
            &extra,
            body.map(|x| ("application/json", x)),
        )
        .map_err(|e| match e.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => GwErr::Timeout,
            io::ErrorKind::InvalidData => GwErr::Bad,
            _ => GwErr::Down,
        })?;
        let v = json::parse(&r.body).map_err(|_| GwErr::Bad)?;
        Ok((r.status, v))
    }

    fn get(&self, path: &str) -> Result<Value, (u16, String, GwErr)> {
        match self.call("GET", path, None, None) {
            Ok((200, v)) => Ok(v),
            Ok((code, v)) => Err((
                code,
                gw_message(&v).unwrap_or_else(|| format!("gateway answered {code}")),
                GwErr::Bad,
            )),
            Err(e) => Err((0, String::new(), e)),
        }
    }

    fn get_out(&self, path: &str, route: &str) -> Result<Value, Out> {
        self.get(path).map_err(|(code, msg, e)| {
            if code == 0 {
                e.out(route)
            } else {
                error(if code == 401 { 502 } else { code }, &msg, route)
            }
        })
    }

    /// Spend guard figures (gateway `/api/spend` + console sessions).
    pub fn spend(&self) -> Result<Spend, Out> {
        let route = "/api/hermes/spend";
        let gateway = self.get_out("/api/spend", route)?;
        let sessions = self.get_out(&format!("/api/sessions?limit={SPEND_SCAN}"), route)?;
        let (spent_usd, k) = console_spend(&sessions);
        Ok(Spend {
            spent_usd,
            sessions: k,
            gateway,
        })
    }

    /// Laptop reachability: `None` = no probe configured.
    pub fn laptop_online(&self) -> Option<bool> {
        self.laptop
            .map(|a| TcpStream::connect_timeout(&a, PROBE_TIMEOUT).is_ok())
    }

    /// `(enabled, status)` for a picker choice: status is `ready`,
    /// `offline`, or `not set up`.
    pub fn provider_state(&self, id: &str, laptop: Option<bool>) -> (bool, &'static str) {
        if !self.providers.iter().any(|p| p == id) {
            return (false, "not set up");
        }
        if id == "laptop" && laptop != Some(true) {
            return (false, "offline");
        }
        (true, "ready")
    }

    pub fn providers_value(&self) -> Value {
        let laptop = if self.providers.iter().any(|p| p == "laptop") {
            self.laptop_online()
        } else {
            None
        };
        let rows: Vec<Value> = PROVIDERS
            .iter()
            .map(|p| {
                let (enabled, status) = self.provider_state(p.id, laptop);
                obj(&[
                    ("id", s(p.id)),
                    ("label", s(p.label)),
                    ("note", s(p.note)),
                    ("enabled", b(enabled)),
                    ("status", s(status)),
                    ("default", b(p.id == DEFAULT_PROVIDER)),
                ])
            })
            .collect();
        obj(&[
            ("ok", b(true)),
            ("default", s(DEFAULT_PROVIDER)),
            ("guard_provider", s(GUARD_PROVIDER)),
            ("providers", Value::Arr(rows)),
        ])
    }

    fn health(&self) -> Out {
        let started = Instant::now();
        let fwd = Forward {
            host: self.addr.to_string(),
            ..Forward::default()
        };
        let plain = client::call(self.addr, "GET", "/health", &fwd, None);
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let (up, status, raw) = match &plain {
            Ok(r) => (
                r.status == 200,
                r.status,
                String::from_utf8_lossy(&r.body).chars().take(400).collect(),
            ),
            Err(_) => (false, 0, String::new()),
        };
        let rawv = json::parse(raw.as_bytes()).unwrap_or(Value::Null);
        let detailed = if up && self.has_key() {
            self.get("/health/detailed").ok()
        } else {
            None
        };
        let d = detailed.clone().unwrap_or(Value::Null);
        let mut v = obj(&[
            ("ok", b(up)),
            ("status", n(status as f64)),
            ("ms", n((ms * 10.0).round() / 10.0)),
            ("loopback", b(self.addr.ip().is_loopback())),
            ("mode", s(if up { "gateway" } else { "down" })),
            (
                "implementation",
                s(rawv.get("implementation").as_str().unwrap_or("")),
            ),
            (
                "version",
                s(rawv.get("implementation_version").as_str().unwrap_or("")),
            ),
            ("key", b(self.has_key())),
            ("auth_ok", b(detailed.is_some())),
            ("active_agents", d.get("active_agents").clone()),
            ("max_concurrent_runs", d.get("max_concurrent_runs").clone()),
            ("raw", s(&raw)),
        ]);
        if !up {
            v = with(
                v,
                &[(
                    "error",
                    s(if plain.is_err() {
                        "the Hermes gateway is not reachable"
                    } else {
                        "the Hermes gateway is not healthy"
                    }),
                )],
            );
        }
        Out {
            status: 200,
            body: v.to_json(),
        }
    }

    fn guard(&self) -> Out {
        let spend = self.spend();
        let mut v = obj(&[
            ("ok", b(true)),
            ("on", b(true)),
            ("autoAccept", b(false)),
            ("approvals", s("off")),
            ("toggles", b(false)),
            ("provider", s(GUARD_PROVIDER)),
            ("cap_usd", n(CAP_USD)),
            ("warn_usd", n(WARN_USD)),
            (
                "note",
                s("Console spend guard: new asks stop at the research cap; the gateway's own session, daily and call caps stay in force. The guard calls no model; it is pinned to River AI, never OpenRouter. Toggles are not a gateway feature."),
            ),
        ]);
        match spend {
            Ok(sp) => {
                v = with(
                    v,
                    &[
                        ("level", s(sp.level())),
                        ("spent_usd", n(sp.spent_usd)),
                        ("spend", sp.to_value()),
                    ],
                )
            }
            Err(e) => {
                v = with(
                    v,
                    &[
                        ("level", s("unknown")),
                        (
                            "spend",
                            json::parse(e.body.as_bytes()).unwrap_or(Value::Null),
                        ),
                    ],
                )
            }
        }
        ok(v)
    }

    /// `GET models?provider=openrouter|river`: that provider's catalog
    /// (server-side fetch, cached). Without `provider`: the gateway aliases.
    fn provider_models(&self, provider: &str, route: &str) -> Out {
        let Some(p) = PROVIDERS.iter().find(|x| x.id == provider) else {
            return error(400, "unknown provider", route);
        };
        let (enabled, status) = self.provider_state(p.id, None);
        let default_name = match p.id {
            "openrouter" => "Gateway default (paid research model)",
            "river" => "River AI default",
            _ => "Default",
        };
        let base = |models: Vec<Value>, extra: &[(&str, Value)]| {
            let mut pairs: Vec<(&str, Value)> = vec![
                ("ok", b(true)),
                ("provider", s(p.id)),
                ("label", s(p.label)),
                ("enabled", b(enabled || p.id == "laptop")),
                ("status", s(status)),
                ("default", obj(&[("id", s("")), ("name", s(default_name))])),
                ("count", n(models.len() as f64)),
                ("models", Value::Arr(models)),
            ];
            pairs.extend(extra.iter().cloned());
            ok(obj(&pairs))
        };
        if !crate::hermes_models::CATALOG_PROVIDERS.contains(&p.id) {
            return base(
                Vec::new(),
                &[("note", s("this provider has no model list"))],
            );
        }
        match self.catalogs.get(p.id) {
            Ok(c) => {
                let mut models = c.models.clone();
                if p.id == DEFAULT_PROVIDER {
                    // Gateway aliases (e.g. `galion`) first, as before.
                    let aliases: Vec<Value> = self
                        .get("/v1/models")
                        .map(|v| {
                            v.get("data")
                                .as_array()
                                .iter()
                                .filter_map(|m| {
                                    let id = m.get("id").as_str()?;
                                    if id == "hermes-agent" || !model_ok(id) {
                                        return None;
                                    }
                                    let root = m.get("root").as_str().unwrap_or("");
                                    Some(obj(&[
                                        ("id", s(id)),
                                        (
                                            "name",
                                            s(&if root.is_empty() || root == id {
                                                format!("{id} (gateway alias)")
                                            } else {
                                                format!("{id} (gateway alias · {root})")
                                            }),
                                        ),
                                        ("alias", b(true)),
                                    ]))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut all = aliases;
                    all.append(&mut models);
                    models = all;
                }
                let mut extra = vec![("fetched_at", n(c.fetched_at)), ("stale", b(c.stale))];
                if let Some(e) = &c.error {
                    extra.push(("error", s(e)));
                }
                base(models, &extra)
            }
            Err(e) => error(
                502,
                &format!("the {} model list did not load: {e}", p.label),
                route,
            ),
        }
    }

    /// Whether `m` may be sent as the model for provider `p` (a catalog
    /// entry, or a gateway alias for OpenRouter). `Err` = could not check.
    fn model_offered(&self, p: &str, m: &str) -> Result<bool, String> {
        if p == DEFAULT_PROVIDER {
            let alias = self
                .get("/v1/models")
                .map(|v| {
                    v.get("data")
                        .as_array()
                        .iter()
                        .any(|x| x.get("id").as_str() == Some(m))
                })
                .unwrap_or(false);
            if alias {
                return Ok(true);
            }
        }
        self.catalogs.get(p).map(|c| c.has(m))
    }

    fn models(&self, route: &str) -> Out {
        match self.get_out("/v1/models", route) {
            Ok(v) => {
                let models: Vec<Value> = v
                    .get("data")
                    .as_array()
                    .iter()
                    .map(|m| {
                        let id = m.get("id").as_str().unwrap_or("");
                        let root = m.get("root").as_str().unwrap_or("");
                        obj(&[
                            ("id", s(id)),
                            (
                                "name",
                                s(if id == "hermes-agent" {
                                    "Gateway default"
                                } else {
                                    id
                                }),
                            ),
                            ("root", s(root)),
                        ])
                    })
                    .collect();
                ok(obj(&[
                    ("ok", b(true)),
                    ("default", s("hermes-agent")),
                    ("models", Value::Arr(models)),
                ]))
            }
            Err(e) => e,
        }
    }

    fn modes() -> Out {
        let rows: Vec<Value> = MODES
            .iter()
            .map(|(id, label, effort)| {
                obj(&[("id", s(id)), ("label", s(label)), ("effort", s(effort))])
            })
            .collect();
        ok(obj(&[
            ("ok", b(true)),
            ("default", s(MODES[0].0)),
            ("modes", Value::Arr(rows)),
        ]))
    }

    fn history(&self, query: &str, route: &str) -> Out {
        let id = query
            .split('&')
            .find_map(|kv| kv.strip_prefix("id="))
            .map(|v| v.replace("%3A", ":").replace("%3a", ":"));
        match id {
            None => match self.get_out(&format!("/api/sessions?limit={HISTORY_LIMIT}"), route) {
                Ok(v) => {
                    let items: Vec<Value> =
                        v.get("data").as_array().iter().map(history_item).collect();
                    ok(obj(&[("ok", b(true)), ("items", Value::Arr(items))]))
                }
                Err(e) => e,
            },
            Some(id) if !id_ok(&id) => error(400, "that thread id is not valid", route),
            Some(id) => {
                let meta = match self.get_out(&format!("/api/sessions/{id}"), route) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                let msgs = match self.get_out(&format!("/api/sessions/{id}/messages"), route) {
                    Ok(v) => v,
                    Err(e) => return e,
                };
                let row = match meta.get("data") {
                    d @ Value::Obj(_) => d.clone(),
                    _ => match meta.get("session") {
                        s2 @ Value::Obj(_) => s2.clone(),
                        _ => meta.clone(),
                    },
                };
                let h = history_item(&row);
                ok(with(
                    h.clone(),
                    &[("ok", b(true)), ("items", Value::Arr(thread_items(&msgs)))],
                ))
            }
        }
    }

    /// `POST ask`: validate, check the spend guard, start a gateway run.
    pub fn ask(&self, body: &[u8], route: &str) -> Out {
        let req = match json::parse(body) {
            Ok(v @ Value::Obj(_)) => v,
            _ => return error(400, "send JSON: {\"input\": \"your question\"}", route),
        };
        let input = req.get("input").as_str().unwrap_or("").trim().to_string();
        if input.is_empty() {
            return error(400, "the question is empty", route);
        }
        if input.chars().count() > MAX_INPUT {
            return error(
                413,
                "the question is too long (12000 characters at most)",
                route,
            );
        }
        let provider = req
            .get("provider")
            .as_str()
            .filter(|p| !p.is_empty())
            .unwrap_or(DEFAULT_PROVIDER);
        let Some(p) = PROVIDERS.iter().find(|x| x.id == provider) else {
            return error(400, "unknown provider", route);
        };
        let laptop = if p.id == "laptop" {
            self.laptop_online()
        } else {
            None
        };
        match self.provider_state(p.id, laptop) {
            (true, _) => {}
            (false, "offline") => {
                return error(503, &format!("{} is offline right now", p.label), route)
            }
            (false, _) => {
                return error(
                    409,
                    &format!("{} is not set up on the gateway yet", p.label),
                    route,
                )
            }
        }
        let model = req
            .get("model")
            .as_str()
            .filter(|m| !m.is_empty() && *m != "hermes-agent");
        if let Some(m) = model {
            if !crate::hermes_models::CATALOG_PROVIDERS.contains(&p.id) || !model_ok(m) {
                return error(
                    400,
                    "a model can only be picked for OpenRouter or River AI",
                    route,
                );
            }
            match self.model_offered(p.id, m) {
                Ok(true) => {}
                Ok(false) => {
                    return error(
                        400,
                        &format!("that model is not offered by {}", p.label),
                        route,
                    )
                }
                Err(e) => {
                    return error(
                        503,
                        &format!("could not check the {} model list: {e}", p.label),
                        route,
                    )
                }
            }
        }
        let mode = req.get("mode").as_str().unwrap_or(MODES[0].0);
        let effort = MODES
            .iter()
            .find(|(id, _, _)| *id == mode)
            .map(|(_, _, e)| *e)
            .unwrap_or(MODES[0].2);
        let session_id = req.get("session_id").as_str().filter(|x| !x.is_empty());
        if session_id.is_some_and(|x| !id_ok(x)) {
            return error(400, "that thread id is not valid", route);
        }
        // Spend guard: fail closed when the figures cannot be read.
        let spend = match self.spend() {
            Ok(sp) => sp,
            Err(e) => {
                let why = json::parse(e.body.as_bytes())
                    .ok()
                    .and_then(|v| v.get("error").as_str().map(str::to_string))
                    .unwrap_or_default();
                return error(
                    503,
                    &format!("spend guard: could not read spend, not asking ({why})"),
                    route,
                );
            }
        };
        if spend.level() == "blocked" {
            let mut o = error(
                429,
                &format!(
                    "research cap reached: ${:.2} of ${CAP_USD:.0} spent",
                    spend.spent_usd
                ),
                route,
            );
            if let Ok(v) = json::parse(o.body.as_bytes()) {
                o.body = with(v, &[("spend", spend.to_value())]).to_json();
            }
            return o;
        }
        let mut pairs: Vec<(&str, Value)> = vec![
            ("input", s(&input)),
            ("instructions", s(INSTRUCTIONS)),
            ("provider", s(p.id)),
            (
                "model_options",
                obj(&[(
                    "reasoning",
                    obj(&[("enabled", b(true)), ("effort", s(effort))]),
                )]),
            ),
        ];
        if let Some(m) = model {
            pairs.push(("model", s(m)));
        }
        if let Some(sid) = session_id {
            pairs.push(("session_id", s(sid)));
        }
        let key = if session_id.is_none() {
            Some(new_session_key())
        } else {
            None
        };
        let body = obj(&pairs).to_json();
        match self.call("POST", "/v1/runs", Some(body.as_bytes()), key.as_deref()) {
            Ok((code, v)) if code == 200 || code == 202 => {
                let run_id = v.get("run_id").as_str().unwrap_or("");
                Out {
                    status: 202,
                    body: obj(&[
                        ("ok", b(true)),
                        ("run_id", s(run_id)),
                        ("status", s(v.get("status").as_str().unwrap_or("started"))),
                        ("provider", s(p.id)),
                        ("model", s(model.unwrap_or(""))),
                        ("mode", s(mode)),
                        ("warn", b(spend.level() == "warn")),
                        ("spend", spend.to_value()),
                    ])
                    .to_json(),
                }
            }
            Ok((code, v)) => error(
                if code == 401 { 502 } else { code },
                &gw_message(&v).unwrap_or_else(|| format!("gateway answered {code}")),
                route,
            ),
            Err(e) => e.out(route),
        }
    }

    /// Route one request (already past the API gate).
    pub fn handle(&self, method: &str, path: &str, query: &str, body: &[u8]) -> Action {
        let p = bare(path);
        let route = p;
        let tail = p.strip_prefix("/api/hermes/").unwrap_or("");
        let seg: Vec<&str> = tail.split('/').collect();
        let get = method == "GET" || method == "HEAD";
        let not_allowed = |allow: &str| {
            Action::Reply(error(
                405,
                &format!("method not allowed (use {allow})"),
                route,
            ))
        };
        let reply = match seg.as_slice() {
            ["health"] if get => self.health(),
            ["guard"] if get => self.guard(),
            ["guard"] => error(
                405,
                "guard toggles are not supported by the Hermes gateway (approvals are off; the spend guard is always on)",
                route,
            ),
            ["spend"] if get => match self.spend() {
                Ok(sp) => ok(sp.to_value()),
                Err(e) => e,
            },
            ["models"] if get => match query
                .split('&')
                .find_map(|kv| kv.strip_prefix("provider="))
                .filter(|p| !p.is_empty())
            {
                Some(p) => self.provider_models(p, route),
                None => self.models(route),
            },
            ["prefs"] if get => ok(self.prefs.read().to_value(self.prefs.path.is_some())),
            ["prefs"] if method == "POST" => match self.prefs.apply(body) {
                Ok(v) => ok(v),
                Err((code, msg)) => error(code, &msg, route),
            },
            ["prefs"] => return not_allowed("GET or POST"),
            ["self-modify"] if get => ok(self_modify_value(&self.self_modify)),
            ["self-modify"] => error(
                405,
                "self-modify is owner-only: it changes on the server with the owner key, not from the console",
                route,
            ),
            ["providers"] if get => ok(self.providers_value()),
            ["modes"] if get => Self::modes(),
            ["skills"] if get => ok(obj(&[
                ("ok", b(true)),
                ("skills", Value::Arr(Vec::new())),
                ("note", s("hermes-rs does not list its skills over the API")),
            ])),
            ["history"] if get => self.history(query, route),
            ["ask"] | ["run"] if method == "POST" => self.ask(body, route),
            ["ask"] | ["run"] => return not_allowed("POST"),
            ["runs", id] if !id_ok(id) => error(400, "that run id is not valid", route),
            ["runs", id] if get => match self.call("GET", &format!("/v1/runs/{id}"), None, None) {
                Ok((code, v)) => Out {
                    status: if code == 401 { 502 } else { code },
                    body: if code == 200 {
                        with(v, &[("ok", b(true))]).to_json()
                    } else {
                        error(code, &gw_message(&v).unwrap_or_default(), route).body
                    },
                },
                Err(e) => e.out(route),
            },
            ["runs", id, "events"] if id_ok(id) && get => {
                if !self.has_key() {
                    GwErr::NoKey.out(route)
                } else {
                    return Action::Stream {
                        target: format!("/v1/runs/{id}/events"),
                    };
                }
            }
            ["runs", id, "stop"] if id_ok(id) && method == "POST" => {
                match self.call("POST", &format!("/v1/runs/{id}/stop"), Some(b"{}"), None) {
                    Ok((code, v)) => Out {
                        status: if code == 401 { 502 } else { code },
                        body: with(v, &[("ok", b(code == 200))]).to_json(),
                    },
                    Err(e) => e.out(route),
                }
            }
            ["health" | "spend" | "models" | "providers" | "modes" | "skills"
            | "history"] => return not_allowed("GET"),
            ["runs", _, "stop"] => return not_allowed("POST"),
            ["runs", _] | ["runs", _, "events"] => return not_allowed("GET"),
            _ => error(404, "unknown Hermes route", route),
        };
        Action::Reply(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_lines() {
        assert_eq!(
            key_from_env_text("A=1\nAPI_SERVER_KEY=abcdefghijklmnopqrst\n").as_deref(),
            Some("abcdefghijklmnopqrst")
        );
        assert_eq!(
            key_from_env_text("export API_SERVER_KEY=\"abcdefghijklmnopqrst\"").as_deref(),
            Some("abcdefghijklmnopqrst")
        );
        assert_eq!(key_from_env_text("API_SERVER_KEY=short"), None);
        assert_eq!(
            key_from_env_text("API_SERVER_KEY=sealed-xxxxxxxxxxxxxxxx"),
            None
        );
        assert_eq!(
            key_from_env_text("OTHER_API_SERVER_KEY=abcdefghijklmnopqrst"),
            None
        );
    }

    #[test]
    fn debug_hides_key() {
        let h = Hermes::new(
            "127.0.0.1:9".parse().unwrap(),
            Some("super-secret-key-123456".into()),
            vec!["openrouter".into()],
            None,
        );
        let d = format!("{h:?}");
        assert!(!d.contains("super-secret"));
        assert!(d.contains("<set>"));
    }

    #[test]
    fn levels_and_spend_sum() {
        assert_eq!(level(0.0), "ok");
        assert_eq!(level(15.99), "ok");
        assert_eq!(level(16.0), "warn");
        assert_eq!(level(20.0), "blocked");
        let v = json::parse(br#"{"data":[{"session_key":"console:hermes:a","estimated_cost_usd":1.5},{"session_key":"other","estimated_cost_usd":9},{"session_key":"console:hermes:b","estimated_cost_usd":0.25},{"estimated_cost_usd":3}]}"#).unwrap();
        assert_eq!(console_spend(&v), (1.75, 2));
    }

    #[test]
    fn providers_gate() {
        let h = Hermes::new(
            "127.0.0.1:9".parse().unwrap(),
            None,
            vec!["openrouter".into(), "river".into(), "laptop".into()],
            None,
        );
        assert_eq!(h.provider_state("openrouter", None), (true, "ready"));
        assert_eq!(h.provider_state("river", None), (true, "ready"));
        assert_eq!(h.provider_state("runpod", None), (false, "not set up"));
        assert_eq!(h.provider_state("laptop", None), (false, "offline"));
        assert_eq!(h.provider_state("laptop", Some(true)), (true, "ready"));
        assert_eq!(PROVIDERS.len(), 4);
        assert_eq!(GUARD_PROVIDER, "river");
        assert_ne!(GUARD_PROVIDER, "openrouter");
    }

    #[test]
    fn thread_mapping_hides_tool_output() {
        let v = json::parse(br#"{"data":[{"role":"user","content":"hi"},{"role":"assistant","content":"","tool_calls":[{"function":{"name":"web_search"}}]},{"role":"tool","name":"web_search","content":"SECRET OUTPUT"},{"role":"assistant","content":[{"type":"text","text":"done"}]}]}"#).unwrap();
        let items = Value::Arr(thread_items(&v)).to_json();
        assert!(items.contains("\"hi\""));
        assert!(items.contains("web_search"));
        assert!(items.contains("\"done\""));
        assert!(!items.contains("SECRET OUTPUT"));
    }

    #[test]
    fn ids_and_routes() {
        assert!(id_ok("run_abc123"));
        assert!(!id_ok("../x"));
        assert!(owns("/api/hermes/health?x=1"));
        assert!(!owns("/api/hermesx"));
        assert!(wants_body("POST", "/api/hermes/ask"));
        assert!(!wants_body("GET", "/api/hermes/ask"));
        let k = new_session_key();
        assert!(k.starts_with(SESSION_KEY_PREFIX) && id_ok(&k));
        assert_ne!(k, new_session_key());
    }
}

#[cfg(test)]
mod self_modify_tests {
    use super::*;

    #[test]
    fn switch_reads_locked_unless_signed_allowed() {
        let d = std::env::temp_dir().join(format!("cs-sm-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("sw.json");
        let st = |p: &Path| {
            self_modify_value(p)
                .get("state")
                .as_str()
                .unwrap_or("")
                .to_string()
        };
        assert_eq!(st(&p), "locked", "missing file");
        std::fs::write(&p, "{bad").unwrap();
        assert_eq!(st(&p), "locked", "bad json");
        std::fs::write(
            &p,
            r#"{"version":"galion/self-modify/v1","state":"allowed"}"#,
        )
        .unwrap();
        assert_eq!(st(&p), "locked", "unsigned allow");
        let sig = "ab".repeat(64);
        std::fs::write(
            &p,
            format!(r#"{{"version":"galion/self-modify/v1","state":"allowed","key_id":"0123456789abcdef","signature":"{sig}"}}"#),
        )
        .unwrap();
        assert_eq!(st(&p), "allowed");
        std::fs::write(
            &p,
            r#"{"version":"galion/self-modify/v1","state":"locked","changed_by":"Mate"}"#,
        )
        .unwrap();
        let v = self_modify_value(&p);
        assert_eq!(v.get("state").as_str(), Some("locked"));
        assert_eq!(v.get("changed_by").as_str(), Some("Mate"));
        assert_eq!(v.get("owner_only").as_bool(), Some(true));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn galion_one_sessions_carry_their_origin() {
        assert_eq!(session_origin("galion-one"), "galion-one");
        assert_eq!(session_origin("one-warm"), "galion-one");
        assert_eq!(session_origin("galion-one:x"), "galion-one");
        assert_eq!(session_origin("console:hermes:ab"), "console");
        assert_eq!(session_origin(""), "");
        assert_eq!(session_origin("galion-oneX"), "");
        let row = json::parse(br#"{"id":"s1","session_key":"galion-one","source":"api_server"}"#)
            .unwrap();
        let h = history_item(&row);
        assert_eq!(h.get("origin").as_str(), Some("galion-one"));
        assert_eq!(h.get("console").as_bool(), Some(false));
    }
}
