// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Local secrets vault for the admin of ONE Galion / Obsidian Open Mind
//! instance (hand-off 02-accounts-security, owner order t579u, 2026-11-01 CET).
//! Design and threat model: `SECRETS-VAULT.md` in the hand-off package.
//!
//! * **Envelope encryption.** A random 256-bit data key (DEK) encrypts every
//!   secret. The DEK is stored only wrapped (AEAD-encrypted) by one or more
//!   key-encryption keys (KEKs), one per *slot*, like LUKS key slots:
//!   - `passphrase`: KEK = Argon2id(passphrase, random salt);
//!   - `prf`: KEK = HMAC-SHA256(salt, 32-byte WebAuthn PRF output);
//!   - `recovery`: KEK = HMAC-SHA256(salt, 32-byte recovery key shown once).
//! * **Per-secret AEAD.** XChaCha20-Poly1305, a fresh 192-bit random nonce
//!   per write. The NAME is encrypted with the value; on disk an entry is
//!   keyed by an opaque id = HMAC(index key, name), so the file shows neither
//!   names nor values. The AAD binds every ciphertext to this vault, its id
//!   and the DEK generation, so entries cannot be swapped between vaults.
//! * **Zeroize.** Keys live in [`zeroize::Zeroizing`] buffers and are wiped
//!   on drop; revealed values are returned as `Zeroizing<Vec<u8>>`.
//! * **Session.** An unlocked [`Vault`] has an idle timeout and a short
//!   re-auth window: [`Vault::reveal`] refuses unless the admin proved the
//!   unlock factor again within [`Policy::reauth_secs`].
//! * **Audit.** Every unlock, reveal, copy, set, rotate and export appends a
//!   line (time, action, opaque entry id, ok) to `<vault>.audit.jsonl`:
//!   never a name, never a value.
//! * **Export stays encrypted.** [`Vault::export_encrypted`] re-encrypts all
//!   entries under a FRESH data key wrapped by an export passphrase.
//!
//! All functions take `now` (unix seconds) so time is testable.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const FORMAT: &str = "galion-vault/v1";
pub const AEAD: &str = "xchacha20poly1305";
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const MAX_NAME: usize = 256;

pub type Key = Zeroizing<[u8; KEY_LEN]>;

#[derive(Debug, PartialEq, Eq)]
pub enum VaultError {
    /// No slot opened with the given factor (wrong passphrase / key).
    BadUnlock,
    /// Ciphertext or file failed authentication or parsing.
    Corrupt(&'static str),
    /// The session idled out; unlock again.
    Locked,
    /// Reveal/copy/change needs a fresh re-auth.
    ReauthRequired,
    NotFound,
    BadName(&'static str),
    Weak(&'static str),
    Io(String),
    Rng,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::BadUnlock => write!(f, "unlock failed"),
            VaultError::Corrupt(w) => write!(f, "vault file damaged or tampered: {w}"),
            VaultError::Locked => write!(f, "vault session timed out; unlock again"),
            VaultError::ReauthRequired => {
                write!(f, "re-authenticate before reveal, copy or change")
            }
            VaultError::NotFound => write!(f, "no such secret"),
            VaultError::BadName(w) => write!(f, "bad secret name: {w}"),
            VaultError::Weak(w) => write!(f, "{w}"),
            VaultError::Io(e) => write!(f, "io: {e}"),
            VaultError::Rng => write!(f, "system random generator failed"),
        }
    }
}

impl std::error::Error for VaultError {}

type R<T> = Result<T, VaultError>;

// ---------------------------------------------------------------- helpers

fn rand_bytes<const N: usize>() -> R<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|_| VaultError::Rng)?;
    Ok(b)
}

fn rand_key() -> R<Key> {
    let mut k = Zeroizing::new([0u8; KEY_LEN]);
    getrandom::getrandom(&mut k[..]).map_err(|_| VaultError::Rng)?;
    Ok(k)
}

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let v = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    s.as_bytes()
        .chunks(2)
        .map(|p| Some(v(p[0])? << 4 | v(p[1])?))
        .collect()
}

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = Zeroizing::new([0u8; 64]);
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = Zeroizing::new([0x36u8; 64]);
    let mut opad = Zeroizing::new([0x5cu8; 64]);
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::new()
        .chain_update(&ipad[..])
        .chain_update(msg)
        .finalize();
    Sha256::new()
        .chain_update(&opad[..])
        .chain_update(inner)
        .finalize()
        .into()
}

fn seal(key: &[u8; KEY_LEN], aad: &[u8], msg: &[u8]) -> R<(Vec<u8>, Vec<u8>)> {
    let c = XChaCha20Poly1305::new_from_slice(key).map_err(|_| VaultError::Corrupt("key"))?;
    let nonce: [u8; NONCE_LEN] = rand_bytes()?;
    let ct = c
        .encrypt((&nonce).into(), Payload { msg, aad })
        .map_err(|_| VaultError::Corrupt("seal"))?;
    Ok((nonce.to_vec(), ct))
}

