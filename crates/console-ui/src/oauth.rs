// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Account sign-in (OAuth 2.0 authorization code + PKCE S256) for the
//! featured `/connectors` that need it: Gmail, Google Calendar, Google
//! Drive, Canva, Box and Wix (owner pt462u, 2026-10-30 CET). GitHub, Stripe
//! and Notion stay on pasted tokens (`connectors.rs`).
//!
//! The whole flow runs on the console server; the browser only ever sees
//! the provider's sign-in page and two redirects:
//! 1. `POST /connectors op=oauth-start` (owner gate + CSRF): a fresh `state`
//!    (32 random bytes), PKCE verifier (32 random bytes, base64url) and a
//!    browser binding (32 random bytes) are made. Only their SHA-256 and the
//!    verifier are kept, in `<connectors-dir>/oauth/pending/<sha(state)>.json`
//!    (0600, dir 0700); the binding goes to the browser as an HttpOnly
//!    `SameSite=Lax` cookie on `/connectors/oauth`. 303 to the provider.
//! 2. `GET /connectors/oauth/callback?code&state` (owner gate): the pending
//!    file must exist, be younger than [`PENDING_TTL_S`], and match the
//!    binding cookie (constant time); it is consumed by renaming it to
//!    `.used` (single use, nothing deleted). The code is exchanged with the
//!    `code_verifier` through the system `curl` (`-q -K -`: URL, client
//!    secret and code go in on stdin, never argv, so nothing is in
//!    `/proc/<pid>/cmdline`).
//! 3. Tokens go to `<connectors-dir>/oauth/<id>.json` (0600). The access
//!    token alone is handed to Hermes in its MCP env file
//!    (`/etc/hermes-rs/mcp.d/<id>.env`, `<ID>_ACCESS_TOKEN=...`, 0600) that
//!    the console's entry for the server names with
//!    `headers: {Authorization: "Bearer ${...}"}`. hermes-rs re-reads that
//!    file on every (re)connect of a lazy server, so refreshed tokens reach
//!    it without a restart; a *new* server still needs one Hermes restart.
//! 4. [`tick`] (a console thread, every 60 s) refreshes tokens that expire
//!    within [`REFRESH_MARGIN_S`]; a per-connector `flock` stops the two
//!    console units (:18080, :8080) from spending a rotating refresh token
//!    twice.
//! 5. Disconnect revokes at the provider (best effort), overwrites both token
//!    files with a note that carries no token and renames them to `.off`.
//!
//! Provider app credentials (client id/secret) are read from
//! `<--connectors-oauth-dir>/<provider>.env` (root-owned, mode 0600 or 0400,
//! otherwise refused). No file: the row says "Needs app credentials".
//! Default scopes are read-only. Tokens, codes, verifiers and secrets never
//! reach a page, a log line or an error message.

use crate::connectors::{now_unix, rename_aside, write_private, Connector, Store};
use crate::html;
use crate::json::{self, Value};
use crate::sha256;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// The provider redirects here (one URI for every provider).
pub const CALLBACK_PATH: &str = "/connectors/oauth/callback";
/// Default public redirect URI (the console's public host).
pub const DEFAULT_REDIRECT: &str = "https://console.galion.app/connectors/oauth/callback";
/// Browser binding cookie (HttpOnly, SameSite=Lax so the provider's top-level
/// redirect back carries it; Path limited to the callback).
pub const BIND_COOKIE: &str = "galion_cn_oauth";
/// A sign-in must come back within 10 minutes.
pub const PENDING_TTL_S: u64 = 600;
/// Refresh when the access token has less than 5 minutes left.
pub const REFRESH_MARGIN_S: u64 = 300;
/// Background refresh period.
pub const TICK_S: u64 = 60;

/// How the client authenticates at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// `client_id` + `client_secret` in the form body.
    Post,
    /// HTTP Basic `client_id:client_secret`.
    Basic,
}

/// Built-in defaults for one provider (checked against the vendor docs and
/// the MCP servers' RFC 8414 metadata, 2026-10-30).
#[derive(Debug, Clone, Copy)]
pub struct Provider {
    pub key: &'static str,
    pub name: &'static str,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub revoke_url: &'static str,
    pub client_auth: ClientAuth,
    /// Extra authorization parameters (Google: offline access for a refresh token).
    pub extra_auth: &'static [(&'static str, &'static str)],
    /// RFC 8707 `resource` (MCP authorization servers), sent on auth + token.
    pub resource: &'static str,
    /// Where the owner registers the app.
    pub register_at: &'static str,
}

