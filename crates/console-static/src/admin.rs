// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Admin-device pairing (`/api/v1/admin` + the `requireAdmin` gate): Rust
//! port of console-ide `src/lib/admin-device.ts` from the former Node board.
//!
//! The store is a small JSON file (`--admin-store FILE`, optional mirror
//! `--admin-mirror FILE`) holding only SHA-256 hashes:
//! `{codeHash, tokenHash, tokenHashes[<=4], deviceHash, fails, lockedUntil}`.
//! A request is admin when its `galion_admin` cookie hashes to one of the
//! token hashes. Pairing takes the code (upper-cased, as in Galion One) or the
//! device secret, issues a fresh 32-byte base64url token and keeps the last 4.
//! Five wrong codes lock pairing for 15 minutes.
//!
//! Hardening over the TypeScript: an unreadable or corrupt store answers
//! `503` and is never overwritten (the TS treated it as empty and wrote over
//! it); writes are atomic (temp + rename, mode 0600); hashes compare in
//! constant time on the decoded bytes; one in-process lock serialises
//! read-modify-write.

use crate::sha256;
use console_ui::json::{self, Value};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const COOKIE: &str = "galion_admin";
/// Token hashes kept (oldest dropped first).
pub const MAX_TOKENS: usize = 4;
/// Wrong codes before the lock.
pub const MAX_FAILS: f64 = 5.0;
/// Lock length after [`MAX_FAILS`] (ms).
pub const LOCK_MS: f64 = 15.0 * 60.0 * 1000.0;
/// Largest store file read (bytes); a real one is ~400 B.
pub const MAX_STORE: u64 = 64 * 1024;

#[derive(Debug)]
pub struct AdminStore {
    path: PathBuf,
    mirror: Option<PathBuf>,
    lock: Mutex<()>,
}