fn open(key: &[u8; KEY_LEN], aad: &[u8], nonce: &[u8], ct: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let c = XChaCha20Poly1305::new_from_slice(key).ok()?;
    let n: [u8; NONCE_LEN] = nonce.try_into().ok()?;
    c.decrypt((&n).into(), Payload { msg: ct, aad })
        .ok()
        .map(Zeroizing::new)
}

fn validate_name(name: &str) -> R<()> {
    if name.is_empty() {
        return Err(VaultError::BadName("empty"));
    }
    if name.len() > MAX_NAME {
        return Err(VaultError::BadName("longer than 256 bytes"));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(VaultError::BadName("control characters"));
    }
    Ok(())
}

// ---------------------------------------------------------------- KDF

/// Argon2id cost for passphrase slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kdf {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl Kdf {
    /// 64 MiB, 3 passes, 1 lane (above the OWASP Argon2id minimum).
    pub const PRODUCTION: Kdf = Kdf {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// Cheap, for unit tests only.
    pub const TEST: Kdf = Kdf {
        m_kib: 64,
        t: 1,
        p: 1,
    };
}

fn argon2_kek(pass: &[u8], salt: &[u8], k: Kdf) -> R<Key> {
    let params = argon2::Params::new(k.m_kib, k.t, k.p, Some(KEY_LEN))
        .map_err(|_| VaultError::Corrupt("kdf params"))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    a.hash_password_into(pass, salt, &mut out[..])
        .map_err(|_| VaultError::Corrupt("kdf"))?;
    Ok(out)
}

/// How the admin unlocks.
pub enum Factor<'a> {
    /// A passphrase (min 12 bytes on init/add).
    Passphrase(&'a [u8]),
    /// The 32-byte output of the WebAuthn PRF extension (hmac-secret) for this
    /// vault's salt, computed by the admin's passkey in the browser/OS.
    Prf(&'a [u8; KEY_LEN]),
    /// The recovery key shown once at init (32 bytes, see [`recovery_from_text`]).
    Recovery(&'a [u8; KEY_LEN]),
}

impl Factor<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Factor::Passphrase(_) => "passphrase",
            Factor::Prf(_) => "prf",
            Factor::Recovery(_) => "recovery",
        }
    }
}

/// Recovery key as text: 64 hex digits in groups of 8.
pub fn recovery_to_text(k: &[u8; KEY_LEN]) -> Zeroizing<String> {
    let h = Zeroizing::new(hex(k));
    let mut s = Zeroizing::new(String::new());
    for (i, ch) in h.chars().enumerate() {
        if i > 0 && i % 8 == 0 {
            s.push('-');
        }
        s.push(ch);
    }
    s
}

pub fn recovery_from_text(t: &str) -> Option<Key> {
    let clean = Zeroizing::new(
        t.chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect::<String>(),
    );
    let b = Zeroizing::new(unhex(&clean)?);
    let arr: [u8; KEY_LEN] = b.as_slice().try_into().ok()?;
    Some(Zeroizing::new(arr))
}

// ---------------------------------------------------------------- file

/// Session rules.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Lock after this many idle seconds.
    pub idle_secs: i64,
    /// Reveal/copy/change only within this many seconds of the last auth.
    pub reauth_secs: i64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            idle_secs: 15 * 60,
            reauth_secs: 60,
        }
    }
}

/// An unlocked vault. Dropping it wipes the data key.
pub struct Vault {
    path: PathBuf,
    doc: Map<String, Value>,
    dek: Key,
    vault_id: String,
    gen: u64,
    policy: Policy,
    last_use: i64,
    last_auth: i64,
}

fn slot_aad(vault_id: &str, kind: &str) -> Vec<u8> {
    format!("{FORMAT}|slot|{vault_id}|{kind}").into_bytes()
}

fn entry_aad(vault_id: &str, gen: u64, id: &str) -> Vec<u8> {
    format!("{FORMAT}|entry|{vault_id}|{gen}|{id}").into_bytes()
}

/// Build a key slot wrapping `dek` under `factor`.
fn make_slot(vault_id: &str, dek: &Key, factor: &Factor<'_>, kdf: Kdf, label: &str) -> R<Value> {
    let salt: [u8; 16] = rand_bytes()?;
    let (kek, kdf_v) = match factor {
        Factor::Passphrase(p) => {
            if p.len() < 12 {
                return Err(VaultError::Weak("passphrase must be at least 12 bytes"));
            }
            (
                argon2_kek(p, &salt, kdf)?,
                json!({"alg": "argon2id", "m_kib": kdf.m_kib, "t": kdf.t, "p": kdf.p}),
            )
        }
        Factor::Prf(k) | Factor::Recovery(k) => (
            Zeroizing::new(hmac_sha256(&salt, &k[..])),
            json!({"alg": "hmac-sha256"}),
        ),
    };
    let (nonce, wrapped) = seal(&kek, &slot_aad(vault_id, factor.kind()), &dek[..])?;
    Ok(json!({
        "kind": factor.kind(),
        "label": label,
        "kdf": kdf_v,
        "salt": hex(&salt),
        "nonce": hex(&nonce),
        "wrapped": hex(&wrapped),
    }))
}