pub const PROVIDERS: &[Provider] = &[
    Provider {
        key: "google",
        name: "Google",
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
        token_url: "https://oauth2.googleapis.com/token",
        revoke_url: "https://oauth2.googleapis.com/revoke",
        client_auth: ClientAuth::Post,
        extra_auth: &[
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("include_granted_scopes", "false"),
        ],
        resource: "",
        register_at: "Google Cloud console > Google Auth Platform > Clients (Web application, \"Use this client for an AI-powered agent\")",
    },
    Provider {
        key: "canva",
        name: "Canva",
        auth_url: "https://mcp.canva.com/authorize",
        token_url: "https://mcp.canva.com/token",
        revoke_url: "https://mcp.canva.com/token",
        client_auth: ClientAuth::Basic,
        extra_auth: &[],
        resource: "https://mcp.canva.com",
        register_at: "Canva MCP client registration (Canva allowlists MCP clients)",
    },
    Provider {
        key: "box",
        name: "Box",
        auth_url: "https://account.box.com/api/oauth2/authorize",
        token_url: "https://api.box.com/oauth2/token",
        revoke_url: "https://api.box.com/oauth2/revoke",
        client_auth: ClientAuth::Post,
        extra_auth: &[],
        resource: "",
        register_at: "Box Admin Console > Integrations > Box MCP server > Configure > Add Integration Credentials",
    },
    Provider {
        key: "wix",
        name: "Wix",
        auth_url: "https://mcp.wix.com/authorize",
        token_url: "https://mcp.wix.com/token",
        revoke_url: "https://mcp.wix.com/token",
        client_auth: ClientAuth::Post,
        extra_auth: &[],
        resource: "https://mcp.wix.com",
        register_at: "Wix MCP OAuth client (mcp.wix.com authorization server)",
    },
];

/// Connector id -> (provider key, default read-only scopes).
pub const CONNECTORS: &[(&str, &str, &str)] = &[
    ("gmail", "google", "https://www.googleapis.com/auth/gmail.readonly"),
    (
        "google-calendar",
        "google",
        "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events.readonly https://www.googleapis.com/auth/calendar.events.freebusy",
    ),
    ("google-drive", "google", "https://www.googleapis.com/auth/drive.readonly"),
    (
        "canva",
        "canva",
        "profile:read design:meta:read design:content:read folder:read asset:read brandtemplate:meta:read comment:read",
    ),
    ("box", "box", "root_readonly"),
    ("wix", "wix", "offline_access"),
];

/// The provider of an OAuth connector this module handles.
pub fn provider_of(id: &str) -> Option<&'static Provider> {
    let (_, p, _) = CONNECTORS.iter().find(|(c, _, _)| *c == id)?;
    PROVIDERS.iter().find(|x| x.key == *p)
}

/// Default scopes of an OAuth connector.
pub fn default_scopes(id: &str) -> Option<&'static str> {
    CONNECTORS.iter().find(|(c, _, _)| *c == id).map(|x| x.2)
}

/// Env name of the access token handed to Hermes (`GOOGLE_DRIVE_ACCESS_TOKEN`).
pub fn token_env(id: &str) -> String {
    format!("{}_ACCESS_TOKEN", id.to_ascii_uppercase().replace('-', "_"))
}

/// Provider settings after the owner's config file is applied.
#[derive(Clone, PartialEq, Eq)]
pub struct ProviderCfg {
    pub key: String,
    pub client_id: String,
    client_secret: String,
    pub redirect_uri: String,
    pub auth_url: String,
    pub token_url: String,
    pub revoke_url: String,
    pub client_auth: ClientAuth,
    pub resource: String,
    /// `SCOPES_<ID>` overrides.
    scopes: Vec<(String, String)>,
    extra_auth: Vec<(String, String)>,
}

impl std::fmt::Debug for ProviderCfg {
    // Never print the secret.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderCfg")
            .field("key", &self.key)
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .field("auth_url", &self.auth_url)
            .field("token_url", &self.token_url)
            .finish_non_exhaustive()
    }
}

impl ProviderCfg {
    pub fn scopes_for(&self, id: &str) -> String {
        self.scopes
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.clone())
            .or_else(|| default_scopes(id).map(str::to_string))
            .unwrap_or_default()
    }
}

/// What the console knows about a provider's app credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderState {
    /// No `<provider>.env` yet: the owner has not registered the app.
    Missing,
    /// The file is there but refused (mode, owner, a bad value).
    Bad(String),
    Ready(Box<ProviderCfg>),
}

/// `https://...`, or `http://` to loopback only (the local mock provider in
/// tests and sandbox runs). No spaces, quotes or credentials.
pub fn url_ok(u: &str) -> bool {
    if u.len() > 512
        || !u.bytes().all(|b| {
            b.is_ascii_graphic() && !matches!(b, b'"' | b'\'' | b'\\' | b'`' | b'<' | b'>')
        })
    {
        return false;
    }
    let rest = match (u.strip_prefix("https://"), u.strip_prefix("http://")) {
        (Some(r), _) => r,
        (None, Some(r)) => {
            let host = r.split(['/', '?']).next().unwrap_or("");
            let h = host.rsplit_once(':').map(|x| x.0).unwrap_or(host);
            if !matches!(h, "127.0.0.1" | "localhost" | "[::1]") {
                return false;
            }
            r
        }
        _ => return false,
    };
    let host = rest.split(['/', '?']).next().unwrap_or("");
    !host.is_empty() && !host.contains('@')
}

