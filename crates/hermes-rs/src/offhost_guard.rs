// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Guards for off-host continuity snapshots (PR #14 review, slot 306
//! findings F1 and F2; fixed in wave-4 slot 314, 2026-10-27 CET).
//!
//! * F1 authenticity: a snapshot that leaves the host is sealed with
//!   HMAC-SHA256 (RFC 2104) under a key that stays on the host
//!   (`HERMES_OFFHOST_HMAC_FILE`). Resume refuses anything that does not
//!   verify, so whoever can write the secondary store can no longer plant a
//!   cron job, a spend ledger or a crafted SQLite file.
//! * F2 secret filter: one strict pattern list is used for the byte scan and
//!   for text redaction (it is a superset of `crate::redact`), and the SQLite
//!   session DB is exported as a sanitised copy (`VACUUM INTO`, per-value
//!   redaction, `secure_delete`, `VACUUM`) instead of the raw file, so freed
//!   pages and overflow-page splits cannot carry a secret off the host.
//!
//! std only (plus rusqlite behind the existing `sqlite` feature). HMAC is the
//! textbook construction over `galion_common::sha256`, checked against the
//! RFC 4231 test vectors below.

use galion_common::sha256::{digest, Sha256};
use std::fs;
use std::path::Path;
#[cfg(feature = "sqlite")]
use std::path::PathBuf;

/// Kind string of the sealed envelope written to every store.
pub const SEALED_KIND: &str = "hermes-offhost-sealed";
/// Only MAC algorithm accepted on resume.
pub const ALG: &str = "hmac-sha256";
const DOMAIN: &[u8] = b"hermes-offhost/v1\0";
const MIN_KEY: usize = 32;
const MAX_KEY_FILE: u64 = 4096;

/// HMAC-SHA256 (RFC 2104) over `parts` concatenated.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.update(&ipad);
    for p in parts {
        inner.update(p);
    }
    let ih = inner.finalize();
    let mut outer = Sha256::new();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.update(&opad);
    outer.update(&ih);
    outer.finalize()
}

/// Constant-time equality for equal-length byte strings.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Host-side sealing key. The bytes are never printed: `Debug` shows the id.
#[derive(Clone)]
pub struct SealKey {
    key: Vec<u8>,
    /// Public key id (first 16 hex of a domain-separated SHA-256).
    pub id: String,
}

impl std::fmt::Debug for SealKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SealKey(id={})", self.id)
    }
}

impl SealKey {
    pub fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        let mut end = raw.len();
        while end > 0 && raw[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        let key = raw[..end].to_vec();
        if key.len() < MIN_KEY {
            return Err(format!(
                "HERMES_OFFHOST_HMAC_FILE must hold at least {MIN_KEY} bytes"
            ));
        }
        let mut h = Sha256::new();
        h.update(b"hermes-offhost-key-id\0");
        h.update(&key);
        let id = galion_common::hex::encode(&h.finalize())[..16].to_string();
        Ok(SealKey { key, id })
    }