/// Store could not be read safely (permissions, corrupt JSON, too large).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pair {
    /// 200: Set-Cookie value and the one-time device secret (first pair only).
    Ok { cookie: String, device: String },
    /// 401 wrong code / 429 locked / 503 store or randomness trouble.
    Err { status: u16, error: &'static str },
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Lower-case hex SHA-256 (`sha()` in the TS).
pub fn sha_hex(v: &str) -> String {
    sha256::hex(v.as_bytes())
}

fn unhex(h: &str) -> Option<Vec<u8>> {
    let b = h.trim().as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    let val = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    b.chunks(2)
        .map(|p| Some(val(p[0])? << 4 | val(p[1])?))
        .collect()
}

/// `same(hash, raw)`: sha256(raw) equals the stored hex hash, constant time
/// over the 32 bytes.
pub fn same(hash: &str, raw: &str) -> bool {
    let next = sha256::digest(raw.as_bytes());
    let Some(prev) = unhex(hash) else {
        return false;
    };
    if prev.len() != next.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in prev.iter().zip(next.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// base64url without padding (`toString("base64url")`).
pub fn b64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        let k = c.len() + 1;
        for i in 0..k {
            out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

fn random_token() -> io::Result<String> {
    let mut buf = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(b64url(&buf))
}

fn pct_decode(v: &str) -> Option<String> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = v.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `cookieOf()`: the decoded `galion_admin` value, or "".
pub fn cookie_of(header: Option<&str>) -> String {
    let Some(h) = header else {
        return String::new();
    };
    for part in h.split(';') {
        let part = part.trim();
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        if k == COOKIE {
            return pct_decode(v).unwrap_or_default();
        }
    }
    String::new()
}

fn hashes_of(store: &Value) -> Vec<String> {
    let mut list: Vec<String> = store
        .get("tokenHashes")
        .as_array()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if let Some(t) = store.get("tokenHash").as_str() {
        if !t.is_empty() && !list.iter().any(|x| x == t) {
            list.push(t.to_string());
        }
    }
    list
}

fn token_ok(store: &Value, token: &str) -> bool {
    !token.is_empty() && hashes_of(store).iter().any(|h| same(h, token))
}

fn set(store: &mut Value, key: &str, v: Value) {
    if let Value::Obj(m) = store {
        if let Some(slot) = m.iter_mut().find(|(k, _)| k == key) {
            slot.1 = v;
        } else {
            m.push((key.to_string(), v));
        }
    }
}

fn write_atomic(path: &Path, raw: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() && !dir.exists() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
        }
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("admin-device.json");
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let res = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(raw)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

impl AdminStore {
    pub fn new(path: PathBuf, mirror: Option<PathBuf>) -> AdminStore {
        AdminStore {
            path,
            mirror,
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Missing file = empty store (never paired). Anything else that fails
    /// is a [`StoreError`].
    fn read(&self) -> Result<Value, StoreError> {
        let f = match fs::File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Value::Obj(Vec::new())),
            Err(_) => return Err(StoreError),
        };
        let mut raw = Vec::new();
        f.take(MAX_STORE + 1)
            .read_to_end(&mut raw)
            .map_err(|_| StoreError)?;
        if raw.len() as u64 > MAX_STORE {
            return Err(StoreError);
        }
        match json::parse(&raw) {
            Ok(v @ Value::Obj(_)) => Ok(v),
            _ => Err(StoreError),
        }
    }

    fn write(&self, store: &Value) -> io::Result<()> {
        let mut raw = store.to_json();
        raw.push('\n');
        write_atomic(&self.path, raw.as_bytes())?;
        if let Some(m) = &self.mirror {
            // Mirror is best effort, as a second copy (the TS wrote both).
            if let Err(e) = write_atomic(m, raw.as_bytes()) {
                eprintln!("console-static: admin mirror write failed: {}", e.kind());
            }
        }
        Ok(())
    }

    /// `requireAdmin()`: true only for a cookie matching a stored token hash.
    pub fn is_admin(&self, cookie_header: Option<&str>) -> bool {
        let token = cookie_of(cookie_header);
        if token.is_empty() {
            return false;
        }
        self.read().map(|s| token_ok(&s, &token)).unwrap_or(false)
    }

    /// `adminSession()` -> `(paired, you)`.
    pub fn session(&self, cookie_header: Option<&str>) -> Result<(bool, bool), StoreError> {
        let s = self.read()?;
        let token = cookie_of(cookie_header);
        Ok((!hashes_of(&s).is_empty(), token_ok(&s, &token)))
    }

    fn issue(&self, store: &Value, device_hash: &str, device: String, secure: bool) -> Pair {
        let token = match random_token() {
            Ok(t) => t,
            Err(_) => {
                return Pair::Err {
                    status: 503,
                    error: "no randomness",
                }
            }
        };
        let token_hash = sha_hex(&token);
        let mut hashes = hashes_of(store);
        if !hashes.contains(&token_hash) {
            hashes.push(token_hash.clone());
        }
        while hashes.len() > MAX_TOKENS {
            hashes.remove(0);
        }
        let had_device = store
            .get("deviceHash")
            .as_str()
            .is_some_and(|d| !d.is_empty());
        let mut next: Vec<(String, Value)> = Vec::new();
        if let Some(c) = store.get("codeHash").as_str() {
            next.push(("codeHash".into(), Value::Str(c.into())));
        }
        next.push(("tokenHash".into(), Value::Str(token_hash)));
        next.push((
            "tokenHashes".into(),
            Value::Arr(hashes.into_iter().map(Value::Str).collect()),
        ));
        let dh = if had_device {
            store.get("deviceHash").as_str().unwrap_or("").to_string()
        } else {
            device_hash.to_string()
        };
        next.push(("deviceHash".into(), Value::Str(dh)));
        next.push(("fails".into(), Value::Num(0.0)));
        if self.write(&Value::Obj(next)).is_err() {
            return Pair::Err {
                status: 503,
                error: "store not writable",
            };
        }
        let cookie = format!(
            "{COOKIE}={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000{}",
            if secure { "; Secure" } else { "" }
        );
        Pair::Ok {
            cookie,
            device: if had_device { String::new() } else { device },
        }
    }

    /// `pairAdmin()`. `secure` = Host ends with galion.app or a trusted
    /// X-Forwarded-Proto says https.
    pub fn pair(&self, code: &str, device: &str, secure: bool) -> Pair {
        let _g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let store = match self.read() {
            Ok(s) => s,
            Err(_) => {
                return Pair::Err {
                    status: 503,
                    error: "admin store unreadable",
                }
            }
        };
        let presented = device.trim();
        if let Some(dh) = store.get("deviceHash").as_str() {
            if !presented.is_empty() && !dh.is_empty() && same(dh, presented) {
                let dh = dh.to_string();
                return self.issue(&store, &dh, String::new(), secure);
            }
        }
        if let Some(until) = store.get("lockedUntil").as_f64() {
            if now_ms() < until {
                return Pair::Err {
                    status: 429,
                    error: "too many attempts",
                };
            }
        }
        let given = code.trim().to_uppercase();
        let code_hash = store.get("codeHash").as_str().unwrap_or("");
        if code_hash.is_empty() || given.is_empty() || !same(code_hash, &given) {
            let fails = store.get("fails").as_f64().unwrap_or(0.0) + 1.0;
            let mut next = store.clone();
            set(&mut next, "fails", Value::Num(fails));
            if fails >= MAX_FAILS {
                set(&mut next, "lockedUntil", Value::Num(now_ms() + LOCK_MS));
            }
            let _ = self.write(&next);
            return Pair::Err {
                status: 401,
                error: "wrong code",
            };
        }
        let secret = match random_token() {
            Ok(t) => t,
            Err(_) => {
                return Pair::Err {
                    status: 503,
                    error: "no randomness",
                }
            }
        };
        let dh = sha_hex(&secret);
        self.issue(&store, &dh, secret, secure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "console-admin-{}-{}-{name}",
            std::process::id(),
            now_ms() as u64
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn token_of(cookie: &str) -> String {
        cookie
            .split(';')
            .next()
            .unwrap()
            .trim_start_matches("galion_admin=")
            .to_string()
    }

    #[test]
    fn b64url_vectors() {
        assert_eq!(b64url(b""), "");
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(b64url(&[0u8; 32]).len(), 43);
    }

    #[test]
    fn same_is_hash_compare() {
        let h = sha_hex("ABC123");
        assert!(same(&h, "ABC123"));
        assert!(same(&h.to_uppercase(), "ABC123"));
        assert!(!same(&h, "abc123"));
        assert!(!same("zz", "ABC123"));
        assert!(!same("", ""));
    }

    #[test]
    fn cookie_parse() {
        assert_eq!(cookie_of(Some("a=1; galion_admin=x%2Dy; b=2")), "x-y");
        assert_eq!(cookie_of(Some("galion_admin")), "");
        assert_eq!(cookie_of(Some("xgalion_admin=1")), "");
        assert_eq!(cookie_of(None), "");
        assert_eq!(cookie_of(Some("galion_admin=%zz")), "");
    }

    #[test]
    fn missing_store_is_unpaired_and_code_never_matches() {
        let d = tmp("missing");
        let st = AdminStore::new(d.join("admin-device.json"), None);
        assert_eq!(st.session(None), Ok((false, false)));
        assert!(!st.is_admin(Some("galion_admin=abc")));
        match st.pair("anything", "", false) {
            Pair::Err { status, .. } => assert_eq!(status, 401),
            p => panic!("{p:?}"),
        }
        // fails counter written to a new 0600 file.
        let raw = fs::read_to_string(st.path()).unwrap();
        assert!(raw.contains(r#""fails":1"#));
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(st.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn pair_code_then_cookie_then_device_resume_and_rotation() {
        let d = tmp("pair");
        let path = d.join("s/admin-device.json");
        let mirror = d.join("m/admin-device.json");
        let st = AdminStore::new(path.clone(), Some(mirror.clone()));
        write_atomic(
            &path,
            format!("{{\"codeHash\":\"{}\"}}\n", sha_hex("K7Q2")).as_bytes(),
        )
        .unwrap();
        assert_eq!(st.session(None), Ok((false, false)));
        // lower-case code is upper-cased before hashing.
        let (cookie, device) = match st.pair("  k7q2 ", "", true) {
            Pair::Ok { cookie, device } => (cookie, device),
            p => panic!("{p:?}"),
        };
        assert!(cookie.starts_with("galion_admin="));
        assert!(cookie.ends_with("; Secure"));
        assert!(cookie.contains("HttpOnly; Path=/; SameSite=Lax; Max-Age=31536000"));
        assert_eq!(device.len(), 43);
        let tok = token_of(&cookie);
        let hdr = format!("galion_admin={tok}");
        assert!(st.is_admin(Some(&hdr)));
        assert_eq!(st.session(Some(&hdr)), Ok((true, true)));
        assert_eq!(st.session(Some("galion_admin=nope")), Ok((true, false)));
        // Store holds hashes only, never the token or device secret.
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(&tok) && !raw.contains(&device));
        assert!(raw.contains(&sha_hex(&tok)));
        assert_eq!(fs::read_to_string(&mirror).unwrap(), raw);
        // Device resume: new token, no device echoed, deviceHash kept.
        let c2 = match st.pair("", &device, false) {
            Pair::Ok { cookie, device } => {
                assert_eq!(device, "");
                assert!(!cookie.contains("Secure"));
                cookie
            }
            p => panic!("{p:?}"),
        };
        assert!(st.is_admin(Some(&format!("galion_admin={}", token_of(&c2)))));
        assert!(st.is_admin(Some(&hdr)), "older token still valid");
        // Rotation keeps the last 4 tokens.
        for _ in 0..4 {
            assert!(matches!(st.pair("", &device, false), Pair::Ok { .. }));
        }
        assert!(!st.is_admin(Some(&hdr)), "first token rotated out");
        let s = st.read().unwrap();
        assert_eq!(s.get("tokenHashes").as_array().len(), MAX_TOKENS);
        assert_eq!(
            s.get("deviceHash").as_str(),
            Some(sha_hex(&device).as_str())
        );
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn lockout_after_five_and_device_bypasses_lock() {
        let d = tmp("lock");
        let path = d.join("admin-device.json");
        let st = AdminStore::new(path.clone(), None);
        let dev = "devsecret-devsecret-devsecret";
        write_atomic(
            &path,
            format!(
                "{{\"codeHash\":\"{}\",\"deviceHash\":\"{}\"}}",
                sha_hex("RIGHT"),
                sha_hex(dev)
            )
            .as_bytes(),
        )
        .unwrap();
        for i in 1..=5 {
            match st.pair("wrong", "", false) {
                Pair::Err { status, .. } => assert_eq!(status, 401, "try {i}"),
                p => panic!("{p:?}"),
            }
        }
        // Locked now, even with the right code.
        assert_eq!(
            st.pair("right", "", false),
            Pair::Err {
                status: 429,
                error: "too many attempts"
            }
        );
        // The paired device still resumes (as in the TS).
        assert!(matches!(st.pair("", dev, false), Pair::Ok { .. }));
        let s = st.read().unwrap();
        assert_eq!(s.get("fails").as_f64(), Some(0.0));
        assert!(s.get("lockedUntil").is_null());
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn corrupt_store_is_503_and_not_overwritten() {
        let d = tmp("corrupt");
        let path = d.join("admin-device.json");
        fs::write(&path, b"{not json").unwrap();
        let st = AdminStore::new(path.clone(), None);
        assert_eq!(st.session(None), Err(StoreError));
        assert!(!st.is_admin(Some("galion_admin=x")));
        assert!(matches!(
            st.pair("x", "", false),
            Pair::Err { status: 503, .. }
        ));
        assert_eq!(fs::read(&path).unwrap(), b"{not json");
        let _ = fs::remove_dir_all(d);
    }
}