fn value_ok(v: &str, max: usize) -> bool {
    !v.is_empty() && v.len() <= max && v.bytes().all(|b| b.is_ascii_graphic())
}

/// `KEY=VALUE` lines (`#` comments, optional quotes).
fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for l in text.lines() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let l = l.strip_prefix("export ").unwrap_or(l);
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
                .unwrap_or(v);
            out.push((k.trim().to_string(), v.to_string()));
        }
    }
    out
}

/// Read `<dir>/<provider>.env`.
pub fn load_provider(dir: &Path, p: &Provider) -> ProviderState {
    let f = dir.join(format!("{}.env", p.key));
    let md = match fs::symlink_metadata(&f) {
        Ok(m) => m,
        Err(_) => return ProviderState::Missing,
    };
    if !md.is_file() {
        return ProviderState::Bad(format!("{}.env is not a regular file", p.key));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if md.mode() & 0o077 != 0 {
            return ProviderState::Bad(format!(
                "{}.env is mode {:o}; it must be 0600 or 0400",
                p.key,
                md.mode() & 0o777
            ));
        }
        let me = fs::metadata("/proc/self").map(|m| m.uid()).ok();
        if md.uid() != 0 && Some(md.uid()) != me {
            return ProviderState::Bad(format!("{}.env must be owned by root", p.key));
        }
    }
    let Ok(text) = fs::read_to_string(&f) else {
        return ProviderState::Bad(format!("{}.env is not readable", p.key));
    };
    let kv = parse_env(&text);
    let get = |k: &str| {
        kv.iter()
            .rev()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
    };
    let Some(client_id) = get("CLIENT_ID").filter(|v| !v.is_empty()) else {
        return ProviderState::Bad(format!("{}.env has no CLIENT_ID", p.key));
    };
    if !value_ok(&client_id, 512) {
        return ProviderState::Bad("CLIENT_ID has characters that are not allowed".into());
    }
    let client_secret = get("CLIENT_SECRET").unwrap_or_default();
    if !client_secret.is_empty() && !value_ok(&client_secret, 1024) {
        return ProviderState::Bad("CLIENT_SECRET has characters that are not allowed".into());
    }
    let pick = |k: &str, d: &str| {
        get(k)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| d.to_string())
    };
    let redirect_uri = pick("REDIRECT_URI", DEFAULT_REDIRECT);
    let auth_url = pick("AUTH_URL", p.auth_url);
    let token_url = pick("TOKEN_URL", p.token_url);
    let revoke_url = get("REVOKE_URL").unwrap_or_else(|| p.revoke_url.to_string());
    for (k, u) in [
        ("REDIRECT_URI", &redirect_uri),
        ("AUTH_URL", &auth_url),
        ("TOKEN_URL", &token_url),
    ] {
        if !url_ok(u) {
            return ProviderState::Bad(format!("{k} must be an https:// URL"));
        }
    }
    if !revoke_url.is_empty() && !url_ok(&revoke_url) {
        return ProviderState::Bad("REVOKE_URL must be an https:// URL".into());
    }
    if !redirect_uri.ends_with(CALLBACK_PATH) {
        return ProviderState::Bad(format!("REDIRECT_URI must end with {CALLBACK_PATH}"));
    }
    let client_auth = match get("TOKEN_AUTH").as_deref() {
        None | Some("") => p.client_auth,
        Some("basic") => ClientAuth::Basic,
        Some("post") => ClientAuth::Post,
        Some(_) => return ProviderState::Bad("TOKEN_AUTH must be basic or post".into()),
    };
    let mut scopes = Vec::new();
    for (id, prov, _) in CONNECTORS.iter().filter(|c| c.1 == p.key) {
        let _ = prov;
        let k = format!("SCOPES_{}", id.to_ascii_uppercase().replace('-', "_"));
        if let Some(v) = get(&k).filter(|v| !v.is_empty()) {
            if v.len() > 2048 || !v.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
                return ProviderState::Bad(format!("{k} has characters that are not allowed"));
            }
            scopes.push((id.to_string(), v));
        }
    }
    ProviderState::Ready(Box::new(ProviderCfg {
        key: p.key.to_string(),
        client_id,
        client_secret,
        redirect_uri,
        auth_url,
        token_url,
        revoke_url,
        client_auth,
        resource: pick("RESOURCE", p.resource),
        scopes,
        extra_auth: p
            .extra_auth
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
    }))
}

// ---- small encoders ----

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 (standard alphabet, padded) or base64url (no padding).
pub fn b64(data: &[u8], url: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for ch in data.chunks(3) {
        let n = (u32::from(ch[0]) << 16)
            | (u32::from(*ch.get(1).unwrap_or(&0)) << 8)
            | u32::from(*ch.get(2).unwrap_or(&0));
        let k = ch.len() + 1;
        for i in 0..4 {
            if i < k {
                let c = B64[((n >> (18 - 6 * i)) & 63) as usize];
                out.push(match (url, c) {
                    (true, b'+') => '-',
                    (true, b'/') => '_',
                    _ => c as char,
                });
            } else if !url {
                out.push('=');
            }
        }
    }
    out
}