    /// Load the key file. It must be a regular file, mode 0600 or stricter,
    /// at most 4 KiB, and it must not live inside the secondary store dir.
    pub fn load(path: &Path, store: Option<&Path>) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(path)
            .map_err(|_| "HERMES_OFFHOST_HMAC_FILE is missing or unreadable".to_string())?;
        if !meta.is_file() {
            return Err("HERMES_OFFHOST_HMAC_FILE is not a regular file".into());
        }
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("HERMES_OFFHOST_HMAC_FILE must be mode 0600 or stricter".into());
        }
        if meta.len() > MAX_KEY_FILE {
            return Err("HERMES_OFFHOST_HMAC_FILE is larger than 4 KiB".into());
        }
        if let Some(store) = store {
            let store_dir = store.parent().unwrap_or(store);
            let kp = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let sd = fs::canonicalize(store_dir).unwrap_or_else(|_| store_dir.to_path_buf());
            if kp.starts_with(&sd) {
                return Err("HERMES_OFFHOST_HMAC_FILE must not live in the secondary store".into());
            }
        }
        let raw = fs::read(path)
            .map_err(|_| "HERMES_OFFHOST_HMAC_FILE is missing or unreadable".to_string())?;
        Self::from_bytes(&raw)
    }

    fn mac(&self, payload: &[u8]) -> [u8; 32] {
        hmac_sha256(&self.key, &[DOMAIN, self.id.as_bytes(), b"\0", payload])
    }

    /// Wrap a snapshot document in a sealed envelope.
    pub fn seal(&self, snap: &serde_json::Value) -> Vec<u8> {
        let payload = serde_json::to_string(snap).unwrap_or_default();
        let mac = galion_common::hex::encode(&self.mac(payload.as_bytes()));
        let env = serde_json::json!({
            "kind": SEALED_KIND,
            "schema": 1,
            "alg": ALG,
            "key_id": self.id,
            "written_at": snap["written_at"],
            "mac": mac,
            "payload": payload,
        });
        serde_json::to_vec_pretty(&env).unwrap_or_default()
    }

    /// Verify an envelope and return the inner snapshot. Errors are fixed
    /// strings (no store content echoed back).
    pub fn open(&self, env: &serde_json::Value) -> Result<serde_json::Value, String> {
        if env["kind"].as_str() != Some(SEALED_KIND) {
            return Err(
                "snapshot is not sealed; an HMAC key is set, so unsigned snapshots are refused"
                    .into(),
            );
        }
        if env["alg"].as_str() != Some(ALG) {
            return Err("snapshot seal algorithm is not hmac-sha256".into());
        }
        if env["key_id"].as_str() != Some(self.id.as_str()) {
            return Err("snapshot was sealed with a different key".into());
        }
        let Some(payload) = env["payload"].as_str() else {
            return Err("sealed snapshot has no payload".into());
        };
        let want = env["mac"]
            .as_str()
            .and_then(galion_common::hex::decode)
            .unwrap_or_default();
        if !ct_eq(&want, &self.mac(payload.as_bytes())) {
            return Err("snapshot MAC mismatch (tampered or wrong key)".into());
        }
        serde_json::from_str(payload).map_err(|_| "sealed payload is not JSON".to_string())
    }
}

/// True when `v` is a sealed envelope (used when no key is configured).
pub fn is_sealed(v: &serde_json::Value) -> bool {
    v["kind"].as_str() == Some(SEALED_KIND)
}

// ---------------------------------------------------------------- F2 ----

const TOKEN: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Prefix, minimum tail of token chars. Superset of `crate::redact` plus the
/// shapes the review found missing (github_pat_, gho_/ghu_/ghs_/ghr_, rv_,
/// AKIA/ASIA, xoxp-/xoxa-/xoxs-/xoxe-/xapp-, glpat-, hf_, AIza, Stripe).
/// No word-boundary check on purpose: inside a SQLite record the byte before
/// a value is the end of the previous column, so a boundary check would let
/// a secret through.
const PREFIXES: &[(&[u8], usize)] = &[
    (b"sk-", 20),
    (b"sk_live_", 16),
    (b"sk_test_", 16),
    (b"rk_live_", 16),
    (b"or-v1-", 20),
    (b"ghp_", 30),
    (b"gho_", 30),
    (b"ghu_", 30),
    (b"ghs_", 30),
    (b"ghr_", 30),
    (b"github_pat_", 22),
    (b"glpat-", 20),
    (b"xoxb-", 10),
    (b"xoxp-", 10),
    (b"xoxa-", 10),
    (b"xoxs-", 10),
    (b"xoxe-", 10),
    (b"xapp-", 10),
    (b"hf_", 30),
    (b"AIza", 30),
    (b"rv_", 20),
    (b"npm_", 30),
    (b"eyJ", 30),
];

fn tail_len(hay: &[u8], from: usize, set: &[u8]) -> usize {
    hay[from..].iter().take_while(|c| set.contains(c)).count()
}

