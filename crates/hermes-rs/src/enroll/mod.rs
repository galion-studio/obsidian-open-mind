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
use std::sync::{Arc, Mutex};
use verify::{hex, is_lower_hex, SKEW};

/// The one path the enroll URL may point at.
pub const ENROLL_PATH: &str = "/openmind/enroll";
/// Long-lived outbound stream a node opens after enrollment (server-sent events).
pub const TUNNEL_PATH: &str = "/openmind/node/tunnel";
/// Body limit for the public routes (design §2).
pub const MAX_BODY: usize = 16 * 1024;
/// Live nonces kept at most; when full, new requests get 429 (never evicted early).
pub const MAX_NONCES: usize = 20_000;
pub const MAX_FLAGS: usize = 100;
pub const MAX_SCOPES: usize = 16;
pub const TOKEN_PREFIX: &str = "omn1_";
/// How long an unapproved request waits for the owner (seconds). Short on
/// purpose: the device shows a QR and a short code while it waits.
pub const DEFAULT_PENDING_TTL: i64 = 900;
/// Refused and timed-out requests are remembered this long, so the waiting
/// device gets a clear answer instead of "not enrolled".
pub const OUTCOME_TTL: i64 = 86_400;
pub const MAX_OUTCOMES: usize = 200;
/// A new tunnel must answer its hello challenge within this many seconds.
pub const PROOF_DEADLINE: u64 = 30;
/// Signed status polls per minute, all devices together (no store write).
pub const STATUS_RATE_PER_MIN: usize = 240;
/// How long the owner `ping` route waits for the node's signed answer.
pub const PING_WAIT_MS: u64 = 10_000;
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
    /// Seconds between tunnel keep-alive pings (`HERMES_RS_ENROLL_TUNNEL_PING`).
    pub tunnel_ping: i64,
    /// Open node tunnels at most (`HERMES_RS_ENROLL_TUNNELS`); each holds one connection.
    pub max_tunnels: usize,
    /// A tunnel is closed after this many seconds; the node reconnects.
    pub tunnel_life: i64,
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
            pending_ttl: num("HERMES_RS_ENROLL_PENDING_TTL", DEFAULT_PENDING_TTL)?,
            cred_ttl: num("HERMES_RS_ENROLL_CRED_TTL", 30 * 86_400)?,
            rate_per_min: num("HERMES_RS_ENROLL_RATE_PER_MIN", 20)?.min(10_000) as usize,
            tunnel_ping: num("HERMES_RS_ENROLL_TUNNEL_PING", 15)?.min(300),
            max_tunnels: num("HERMES_RS_ENROLL_TUNNELS", 8)?.min(32) as usize,
            tunnel_life: num("HERMES_RS_ENROLL_TUNNEL_LIFE", 3600)?.max(10),
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
    /// Refused / timed-out requests, kept [`OUTCOME_TTL`] (absent in older stores).
    pub outcomes: Vec<Outcome>,
}

/// What happened to a request that did not become a device.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub device_id: String,
    pub public_key: String,
    pub fingerprint: String,
    /// `rejected` (owner refused) or `expired` (owner did not answer in time).
    pub status: String,
    pub at: String,
    pub until_unix: i64,
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
    /// Short code the device shows next to its QR ([`pairing_code`]);
    /// empty for requests stored by an older build.
    pub code: String,
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

/// Canonical string of a signed status poll (no store write, no token).
pub fn status_canonical(device_id: &str, ts: i64, url: &str) -> String {
    format!("omnode-status-v1|{device_id}|{ts}|{url}")
}

/// Canonical string a node signs to answer a tunnel challenge.
pub fn tunnel_canonical(
    device_id: &str,
    session: &str,
    id: &str,
    challenge: &str,
    url: &str,
) -> String {
    format!("omnode-tunnel-v1|{device_id}|{session}|{id}|{challenge}|{url}")
}

/// Crockford base32 without I, L, O, U.
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The short code a device prints next to its QR: 40 bits of
/// SHA-256(`omnode-code-v1|public_key|nonce`) as `XXXX-XXXX`. Both sides
/// compute it from the signed payload, so the owner approving by code proves
/// they are looking at the screen of the device that made this request.
pub fn pairing_code(public_key: &str, nonce: &str) -> String {
    let d = Sha256::digest(format!("omnode-code-v1|{public_key}|{nonce}").as_bytes());
    let v = d[..5].iter().fold(0u64, |acc, b| acc << 8 | u64::from(*b));
    let mut s = String::with_capacity(9);
    for i in (0..8).rev() {
        s.push(CODE_ALPHABET[((v >> (i * 5)) & 31) as usize] as char);
        if i == 4 {
            s.push('-');
        }
    }
    s
}