/// PKCE S256: `BASE64URL(SHA256(ASCII(code_verifier)))` (RFC 7636 4.2).
pub fn pkce_challenge(verifier: &str) -> String {
    b64(&sha256::digest(verifier.as_bytes()), true)
}

/// 32 random bytes from the OS as base64url (43 characters).
pub fn random_token() -> Option<String> {
    let mut b = [0u8; 32];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .ok()?;
    Some(b64(&b, true))
}

/// A value we issued with [`random_token`].
pub fn token_shape_ok(t: &str) -> bool {
    t.len() == 43
        && t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", html::url_enc(k), html::url_enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// The provider sign-in URL.
pub fn authorize_url(cfg: &ProviderCfg, id: &str, state: &str, challenge: &str) -> String {
    let scopes = cfg.scopes_for(id);
    let mut p: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", &cfg.client_id),
        ("redirect_uri", &cfg.redirect_uri),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    if !scopes.is_empty() {
        p.push(("scope", &scopes));
    }
    if !cfg.resource.is_empty() {
        p.push(("resource", &cfg.resource));
    }
    for (a, b) in &cfg.extra_auth {
        p.push((a, b));
    }
    let sep = if cfg.auth_url.contains('?') { '&' } else { '?' };
    format!("{}{sep}{}", cfg.auth_url, form(&p))
}

// ---- HTTP through the system curl ----

/// Escape for a double-quoted curl config value.
fn curl_quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' | '\r' | '\t' => o.push(' '),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `curl` binary (tests may point `GALION_CN_CURL` at another absolute path).
fn curl_bin() -> String {
    std::env::var("GALION_CN_CURL")
        .ok()
        .filter(|p| p.starts_with('/'))
        .unwrap_or_else(|| "curl".into())
}

/// POST a form; returns (HTTP status, body). Errors never contain the body.
pub fn post_form(
    cfg: &ProviderCfg,
    url: &str,
    pairs: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), String> {
    if !url_ok(url) {
        return Err("endpoint URL refused".into());
    }
    let mut body: Vec<(&str, &str)> = pairs.to_vec();
    if cfg.client_auth == ClientAuth::Post {
        body.push(("client_id", &cfg.client_id));
        if !cfg.client_secret.is_empty() {
            body.push(("client_secret", &cfg.client_secret));
        }
    }
    let mut c = String::new();
    let mut line = |k: &str, v: &str| {
        c.push_str(k);
        c.push_str(" = ");
        c.push_str(&curl_quote(v));
        c.push('\n');
    };
    line("url", url);
    line("request", "POST");
    line(
        "proto",
        if url.starts_with("https://") {
            "=https"
        } else {
            "=http"
        },
    );
    line("max-time", "20");
    line("connect-timeout", "10");
    line("max-filesize", "1048576");
    line("header", "Content-Type: application/x-www-form-urlencoded");
    line("header", "Accept: application/json");
    if cfg.client_auth == ClientAuth::Basic {
        let b = b64(
            format!(
                "{}:{}",
                html::url_enc(&cfg.client_id),
                html::url_enc(&cfg.client_secret)
            )
            .as_bytes(),
            false,
        );
        line("header", &format!("Authorization: Basic {b}"));
    }
    line("data-raw", &form(&body));
    line("write-out", "\\n%{http_code}");
    c.push_str("silent\n");
    let mut child = Command::new(curl_bin())
        .arg("-q")
        .arg("-K")
        .arg("-")
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start curl ({})", e.kind()))?;
    if let Some(mut si) = child.stdin.take() {
        si.write_all(c.as_bytes())
            .map_err(|e| format!("curl stdin ({})", e.kind()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("curl ({})", e.kind()))?;
    let raw = out.stdout;
    let cut = raw.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let code: u16 = String::from_utf8_lossy(&raw[(cut + 1).min(raw.len())..])
        .trim()
        .parse()
        .unwrap_or(0);
    if code == 0 {
        return Err(format!(
            "the provider did not answer (curl exit {})",
            out.status.code().unwrap_or(-1)
        ));
    }
    Ok((code, raw[..cut].to_vec()))
}

/// Tokens from a token endpoint answer.
#[derive(Clone, PartialEq, Eq)]
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub expires_at: Option<u64>,
    pub scope: String,
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tokens(<redacted>)")
    }
}

fn tok_ok(t: &str) -> bool {
    (8..=8192).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_graphic())
}

