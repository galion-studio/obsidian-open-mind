// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Hermes device enrollment (docs/hermes-enrollment.md, implemented in
//! docs/hermes-enroll.md): an omnode edge node posts its signed pairing
//! payload, the owner approves it by fingerprint, the node claims a scoped
//! bearer credential with a fresh signature, and node routes accept only
//! enrolled, unrevoked nodes.
//!
//! Fail closed everywhere:
//! * off unless `HERMES_RS_ENROLL=1`; when off, every `/openmind/*` path is 404;
//! * enabled with a missing/odd URL, a non-loopback bind without
//!   `HERMES_RS_ENROLL_ALLOW_PUBLIC=1`, or an unreadable/corrupt/too-open
//!   store = Hermes refuses to start (never resets the store);
//! * every nonce (pairing and claim) is persisted BEFORE the answer; a failed
//!   write answers 503 and changes nothing in memory;
//! * credentials are stored as SHA-256 only, file mode 600;
//! * money/payment capabilities are never granted.

pub mod verify;

use crate::config::Config;
use crate::http::{Request, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use verify::{hex, is_lower_hex, SKEW};

/// The one path the enroll URL may point at.
pub const ENROLL_PATH: &str = "/openmind/enroll";
/// Body limit for the public routes (design §2).
pub const MAX_BODY: usize = 16 * 1024;
/// Live nonces kept at most; when full, new requests get 429 (never evicted early).
pub const MAX_NONCES: usize = 20_000;
pub const MAX_FLAGS: usize = 100;
pub const MAX_SCOPES: usize = 16;
pub const TOKEN_PREFIX: &str = "omn1_";
/// Scope words that are never granted by enrollment (design §5).
const DENIED_SCOPE_WORDS: [&str; 7] =
    ["money", "revolut", "pay", "bank", "wallet", "card", "spend"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollCfg {
    /// The URL Hermes is served at; payloads must name exactly this.
    pub url: String,
    pub store: PathBuf,
    pub max_pending: usize,
    pub nonce_ttl: i64,
    pub pending_ttl: i64,
    pub cred_ttl: i64,
    /// Public POSTs (enroll + claim) per minute, all sources together.
    pub rate_per_min: usize,
}

fn truthy(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn url_host(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or("");
    let authority = rest.split('/').next().unwrap_or("");
    if let Some(v6) = authority.strip_prefix('[') {
        return v6.split(']').next().unwrap_or("");
    }
    authority.split(':').next().unwrap_or("")
}

fn url_path(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or("");
    rest.find('/').map(|i| &rest[i..]).unwrap_or("")
}

pub fn is_loopback_host(h: &str) -> bool {
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

impl EnrollCfg {
    /// `Ok(None)`: disabled, route not mounted. `Err`: enabled but unsafe or
    /// incomplete; the caller must refuse to start.
    pub fn from_env(
        cfg: &Config,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Option<EnrollCfg>, String> {
        let get = |k: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let num = |k: &str, d: i64| -> Result<i64, String> {
            match get(k) {
                None => Ok(d),
                Some(v) => v
                    .parse::<i64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("{k}: not a positive number")),
            }
        };
        match get("HERMES_RS_ENROLL").as_deref().map(truthy) {
            None | Some(Some(false)) => return Ok(None),
            Some(None) => return Err("HERMES_RS_ENROLL: use 1 or 0".into()),
            Some(Some(true)) => {}
        }
        let url = get("HERMES_RS_ENROLL_URL")
            .ok_or("enrollment on but HERMES_RS_ENROLL_URL is not set")?;
        verify::validate_url(&url)?;
        if url_path(&url) != ENROLL_PATH {
            return Err(format!("HERMES_RS_ENROLL_URL path must be {ENROLL_PATH}"));
        }
        if url.starts_with("http://") && !is_loopback_host(url_host(&url)) {
            return Err(
                "HERMES_RS_ENROLL_URL: plain http is only allowed for a loopback host".into(),
            );
        }
        let public = get("HERMES_RS_ENROLL_ALLOW_PUBLIC")
            .and_then(|v| truthy(&v))
            .unwrap_or(false);
        if !cfg.is_loopback() && !public {
            return Err(format!(
                "enrollment on a non-loopback bind ({}) needs HERMES_RS_ENROLL_ALLOW_PUBLIC=1",
                cfg.host
            ));
        }
        let min_ttl = verify::PAYLOAD_TTL + 2 * SKEW;
        let nonce_ttl = num("HERMES_RS_ENROLL_NONCE_TTL", 900)?;
        if nonce_ttl < min_ttl {
            return Err(format!(
                "HERMES_RS_ENROLL_NONCE_TTL must be >= {min_ttl} (payload lifetime + skew)"
            ));
        }
        Ok(Some(EnrollCfg {
            url,
            store: get("HERMES_RS_ENROLL_STORE")
                .map(PathBuf::from)
                .unwrap_or_else(|| cfg.home.join("rs-enroll.json")),
            max_pending: num("HERMES_RS_ENROLL_MAX_PENDING", 8)?.min(256) as usize,
            nonce_ttl,
            pending_ttl: num("HERMES_RS_ENROLL_PENDING_TTL", 3600)?,
            cred_ttl: num("HERMES_RS_ENROLL_CRED_TTL", 30 * 86_400)?,
            rate_per_min: num("HERMES_RS_ENROLL_RATE_PER_MIN", 20)?.min(10_000) as usize,
        }))
    }
}

// ------------------------------------------------------------------ store

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub v: u32,
    /// nonce -> unix second after which it may be forgotten.
    pub nonces: BTreeMap<String, i64>,
    pub pending: Vec<Pending>,
    pub devices: BTreeMap<String, Device>,
    pub flags: Vec<Flag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub device_id: String,
    pub public_key: String,
    pub fingerprint: String,
    pub capabilities: Vec<String>,
    pub network_hash: Option<String>,
    pub arch: String,
    pub cores: u64,
    pub ram_mb: u64,
    pub claimed_platform: String,
    pub claimed_network_trusted: bool,
    pub requested_at: String,
    pub requested_unix: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Credential {
    pub token_sha256: String,
    pub issued_at: String,
    pub expires_unix: i64,
    pub expires: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub device_id: String,
    pub public_key: String,
    pub fingerprint: String,
    pub scopes: Vec<String>,
    pub claimed_capabilities: Vec<String>,
    pub arch: String,
    pub cores: u64,
    pub ram_mb: u64,
    pub network_hash: Option<String>,
    pub approved_at: String,
    pub approved_by: String,
    pub credential: Option<Credential>,
    pub revoked_at: Option<String>,
    pub revoked_by: Option<String>,
}

impl Device {
    /// Owner view: never the token hash.
    fn public_json(&self) -> Value {
        json!({
            "device_id": self.device_id, "fingerprint": self.fingerprint, "public_key": self.public_key,
            "scopes": self.scopes, "claimed_capabilities": self.claimed_capabilities,
            "arch": self.arch, "cores": self.cores, "ram_mb": self.ram_mb, "network_hash": self.network_hash,
            "approved_at": self.approved_at, "approved_by": self.approved_by,
            "credential_issued_at": self.credential.as_ref().map(|c| &c.issued_at),
            "credential_expires": self.credential.as_ref().map(|c| &c.expires),
            "revoked_at": self.revoked_at, "revoked_by": self.revoked_by,
            "status": if self.revoked_at.is_some() { "revoked" } else if self.credential.is_some() { "active" } else { "approved" },
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Flag {
    pub at: String,
    pub device_id: String,
    pub reason: String,
}

fn read_state(path: &Path) -> Result<Option<State>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("enroll store {}: {e}", path.display())),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|e| format!("enroll store {}: {e}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(format!(
                "enroll store {} is mode {:o}; it must be 600 (refusing to use it)",
                path.display(),
                mode & 0o777
            ));
        }
    }
    let s = serde_json::from_str::<Value>(&text)
        .map_err(|e| e.to_string())
        .and_then(|v| State::from_json(&v))
        .map_err(|e| format!("enroll store {} is corrupt: {e}", path.display()))?;
    if s.v != 1 {
        return Err(format!(
            "enroll store {}: unknown version {}",
            path.display(),
            s.v
        ));
    }
    Ok(Some(s))
}

/// Write-temp, fsync, rename; mode 600 from creation.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o
        .open(&tmp)
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    f.write_all(data)
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename {}: {e}", path.display())
    })
}

