// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Pairing **client** for the console admin gate (wave-4 slot 319).
//!
//! Speaks the `/api/v1/admin` contract served by the Rust console
//! (`console-static --local-api --admin-store`, slot 303) and by this crate's
//! own `serve` (slot 307):
//!
//! * `GET  /api/v1/admin` -> `{"paired":bool,"you":bool[,"node"]}`
//! * `POST /api/v1/admin {"code":".."}` -> 200 + `Set-Cookie: galion_admin=..`
//!   (+ a one-time `"device"` secret on console-static's first pair),
//!   401 wrong code, 429 locked, 503 store trouble;
//! * `POST /api/v1/admin {"device":".."}` -> device resume: a fresh token
//!   without a code (console-static only).
//!
//! Safety rules the client keeps:
//! * plain HTTP to a **loopback** address only (an `ssh -L` forward or a
//!   local cloudflared); the token never crosses a network in clear;
//! * the token and device secret live only in `<dir>/client.creds`
//!   (dir 0700, file 0600, written atomically); a group/other readable file
//!   is refused, like ssh does with keys;
//! * nothing here prints them: [`Creds`] has a redacting `Debug` and the
//!   CLI shows only the node id (`node-<8 hex>`, a one-way hash);
//! * the pairing code is taken from stdin or a file, never argv (argv shows
//!   in `ps`).
//!
//! Nothing runs on the phone and no device is touched (§11): this is the
//! same client a phone browser is, in Rust, for the operator and for tests.

use galion_common::http::{self, ClientRequest, ClientResponse};
use serde_json::{json, Value};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const COOKIE: &str = "galion_admin";
pub const CREDS_FILE: &str = "client.creds";
const TIMEOUT: Duration = Duration::from_secs(10);
/// Admin replies are tiny; a bigger one is not the gate.
const MAX_ADMIN: usize = 64 * 1024;
/// Largest reply for [`get`] (board catalog pages are ~100 KiB).
pub const MAX_GET: usize = 4 * 1024 * 1024;
/// Longest code accepted from the operator (console-static codes are free
/// text, ours are 8 chars).
pub const MAX_CODE: usize = 128;

/// Where the console is and who we say we are.
#[derive(Debug, Clone)]
pub struct Target {
    pub addr: SocketAddr,
    /// `Host` header, e.g. `console.galion.app` (a remote console host, so
    /// the real admin gate applies) or `127.0.0.1`.
    pub host: String,
    /// `Origin` sent on POST (`https://<host>` by default).
    pub origin: String,
}

impl Target {
    /// Refuses a non-loopback address: the client speaks plain HTTP.
    pub fn new(addr: SocketAddr, host: &str) -> Result<Target, ClientError> {
        if !addr.ip().is_loopback() {
            return Err(ClientError::NotLoopback);
        }
        if host.is_empty() || host.bytes().any(|c| !(c.is_ascii_graphic()) || c == b'/') {
            return Err(ClientError::Invalid("host"));
        }
        Ok(Target {
            addr,
            host: host.to_string(),
            origin: format!("https://{host}"),
        })
    }
}

/// Device credentials. `Debug` never shows them.
#[derive(Clone, PartialEq, Eq)]
pub struct Creds {
    token: String,
    device: String,
}

impl fmt::Debug for Creds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Creds {{ node: {}, device: {} }}",
            self.node(),
            if self.device.is_empty() {
                "none"
            } else {
                "<redacted>"
            }
        )
    }
}