/// An OAuth error code from an error body (`invalid_grant`), safe to show.
fn error_code(body: &[u8]) -> String {
    let v = json::parse(body).unwrap_or(Value::Null);
    let e = v.get("error").as_str().unwrap_or("");
    let e: String = e
        .chars()
        .filter(|c| c.is_ascii_lowercase() || *c == '_')
        .take(40)
        .collect();
    if e.is_empty() {
        "unknown_error".into()
    } else {
        e
    }
}

/// Parse a 200 token answer.
pub fn parse_tokens(code: u16, body: &[u8], now: u64) -> Result<Tokens, String> {
    if code != 200 {
        return Err(format!(
            "token endpoint said HTTP {code} ({})",
            error_code(body)
        ));
    }
    let v = json::parse(body).map_err(|_| "token endpoint answer is not JSON".to_string())?;
    let access = v.get("access_token").as_str().unwrap_or("").to_string();
    if !tok_ok(&access) {
        return Err("token endpoint gave no usable access_token".into());
    }
    if let Some(t) = v.get("token_type").as_str() {
        if !t.eq_ignore_ascii_case("bearer") {
            return Err("token type is not Bearer".into());
        }
    }
    let refresh = v
        .get("refresh_token")
        .as_str()
        .filter(|t| tok_ok(t))
        .map(str::to_string);
    let exp = v
        .get("expires_in")
        .as_f64()
        .or_else(|| v.get("expires_in").as_str().and_then(|s| s.parse().ok()))
        .filter(|s| *s > 0.0 && *s < 1e9)
        .map(|s| now + s as u64);
    let scope: String = v
        .get("scope")
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(2048)
        .collect();
    Ok(Tokens {
        access,
        refresh,
        expires_at: exp,
        scope,
    })
}

// ---- server-side stores ----

/// One connector's sign-in record (`oauth/<id>.json`).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Record {
    /// `connected` or `error`.
    pub status: String,
    pub error: String,
    pub access: String,
    pub refresh: String,
    pub expires_at: u64,
    pub scope: String,
    /// When the account was signed in (not changed by refreshes).
    pub connected_at: u64,
    pub refreshed_at: u64,
    pub updated_at: u64,
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Record")
            .field("status", &self.status)
            .field("error", &self.error)
            .field("expires_at", &self.expires_at)
            .field("has_refresh", &!self.refresh.is_empty())
            .finish_non_exhaustive()
    }
}

impl Record {
    fn to_json(&self) -> String {
        let mut s = String::from("{");
        let mut first = true;
        let mut kv = |k: &str, v: &str| {
            if !first {
                s.push(',');
            }
            first = false;
            json::write_str(&mut s, k);
            s.push(':');
            json::write_str(&mut s, v);
        };
        kv("status", &self.status);
        kv("error", &self.error);
        kv("access_token", &self.access);
        kv("refresh_token", &self.refresh);
        kv("expires_at", &self.expires_at.to_string());
        kv("scope", &self.scope);
        kv("connected_at", &self.connected_at.to_string());
        kv("refreshed_at", &self.refreshed_at.to_string());
        kv("updated_at", &self.updated_at.to_string());
        s.push_str("}\n");
        s
    }
    fn from_json(t: &str) -> Option<Record> {
        let v = json::parse(t.as_bytes()).ok()?;
        let s = |k: &str| v.get(k).as_str().unwrap_or("").to_string();
        let n = |k: &str| s(k).parse().unwrap_or(0);
        Some(Record {
            status: s("status"),
            error: s("error"),
            access: s("access_token"),
            refresh: s("refresh_token"),
            expires_at: n("expires_at"),
            scope: s("scope"),
            connected_at: n("connected_at"),
            refreshed_at: n("refreshed_at"),
            updated_at: n("updated_at"),
        })
    }
    pub fn connected(&self) -> bool {
        self.status == "connected" && !self.access.is_empty()
    }
}

/// Where the console keeps OAuth state (`<connectors-dir>/oauth`).
pub fn oauth_dir(st: &Store) -> PathBuf {
    st.dir.join("oauth")
}

fn record_path(st: &Store, id: &str) -> PathBuf {
    oauth_dir(st).join(format!("{id}.json"))
}

fn pending_dir(st: &Store) -> PathBuf {
    oauth_dir(st).join("pending")
}