fn try_slot(vault_id: &str, slot: &Value, factor: &Factor<'_>) -> R<Option<Key>> {
    if slot["kind"].as_str() != Some(factor.kind()) {
        return Ok(None);
    }
    let g = |k: &str| {
        slot[k]
            .as_str()
            .and_then(unhex)
            .ok_or(VaultError::Corrupt("slot field"))
    };
    let salt = g("salt")?;
    let kek = match factor {
        Factor::Passphrase(p) => {
            let n = |k: &str| slot["kdf"][k].as_u64().map(|v| v as u32);
            let kdf = Kdf {
                m_kib: n("m_kib").ok_or(VaultError::Corrupt("kdf"))?,
                t: n("t").ok_or(VaultError::Corrupt("kdf"))?,
                p: n("p").ok_or(VaultError::Corrupt("kdf"))?,
            };
            argon2_kek(p, &salt, kdf)?
        }
        Factor::Prf(k) | Factor::Recovery(k) => Zeroizing::new(hmac_sha256(&salt, &k[..])),
    };
    let Some(pt) = open(
        &kek,
        &slot_aad(vault_id, factor.kind()),
        &g("nonce")?,
        &g("wrapped")?,
    ) else {
        return Ok(None);
    };
    let arr: [u8; KEY_LEN] = pt
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::Corrupt("dek length"))?;
    Ok(Some(Zeroizing::new(arr)))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> R<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let n: [u8; 6] = rand_bytes()?;
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("vault"),
        hex(&n)
    ));
    let io = |e: std::io::Error| VaultError::Io(e.to_string());
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(io)?;
    f.write_all(bytes).map_err(io)?;
    f.sync_all().map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

/// Audit file next to the vault.
pub fn audit_path(vault: &Path) -> PathBuf {
    let mut s = vault.as_os_str().to_owned();
    s.push(".audit.jsonl");
    PathBuf::from(s)
}