/// Owner input -> canonical `XXXX-XXXX` (case, spaces and dashes ignored;
/// O->0, I/L->1 as Crockford does). `None` when it can not be a code.
pub fn normalize_code(s: &str) -> Option<String> {
    let mut out = String::new();
    for c in s.chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !CODE_ALPHABET.contains(&(c as u8)) || !c.is_ascii() {
            return None;
        }
        out.push(c);
        if out.len() > 8 {
            return None;
        }
    }
    (out.len() == 8).then(|| format!("{}-{}", &out[..4], &out[4..]))
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

fn bearer(req: &Request) -> &str {
    req.header("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("")
}

fn reply(status: u16, v: Value) -> Response {
    Response::json(status, &v).with_header("Cache-Control", "no-store")
}

fn refuse(status: u16, msg: &str) -> Response {
    reply(status, json!({"ok": false, "error": msg}))
}

/// What a waiting device is told after a refusal or a timeout.
fn outcome_reply(o: &Outcome) -> (u16, Value) {
    if o.status == "rejected" {
        (
            403,
            json!({"ok": false, "status": "rejected", "error": "the owner refused this device", "at": o.at}),
        )
    } else {
        (
            410,
            json!({"ok": false, "status": "expired", "error": "the owner did not approve in time; pair again", "at": o.at}),
        )
    }
}

// ------------------------------------------------------------------ service

pub struct Enroll {
    pub cfg: EnrollCfg,
    state: Mutex<State>,
    hits: Mutex<VecDeque<i64>>,
    status_hits: Mutex<VecDeque<i64>>,
    tunnels: Mutex<BTreeMap<String, Tunnel>>,
}

/// One live outbound node tunnel (memory only; a restart drops it and the
/// node reconnects).
#[derive(Debug, Clone)]
struct Tunnel {
    session: String,
    connected_at: String,
    verified: bool,
    ready_sent: bool,
    last_ping: String,
    pings: u64,
    /// Challenges still to be written into the stream: (id, challenge).
    outbox: VecDeque<(String, String)>,
    /// Challenges written or queued and not answered yet: id -> challenge.
    open: BTreeMap<String, String>,
    /// Answered challenge ids.
    answered: Vec<String>,
    /// Set when the hello answer was wrong: the stream says bye and closes.
    failed: Option<String>,
}

fn lock_or<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn rand_hex(n: usize) -> Result<String, String> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    Ok(hex(&b))
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
            status_hits: Mutex::new(VecDeque::new()),
            tunnels: Mutex::new(BTreeMap::new()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        lock_or(&self.state)
    }

    /// Persist `next` and only then make it current. A failed write leaves
    /// the old state in memory and on disk.
    fn commit(&self, cur: &mut State, next: State) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(&next.to_json()).map_err(|e| e.to_string())?;
        write_atomic(&self.cfg.store, &data)?;
        for o in next.outcomes.iter().filter(|o| !cur.outcomes.contains(o)) {
            log(&format!(
                "request from {} is now {} ({})",
                o.device_id, o.status, o.at
            ));
        }
        *cur = next;
        Ok(())
    }

    /// Forget old nonces; a pending request the owner did not answer within
    /// `pending_ttl` becomes an `expired` outcome (the device is told so).
    fn prune(&self, s: &mut State, now: i64) {
        s.nonces.retain(|_, until| *until > now);
        let ttl = self.cfg.pending_ttl;
        let (keep, gone): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut s.pending)
            .into_iter()
            .partition(|p| p.requested_unix + ttl > now);
        s.pending = keep;
        for p in gone {
            let at = crate::time::zagreb_rfc3339(p.requested_unix + ttl);
            s.outcomes.retain(|o| o.device_id != p.device_id);
            s.outcomes.push(Outcome {
                device_id: p.device_id,
                public_key: p.public_key,
                fingerprint: p.fingerprint,
                status: "expired".into(),
                at,
                until_unix: now + OUTCOME_TTL,
            });
        }
        s.outcomes.retain(|o| o.until_unix > now);
        let excess = s.outcomes.len().saturating_sub(MAX_OUTCOMES);
        s.outcomes.drain(..excess);
    }

    fn bucket_ok(bucket: &Mutex<VecDeque<i64>>, per_min: usize, now: i64) -> bool {
        let mut h = lock_or(bucket);
        while h.front().is_some_and(|t| *t <= now - 60) {
            h.pop_front();
        }
        if h.len() >= per_min {
            return false;
        }
        h.push_back(now);
        true
    }

    fn rate_ok(&self, now: i64) -> bool {
        Self::bucket_ok(&self.hits, self.cfg.rate_per_min, now)
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

    /// Entry point used by the gateway: the tunnel route needs an `Arc` (the
    /// stream outlives this call); everything else is [`Enroll::handle`].
    pub fn route(self: &Arc<Self>, req: &Request, owner: bool, now: i64) -> Response {
        let path = req.path.trim_end_matches('/');
        if req.method == "GET" && path == TUNNEL_PATH {
            return self.tunnel_open(req, now);
        }
        self.handle(req, owner, now)
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
            ("POST", "/openmind/enroll/status") => {
                if !Self::bucket_ok(&self.status_hits, STATUS_RATE_PER_MIN, now) {
                    return refuse(429, "rate limited, try again in a minute");
                }
                match Self::body(req) {
                    Ok(b) => self.status(&b, now),
                    Err(r) => r,
                }
            }
            ("GET", "/openmind/node/whoami") => self.node(req, None, now),
            ("GET", "/openmind/node/allowed") => match req.query_param("scope") {
                Some(sc) if scope_ok(&sc) => self.node(req, Some(&sc), now),
                _ => refuse(400, "scope query parameter missing or invalid"),
            },
            ("POST", "/openmind/node/tunnel/answer") => self.tunnel_answer(req, now),
            ("GET", TUNNEL_PATH) => refuse(500, "tunnel route needs the shared service"),
            ("GET" | "POST", p) if p.starts_with("/openmind/enroll/") && !owner => {
                reply(401, json!({"ok": false, "error": "owner API key required"}))
            }
            ("GET", "/openmind/enroll/pending") => self.list_pending(now),
            ("GET", "/openmind/enroll/devices") => self.list_devices(),
            ("POST", "/openmind/enroll/approve") => self.owner_post(req, now, Self::approve),
            ("POST", "/openmind/enroll/reject") => self.owner_post(req, now, Self::reject),
            ("POST", "/openmind/enroll/revoke") => self.owner_post(req, now, Self::revoke),
            ("POST", "/openmind/enroll/ping") => self.owner_post(req, now, Self::ping_node),
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
                // A new request after a refusal or a timeout starts over.
                next.outcomes.retain(|o| o.device_id != v.device_id);
                let code = pairing_code(&v.public_key, &v.nonce);
                let until = now + self.cfg.pending_ttl;
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
                        code: code.clone(),
                    });
                    (
                        202,
                        json!({"ok": true, "status": "pending", "device_id": v.device_id,
                               "fingerprint": v.fingerprint, "code": code,
                               "pending_expires": crate::time::zagreb_rfc3339(until),
                               "pending_expires_unix": until, "poll_secs": 3}),
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
                "pending {} {} code {} caps={:?} (owner: approve or reject)",
                v.device_id,
                v.fingerprint,
                pairing_code(&v.public_key, &v.nonce),
                v.capabilities
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
            })
            .or_else(|| {
                next.outcomes
                    .iter()
                    .find(|o| o.device_id == dev)
                    .map(|o| o.public_key.clone())
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
            None => match next.outcomes.iter().find(|o| o.device_id == dev) {
                Some(o) => outcome_reply(o),
                None => (202, json!({"ok": true, "status": "pending"})),
            },
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
        let tok = bearer(req);
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
        let ttl = self.cfg.pending_ttl;
        let pending: Vec<Value> = s
            .pending
            .iter()
            .map(|p| {
                let mut v = p.to_json();
                v["expires"] = json!(crate::time::zagreb_rfc3339(p.requested_unix + ttl));
                v["waiting_secs"] = json!(now - p.requested_unix);
                v
            })
            .collect();
        let recent: Vec<Value> = s.outcomes.iter().map(Outcome::to_json).collect();
        reply(
            200,
            json!({"ok": true, "pending": pending, "recent": recent, "pending_ttl_secs": ttl}),
        )
    }

    fn list_devices(&self) -> Response {
        let tunnels = lock_or(&self.tunnels).clone();
        let s = self.lock();
        let d: Vec<Value> = s
            .devices
            .values()
            .map(|d| {
                let mut v = d.public_json();
                v["tunnel"] = match tunnels.get(&d.device_id) {
                    Some(t) => json!({"connected": true, "verified": t.verified, "since": t.connected_at,
                                      "session": t.session, "pings": t.pings, "last_ping": t.last_ping}),
                    None => json!({"connected": false}),
                };
                v
            })
            .collect();
        reply(
            200,
            json!({"ok": true, "devices": d, "flags": s.flags.iter().map(Flag::to_json).collect::<Vec<_>>()}),
        )
    }

    /// `POST /openmind/enroll/status` `{"device_id","ts","signature"}`: a
    /// signed poll while the device waits for the owner. Never a token, and no
    /// store write except recording a timeout it noticed; its own rate bucket, so polling never starves the
    /// enroll and claim POSTs.
    fn status(&self, b: &Value, now: i64) -> Response {
        let Some(o) = b.as_object() else {
            return refuse(400, "status poll is not an object");
        };
        if o.keys()
            .any(|k| !matches!(k.as_str(), "device_id" | "ts" | "signature"))
        {
            return refuse(400, "status poll has unknown fields");
        }
        let (Some(dev), Some(ts), Some(sig)) = (
            o.get("device_id").and_then(Value::as_str),
            o.get("ts").and_then(Value::as_i64),
            o.get("signature").and_then(Value::as_str),
        ) else {
            return refuse(400, "status poll needs device_id, ts, signature");
        };
        if (now - ts).abs() > SKEW {
            return refuse(400, "status timestamp outside the allowed skew");
        }
        let s = {
            let mut cur = self.lock();
            let mut next = cur.clone();
            self.prune(&mut next, now);
            // Persist only a timeout this poll noticed (once per request), so
            // the owner log and the store say "expired" too. Best effort: a
            // failed write changes nothing and the answer is the same.
            if next.outcomes != cur.outcomes {
                let _ = self.commit(&mut cur, next.clone());
            }
            next
        };
        let pk = s
            .devices
            .get(dev)
            .map(|d| d.public_key.clone())
            .or_else(|| {
                s.pending
                    .iter()
                    .find(|p| p.device_id == dev)
                    .map(|p| p.public_key.clone())
            })
            .or_else(|| {
                s.outcomes
                    .iter()
                    .find(|o| o.device_id == dev)
                    .map(|o| o.public_key.clone())
            });
        let Some(pk) = pk else {
            return refuse(403, "not enrolled");
        };
        if let Err(e) = verify::parse_key(&pk).and_then(|vk| {
            verify::check_sig(
                &vk,
                status_canonical(dev, ts, &self.cfg.url).as_bytes(),
                sig.trim(),
            )
        }) {
            return refuse(401, &e);
        }
        if let Some(d) = s.devices.get(dev) {
            if d.revoked_at.is_some() {
                return reply(
                    403,
                    json!({"ok": false, "status": "revoked", "error": "device revoked"}),
                );
            }
            return reply(
                200,
                json!({"ok": true, "status": "approved", "device_id": dev, "scopes": d.scopes,
                       "credential": d.credential.is_some(), "next": "claim"}),
            );
        }
        if let Some(p) = s.pending.iter().find(|p| p.device_id == dev) {
            let until = p.requested_unix + self.cfg.pending_ttl;
            return reply(
                202,
                json!({"ok": true, "status": "pending", "device_id": dev, "code": p.code,
                       "pending_expires": crate::time::zagreb_rfc3339(until), "pending_expires_unix": until}),
            );
        }
        match s.outcomes.iter().find(|o| o.device_id == dev) {
            Some(o) => {
                let (c, v) = outcome_reply(o);
                reply(c, v)
            }
            None => refuse(403, "not enrolled"),
        }
    }

    // -------------------------------------------------------------- tunnel

    /// `GET /openmind/node/tunnel` (Bearer node credential): the node dials
    /// out and keeps this response open. Server -> node frames are SSE:
    /// `hello` (with a challenge the node must sign within
    /// [`PROOF_DEADLINE`]), `ready`, `challenge`, `ping`, `bye`. The node
    /// answers challenges with `POST /openmind/node/tunnel/answer`. Hermes
    /// never connects to the node; one tunnel per device (a new one
    /// replaces the old), at most `max_tunnels` in total.
    fn tunnel_open(self: &Arc<Self>, req: &Request, now: i64) -> Response {
        let tok = bearer(req).to_string();
        let auth = match self.authorize(&tok, None, now) {
            Ok(a) => a,
            Err(_) => return refuse(401, "not enrolled"),
        };
        let (Ok(session), Ok(challenge)) = (rand_hex(8), rand_hex(16)) else {
            return refuse(503, "no randomness");
        };
        let dev = auth.device_id;
        {
            let mut t = lock_or(&self.tunnels);
            if !t.contains_key(&dev) && t.len() >= self.cfg.max_tunnels {
                log(&format!("tunnel for {dev} refused: {} open", t.len()));
                return refuse(503, "too many tunnels");
            }
            let mut open = BTreeMap::new();
            open.insert("hello".to_string(), challenge.clone());
            let at = crate::time::zagreb_rfc3339(now);
            t.insert(
                dev.clone(),
                Tunnel {
                    session: session.clone(),
                    connected_at: at.clone(),
                    verified: false,
                    ready_sent: false,
                    last_ping: at,
                    pings: 0,
                    outbox: VecDeque::new(),
                    open,
                    answered: Vec::new(),
                    failed: None,
                },
            );
        }
        log(&format!("tunnel open: {dev} session {session}"));
        let me = Arc::clone(self);
        Response::Stream {
            status: 200,
            headers: vec![("Cache-Control".into(), "no-store".into())],
            writer: Box::new(move |w| me.tunnel_loop(w, &dev, &session, &challenge, &tok)),
        }
    }

    fn tunnel_loop(
        &self,
        w: &mut crate::http::SseWriter,
        dev: &str,
        session: &str,
        challenge: &str,
        tok: &str,
    ) {
        use std::time::{Duration, Instant};
        let started = Instant::now();
        let mut last_ping = Instant::now();
        let ping = Duration::from_secs(self.cfg.tunnel_ping.max(1) as u64);
        let life = Duration::from_secs(self.cfg.tunnel_life.max(1) as u64);
        let now = crate::time::now_secs();
        let hello = json!({"session": session, "device_id": dev, "id": "hello", "challenge": challenge,
                           "ping_secs": self.cfg.tunnel_ping, "max_life_secs": self.cfg.tunnel_life,
                           "server_time": crate::time::zagreb_rfc3339(now)});
        let mut ours = true;
        let bye = |w: &mut crate::http::SseWriter, r: &str| {
            w.frame(Some("bye"), &json!({"reason": r}));
            r.to_string()
        };
        let reason: String = 'run: {
            if !w.frame(Some("hello"), &hello) {
                break 'run "node gone before hello".into();
            }
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let now = crate::time::now_secs();
                if self.authorize(tok, None, now).is_err() {
                    break 'run bye(w, "credential revoked or expired");
                }
                let mut frames: Vec<(&str, Value)> = Vec::new();
                {
                    let mut t = lock_or(&self.tunnels);
                    let Some(e) = t.get_mut(dev).filter(|e| e.session == session) else {
                        ours = false;
                        drop(t);
                        break 'run bye(w, "replaced by a newer tunnel");
                    };
                    if let Some(f) = e.failed.clone() {
                        drop(t);
                        break 'run bye(w, &f);
                    }
                    if !e.verified && started.elapsed() > Duration::from_secs(PROOF_DEADLINE) {
                        drop(t);
                        break 'run bye(w, "no signed answer to the hello challenge");
                    }
                    if e.verified && !e.ready_sent {
                        e.ready_sent = true;
                        frames.push(("ready", json!({"session": session, "verified": true})));
                    }
                    while let Some((id, ch)) = e.outbox.pop_front() {
                        frames.push(("challenge", json!({"id": id, "challenge": ch})));
                    }
                    if last_ping.elapsed() >= ping {
                        e.pings += 1;
                        e.last_ping = crate::time::zagreb_rfc3339(now);
                        frames.push((
                            "ping",
                            json!({"n": e.pings, "t": crate::time::zagreb_rfc3339(now)}),
                        ));
                        last_ping = Instant::now();
                    }
                }
                for (ev, d) in frames {
                    if !w.frame(Some(ev), &d) {
                        break 'run "node gone (write failed)".into();
                    }
                }
                if started.elapsed() >= life {
                    break 'run bye(w, "max lifetime reached, reconnect");
                }
            }
        };
        if ours {
            let mut t = lock_or(&self.tunnels);
            if t.get(dev).is_some_and(|e| e.session == session) {
                t.remove(dev);
            }
        }
        log(&format!(
            "tunnel closed: {dev} session {session} after {} s: {reason}",
            started.elapsed().as_secs()
        ));
    }

    /// `POST /openmind/node/tunnel/answer` (Bearer) `{"session","id","signature"}`,
    /// signature over [`tunnel_canonical`] with the enrolled device key.
    fn tunnel_answer(&self, req: &Request, now: i64) -> Response {
        let auth = match self.authorize(bearer(req), None, now) {
            Ok(a) => a,
            Err(_) => return refuse(401, "not enrolled"),
        };
        let b = match Self::body(req) {
            Ok(b) => b,
            Err(r) => return r,
        };
        let Some(o) = b.as_object() else {
            return refuse(400, "answer is not an object");
        };
        if o.keys()
            .any(|k| !matches!(k.as_str(), "session" | "id" | "signature"))
        {
            return refuse(400, "answer has unknown fields");
        }
        let (Some(session), Some(id), Some(sig)) = (
            o.get("session").and_then(Value::as_str),
            o.get("id").and_then(Value::as_str),
            o.get("signature").and_then(Value::as_str),
        ) else {
            return refuse(400, "answer needs session, id, signature");
        };
        let dev = auth.device_id;
        let Some(pk) = self.lock().devices.get(&dev).map(|d| d.public_key.clone()) else {
            return refuse(401, "not enrolled");
        };
        let challenge = {
            let mut t = lock_or(&self.tunnels);
            let Some(e) = t.get_mut(&dev).filter(|e| e.session == session) else {
                return refuse(404, "no such tunnel session");
            };
            match e.open.remove(id) {
                Some(c) => c,
                None => return refuse(409, "unknown or already answered challenge"),
            }
        };
        let checked = verify::parse_key(&pk).and_then(|vk| {
            verify::check_sig(
                &vk,
                tunnel_canonical(&dev, session, id, &challenge, &self.cfg.url).as_bytes(),
                sig.trim(),
            )
        });
        let mut t = lock_or(&self.tunnels);
        let Some(e) = t.get_mut(&dev).filter(|e| e.session == session) else {
            return refuse(404, "no such tunnel session");
        };
        if let Err(err) = checked {
            log(&format!(
                "tunnel {dev} {session}: bad answer to {id}: {err}"
            ));
            if id == "hello" {
                e.failed = Some("hello answer did not verify".into());
            }
            return refuse(401, &err);
        }
        if id == "hello" {
            e.verified = true;
            log(&format!(
                "tunnel verified: {dev} session {session} (device key)"
            ));
        }
        e.answered.push(id.to_string());
        let excess = e.answered.len().saturating_sub(64);
        e.answered.drain(..excess);
        reply(200, json!({"ok": true, "id": id, "verified": true}))
    }

    /// Owner `POST /openmind/enroll/ping` `{"device_id"}`: send a challenge
    /// down the node's open tunnel and wait (at most [`PING_WAIT_MS`]) for
    /// its signed answer. Proves the tunnel carries VPS -> node requests and
    /// that the answer comes from the enrolled key.
    fn ping_node(&self, b: &Value, _now: i64) -> Response {
        use std::time::{Duration, Instant};
        let Some(dev) = b["device_id"].as_str() else {
            return refuse(400, "ping needs device_id");
        };
        let (Ok(r), Ok(ch)) = (rand_hex(4), rand_hex(16)) else {
            return refuse(503, "no randomness");
        };
        let id = format!("p{r}");
        let session = {
            let mut t = lock_or(&self.tunnels);
            let Some(e) = t.get_mut(dev) else {
                return refuse(404, "no open tunnel for that device");
            };
            if !e.verified {
                return refuse(409, "tunnel not verified yet");
            }
            e.outbox.push_back((id.clone(), ch.clone()));
            e.open.insert(id.clone(), ch);
            e.session.clone()
        };
        let start = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(20));
            {
                let mut t = lock_or(&self.tunnels);
                match t.get_mut(dev) {
                    Some(e) if e.session == session => {
                        if e.answered.contains(&id) {
                            let ms = start.elapsed().as_millis() as u64;
                            log(&format!(
                                "ping {dev} {session} {id}: signed answer in {ms} ms"
                            ));
                            return reply(
                                200,
                                json!({"ok": true, "device_id": dev, "session": session, "id": id, "rtt_ms": ms}),
                            );
                        }
                        if start.elapsed() > Duration::from_millis(PING_WAIT_MS) {
                            e.open.remove(&id);
                            return refuse(504, "node did not answer in time");
                        }
                    }
                    _ => return refuse(502, "tunnel closed while waiting"),
                }
            }
        }
    }

    /// Which pending request an owner command means: `{"code"}` (the short
    /// code on the device screen) and/or `{"device_id"}`; a given
    /// `fingerprint` must match too. Returns the index into `pending`.
    fn find_pending(&self, s: &State, b: &Value, need_fp: bool) -> Result<usize, Response> {
        let code = match b.get("code") {
            None | Some(Value::Null) => None,
            Some(Value::String(c)) => Some(
                normalize_code(c)
                    .ok_or_else(|| refuse(400, "code is not a XXXX-XXXX pairing code"))?,
            ),
            Some(_) => return Err(refuse(400, "code must be a string")),
        };
        let dev = b["device_id"].as_str();
        let fp = b["fingerprint"].as_str();
        if code.is_none() && (dev.is_none() || (need_fp && fp.is_none())) {
            return Err(refuse(
                400,
                if need_fp {
                    "approve needs code, or device_id and fingerprint"
                } else {
                    "reject needs code or device_id"
                },
            ));
        }
        let i = match &code {
            Some(c) => s
                .pending
                .iter()
                .position(|p| !p.code.is_empty() && ct_eq(&p.code, c)),
            None => s
                .pending
                .iter()
                .position(|p| Some(p.device_id.as_str()) == dev),
        }
        .ok_or_else(|| refuse(404, "no pending request for that code or device"))?;
        let p = &s.pending[i];
        if dev.is_some_and(|d| d != p.device_id) {
            return Err(refuse(409, "code and device_id name different requests"));
        }
        if fp.is_some_and(|f| !ct_eq(&p.fingerprint, f)) {
            return Err(refuse(
                409,
                "fingerprint does not match the pending request",
            ));
        }
        Ok(i)
    }

    /// `{"code"}` or `{"device_id","fingerprint"}`, plus `"scopes"?`,
    /// `"approved_by"?`. The code / fingerprint is what the owner read on the
    /// device screen (`omnode pair` or `omnode identity show`).
    fn approve(&self, b: &Value, now: i64) -> Response {
        let by = b["approved_by"].as_str().unwrap_or("owner");
        if by.is_empty() || by.len() > 64 || by.chars().any(|c| c.is_control()) {
            return refuse(400, "approved_by is invalid");
        }
        let mut cur = self.lock();
        let mut next = cur.clone();
        self.prune(&mut next, now);
        let i = match self.find_pending(&next, b, true) {
            Ok(i) => i,
            Err(r) => return r,
        };
        let p = next.pending.remove(i);
        let dev = p.device_id.clone();
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
        next.outcomes.retain(|o| o.device_id != dev);
        next.devices.insert(
            dev.clone(),
            Device {
                device_id: dev.clone(),
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

    /// `{"code"}` or `{"device_id"}`: refuse a pending request. The waiting
    /// device is told `rejected` (403) on its next poll.
    fn reject(&self, b: &Value, now: i64) -> Response {
        let mut cur = self.lock();
        let mut next = cur.clone();
        self.prune(&mut next, now);
        let i = match self.find_pending(&next, b, false) {
            Ok(i) => i,
            Err(r) => return r,
        };
        let p = next.pending.remove(i);
        let dev = p.device_id.clone();
        next.outcomes.retain(|o| o.device_id != dev);
        next.outcomes.push(Outcome {
            device_id: p.device_id,
            public_key: p.public_key,
            fingerprint: p.fingerprint,
            status: "rejected".into(),
            at: crate::time::zagreb_rfc3339(now),
            until_unix: now + OUTCOME_TTL,
        });
        let excess = next.outcomes.len().saturating_sub(MAX_OUTCOMES);
        next.outcomes.drain(..excess);
        if let Err(e) = self.commit(&mut cur, next) {
            log(&format!("store write failed: {e}"));
            return refuse(503, "enrollment store unavailable");
        }
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