#[cfg(unix)]
fn mkdir_private(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !p.is_dir() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(p)?;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
#[cfg(not(unix))]
fn mkdir_private(p: &Path) -> std::io::Result<()> {
    fs::create_dir_all(p)
}

pub fn read_record(st: &Store, id: &str) -> Option<Record> {
    fs::read_to_string(record_path(st, id))
        .ok()
        .and_then(|t| Record::from_json(&t))
}

fn write_record(st: &Store, id: &str, r: &Record) -> std::io::Result<()> {
    mkdir_private(&oauth_dir(st))?;
    write_private(&record_path(st, id), r.to_json().as_bytes())
}

/// Exclusive per-connector lock across the console processes (the lock file
/// is created once and kept; the lock ends when the handle drops).
fn lock(st: &Store, id: &str) -> std::io::Result<fs::File> {
    mkdir_private(&oauth_dir(st))?;
    let mut o = fs::OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let f = o.open(oauth_dir(st).join(format!("{id}.lock")))?;
    f.lock()?;
    Ok(f)
}

/// Hand the access token to Hermes: `<ID>_ACCESS_TOKEN=` in the server's
/// env file (0600), nothing else.
fn write_hermes_env(st: &Store, id: &str, r: &Record) -> std::io::Result<()> {
    let p = st.secret_path(id);
    if let Some(d) = p.parent() {
        mkdir_private(d)?;
    }
    let body = format!(
        "# {id} OAuth access token for Hermes MCP. Written and refreshed by the console /connectors sign-in; expires at unix {}.\n{}={}\n",
        r.expires_at,
        token_env(id),
        r.access
    );
    write_private(&p, body.as_bytes())
}

/// The sign-in result shown for an OAuth row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// `--connectors-oauth-dir` is not set on this console.
    NotEnabled,
    NeedsCredentials,
    ConfigError(String),
    /// App credentials present; nobody signed in yet (or disconnected).
    Ready,
    /// Signed in; `live` = Hermes started after the sign-in.
    Connected {
        live: bool,
        expires_at: u64,
    },
    Error(String),
}

/// Status of an OAuth connector `id`.
pub fn status(st: &Store, id: &str) -> Status {
    let (Some(dir), Some(p)) = (st.oauth_cfg_dir.as_deref(), provider_of(id)) else {
        return Status::NotEnabled;
    };
    match load_provider(dir, p) {
        ProviderState::Missing => return Status::NeedsCredentials,
        ProviderState::Bad(e) => return Status::ConfigError(e),
        ProviderState::Ready(_) => {}
    }
    match read_record(st, id) {
        Some(r) if r.connected() => {
            if r.expires_at != 0 && r.expires_at <= now_unix() && r.refresh.is_empty() {
                return Status::Error(
                    "the access token expired and there is no refresh token; sign in again".into(),
                );
            }
            let live = st.hermes_started.is_some_and(|h| r.connected_at < h)
                && st.secret_path(id).is_file();
            Status::Connected {
                live,
                expires_at: r.expires_at,
            }
        }
        Some(r) if r.status == "error" => Status::Error(r.error),
        _ => Status::Ready,
    }
}

fn provider_cfg(st: &Store, id: &str) -> Result<ProviderCfg, String> {
    let dir = st
        .oauth_cfg_dir
        .as_deref()
        .ok_or("account sign-in is not enabled on this console")?;
    let p = provider_of(id).ok_or("this connector does not use account sign-in")?;
    match load_provider(dir, p) {
        ProviderState::Ready(c) => Ok(*c),
        ProviderState::Missing => Err(format!(
            "needs app credentials: register a {} OAuth app and save {}.env (see the row)",
            p.name, p.key
        )),
        ProviderState::Bad(e) => Err(format!("app credentials refused: {e}")),
    }
}

/// Step 1: make the pending sign-in. Returns (provider URL, binding cookie value).
pub fn start(st: &Store, c: &Connector) -> Result<(String, String), String> {
    let cfg = provider_cfg(st, &c.id)?;
    let (Some(state), Some(verifier), Some(bind)) =
        (random_token(), random_token(), random_token())
    else {
        return Err("no OS randomness".into());
    };
    let io = |e: std::io::Error| format!("could not save the sign-in ({})", e.kind());
    mkdir_private(&pending_dir(st)).map_err(io)?;
    let mut s = String::from("{");
    json::write_str(&mut s, "id");
    s.push(':');
    json::write_str(&mut s, &c.id);
    s.push(',');
    json::write_str(&mut s, "verifier");
    s.push(':');
    json::write_str(&mut s, &verifier);
    s.push(',');
    json::write_str(&mut s, "bind");
    s.push(':');
    json::write_str(&mut s, &sha256::hex(bind.as_bytes()));
    s.push(',');
    json::write_str(&mut s, "created");
    s.push(':');
    json::write_str(&mut s, &now_unix().to_string());
    s.push_str("}\n");
    let p = pending_dir(st).join(format!("{}.json", sha256::hex(state.as_bytes())));
    write_private(&p, s.as_bytes()).map_err(io)?;
    Ok((
        authorize_url(&cfg, &c.id, &state, &pkce_challenge(&verifier)),
        bind,
    ))
}

/// Why a callback was refused (fixed texts; never echo what came in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackError {
    /// No such pending sign-in, already used, or the browser binding differs.
    StateMismatch,
    Expired,
}