pub fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    Ok(format!("{TOKEN_PREFIX}{}", hex(&b)))
}

/// Canonical string a node signs to claim its credential.
pub fn claim_canonical(device_id: &str, nonce: &str, ts: i64, url: &str) -> String {
    format!("omnode-claim-v1|{device_id}|{nonce}|{ts}|{url}")
}

fn scope_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
}

fn scope_denied(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    DENIED_SCOPE_WORDS.iter().any(|w| l.contains(w))
}

fn log(msg: &str) {
    eprintln!(
        "{} hermes-gateway enroll: {msg}",
        crate::time::zagreb_rfc3339(crate::time::now_secs())
    );
}

fn reply(status: u16, v: Value) -> Response {
    Response::json(status, &v).with_header("Cache-Control", "no-store")
}

fn refuse(status: u16, msg: &str) -> Response {
    reply(status, json!({"ok": false, "error": msg}))
}

// ------------------------------------------------------------------ service

pub struct Enroll {
    pub cfg: EnrollCfg,
    state: Mutex<State>,
    hits: Mutex<VecDeque<i64>>,
}

/// Outcome of a node credential check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAuth {
    pub device_id: String,
    pub scopes: Vec<String>,
    pub expires: String,
}

impl Enroll {
    /// Load (or create) the store. Any doubt = `Err`, and Hermes must not start.
    pub fn open(cfg: EnrollCfg) -> Result<Enroll, String> {
        let state = match read_state(&cfg.store)? {
            Some(s) => s,
            None => {
                let s = State {
                    v: 1,
                    ..State::default()
                };
                // Prove the store is writable now, not at the first request.
                let data = serde_json::to_vec_pretty(&s.to_json()).map_err(|e| e.to_string())?;
                write_atomic(&cfg.store, &data)?;
                s
            }
        };
        Ok(Enroll {
            cfg,
            state: Mutex::new(state),
            hits: Mutex::new(VecDeque::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// Persist `next` and only then make it current. A failed write leaves
    /// the old state in memory and on disk.
    fn commit(&self, cur: &mut State, next: State) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(&next.to_json()).map_err(|e| e.to_string())?;
        write_atomic(&self.cfg.store, &data)?;
        *cur = next;
        Ok(())
    }

    fn prune(&self, s: &mut State, now: i64) {
        s.nonces.retain(|_, until| *until > now);
        let ttl = self.cfg.pending_ttl;
        s.pending.retain(|p| p.requested_unix + ttl > now);
    }

    fn rate_ok(&self, now: i64) -> bool {
        let mut h = match self.hits.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        while h.front().is_some_and(|t| *t <= now - 60) {
            h.pop_front();
        }
        if h.len() >= self.cfg.rate_per_min {
            return false;
        }
        h.push_back(now);
        true
    }

    /// Snapshot for tests and the console.
    pub fn snapshot(&self) -> State {
        self.lock().clone()
    }

    /// Check a node bearer token (and optionally one scope).
    pub fn authorize(&self, token: &str, scope: Option<&str>, now: i64) -> Result<NodeAuth, u16> {
        let token = token.trim();
        if !token.starts_with(TOKEN_PREFIX) || !is_lower_hex(&token[TOKEN_PREFIX.len()..], 64) {
            return Err(401);
        }
        let h = sha256_hex(token);
        let s = self.lock();
        let mut found = None;
        for d in s.devices.values() {
            if let Some(c) = &d.credential {
                if ct_eq(&c.token_sha256, &h) {
                    found = Some((d, c));
                }
            }
        }
        let (d, c) = found.ok_or(401u16)?;
        if d.revoked_at.is_some() || now > c.expires_unix {
            return Err(401);
        }
        if let Some(sc) = scope {
            if !d.scopes.iter().any(|x| x == sc) {
                return Err(403);
            }
        }
        Ok(NodeAuth {
            device_id: d.device_id.clone(),
            scopes: d.scopes.clone(),
            expires: c.expires.clone(),
        })
    }

    /// Route one `/openmind/*` request. `owner` = the caller presented the
    /// gateway API key (checked by the gateway, fail closed).
    pub fn handle(&self, req: &Request, owner: bool, now: i64) -> Response {
        let path = req.path.trim_end_matches('/');
        let m = req.method.as_str();
        match (m, path) {
            ("POST", ENROLL_PATH) => self.public_post(req, now, |e, b, n| e.enroll(b, n)),
            ("POST", "/openmind/enroll/claim") => {
                self.public_post(req, now, |e, b, n| e.claim(b, n))
            }
            ("GET", "/openmind/node/whoami") => self.node(req, None, now),
            ("GET", "/openmind/node/allowed") => match req.query_param("scope") {
                Some(sc) if scope_ok(&sc) => self.node(req, Some(&sc), now),
                _ => refuse(400, "scope query parameter missing or invalid"),
            },
            ("GET" | "POST", p) if p.starts_with("/openmind/enroll/") && !owner => {
                reply(401, json!({"ok": false, "error": "owner API key required"}))
            }
            ("GET", "/openmind/enroll/pending") => self.list_pending(now),
            ("GET", "/openmind/enroll/devices") => self.list_devices(),
            ("POST", "/openmind/enroll/approve") => self.owner_post(req, now, Self::approve),
            ("POST", "/openmind/enroll/reject") => self.owner_post(req, now, Self::reject),
            ("POST", "/openmind/enroll/revoke") => self.owner_post(req, now, Self::revoke),
            _ => refuse(404, "not found"),
        }
    }

    fn body(req: &Request) -> Result<Value, Response> {
        if req.body.len() > MAX_BODY {
            return Err(refuse(413, "body too large"));
        }
        let ct = req.header("content-type").unwrap_or("");
        if !ct.to_ascii_lowercase().starts_with("application/json") {
            return Err(refuse(415, "content-type must be application/json"));
        }
        serde_json::from_slice(&req.body).map_err(|_| refuse(400, "body is not JSON"))
    }

    fn public_post(
        &self,
        req: &Request,
        now: i64,
        f: impl Fn(&Self, &Value, i64) -> Response,
    ) -> Response {
        if !self.rate_ok(now) {
            log("rate limit hit");
            return refuse(429, "rate limited, try again in a minute");
        }
        match Self::body(req) {
            Ok(b) => f(self, &b, now),
            Err(r) => r,
        }
    }

    fn owner_post(
        &self,
        req: &Request,
        now: i64,
        f: impl Fn(&Self, &Value, i64) -> Response,
    ) -> Response {
        match Self::body(req) {
            Ok(b) => f(self, &b, now),
            Err(r) => r,
        }
    }

    /// `POST /openmind/enroll`.
    fn enroll(&self, doc: &Value, now: i64) -> Response {
        let v = match verify::verify_document(doc, &self.cfg.url, now) {
            Ok(v) => v,
            Err(e) => {
                log(&format!("refused payload: {e}"));
                return refuse(400, &e);
            }
        };
        let mut cur = self.lock();
        let mut next = cur.clone();
        self.prune(&mut next, now);
        if next.nonces.contains_key(&v.nonce) {
            log(&format!("replayed nonce from {}", v.device_id));
            return refuse(409, "nonce already used");
        }
        if next.nonces.len() >= MAX_NONCES {
            return refuse(429, "too many recent requests");
        }
        // Remember the nonce whatever the answer below is (design §5: kept until TTL).
        next.nonces
            .insert(v.nonce.clone(), now + self.cfg.nonce_ttl);
        let at = crate::time::zagreb_rfc3339(now);
        let known_key = next
            .devices
            .get(&v.device_id)
            .map(|d| (d.public_key.clone(), d.revoked_at.is_some()))
            .or_else(|| {
                next.pending
                    .iter()
                    .find(|p| p.device_id == v.device_id)
                    .map(|p| (p.public_key.clone(), false))
            });
        let (status, body) = match known_key {
            Some((pk, _)) if pk != v.public_key => {
                next.flags.push(Flag {
                    at: at.clone(),
                    device_id: v.device_id.clone(),
                    reason: "same device_id with a different public key".into(),
                });
                let excess = next.flags.len().saturating_sub(MAX_FLAGS);
                next.flags.drain(..excess);
                log(&format!(
                    "FLAG {}: different public key, refused",
                    v.device_id
                ));
                (
                    409,
                    json!({"ok": false, "error": "device_id is bound to another key (flagged)"}),
                )
            }
            Some((_, true)) => (403, json!({"ok": false, "error": "device revoked"})),
            Some(_) if next.devices.contains_key(&v.device_id) => (
                409,
                json!({"ok": false, "error": "device already enrolled"}),
            ),
            _ => {
                next.pending.retain(|p| p.device_id != v.device_id);
                if next.pending.len() >= self.cfg.max_pending {
                    (429, json!({"ok": false, "error": "too many pending"}))
                } else {
                    next.pending.push(Pending {
                        device_id: v.device_id.clone(),
                        public_key: v.public_key.clone(),
                        fingerprint: v.fingerprint.clone(),
                        capabilities: v.capabilities.clone(),
                        network_hash: v.network_hash.clone(),
                        arch: v.arch.clone(),
                        cores: v.cores,
                        ram_mb: v.ram_mb,
                        claimed_platform: v.claimed_platform.clone(),
                        claimed_network_trusted: v.claimed_network_trusted,
                        requested_at: at.clone(),
                        requested_unix: now,
                    });
                    (
                        202,
                        json!({"ok": true, "status": "pending", "device_id": v.device_id, "fingerprint": v.fingerprint}),
                    )
                }
            }
        };
        if let Err(e) = self.commit(&mut cur, next) {
            log(&format!("store write failed, refusing: {e}"));
            return refuse(503, "enrollment store unavailable");
        }
        if status == 202 {
            log(&format!(
                "pending {} {} caps={:?}",
                v.device_id, v.fingerprint, v.capabilities
            ));
        }
        reply(status, body)
    }

    /// `POST /openmind/enroll/claim`:
    /// `{"device_id","nonce","ts","signature"}`, signature over
    /// [`claim_canonical`] with the enrolled key.
    fn claim(&self, b: &Value, now: i64) -> Response {
        let Some(o) = b.as_object() else {
            return refuse(400, "claim is not an object");
        };
        if o.keys()
            .any(|k| !matches!(k.as_str(), "device_id" | "nonce" | "ts" | "signature"))
        {
            return refuse(400, "claim has unknown fields");
        }
        let (Some(dev), Some(nonce), Some(ts), Some(sig)) = (
            o.get("device_id").and_then(Value::as_str),
            o.get("nonce").and_then(Value::as_str),
            o.get("ts").and_then(Value::as_i64),
            o.get("signature").and_then(Value::as_str),
        ) else {
            return refuse(400, "claim needs device_id, nonce, ts, signature");
        };
        if !is_lower_hex(nonce, 24) {
            return refuse(400, "nonce is not 96-bit lower-case hex");
        }
        if (now - ts).abs() > SKEW {
            return refuse(400, "claim timestamp outside the allowed skew");
        }
        let mut cur = self.lock();
        let mut next = cur.clone();
        self.prune(&mut next, now);
        let pk = next
            .devices
            .get(dev)
            .map(|d| d.public_key.clone())
            .or_else(|| {
                next.pending
                    .iter()
                    .find(|p| p.device_id == dev)
                    .map(|p| p.public_key.clone())
            });
        let Some(pk) = pk else {
            log(&format!("claim from unenrolled device {dev}"));
            return refuse(403, "not enrolled");
        };
        let checked = verify::parse_key(&pk).and_then(|vk| {
            verify::check_sig(
                &vk,
                claim_canonical(dev, nonce, ts, &self.cfg.url).as_bytes(),
                sig.trim(),
            )
        });
        if let Err(e) = checked {
            log(&format!("claim for {dev} refused: {e}"));
            return refuse(401, &e);
        }
        if next.nonces.contains_key(nonce) {
            log(&format!("replayed claim nonce for {dev}"));
            return refuse(409, "nonce already used");
        }
        if next.nonces.len() >= MAX_NONCES {
            return refuse(429, "too many recent requests");
        }
        next.nonces
            .insert(nonce.to_string(), now + self.cfg.nonce_ttl);
        let mut token = None;
        let (status, mut body) = match next.devices.get_mut(dev) {
            None => (202, json!({"ok": true, "status": "pending"})),
            Some(d) if d.revoked_at.is_some() => {
                (403, json!({"ok": false, "error": "device revoked"}))
            }
            Some(d) => match new_token() {
                Err(e) => {
                    log(&e);
                    return refuse(503, "cannot issue a credential now");
                }
                Ok(t) => {
                    let exp = now + self.cfg.cred_ttl;
                    d.credential = Some(Credential {
                        token_sha256: sha256_hex(&t),
                        issued_at: crate::time::zagreb_rfc3339(now),
                        expires_unix: exp,
                        expires: crate::time::zagreb_rfc3339(exp),
                    });
                    token = Some(t);
                    (
                        200,
                        json!({"ok": true, "status": "enrolled", "device_id": dev, "scopes": d.scopes,
                               "expires": crate::time::zagreb_rfc3339(exp), "expires_unix": exp}),
                    )
                }
            },
        };
        if let Err(e) = self.commit(&mut cur, next) {
            log(&format!("store write failed, refusing: {e}"));
            return refuse(503, "enrollment store unavailable");
        }
        if let Some(t) = token {
            // Only now, after the hash is on disk, does the token leave.
            body["token"] = json!(t);
            log(&format!(
                "credential issued to {dev} (any earlier one replaced)"
            ));
        }
        reply(status, body)
    }

    fn node(&self, req: &Request, scope: Option<&str>, now: i64) -> Response {
        let tok = req
            .header("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or("");
        match self.authorize(tok, scope, now) {
            Ok(a) => reply(
                200,
                json!({"ok": true, "device_id": a.device_id, "scopes": a.scopes, "expires": a.expires, "scope": scope}),
            ),
            Err(403) => refuse(403, "scope not granted"),
            Err(_) => refuse(401, "not enrolled"),
        }
    }

    fn list_pending(&self, now: i64) -> Response {
        let mut s = self.lock().clone();
        self.prune(&mut s, now);
        reply(
            200,
            json!({"ok": true, "pending": s.pending.iter().map(Pending::to_json).collect::<Vec<_>>()}),
        )
    }

    fn list_devices(&self) -> Response {
        let s = self.lock();
        let d: Vec<Value> = s.devices.values().map(Device::public_json).collect();
        reply(
            200,
            json!({"ok": true, "devices": d, "flags": s.flags.iter().map(Flag::to_json).collect::<Vec<_>>()}),
        )
    }

    /// `{"device_id","fingerprint","scopes"?,"approved_by"?}`; the fingerprint
    /// is what the owner compared with `omnode identity show` on the device.
    fn approve(&self, b: &Value, now: i64) -> Response {
        let (Some(dev), Some(fp)) = (b["device_id"].as_str(), b["fingerprint"].as_str()) else {
            return refuse(400, "approve needs device_id and fingerprint");
        };
        let by = b["approved_by"].as_str().unwrap_or("owner");
        if by.is_empty() || by.len() > 64 || by.chars().any(|c| c.is_control()) {
            return refuse(400, "approved_by is invalid");
        }
        let mut cur = self.lock();
        let mut next = cur.clone();
        self.prune(&mut next, now);
        let Some(i) = next.pending.iter().position(|p| p.device_id == dev) else {
            return refuse(404, "no pending request for that device");
        };
        if !ct_eq(&next.pending[i].fingerprint, fp) {
            return refuse(409, "fingerprint does not match the pending request");
        }
        let p = next.pending.remove(i);
        let wanted: Vec<String> = match b.get("scopes") {
            None | Some(Value::Null) => p.capabilities.clone(),
            Some(Value::Array(a)) => match a
                .iter()
                .map(|x| x.as_str().map(String::from))
                .collect::<Option<Vec<_>>>()
            {
                Some(v) => v,
                None => return refuse(400, "scopes must be strings"),
            },
            Some(_) => return refuse(400, "scopes must be a list"),
        };
        let mut scopes = Vec::new();
        let mut denied = Vec::new();
        for s in wanted {
            if scope_ok(&s)
                && !scope_denied(&s)
                && p.capabilities.contains(&s)
                && !scopes.contains(&s)
                && scopes.len() < MAX_SCOPES
            {
                scopes.push(s);
            } else {
                denied.push(s);
            }
        }
        next.devices.insert(
            p.device_id.clone(),
            Device {
                device_id: p.device_id.clone(),
                public_key: p.public_key,
                fingerprint: p.fingerprint,
                scopes: scopes.clone(),
                claimed_capabilities: p.capabilities,
                arch: p.arch,
                cores: p.cores,
                ram_mb: p.ram_mb,
                network_hash: p.network_hash,
                approved_at: crate::time::zagreb_rfc3339(now),
                approved_by: by.into(),
                credential: None,
                revoked_at: None,
                revoked_by: None,
            },
        );
        if let Err(e) = self.commit(&mut cur, next) {
            log(&format!("store write failed: {e}"));
            return refuse(503, "enrollment store unavailable");
        }
        log(&format!(
            "approved {dev} by {by} scopes={scopes:?} denied={denied:?}"
        ));
        reply(
            200,
            json!({"ok": true, "device_id": dev, "status": "approved", "scopes": scopes, "denied": denied}),
        )
    }

    fn reject(&self, b: &Value, now: i64) -> Response {
        let Some(dev) = b["device_id"].as_str() else {
            return refuse(400, "reject needs device_id");
        };
        let mut cur = self.lock();
        let mut next = cur.clone();
        self.prune(&mut next, now);
        let before = next.pending.len();
        next.pending.retain(|p| p.device_id != dev);
        if next.pending.len() == before {
            return refuse(404, "no pending request for that device");
        }
        if let Err(e) = self.commit(&mut cur, next) {
            log(&format!("store write failed: {e}"));
            return refuse(503, "enrollment store unavailable");
        }
        log(&format!("rejected {dev}"));
        reply(
            200,
            json!({"ok": true, "device_id": dev, "status": "rejected"}),
        )
    }

    /// `{"device_id","revoked_by"?}`: the credential stops working at once and
    /// the device can not re-enroll with the same key.
    fn revoke(&self, b: &Value, now: i64) -> Response {
        let Some(dev) = b["device_id"].as_str() else {
            return refuse(400, "revoke needs device_id");
        };
        let by = b["revoked_by"].as_str().unwrap_or("owner");
        if by.is_empty() || by.len() > 64 || by.chars().any(|c| c.is_control()) {
            return refuse(400, "revoked_by is invalid");
        }
        let mut cur = self.lock();
        let mut next = cur.clone();
        let Some(d) = next.devices.get_mut(dev) else {
            return refuse(404, "no such device");
        };
        if d.revoked_at.is_none() {
            d.revoked_at = Some(crate::time::zagreb_rfc3339(now));
            d.revoked_by = Some(by.into());
        }
        d.credential = None;
        let at = d.revoked_at.clone();
        if let Err(e) = self.commit(&mut cur, next) {
            log(&format!("store write failed: {e}"));
            return refuse(503, "enrollment store unavailable");
        }
        log(&format!("revoked {dev} by {by}"));
        reply(
            200,
            json!({"ok": true, "device_id": dev, "status": "revoked", "revoked_at": at}),
        )
    }
}

mod codec;

#[cfg(test)]
mod tests;