fn upper_alnum(c: &u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

/// Byte spans of secret-shaped tokens in `hay` (sorted, merged).
pub fn secret_spans(hay: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let n = hay.len();
    let mut i = 0;
    while i < n {
        let c = hay[i];
        // Prefix tokens.
        for (p, min) in PREFIXES {
            if hay[i..].starts_with(p) {
                let start = i + p.len();
                let mut t = tail_len(hay, start, TOKEN);
                if t >= *min {
                    if *p == b"eyJ" {
                        // JWT: header.payload.signature
                        let mut end = start + t;
                        while end < n && hay[end] == b'.' {
                            let more = tail_len(hay, end + 1, TOKEN);
                            if more == 0 {
                                break;
                            }
                            end += 1 + more;
                        }
                        t = end - start;
                    }
                    spans.push((i, start + t));
                }
            }
        }
        // AWS access key ids.
        if (hay[i..].starts_with(b"AKIA") || hay[i..].starts_with(b"ASIA"))
            && hay[i + 4..].iter().take_while(|c| upper_alnum(c)).count() >= 16
        {
            spans.push((i, i + 4 + 16));
        }
        // Bearer <token>.
        if (c == b'b' || c == b'B')
            && hay.len() >= i + 7
            && hay[i..i + 6].eq_ignore_ascii_case(b"bearer")
        {
            let mut j = i + 6;
            let sp = hay[j..].iter().take_while(|c| **c == b' ').count();
            if sp > 0 {
                j += sp;
                let t = tail_len(
                    hay,
                    j,
                    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~+/=",
                );
                if t >= 16 {
                    spans.push((j, j + t));
                }
            }
        }
        // Telegram bot token: 8-10 digits ':' 30+ token chars.
        if c == b':' && i >= 8 {
            let digits = hay[..i]
                .iter()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .count();
            if (8..=10).contains(&digits) {
                let t = tail_len(hay, i + 1, TOKEN);
                if t >= 30 {
                    spans.push((i - digits, i + 1 + t));
                }
            }
        }
        // PEM private keys.
        if c == b'-' && hay[i..].starts_with(b"-----BEGIN ") {
            let look = &hay[i..n.min(i + 64)];
            if let Some(k) = find(look, b"PRIVATE KEY-----") {
                let after = i + k + 16;
                let end = find(&hay[after..], b"PRIVATE KEY-----")
                    .map(|e| after + e + 16)
                    .unwrap_or(after);
                spans.push((i, end));
            }
        }
        i += 1;
    }
    spans.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in spans {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Strict byte scan used before anything leaves the host.
pub fn secret_like(bytes: &[u8]) -> bool {
    !secret_spans(bytes).is_empty()
}

/// Replace every secret span with `[REDACTED]`. Returns the count replaced.
pub fn redact_bytes(bytes: &[u8]) -> (Vec<u8>, usize) {
    let spans = secret_spans(bytes);
    if spans.is_empty() {
        return (bytes.to_vec(), 0);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    for (s, e) in &spans {
        out.extend_from_slice(&bytes[at..*s]);
        out.extend_from_slice(b"[REDACTED]");
        at = *e;
    }
    out.extend_from_slice(&bytes[at..]);
    (out, spans.len())
}

/// `crate::redact` followed by the strict span redaction.
pub fn strict_redact(text: &str) -> String {
    let first = crate::redact(text);
    let (out, n) = redact_bytes(first.as_bytes());
    if n == 0 {
        return first;
    }
    // Spans start and end on ASCII bytes, so the result is still UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| "[REDACTED]".into())
}

// ------------------------------------------------- F2: SQLite export ----

/// Result of exporting the session DB.
pub struct DbExport {
    pub bytes: Vec<u8>,
    pub redacted_values: usize,
    pub dropped_virtual: usize,
}

#[cfg(feature = "sqlite")]
fn tmp_file(dir: &Path) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Wall clock only makes the temp name unique; it is never shown as a
    // time (so the repo-check `utc` review warning is expected here).
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dir.join(format!(".offhost-db-{}-{nanos}.tmp", std::process::id()))
}

#[cfg(feature = "sqlite")]
fn cleanup(p: &Path) {
    let _ = fs::remove_file(p);
    let mut j = p.as_os_str().to_os_string();
    j.push("-journal");
    let _ = fs::remove_file(PathBuf::from(j));
}

/// Sanitised copy of a SQLite DB: `VACUUM INTO` a private temp file (drops
/// freed pages), drop virtual tables (their shadow copies), redact every
/// TEXT/BLOB value with the strict patterns under `secure_delete`, `VACUUM`
/// again, read the bytes, delete the temp file. Fails closed with a short
/// reason; the caller omits the DB then.
#[cfg(feature = "sqlite")]
pub fn export_sqlite(src: &Path, tmp_dir: &Path) -> Result<DbExport, &'static str> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::create_dir_all(tmp_dir).map_err(|_| "tmp_dir")?;
    let tmp = tmp_file(tmp_dir);
    cleanup(&tmp);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|_| "tmp_create")?;
    let r = export_sqlite_inner(src, &tmp);
    cleanup(&tmp);
    r
}

#[cfg(feature = "sqlite")]
fn export_sqlite_inner(src: &Path, tmp: &Path) -> Result<DbExport, &'static str> {
    use rusqlite::types::{Value as SqlValue, ValueRef};
    use rusqlite::{Connection, OpenFlags};
    let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    {
        let c = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| "not_sqlite")?;
        let _ = c.busy_timeout(std::time::Duration::from_secs(5));
        let t = tmp.to_str().ok_or("tmp_path")?;
        c.execute("VACUUM INTO ?1", [t]).map_err(|_| "not_sqlite")?;
    }
    let c = Connection::open(tmp).map_err(|_| "copy_open")?;
    c.execute_batch("PRAGMA secure_delete=ON; PRAGMA journal_mode=MEMORY;")
        .map_err(|_| "copy_pragma")?;
    let names = |sql: &str| -> Result<Vec<(String, String)>, &'static str> {
        let mut st = c.prepare(sql).map_err(|_| "schema")?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                ))
            })
            .map_err(|_| "schema")?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|_| "schema")
    };
    let virt = names(
        "SELECT name, sql FROM sqlite_master WHERE type='table' AND upper(sql) LIKE 'CREATE VIRTUAL TABLE%'",
    )?;
    for (name, _) in &virt {
        c.execute_batch(&format!("DROP TABLE {}", q(name)))
            .map_err(|_| "virtual_table")?;
    }
    let tables = names(
        "SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let mut redacted = 0usize;
    c.execute_batch("BEGIN").map_err(|_| "txn")?;
    for (table, sql) in &tables {
        if sql.to_ascii_uppercase().contains("WITHOUT ROWID") {
            return Err("without_rowid_table");
        }
        let cols: Vec<String> = {
            let mut st = c
                .prepare(&format!("PRAGMA table_info({})", q(table)))
                .map_err(|_| "schema")?;
            let rows = st
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|_| "schema")?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|_| "schema")?
        };
        for col in &cols {
            let mut fixes: Vec<(i64, SqlValue)> = Vec::new();
            {
                let mut st = c
                    .prepare(&format!(
                        "SELECT rowid, {c} FROM {t} WHERE typeof({c}) IN ('text','blob')",
                        c = q(col),
                        t = q(table)
                    ))
                    .map_err(|_| "scan")?;
                let mut rows = st.query([]).map_err(|_| "scan")?;
                while let Some(r) = rows.next().map_err(|_| "scan")? {
                    let id: i64 = r.get(0).map_err(|_| "scan")?;
                    match r.get_ref(1).map_err(|_| "scan")? {
                        ValueRef::Text(t) => {
                            let s = String::from_utf8_lossy(t);
                            let n = strict_redact(&s);
                            if n.as_bytes() != t {
                                fixes.push((id, SqlValue::Text(n)));
                            }
                        }
                        ValueRef::Blob(b) => {
                            let (out, k) = redact_bytes(b);
                            if k > 0 {
                                fixes.push((id, SqlValue::Blob(out)));
                            }
                        }
                        _ => {}
                    }
                }
            }
            for (id, v) in fixes {
                c.execute(
                    &format!("UPDATE {} SET {} = ?1 WHERE rowid = ?2", q(table), q(col)),
                    rusqlite::params![v, id],
                )
                .map_err(|_| "update")?;
                redacted += 1;
            }
        }
    }
    c.execute_batch("COMMIT").map_err(|_| "txn")?;
    c.execute_batch("VACUUM").map_err(|_| "vacuum")?;
    drop(c);
    let bytes = fs::read(tmp).map_err(|_| "copy_read")?;
    Ok(DbExport {
        bytes,
        redacted_values: redacted,
        dropped_virtual: virt.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        galion_common::hex::encode(b)
    }

    #[test]
    fn rfc4231_vectors() {
        // Test case 1
        let k = [0x0bu8; 20];
        assert_eq!(
            hex(&hmac_sha256(&k, &[b"Hi There"])),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        // Test case 2
        assert_eq!(
            hex(&hmac_sha256(
                b"Jefe",
                &[b"what do ya want ", b"for nothing?"]
            )),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6 (key longer than block)
        let k = [0xaau8; 131];
        assert_eq!(
            hex(&hmac_sha256(
                &k,
                &[b"Test Using Larger Than Block-Size Key - Hash Key First"]
            )),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    fn key() -> SealKey {
        SealKey::from_bytes(b"0123456789abcdef0123456789abcdef\n").unwrap()
    }

    #[test]
    fn seal_open_roundtrip_and_tamper() {
        let k = key();
        assert_eq!(k.id.len(), 16);
        assert!(!format!("{k:?}").contains("0123456789abcdef"));
        let snap =
            serde_json::json!({"schema":1,"kind":"hermes-offhost","written_at":"x","files":[]});
        let sealed = k.seal(&snap);
        let env: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
        assert_eq!(k.open(&env).unwrap(), snap);
        // Tamper the payload.
        let mut bad = env.clone();
        bad["payload"] = serde_json::json!(env["payload"].as_str().unwrap().replace("[]", "[1]"));
        assert!(k.open(&bad).unwrap_err().contains("MAC mismatch"));
        // Recompute a plain sha256 "mac": still refused.
        let mut forged = bad.clone();
        forged["mac"] = serde_json::json!(galion_common::sha256::hex(
            bad["payload"].as_str().unwrap().as_bytes()
        ));
        assert!(k.open(&forged).is_err());
        // Other key.
        let other = SealKey::from_bytes(&[b'z'; 40]).unwrap();
        assert!(other.open(&env).unwrap_err().contains("different key"));
        // Unsigned document.
        assert!(k.open(&snap).unwrap_err().contains("unsigned"));
        // Wrong alg.
        let mut alg = env.clone();
        alg["alg"] = serde_json::json!("none");
        assert!(k.open(&alg).is_err());
    }

    #[test]
    fn short_key_and_loose_mode_are_refused() {
        assert!(SealKey::from_bytes(b"short").is_err());
        let dir = std::env::temp_dir().join(format!("offhost-guard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("store")).unwrap();
        let kf = dir.join("k");
        fs::write(&kf, [b'k'; 48]).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&kf, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(SealKey::load(&kf, None).unwrap_err().contains("0600"));
        fs::set_permissions(&kf, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(SealKey::load(&kf, None).is_ok());
        let inside = dir.join("store").join("k");
        fs::write(&inside, [b'k'; 48]).unwrap();
        fs::set_permissions(&inside, fs::Permissions::from_mode(0o600)).unwrap();
        let store = dir.join("store").join("continuity.json");
        assert!(SealKey::load(&inside, Some(&store))
            .unwrap_err()
            .contains("secondary store"));
        assert!(SealKey::load(&kf, Some(&store)).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    fn fake(parts: &[&str]) -> String {
        parts.concat()
    }

    #[test]
    fn strict_patterns_catch_review_misses() {
        let cases = [
            fake(&[
                "github",
                "_pat_",
                "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
            ]),
            fake(&["gh", "o_", "abcdefghijklmnopqrstuvwxyz0123456789"]),
            fake(&["gh", "s_", "abcdefghijklmnopqrstuvwxyz0123456789"]),
            fake(&["xo", "xp-", "1234567890-abcdef"]),
            fake(&["AK", "IA", "ABCDEFGHIJKLMNOP"]),
            fake(&["r", "v_", "abcdefghijklmnopqrstuvwxyz"]),
            fake(&["s", "k-", "or-v1-abcdefghijklmnopqrstuvwxyz"]),
            fake(&["gl", "pat-", "abcdefghijklmnopqrstu"]),
            fake(&["Bearer ", "abcdefghijklmnopqrstuvwxyz.012"]),
            fake(&["123456789", ":", "AAHabcdefghijklmnopqrstuvwxyz01234"]),
            fake(&[
                "-----BEGIN ",
                "OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----",
            ]),
            fake(&[
                "ey",
                "JhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
                ".eyJzdWIiOiIxIn0.sig_abc",
            ]),
        ];
        for c in &cases {
            assert!(secret_like(c.as_bytes()), "missed {}", &c[..6]);
            let r = strict_redact(&format!("before {c} after"));
            assert!(!r.contains(c.as_str()), "not redacted {}", &c[..6]);
            assert!(r.contains("[REDACTED]"));
            assert!(r.starts_with("before ") && r.ends_with(" after"), "{r}");
        }
        // Binary context: preceded by a record byte, no word boundary.
        let mut b = vec![0x13u8, b'x'];
        b.extend(cases[0].as_bytes());
        assert!(secret_like(&b));
        // Ordinary text stays.
        for ok in [
            "hello world",
            "task-1234 sk-short",
            "ghp_short",
            "Bearer x",
            "time 12:30:00",
            "[REDACTED]",
        ] {
            assert!(!secret_like(ok.as_bytes()), "{ok}");
            assert_eq!(strict_redact(ok), crate::redact(ok));
        }
        // UTF-8 around a secret survives.
        let s = format!("žđ {} čć", cases[0]);
        assert_eq!(strict_redact(&s), "žđ [REDACTED] čć");
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_export_redacts_values_and_freed_pages() {
        use rusqlite::Connection;
        let dir = std::env::temp_dir().join(format!("offhost-guard-db-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join("rs-state.db");
        let pat = fake(&[
            "github",
            "_pat_",
            "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz",
        ]);
        let gone = fake(&["gh", "p_", "DELETEDabcdefghijklmnopqrstuvwxyz0123456789"]);
        // Long value spills to overflow pages (> page size).
        let long = format!("{}{}{}", "a".repeat(4090), pat, "b".repeat(5000));
        {
            let c = Connection::open(&db).unwrap();
            c.execute_batch(crate::session_sqlite::SCHEMA).unwrap();
            c.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('s1','api_server',1.0)",
                [],
            )
            .unwrap();
            for (i, body) in [
                format!("hi {pat}"),
                long.clone(),
                gone.clone(),
                "plain".into(),
            ]
            .iter()
            .enumerate()
            {
                c.execute(
                    "INSERT INTO messages (session_id, role, content, timestamp, display_identity) VALUES ('s1','user',?1,?2,?3)",
                    rusqlite::params![body, i as f64, pat.as_bytes()],
                )
                .unwrap();
            }
            c.execute("DELETE FROM messages WHERE content = ?1", [&gone])
                .unwrap();
        }
        let raw = fs::read(&db).unwrap();
        assert!(secret_like(&raw));
        // The deleted row is still in a freed page of the source file.
        assert!(
            find(&raw, gone.as_bytes()).is_some(),
            "fixture: no freed-page residue"
        );
        let out = export_sqlite(&db, &dir).unwrap();
        assert!(!secret_like(&out.bytes), "secret left in export");
        assert!(
            find(&out.bytes, gone.as_bytes()).is_none(),
            "freed page leaked"
        );
        assert!(out.redacted_values >= 5, "{}", out.redacted_values);
        // The export is a valid DB with the plain rows intact.
        let back = dir.join("back.db");
        fs::write(&back, &out.bytes).unwrap();
        let c = Connection::open(&back).unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
        let plain: String = c
            .query_row(
                "SELECT content FROM messages WHERE content='plain'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plain, "plain");
        let first: String = c
            .query_row(
                "SELECT content FROM messages ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(first, "hi [REDACTED]");
        // No temp files left behind.
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".offhost-db-"))
            .collect();
        assert!(left.is_empty());
        // The source DB is untouched.
        assert_eq!(fs::read(&db).unwrap(), raw);
        let _ = fs::remove_dir_all(&dir);
    }
}