/// Step 2: check `state` + binding and consume the pending sign-in.
/// Returns (connector id, verifier).
pub fn take_pending(
    st: &Store,
    state: &str,
    bind: Option<&str>,
) -> Result<(String, String), CallbackError> {
    if !token_shape_ok(state) {
        return Err(CallbackError::StateMismatch);
    }
    let p = pending_dir(st).join(format!("{}.json", sha256::hex(state.as_bytes())));
    let Ok(text) = fs::read_to_string(&p) else {
        return Err(CallbackError::StateMismatch);
    };
    let v = json::parse(text.as_bytes()).map_err(|_| CallbackError::StateMismatch)?;
    let want = v.get("bind").as_str().unwrap_or("");
    let got = bind
        .filter(|b| token_shape_ok(b))
        .map(|b| sha256::hex(b.as_bytes()))
        .unwrap_or_default();
    if want.is_empty() || !crate::connectors::ct_eq(want, &got) {
        return Err(CallbackError::StateMismatch);
    }
    // Single use: whoever renames it first owns it.
    let used = PathBuf::from(format!("{}.used", p.display()));
    if used.exists() || fs::rename(&p, &used).is_err() {
        return Err(CallbackError::StateMismatch);
    }
    // The used marker keeps no verifier.
    let _ = write_private(
        &used,
        format!("{{\"used\":\"{}\"}}\n", now_unix()).as_bytes(),
    );
    let created: u64 = v
        .get("created")
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if now_unix().saturating_sub(created) > PENDING_TTL_S {
        return Err(CallbackError::Expired);
    }
    let id = v.get("id").as_str().unwrap_or("").to_string();
    let verifier = v.get("verifier").as_str().unwrap_or("").to_string();
    if provider_of(&id).is_none() || !token_shape_ok(&verifier) {
        return Err(CallbackError::StateMismatch);
    }
    Ok((id, verifier))
}

/// Note a failed sign-in. A working earlier sign-in is kept (its error
/// line is updated); otherwise the row shows Error.
fn record_error(st: &Store, id: &str, msg: &str) {
    let Ok(_l) = lock(st, id) else { return };
    let now = now_unix();
    let r = match read_record(st, id).filter(Record::connected) {
        Some(mut r) => {
            r.error = msg.to_string();
            r.updated_at = now;
            r
        }
        None => Record {
            status: "error".into(),
            error: msg.to_string(),
            updated_at: now,
            ..Record::default()
        },
    };
    let _ = write_record(st, id, &r);
}

/// The provider sent `error=` back (the owner declined, or a setup problem).
pub fn provider_said_no(st: &Store, id: &str, error: &str) {
    let e: String = error
        .chars()
        .filter(|c| c.is_ascii_lowercase() || *c == '_')
        .take(40)
        .collect();
    record_error(
        st,
        id,
        &format!(
            "the provider refused the sign-in ({})",
            if e.is_empty() { "unknown_error" } else { &e }
        ),
    );
}

/// Step 3: exchange the code, store the tokens, hand the access token to Hermes.
pub fn finish(st: &Store, id: &str, code: &str, verifier: &str) -> Result<(), String> {
    let cfg = provider_cfg(st, id)?;
    if code.is_empty() || code.len() > 4096 || !code.bytes().all(|b| b.is_ascii_graphic()) {
        record_error(st, id, "the provider sent no usable code");
        return Err("no usable code".into());
    }
    let mut pairs: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &cfg.redirect_uri),
        ("code_verifier", verifier),
    ];
    if !cfg.resource.is_empty() {
        pairs.push(("resource", &cfg.resource));
    }
    let now = now_unix();
    let got = post_form(&cfg, &cfg.token_url, &pairs).and_then(|(c, b)| parse_tokens(c, &b, now));
    let t = match got {
        Ok(t) => t,
        Err(e) => {
            let msg = format!("sign-in failed: {e}");
            record_error(st, id, &msg);
            return Err(msg);
        }
    };
    let io = |e: std::io::Error| format!("could not store the tokens ({})", e.kind());
    let _l = lock(st, id).map_err(io)?;
    let r = Record {
        status: "connected".into(),
        error: String::new(),
        access: t.access,
        refresh: t.refresh.unwrap_or_default(),
        expires_at: t.expires_at.unwrap_or(0),
        scope: t.scope,
        connected_at: now,
        refreshed_at: 0,
        updated_at: now,
    };
    write_record(st, id, &r).map_err(io)?;
    write_hermes_env(st, id, &r).map_err(io)?;
    st.sync_oauth_entries().map_err(io)?;
    Ok(())
}