/// base64url-ish secret: 16..=128 chars of `[A-Za-z0-9_-]`.
pub fn valid_secret(s: &str) -> bool {
    (16..=128).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

impl Creds {
    pub fn new(token: &str, device: &str) -> Result<Creds, ClientError> {
        if !valid_secret(token) || !(device.is_empty() || valid_secret(device)) {
            return Err(ClientError::BadCookie);
        }
        Ok(Creds {
            token: token.into(),
            device: device.into(),
        })
    }
    /// Same node id the 307 server shows (`pairing::node_of`).
    pub fn node(&self) -> String {
        crate::pairing::node_of(&self.token)
    }
    pub fn has_device(&self) -> bool {
        !self.device.is_empty()
    }
    fn cookie(&self) -> String {
        format!("{COOKIE}={}", self.token)
    }
    /// True when both secrets are equal in `other` (tests only need this
    /// without seeing the values).
    pub fn same_token(&self, other: &Creds) -> bool {
        self.token == other.token
    }
    /// True if `text` contains either secret (leak checks in tests/CLI).
    pub fn leaks_into(&self, text: &str) -> bool {
        text.contains(&self.token) || (self.has_device() && text.contains(&self.device))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    NotLoopback,
    Invalid(&'static str),
    Transport(String),
    /// 401 wrong code / unknown device.
    Refused,
    /// 429; `Retry-After` seconds when sent.
    Locked(Option<u64>),
    /// 403 origin refused (307 server).
    Origin,
    /// 503 store unreadable / not configured / no randomness.
    Store,
    /// 404 on `/api/v1/admin`: this console build does not serve the admin
    /// gate (live :8080 before D-303-1 / D-313-1).
    NoGate,
    /// 200 without a usable `galion_admin` cookie.
    BadCookie,
    /// Anything else: status and a short, secret-free reason.
    Unexpected(u16, String),
    NoCreds,
    CredsFile(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::NotLoopback => write!(
                f,
                "console address must be loopback (use an ssh -L forward)"
            ),
            ClientError::Invalid(w) => write!(f, "invalid {w}"),
            ClientError::Transport(e) => write!(f, "transport: {e}"),
            ClientError::Refused => write!(f, "refused (wrong code or unknown device)"),
            ClientError::Locked(Some(s)) => write!(f, "locked, retry after {s} s"),
            ClientError::Locked(None) => write!(f, "locked (too many attempts)"),
            ClientError::Origin => write!(f, "origin refused"),
            ClientError::Store => write!(f, "console admin store unavailable (503)"),
            ClientError::NoGate => write!(
                f,
                "console does not serve /api/v1/admin (404): admin store not deployed"
            ),
            ClientError::BadCookie => write!(f, "no usable galion_admin cookie in reply"),
            ClientError::Unexpected(s, why) => write!(f, "unexpected {s}: {why}"),
            ClientError::NoCreds => write!(f, "not paired (no client.creds)"),
            ClientError::CredsFile(e) => write!(f, "client.creds: {e}"),
        }
    }
}

impl std::error::Error for ClientError {}

/// What the gate says about us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub paired: bool,
    pub you: bool,
    pub node: Option<String>,
}

/// Cookie attributes as received (for the security checks in tests and the
/// CLI summary).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CookieAttrs {
    pub http_only: bool,
    pub secure: bool,
    pub same_site: String,
    pub path: String,
    pub max_age: Option<i64>,
}

/// A successful pair or resume.
#[derive(Debug, Clone)]
pub struct Paired {
    pub creds: Creds,
    pub attrs: CookieAttrs,
    /// True when the server issued a device secret in this reply.
    pub device_issued: bool,
}

/// Parse one `Set-Cookie` value for `galion_admin`. `None` if it is another
/// cookie or the value is not a plausible token (an empty value = cleared).
pub fn parse_set_cookie(v: &str) -> Option<(String, CookieAttrs)> {
    let mut parts = v.split(';').map(str::trim);
    let (k, val) = parts.next()?.split_once('=')?;
    if k != COOKIE || !valid_secret(val) {
        return None;
    }
    let mut a = CookieAttrs::default();
    for p in parts {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        match k.to_ascii_lowercase().as_str() {
            "httponly" => a.http_only = true,
            "secure" => a.secure = true,
            "samesite" => a.same_site = v.to_string(),
            "path" => a.path = v.to_string(),
            "max-age" => a.max_age = v.parse().ok(),
            _ => {}
        }
    }
    Some((val.to_string(), a))
}

fn transport(e: http::ClientError) -> ClientError {
    ClientError::Transport(e.to_string())
}

fn short(body: &[u8]) -> String {
    // Error bodies are `{"error":".."}`; keep only that field, else a stub.
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .map(|s| s.chars().take(80).collect())
        .unwrap_or_else(|| format!("{} bytes", body.len()))
}

fn send(t: &Target, req: &ClientRequest<'_>, max: usize) -> Result<ClientResponse, ClientError> {
    let r = http::send(t.addr, req, TIMEOUT, max).map_err(transport)?;
    if !r.complete {
        return Err(ClientError::Transport("short reply".into()));
    }
    Ok(r)
}