fn audit(vault: &Path, now: i64, action: &str, entry: &str, ok: bool) {
    use std::os::unix::fs::OpenOptionsExt;
    let line = json!({"at": now, "action": action, "entry": entry, "ok": ok}).to_string();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(audit_path(vault))
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Create a new vault file (must not exist). Returns the open vault and the
/// recovery key, which the caller shows ONCE and never stores.
pub fn init(path: &Path, passphrase: &[u8], kdf: Kdf, now: i64) -> R<(Vault, Key)> {
    if path.exists() {
        return Err(VaultError::Io(format!("{} already exists", path.display())));
    }
    let vault_id = hex(&rand_bytes::<16>()?);
    let dek = rand_key()?;
    let recovery = rand_key()?;
    let slots = vec![
        make_slot(
            &vault_id,
            &dek,
            &Factor::Passphrase(passphrase),
            kdf,
            "admin passphrase",
        )?,
        make_slot(
            &vault_id,
            &dek,
            &Factor::Recovery(&recovery),
            kdf,
            "recovery key",
        )?,
    ];
    let mut doc = Map::new();
    doc.insert("format".into(), json!(FORMAT));
    doc.insert("aead".into(), json!(AEAD));
    doc.insert("vault_id".into(), json!(vault_id));
    doc.insert("dek_gen".into(), json!(1));
    doc.insert("slots".into(), Value::Array(slots));
    doc.insert("entries".into(), Value::Object(Map::new()));
    let v = Vault {
        path: path.to_path_buf(),
        doc,
        dek,
        vault_id,
        gen: 1,
        policy: Policy::default(),
        last_use: now,
        last_auth: now,
    };
    v.save()?;
    audit(path, now, "init", "-", true);
    Ok((v, recovery))
}

/// Open and unlock a vault file with one factor.
pub fn unlock(path: &Path, factor: Factor<'_>, now: i64) -> R<Vault> {
    let text = std::fs::read_to_string(path).map_err(|e| VaultError::Io(e.to_string()))?;
    let res = unlock_text(path, &text, factor, now);
    audit(path, now, "unlock", "-", res.is_ok());
    res
}

fn unlock_text(path: &Path, text: &str, factor: Factor<'_>, now: i64) -> R<Vault> {
    let doc: Map<String, Value> = match serde_json::from_str(text) {
        Ok(Value::Object(m)) => m,
        _ => return Err(VaultError::Corrupt("not a vault file")),
    };
    if doc.get("format").and_then(Value::as_str) != Some(FORMAT)
        || doc.get("aead").and_then(Value::as_str) != Some(AEAD)
    {
        return Err(VaultError::Corrupt("unknown format"));
    }
    let vault_id = doc
        .get("vault_id")
        .and_then(Value::as_str)
        .ok_or(VaultError::Corrupt("vault_id"))?
        .to_string();
    let gen = doc
        .get("dek_gen")
        .and_then(Value::as_u64)
        .ok_or(VaultError::Corrupt("dek_gen"))?;
    let slots = doc
        .get("slots")
        .and_then(Value::as_array)
        .ok_or(VaultError::Corrupt("slots"))?;
    let mut dek = None;
    for s in slots {
        if let Some(k) = try_slot(&vault_id, s, &factor)? {
            dek = Some(k);
            break;
        }
    }
    let dek = dek.ok_or(VaultError::BadUnlock)?;
    Ok(Vault {
        path: path.to_path_buf(),
        doc,
        dek,
        vault_id,
        gen,
        policy: Policy::default(),
        last_use: now,
        last_auth: now,
    })
}

impl Vault {
    pub fn set_policy(&mut self, p: Policy) {
        self.policy = p;
    }

    pub fn vault_id(&self) -> &str {
        &self.vault_id
    }

    fn save(&self) -> R<()> {
        let mut text = serde_json::to_string_pretty(&Value::Object(self.doc.clone()))
            .map_err(|_| VaultError::Corrupt("serialise"))?;
        text.push('\n');
        write_atomic(&self.path, text.as_bytes())
    }

    /// Idle check: every operation calls this first.
    fn alive(&mut self, now: i64) -> R<()> {
        if now - self.last_use > self.policy.idle_secs {
            return Err(VaultError::Locked);
        }
        self.last_use = now;
        Ok(())
    }

    fn fresh(&self, now: i64) -> R<()> {
        if now - self.last_auth > self.policy.reauth_secs {
            return Err(VaultError::ReauthRequired);
        }
        Ok(())
    }

    /// Prove a factor again (opens a short reveal/copy/change window).
    pub fn reauth(&mut self, factor: Factor<'_>, now: i64) -> R<()> {
        self.alive(now)?;
        let slots = self.doc["slots"].as_array().cloned().unwrap_or_default();
        for s in &slots {
            if let Some(k) = try_slot(&self.vault_id, s, &factor)? {
                if k[..] == self.dek[..] {
                    self.last_auth = now;
                    audit(&self.path, now, "reauth", "-", true);
                    return Ok(());
                }
            }
        }
        audit(&self.path, now, "reauth", "-", false);
        Err(VaultError::BadUnlock)
    }

    fn index_key(&self) -> Key {
        Zeroizing::new(hmac_sha256(&self.dek[..], b"galion-vault/v1 index"))
    }

    fn entry_id(&self, name: &str) -> String {
        hex(&hmac_sha256(&self.index_key()[..], name.as_bytes())[..16])
    }

    fn entries(&self) -> &Map<String, Value> {
        static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
        self.doc
            .get("entries")
            .and_then(Value::as_object)
            .unwrap_or_else(|| EMPTY.get_or_init(Map::new))
    }

    fn decrypt_entry(&self, id: &str, e: &Value) -> R<(String, Zeroizing<Vec<u8>>)> {
        let g = |k: &str| {
            e[k].as_str()
                .and_then(unhex)
                .ok_or(VaultError::Corrupt("entry field"))
        };
        let pt = open(
            &self.dek,
            &entry_aad(&self.vault_id, self.gen, id),
            &g("nonce")?,
            &g("ct")?,
        )
        .ok_or(VaultError::Corrupt("entry failed authentication"))?;
        if pt.len() < 2 {
            return Err(VaultError::Corrupt("entry too short"));
        }
        let n = u16::from_be_bytes([pt[0], pt[1]]) as usize;
        if pt.len() < 2 + n {
            return Err(VaultError::Corrupt("entry name length"));
        }
        let name = String::from_utf8(pt[2..2 + n].to_vec())
            .map_err(|_| VaultError::Corrupt("entry name"))?;
        Ok((name, Zeroizing::new(pt[2 + n..].to_vec())))
    }

    fn encrypt_entry(&self, name: &str, value: &[u8], now: i64) -> R<(String, Value)> {
        let id = self.entry_id(name);
        let mut pt = Zeroizing::new(Vec::with_capacity(2 + name.len() + value.len()));
        pt.extend_from_slice(&(name.len() as u16).to_be_bytes());
        pt.extend_from_slice(name.as_bytes());
        pt.extend_from_slice(value);
        let (nonce, ct) = seal(&self.dek, &entry_aad(&self.vault_id, self.gen, &id), &pt)?;
        Ok((
            id,
            json!({"nonce": hex(&nonce), "ct": hex(&ct), "updated_at": now}),
        ))
    }

    /// Names only, sorted. Values stay encrypted.
    pub fn list(&mut self, now: i64) -> R<Vec<String>> {
        self.alive(now)?;
        let mut names = Vec::new();
        for (id, e) in self.entries() {
            names.push(self.decrypt_entry(id, e)?.0);
        }
        names.sort();
        audit(&self.path, now, "list", "-", true);
        Ok(names)
    }

    /// Add or change a secret. Needs a fresh re-auth.
    pub fn set(&mut self, name: &str, value: &[u8], now: i64) -> R<()> {
        validate_name(name)?;
        self.alive(now)?;
        self.fresh(now)?;
        let (id, e) = self.encrypt_entry(name, value, now)?;
        if let Some(Value::Object(m)) = self.doc.get_mut("entries") {
            m.insert(id.clone(), e);
        }
        self.save()?;
        audit(&self.path, now, "set", &id, true);
        Ok(())
    }

    /// Reveal one value (`action` is "reveal" or "copy" for the audit log).
    pub fn reveal(&mut self, name: &str, action: &str, now: i64) -> R<Zeroizing<Vec<u8>>> {
        self.alive(now)?;
        let id = self.entry_id(name);
        if let Err(e) = self.fresh(now) {
            audit(&self.path, now, action, &id, false);
            return Err(e);
        }
        let e = self
            .entries()
            .get(&id)
            .cloned()
            .ok_or(VaultError::NotFound)?;
        let (n, v) = self.decrypt_entry(&id, &e)?;
        if n != name {
            return Err(VaultError::Corrupt("entry name mismatch"));
        }
        audit(&self.path, now, action, &id, true);
        Ok(v)
    }

    /// Decrypt everything (zeroized on drop).
    fn all(&self) -> R<BTreeMap<String, Zeroizing<Vec<u8>>>> {
        let mut out = BTreeMap::new();
        for (id, e) in self.entries() {
            let (n, v) = self.decrypt_entry(id, e)?;
            out.insert(n, v);
        }
        Ok(out)
    }

    fn reencrypt_all(&mut self, plain: &BTreeMap<String, Zeroizing<Vec<u8>>>, now: i64) -> R<()> {
        let mut m = Map::new();
        for (n, v) in plain {
            let (id, e) = self.encrypt_entry(n, v, now)?;
            m.insert(id, e);
        }
        self.doc.insert("entries".into(), Value::Object(m));
        Ok(())
    }

    /// Rotate the DATA key: a new DEK, every entry re-encrypted, the
    /// passphrase slot re-wrapped and a NEW recovery key issued (the old one
    /// stops working; PRF slots must be re-enrolled). Needs re-auth with the
    /// current passphrase.
    pub fn rotate_key(&mut self, passphrase: &[u8], kdf: Kdf, now: i64) -> R<Key> {
        self.alive(now)?;
        self.reauth(Factor::Passphrase(passphrase), now)?;
        let plain = self.all()?;
        self.dek = rand_key()?;
        self.gen += 1;
        self.doc.insert("dek_gen".into(), json!(self.gen));
        self.reencrypt_all(&plain, now)?;
        let recovery = rand_key()?;
        let slots = vec![
            make_slot(
                &self.vault_id,
                &self.dek,
                &Factor::Passphrase(passphrase),
                kdf,
                "admin passphrase",
            )?,
            make_slot(
                &self.vault_id,
                &self.dek,
                &Factor::Recovery(&recovery),
                kdf,
                "recovery key",
            )?,
        ];
        self.doc.insert("slots".into(), Value::Array(slots));
        self.save()?;
        audit(&self.path, now, "rotate-key", "-", true);
        Ok(recovery)
    }

    /// Change the admin passphrase (re-wrap only; entries untouched).
    pub fn change_passphrase(&mut self, old: &[u8], new: &[u8], kdf: Kdf, now: i64) -> R<()> {
        self.reauth(Factor::Passphrase(old), now)?;
        let slot = make_slot(
            &self.vault_id,
            &self.dek,
            &Factor::Passphrase(new),
            kdf,
            "admin passphrase",
        )?;
        if let Some(Value::Array(s)) = self.doc.get_mut("slots") {
            s.retain(|x| x["kind"].as_str() != Some("passphrase"));
            s.insert(0, slot);
        }
        self.save()?;
        audit(&self.path, now, "change-passphrase", "-", true);
        Ok(())
    }

    /// Lost passphrase: prove the recovery key, then set a new passphrase
    /// slot. Entries are untouched; run [`Vault::rotate_key`] afterwards if
    /// the old passphrase may have leaked.
    pub fn set_passphrase_with_recovery(
        &mut self,
        recovery: &[u8; KEY_LEN],
        new: &[u8],
        kdf: Kdf,
        now: i64,
    ) -> R<()> {
        self.reauth(Factor::Recovery(recovery), now)?;
        let slot = make_slot(
            &self.vault_id,
            &self.dek,
            &Factor::Passphrase(new),
            kdf,
            "admin passphrase",
        )?;
        if let Some(Value::Array(s)) = self.doc.get_mut("slots") {
            s.retain(|x| x["kind"].as_str() != Some("passphrase"));
            s.insert(0, slot);
        }
        self.save()?;
        audit(&self.path, now, "recover-passphrase", "-", true);
        Ok(())
    }

    /// Enrol a passkey PRF output as an extra unlock slot.
    pub fn add_prf(&mut self, prf: &[u8; KEY_LEN], label: &str, now: i64) -> R<()> {
        self.alive(now)?;
        self.fresh(now)?;
        let slot = make_slot(
            &self.vault_id,
            &self.dek,
            &Factor::Prf(prf),
            Kdf::TEST,
            label,
        )?;
        if let Some(Value::Array(s)) = self.doc.get_mut("slots") {
            s.push(slot);
        }
        self.save()?;
        audit(&self.path, now, "add-prf", "-", true);
        Ok(())
    }

    /// An encrypted copy for backup or moving to another machine: a FRESH
    /// vault id and data key, every entry re-encrypted, one passphrase slot
    /// for `export_pass`, no recovery or PRF slot. Never plaintext.
    pub fn export_encrypted(&mut self, export_pass: &[u8], kdf: Kdf, now: i64) -> R<String> {
        self.alive(now)?;
        self.fresh(now)?;
        let plain = self.all()?;
        let mut copy = Vault {
            path: self.path.clone(),
            doc: Map::new(),
            dek: rand_key()?,
            vault_id: hex(&rand_bytes::<16>()?),
            gen: 1,
            policy: self.policy,
            last_use: now,
            last_auth: now,
        };
        copy.doc.insert("format".into(), json!(FORMAT));
        copy.doc.insert("aead".into(), json!(AEAD));
        copy.doc.insert("vault_id".into(), json!(copy.vault_id));
        copy.doc.insert("dek_gen".into(), json!(1));
        copy.doc.insert("exported_at".into(), json!(now));
        let slot = make_slot(
            &copy.vault_id,
            &copy.dek,
            &Factor::Passphrase(export_pass),
            kdf,
            "export",
        )?;
        copy.doc.insert("slots".into(), json!([slot]));
        copy.reencrypt_all(&plain, now)?;
        audit(&self.path, now, "export", "-", true);
        let mut t = serde_json::to_string_pretty(&Value::Object(copy.doc.clone()))
            .map_err(|_| VaultError::Corrupt("serialise"))?;
        t.push('\n');
        Ok(t)
    }

    /// The audit log with entry ids mapped back to names (admin view).
    pub fn audit_view(&mut self, now: i64) -> R<Vec<String>> {
        self.alive(now)?;
        let mut names = BTreeMap::new();
        for (id, e) in self.entries() {
            names.insert(id.clone(), self.decrypt_entry(id, e)?.0);
        }
        let text = std::fs::read_to_string(audit_path(&self.path)).unwrap_or_default();
        Ok(text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .map(|v| {
                let id = v["entry"].as_str().unwrap_or("-");
                let who = names.get(id).map(String::as_str).unwrap_or(id);
                format!(
                    "{} {} {} {}",
                    v["at"].as_i64().unwrap_or(0),
                    v["action"].as_str().unwrap_or("?"),
                    who,
                    if v["ok"].as_bool() == Some(true) {
                        "ok"
                    } else {
                        "refused"
                    }
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &[u8] = b"correct horse battery staple";

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("galion-vault-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fresh_path(tag: &str) -> PathBuf {
        let d = dir(tag);
        let p = d.join("vault.json");
        // Never delete: move any leftover from an earlier run aside.
        if p.exists() {
            let n: [u8; 4] = rand_bytes().unwrap();
            std::fs::rename(&p, d.join(format!("vault.json.old-{}", hex(&n)))).unwrap();
            let a = audit_path(&p);
            if a.exists() {
                std::fs::rename(&a, d.join(format!("audit.old-{}", hex(&n)))).unwrap();
            }
        }
        p
    }

    fn assert_no_plain(bytes: &[u8], needles: &[&[u8]]) {
        for n in needles {
            assert!(
                !bytes.windows(n.len()).any(|w| w == *n),
                "plaintext {:?} found on disk",
                String::from_utf8_lossy(n)
            );
        }
    }

    const NAME: &str = "STRIPE_SECRET_KEY";
    const VALUE: &[u8] = b"sk_test_THIS-IS-A-FAKE-TEST-VALUE-0123456789";

    #[test]
    fn hmac_rfc4231_case2() {
        let m = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&m),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn roundtrip_and_no_plaintext_on_disk() {
        let p = fresh_path("rt");
        let (mut v, rec) = init(&p, PASS, Kdf::TEST, 1000).unwrap();
        v.set(NAME, VALUE, 1001).unwrap();
        v.set("ACCOUNTS_ADMIN_TOKEN", b"another-fake-test-token", 1002)
            .unwrap();
        drop(v);
        let disk = std::fs::read(&p).unwrap();
        let audit = std::fs::read(audit_path(&p)).unwrap();
        let rec_text = recovery_to_text(&rec);
        for bytes in [&disk, &audit] {
            assert_no_plain(
                bytes,
                &[
                    VALUE,
                    b"sk_test",
                    b"another-fake-test-token",
                    NAME.as_bytes(),
                    b"STRIPE",
                    b"ACCOUNTS_ADMIN_TOKEN",
                    PASS,
                    b"horse",
                    hex(&rec[..]).as_bytes(),
                    rec_text.as_bytes(),
                ],
            );
        }
        // File mode 0600.
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut v = unlock(&p, Factor::Passphrase(PASS), 2000).unwrap();
        assert_eq!(v.list(2000).unwrap(), vec!["ACCOUNTS_ADMIN_TOKEN", NAME]);
        assert_eq!(&v.reveal(NAME, "reveal", 2001).unwrap()[..], VALUE);
    }

    #[test]
    fn wrong_unlock_and_tamper() {
        let p = fresh_path("bad");
        let (mut v, _) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        v.set("A_SECRET", b"value-one", 1).unwrap();
        drop(v);
        assert_eq!(
            unlock(&p, Factor::Passphrase(b"wrong passphrase!!"), 2).err(),
            Some(VaultError::BadUnlock)
        );
        assert_eq!(
            unlock(&p, Factor::Recovery(&[7u8; 32]), 2).err(),
            Some(VaultError::BadUnlock)
        );
        // Flip one hex digit of the entry ciphertext: authentication fails.
        let text = std::fs::read_to_string(&p).unwrap();
        let mut doc: Value = serde_json::from_str(&text).unwrap();
        let (id, e) = doc["entries"]
            .as_object()
            .unwrap()
            .iter()
            .next()
            .map(|(k, v)| (k.clone(), v.clone()))
            .unwrap();
        let mut ct = e["ct"].as_str().unwrap().to_string();
        let first = if ct.starts_with('0') { "1" } else { "0" };
        ct.replace_range(0..1, first);
        doc["entries"][&id]["ct"] = json!(ct);
        std::fs::write(&p, doc.to_string()).unwrap();
        let mut v = unlock(&p, Factor::Passphrase(PASS), 3).unwrap();
        assert!(matches!(v.list(3), Err(VaultError::Corrupt(_))));
    }

    #[test]
    fn recovery_and_prf_unlock() {
        let p = fresh_path("rec");
        let (mut v, rec) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        v.set("K", b"v", 1).unwrap();
        let prf = [42u8; 32];
        v.add_prf(&prf, "admin phone passkey", 2).unwrap();
        drop(v);
        let parsed = recovery_from_text(&recovery_to_text(&rec)).unwrap();
        let mut r = unlock(&p, Factor::Recovery(&parsed), 3).unwrap();
        assert_eq!(&r.reveal("K", "reveal", 3).unwrap()[..], b"v");
        // Lost passphrase: set a new one with the recovery key.
        r.set_passphrase_with_recovery(&parsed, b"new admin passphrase", Kdf::TEST, 4)
            .unwrap();
        assert!(unlock(&p, Factor::Passphrase(b"new admin passphrase"), 4).is_ok());
        assert_eq!(
            unlock(&p, Factor::Passphrase(PASS), 4).err(),
            Some(VaultError::BadUnlock)
        );
        let mut f = unlock(&p, Factor::Prf(&prf), 5).unwrap();
        assert_eq!(&f.reveal("K", "copy", 5).unwrap()[..], b"v");
        assert_eq!(
            unlock(&p, Factor::Prf(&[1u8; 32]), 6).err(),
            Some(VaultError::BadUnlock)
        );
    }

    #[test]
    fn session_timeout_and_reauth() {
        let p = fresh_path("sess");
        let (mut v, _) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        v.set("K", b"v", 0).unwrap();
        v.set_policy(Policy {
            idle_secs: 100,
            reauth_secs: 10,
        });
        // Past the re-auth window: list is fine, reveal and set are not.
        assert!(v.list(50).is_ok());
        assert_eq!(
            v.reveal("K", "reveal", 60).err(),
            Some(VaultError::ReauthRequired)
        );
        assert_eq!(v.set("K", b"w", 61).err(), Some(VaultError::ReauthRequired));
        assert_eq!(
            v.reauth(Factor::Passphrase(b"not the passphrase"), 62)
                .err(),
            Some(VaultError::BadUnlock)
        );
        v.reauth(Factor::Passphrase(PASS), 63).unwrap();
        assert_eq!(&v.reveal("K", "reveal", 64).unwrap()[..], b"v");
        // Idle past 100 s: locked.
        assert_eq!(v.list(200).err(), Some(VaultError::Locked));
        // The refused reveal is in the audit log.
        let a = std::fs::read_to_string(audit_path(&p)).unwrap();
        assert!(a
            .lines()
            .any(|l| l.contains("\"reveal\"") && l.contains("\"ok\":false")));
    }

    #[test]
    fn rotate_key_reencrypts_and_retires_old_recovery() {
        let p = fresh_path("rot");
        let (mut v, old_rec) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        v.set(NAME, VALUE, 1).unwrap();
        let before = std::fs::read_to_string(&p).unwrap();
        let new_rec = v.rotate_key(PASS, Kdf::TEST, 2).unwrap();
        drop(v);
        let after = std::fs::read_to_string(&p).unwrap();
        let ct = |t: &str| -> Vec<String> {
            let d: Value = serde_json::from_str(t).unwrap();
            d["entries"]
                .as_object()
                .unwrap()
                .values()
                .map(|e| e["ct"].as_str().unwrap().to_string())
                .collect()
        };
        assert_ne!(ct(&before), ct(&after));
        assert_no_plain(after.as_bytes(), &[VALUE, NAME.as_bytes()]);
        assert_eq!(
            unlock(&p, Factor::Recovery(&old_rec), 3).err(),
            Some(VaultError::BadUnlock)
        );
        let mut v = unlock(&p, Factor::Recovery(&new_rec), 3).unwrap();
        assert_eq!(&v.reveal(NAME, "reveal", 3).unwrap()[..], VALUE);
    }

    #[test]
    fn change_passphrase() {
        let p = fresh_path("chp");
        let (mut v, _) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        v.set("K", b"v", 0).unwrap();
        assert_eq!(
            v.change_passphrase(PASS, b"short", Kdf::TEST, 1).err(),
            Some(VaultError::Weak("passphrase must be at least 12 bytes"))
        );
        v.change_passphrase(PASS, b"a brand new passphrase", Kdf::TEST, 1)
            .unwrap();
        drop(v);
        assert_eq!(
            unlock(&p, Factor::Passphrase(PASS), 2).err(),
            Some(VaultError::BadUnlock)
        );
        assert!(unlock(&p, Factor::Passphrase(b"a brand new passphrase"), 2).is_ok());
    }

    #[test]
    fn export_is_encrypted_and_importable() {
        let p = fresh_path("exp");
        let (mut v, rec) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        v.set(NAME, VALUE, 0).unwrap();
        let exp = v
            .export_encrypted(b"export passphrase 1", Kdf::TEST, 1)
            .unwrap();
        assert_no_plain(
            exp.as_bytes(),
            &[VALUE, NAME.as_bytes(), PASS, b"export passphrase"],
        );
        let d: Value = serde_json::from_str(&exp).unwrap();
        assert_ne!(d["vault_id"], json!(v.vault_id()), "fresh vault id");
        assert_eq!(d["slots"].as_array().unwrap().len(), 1);
        // Write it elsewhere and open it with the export passphrase only.
        let q = fresh_path("exp-copy");
        std::fs::write(&q, &exp).unwrap();
        assert_eq!(
            unlock(&q, Factor::Passphrase(PASS), 2).err(),
            Some(VaultError::BadUnlock)
        );
        assert_eq!(
            unlock(&q, Factor::Recovery(&rec), 2).err(),
            Some(VaultError::BadUnlock)
        );
        let mut w = unlock(&q, Factor::Passphrase(b"export passphrase 1"), 2).unwrap();
        assert_eq!(&w.reveal(NAME, "reveal", 2).unwrap()[..], VALUE);
    }

    #[test]
    fn entries_cannot_move_between_vaults() {
        let p1 = fresh_path("mv1");
        let p2 = fresh_path("mv2");
        let (mut a, _) = init(&p1, PASS, Kdf::TEST, 0).unwrap();
        let (mut b, _) = init(&p2, PASS, Kdf::TEST, 0).unwrap();
        a.set("K", b"from-a", 0).unwrap();
        b.set("K", b"from-b", 0).unwrap();
        // Paste a's entries into b's file: b must refuse them.
        let da: Value = serde_json::from_str(&std::fs::read_to_string(&p1).unwrap()).unwrap();
        let mut db: Value = serde_json::from_str(&std::fs::read_to_string(&p2).unwrap()).unwrap();
        db["entries"] = da["entries"].clone();
        std::fs::write(&p2, db.to_string()).unwrap();
        let mut b = unlock(&p2, Factor::Passphrase(PASS), 1).unwrap();
        assert!(b.list(1).is_err());
    }

    #[test]
    fn names_validated_and_weak_passphrase_refused() {
        let p = fresh_path("names");
        assert_eq!(
            init(&p, b"short", Kdf::TEST, 0).err(),
            Some(VaultError::Weak("passphrase must be at least 12 bytes"))
        );
        let (mut v, _) = init(&p, PASS, Kdf::TEST, 0).unwrap();
        assert!(v.set("", b"x", 0).is_err());
        assert!(v.set("A\nB", b"x", 0).is_err());
        assert!(v.set(&"x".repeat(300), b"x", 0).is_err());
        assert_eq!(
            v.reveal("missing", "reveal", 0).err(),
            Some(VaultError::NotFound)
        );
        assert!(
            init(&p, PASS, Kdf::TEST, 0).is_err(),
            "init never overwrites"
        );
    }
}