/// Refresh one connector if its token runs out within `margin` seconds
/// (`force`: refresh now). Ok(true) = refreshed.
pub fn refresh(st: &Store, id: &str, margin: u64, force: bool) -> Result<bool, String> {
    let cfg = provider_cfg(st, id)?;
    let io = |e: std::io::Error| format!("could not store the tokens ({})", e.kind());
    let _l = lock(st, id).map_err(io)?;
    // Re-read under the lock: the other console unit may have refreshed.
    let Some(mut r) = read_record(st, id).filter(Record::connected) else {
        return Err("not signed in".into());
    };
    let now = now_unix();
    let due = r.expires_at != 0 && r.expires_at <= now + margin;
    if !force && !due {
        return Ok(false);
    }
    if r.refresh.is_empty() {
        if r.expires_at != 0 && r.expires_at <= now {
            r.status = "error".into();
            r.error =
                "the access token expired and there is no refresh token; sign in again".into();
            r.updated_at = now;
            write_record(st, id, &r).map_err(io)?;
        }
        return Err("no refresh token; sign in again".into());
    }
    let mut pairs: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", &r.refresh),
    ];
    if !cfg.resource.is_empty() {
        pairs.push(("resource", &cfg.resource));
    }
    let got = post_form(&cfg, &cfg.token_url, &pairs).and_then(|(c, b)| parse_tokens(c, &b, now));
    match got {
        Ok(t) => {
            r.access = t.access;
            if let Some(n) = t.refresh {
                r.refresh = n;
            }
            r.expires_at = t.expires_at.unwrap_or(0);
            if !t.scope.is_empty() {
                r.scope = t.scope;
            }
            r.refreshed_at = now;
            r.updated_at = now;
            r.error.clear();
            write_record(st, id, &r).map_err(io)?;
            write_hermes_env(st, id, &r).map_err(io)?;
            Ok(true)
        }
        Err(e) => {
            if e.contains("invalid_grant") {
                r.status = "error".into();
                r.error =
                    "the provider no longer accepts this sign-in (invalid_grant); sign in again"
                        .into();
            } else {
                r.error = format!("last refresh failed: {e}");
            }
            r.updated_at = now;
            write_record(st, id, &r).map_err(io)?;
            Err(e)
        }
    }
}

/// Step 5: revoke (best effort), then replace both token files with a note
/// that holds no token and rename them to `.off`. Ok(revoked at provider).
pub fn disconnect(st: &Store, id: &str) -> Result<bool, String> {
    if provider_of(id).is_none() {
        return Err("this connector does not use account sign-in".into());
    }
    let io = |e: std::io::Error| format!("could not remove the tokens ({})", e.kind());
    let _l = lock(st, id).map_err(io)?;
    let rec = read_record(st, id);
    let hermes = st.secret_path(id);
    if rec.is_none() && !hermes.is_file() {
        return Err("not signed in".into());
    }
    let mut revoked = false;
    if let (Some(r), Ok(cfg)) = (rec.as_ref(), provider_cfg(st, id)) {
        if !cfg.revoke_url.is_empty() {
            let tok = if r.refresh.is_empty() {
                &r.access
            } else {
                &r.refresh
            };
            if !tok.is_empty() {
                let hint = if r.refresh.is_empty() {
                    "access_token"
                } else {
                    "refresh_token"
                };
                revoked = matches!(
                    post_form(
                        &cfg,
                        &cfg.revoke_url,
                        &[("token", tok), ("token_type_hint", hint)]
                    ),
                    Ok((200..=299, _))
                );
            }
        }
    }
    let now = now_unix();
    let note = format!(
        "{{\"status\":\"disconnected\",\"revoked_at_provider\":{revoked},\"updated_at\":\"{now}\"}}\n"
    );
    let rp = record_path(st, id);
    if rp.is_file() {
        write_private(&rp, note.as_bytes()).map_err(io)?;
        rename_aside(&rp, ".off").map_err(io)?;
    }
    if hermes.is_file() {
        let body = format!("# {id}: disconnected from the console at unix {now}; token removed.\n");
        write_private(&hermes, body.as_bytes()).map_err(io)?;
        rename_aside(&hermes, ".off").map_err(io)?;
    }
    Ok(revoked)
}

/// Background work for one console process: refresh tokens that are due.
/// Errors are recorded per connector (never with a token in them).
pub fn tick(st: &Store) -> usize {
    let mut n = 0;
    for (id, _, _) in CONNECTORS {
        if read_record(st, id).is_some_and(|r| r.connected()) {
            if let Ok(true) = refresh(st, id, REFRESH_MARGIN_S, false) {
                n += 1;
            }
        }
    }
    n
}

/// Run [`tick`] every [`TICK_S`] seconds on a named thread.
pub fn spawn_refresher(st: Store) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("cn-oauth-refresh".into())
        .spawn(move || loop {
            let _ = tick(&st);
            std::thread::sleep(Duration::from_secs(TICK_S));
        })
}

/// `scheme://host[:port]` of a URL.
fn origin(u: &str) -> String {
    let (scheme, rest) = u.split_once("://").unwrap_or(("https", u));
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    format!("{scheme}://{host}")
}

/// Sign-in origins of the providers with app credentials (for the page's
/// CSP `form-action`: the start form 303s there).
pub fn form_origins(st: &Store) -> Vec<String> {
    let Some(dir) = st.oauth_cfg_dir.as_deref() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for p in PROVIDERS {
        if let ProviderState::Ready(c) = load_provider(dir, p) {
            let o = origin(&c.auth_url);
            if url_ok(&o) && !out.contains(&o) {
                out.push(o);
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "oauth_tests.rs"]
mod tests;