/// `GET /api/v1/admin` with or without our cookie.
pub fn session(t: &Target, creds: Option<&Creds>) -> Result<Session, ClientError> {
    let mut req = ClientRequest::get("/api/v1/admin", &t.host);
    if let Some(c) = creds {
        req.headers.push(("Cookie".into(), c.cookie()));
    }
    let r = send(t, &req, MAX_ADMIN)?;
    match r.status {
        200 => {}
        401 => return Err(ClientError::Refused),
        404 => return Err(ClientError::NoGate),
        503 => return Err(ClientError::Store),
        s => return Err(ClientError::Unexpected(s, short(&r.body))),
    }
    let v: Value = serde_json::from_slice(&r.body)
        .map_err(|_| ClientError::Unexpected(200, "not json".into()))?;
    let you = v.get("you").and_then(Value::as_bool).unwrap_or(false);
    // console-static always sends "paired"; the 307 server sends it too.
    let paired = v.get("paired").and_then(Value::as_bool).unwrap_or(you);
    let node = v.get("node").and_then(Value::as_str).map(str::to_string);
    Ok(Session { paired, you, node })
}

fn post_admin(t: &Target, body: &Value, keep_device: &str) -> Result<Paired, ClientError> {
    let raw = body.to_string();
    let mut req = ClientRequest::post("/api/v1/admin", &t.host, "application/json", raw.as_bytes());
    req.headers.push(("Origin".into(), t.origin.clone()));
    let r = send(t, &req, MAX_ADMIN)?;
    match r.status {
        200 => {}
        401 => return Err(ClientError::Refused),
        403 => return Err(ClientError::Origin),
        404 => return Err(ClientError::NoGate),
        429 => {
            return Err(ClientError::Locked(
                r.header("retry-after").and_then(|s| s.trim().parse().ok()),
            ))
        }
        503 => return Err(ClientError::Store),
        s => return Err(ClientError::Unexpected(s, short(&r.body))),
    }
    let (token, attrs) = r
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .find_map(|(_, v)| parse_set_cookie(v))
        .ok_or(ClientError::BadCookie)?;
    let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
    let issued = v
        .get("device")
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .unwrap_or("");
    let device_issued = !issued.is_empty();
    let device = if device_issued { issued } else { keep_device };
    Ok(Paired {
        creds: Creds::new(&token, device)?,
        attrs,
        device_issued,
    })
}

/// Pair with a one-time code (normalised: trimmed; the servers upper-case).
pub fn pair_code(t: &Target, code: &str, label: Option<&str>) -> Result<Paired, ClientError> {
    let code = code.trim();
    if code.is_empty() || code.len() > MAX_CODE || code.chars().any(char::is_control) {
        return Err(ClientError::Invalid("code"));
    }
    let mut body = json!({ "code": code });
    if let Some(l) = label {
        body["label"] = Value::String(crate::pairing::clean_label(l));
    }
    post_admin(t, &body, "")
}

/// Fresh token from the stored device secret (no code). The device secret
/// is kept; the old token stays valid server-side until rotated out.
pub fn resume(t: &Target, creds: &Creds) -> Result<Paired, ClientError> {
    if !creds.has_device() {
        return Err(ClientError::Invalid(
            "no device secret (server issued none)",
        ));
    }
    post_admin(t, &json!({ "device": creds.device }), &creds.device)
}

/// Authenticated GET (`Cookie: galion_admin=..`) of any console path.
pub fn get(t: &Target, creds: &Creds, path: &str) -> Result<ClientResponse, ClientError> {
    if !path.starts_with('/') {
        return Err(ClientError::Invalid("path"));
    }
    let mut req = ClientRequest::get(path, &t.host);
    req.headers.push(("Cookie".into(), creds.cookie()));
    send(t, &req, MAX_GET)
}

// ------------------------------------------------------------ creds file

pub fn creds_path(dir: &Path) -> PathBuf {
    dir.join(CREDS_FILE)
}

/// Write `<dir>/client.creds` (dir 0700, file 0600, temp + rename).
pub fn save(dir: &Path, c: &Creds) -> Result<(), ClientError> {
    let e = |e: io::Error| ClientError::CredsFile(e.kind().to_string());
    if !dir.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(e)?;
    }
    let path = creds_path(dir);
    let tmp = dir.join(format!(".{CREDS_FILE}.tmp-{}", std::process::id()));
    let raw = format!(
        "# galion-admin-console client credentials: secret, do not copy\ntoken={}\ndevice={}\n",
        c.token, c.device
    );
    let res = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(raw.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, &path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(e)
}

/// Read `<dir>/client.creds`. Missing = [`ClientError::NoCreds`]; a
/// group/other readable file is refused.
pub fn load(dir: &Path) -> Result<Creds, ClientError> {
    let path = creds_path(dir);
    let meta = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(ClientError::NoCreds),
        Err(e) => return Err(ClientError::CredsFile(e.kind().to_string())),
    };
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(ClientError::CredsFile(
            "permissions too open (want 0600)".into(),
        ));
    }
    if meta.len() > 4096 {
        return Err(ClientError::CredsFile("too large".into()));
    }
    let raw =
        fs::read_to_string(&path).map_err(|e| ClientError::CredsFile(e.kind().to_string()))?;
    let field = |k: &str| {
        raw.lines()
            .find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    Creds::new(&field("token"), &field("device"))
        .map_err(|_| ClientError::CredsFile("corrupt".into()))
}

/// Remove the local credentials (server side stays until rotated/revoked).
pub fn forget(dir: &Path) -> Result<bool, ClientError> {
    match fs::remove_file(creds_path(dir)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(ClientError::CredsFile(e.kind().to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use galion_common::http::{Limits, Request, Response};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const TOK: &str = "AAAAbbbbCCCCddddEEEEffffGGGGhhhhIIIIjjjjKKK";
    const DEV: &str = "devdevdevdevdevdevdevdevdevdevdevdevdevdev0";

    fn tmp(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gac-client-{n}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    /// Tiny console-static-shaped gate on an ephemeral loopback port.
    fn fake() -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let h = move |r: &Request| {
            let line = format!(
                "{} {} host={} origin={} cookie={}",
                r.method(),
                r.path(),
                r.header("host").unwrap_or(""),
                r.header("origin").unwrap_or(""),
                r.header("cookie").unwrap_or("")
            );
            s2.lock().unwrap().push(line);
            let body = String::from_utf8_lossy(&r.body).to_string();
            let ok_cookie = r.header("cookie") == Some(&format!("galion_admin={TOK}")[..]);
            match (r.method(), r.path()) {
                ("GET", "/api/v1/admin") => Response::json(
                    200,
                    format!(r#"{{"ok":true,"paired":true,"you":{ok_cookie}}}"#),
                ),
                ("POST", "/api/v1/admin") if body.contains("RIGHT") => Response::json(
                    200,
                    format!(r#"{{"ok":true,"you":true,"device":"{DEV}"}}"#),
                )
                .with_header(
                    "set-cookie",
                    &format!("galion_admin={TOK}; HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000; Secure"),
                ),
                ("POST", "/api/v1/admin") if body.contains(DEV) => {
                    Response::json(200, r#"{"ok":true,"you":true,"device":""}"#).with_header(
                        "set-cookie",
                        &format!("galion_admin={TOK}; HttpOnly; Path=/; SameSite=Lax"),
                    )
                }
                ("POST", "/api/v1/admin") if body.contains("LOCK") => {
                    Response::json(429, r#"{"error":"too many attempts"}"#)
                        .with_header("retry-after", "900")
                }
                ("POST", "/api/v1/admin") if body.contains("NOCOOKIE") => {
                    Response::json(200, r#"{"ok":true}"#)
                }
                ("POST", "/api/v1/admin") if body.contains("STORE") => {
                    Response::json(503, r#"{"error":"admin store unreadable"}"#)
                }
                ("POST", "/api/v1/admin") => Response::json(401, r#"{"error":"wrong code"}"#),
                ("GET", "/api/v1/services") if ok_cookie => Response::json(200, r#"{"count":1}"#),
                _ => Response::json(401, r#"{"error":"phone required"}"#),
            }
        };
        std::thread::spawn(move || {
            let _ = http::serve(l, Limits::default(), Arc::new(h));
        });
        (a, seen)
    }

    #[test]
    fn refuses_non_loopback_and_bad_host() {
        let a: SocketAddr = "10.0.0.1:80".parse().unwrap();
        assert_eq!(Target::new(a, "x").unwrap_err(), ClientError::NotLoopback);
        let l: SocketAddr = "127.0.0.1:1".parse().unwrap();
        assert!(Target::new(l, "a b").is_err());
        assert!(Target::new(l, "").is_err());
        assert!(Target::new(l, "x/y").is_err());
        assert_eq!(
            Target::new(l, "console.galion.app").unwrap().origin,
            "https://console.galion.app"
        );
    }

    #[test]
    fn cookie_parse() {
        let (t, a) = parse_set_cookie(&format!(
            "galion_admin={TOK}; HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000; Secure"
        ))
        .unwrap();
        assert_eq!(t, TOK);
        assert!(a.http_only && a.secure);
        assert_eq!(
            (a.same_site.as_str(), a.path.as_str(), a.max_age),
            ("Lax", "/", Some(31536000))
        );
        assert!(
            parse_set_cookie("galion_admin=; Max-Age=0").is_none(),
            "cleared"
        );
        assert!(parse_set_cookie("other=AAAAAAAAAAAAAAAAAAAA").is_none());
        assert!(parse_set_cookie("galion_admin=has space inside here").is_none());
        assert!(parse_set_cookie("galion_admin=short").is_none());
    }

    #[test]
    fn debug_redacts() {
        let c = Creds::new(TOK, DEV).unwrap();
        let d = format!("{c:?}");
        assert!(!c.leaks_into(&d), "{d}");
        assert!(d.contains("node-") && d.contains("<redacted>"));
        assert!(Creds::new("x", "").is_err());
        assert!(Creds::new(TOK, "bad secret!").is_err());
    }

    #[test]
    fn flow_against_fake_gate() {
        let (a, seen) = fake();
        let t = Target::new(a, "console.galion.app").unwrap();
        assert_eq!(
            session(&t, None).unwrap(),
            Session {
                paired: true,
                you: false,
                node: None
            }
        );
        assert_eq!(
            pair_code(&t, "wrong", None).unwrap_err(),
            ClientError::Refused
        );
        assert_eq!(
            pair_code(&t, "LOCK", None).unwrap_err(),
            ClientError::Locked(Some(900))
        );
        assert_eq!(
            pair_code(&t, "NOCOOKIE", None).unwrap_err(),
            ClientError::BadCookie
        );
        assert_eq!(
            pair_code(&t, "STORE", None).unwrap_err(),
            ClientError::Store
        );
        assert_eq!(
            pair_code(&t, "  ", None).unwrap_err(),
            ClientError::Invalid("code")
        );
        assert_eq!(
            pair_code(&t, "a\nb", None).unwrap_err(),
            ClientError::Invalid("code")
        );
        let p = pair_code(&t, " RIGHT ", Some("my phone!")).unwrap();
        assert!(p.device_issued && p.attrs.secure && p.attrs.http_only);
        assert!(session(&t, Some(&p.creds)).unwrap().you);
        assert_eq!(get(&t, &p.creds, "/api/v1/services").unwrap().status, 200);
        let r = resume(&t, &p.creds).unwrap();
        assert!(!r.device_issued && r.creds.has_device(), "device kept");
        let log = seen.lock().unwrap().join("\n");
        assert!(log.contains("origin=https://console.galion.app"), "{log}");
        assert!(log.contains("host=console.galion.app"));
    }

    #[test]
    fn creds_file_roundtrip_and_perms() {
        let d = tmp("creds");
        assert_eq!(load(&d).unwrap_err(), ClientError::NoCreds);
        let c = Creds::new(TOK, DEV).unwrap();
        save(&d, &c).unwrap();
        let m = fs::metadata(creds_path(&d)).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o600);
        assert_eq!(
            fs::metadata(&d).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(load(&d).unwrap(), c);
        fs::set_permissions(creds_path(&d), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(load(&d), Err(ClientError::CredsFile(_))));
        fs::set_permissions(creds_path(&d), fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(creds_path(&d), "token=nope\n").unwrap();
        fs::set_permissions(creds_path(&d), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(load(&d), Err(ClientError::CredsFile(_))));
        assert!(forget(&d).unwrap());
        assert!(!forget(&d).unwrap());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_gate_is_404() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let h = |_: &Request| Response::text(404, "not found\n");
            let _ = http::serve(l, Limits::default(), Arc::new(h));
        });
        let t = Target::new(a, "console.galion.app").unwrap();
        assert_eq!(session(&t, None).unwrap_err(), ClientError::NoGate);
        assert_eq!(pair_code(&t, "X1", None).unwrap_err(), ClientError::NoGate);
    }

    #[test]
    fn unreachable_is_transport_error() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        drop(l);
        let t = Target::new(a, "127.0.0.1").unwrap();
        assert!(matches!(session(&t, None), Err(ClientError::Transport(_))));
    }
}
