// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Owner key for the Hermes self-change approval card (approval-system C,
//! 2026-10-31 CET). Matches `A-core/FORMATS.md` v1 sections 4 and 5:
//!
//! * [`OwnerKey`]: `/etc/hermes-self-modify/owner.pub` (Ed25519 key file or a
//!   WebAuthn ES256 passkey), its fingerprint (sha256 of the raw public key)
//!   and key id.
//! * [`verify_assertion`] / [`verify_owner_approval`]: the owner signature on
//!   an approval body, either a plain Ed25519 signature over
//!   `domain + "\n" + body` or a WebAuthn assertion whose challenge is the 32
//!   raw bytes `sha256(domain + "\n" + body)`.
//! * Key setup: the registration check for a new passkey, the activation body
//!   (proof the new key is on Slavik's device AND that he confirmed the
//!   fingerprint), the change body (the CURRENT key agrees to the
//!   replacement) and [`install`], the only code that writes owner.pub. It is
//!   run by root through the `galion-owner-key` helper, never by the console.
//!
//! The private half of the owner key never reaches this code: it only ever
//! sees public keys and signatures.

#![forbid(unsafe_code)]

use ed25519_dalek::Verifier as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const OWNER_KEY_V: &str = "galion-sm/owner-key/v1";
pub const APPROVAL_V: &str = "galion-sm/owner-approval/v1";
pub const REGISTER_V: &str = "galion-one/owner-key-register/v1";
pub const ACTIVATE_V: &str = "galion-one/owner-key-activate/v1";
pub const CHANGE_V: &str = "galion-one/owner-key-change/v1";
pub const INSTALL_V: &str = "galion-one/owner-key-install/v1";
pub const SCHEME_ED25519: &str = "ed25519";
pub const SCHEME_ES256: &str = "webauthn-es256";
/// A one-time code (challenge) lives at most 30 minutes.
pub const MAX_TTL: i64 = 1800;
/// Clock slack for `issued_at` in the future.
pub const SKEW: i64 = 120;
/// How long after Slavik's on-device activation the root helper may install.
pub const INSTALL_WINDOW: i64 = 86_400;
/// The default (and only sanctioned) owner key path.
pub const OWNER_PUB: &str = "/etc/hermes-self-modify/owner.pub";
/// Never written, never trusted as the owner key (10-29 gate key, not Slavik's).
pub const FORBIDDEN_DIR: &str = "/var/lib/galion-gate";
/// SubjectPublicKeyInfo prefix of an uncompressed P-256 key (91 bytes total).
pub const SPKI_P256_PREFIX: &str = "3059301306072a8648ce3d020106082a8648ce3d030107034200";
/// SubjectPublicKeyInfo prefix of an Ed25519 key (44 bytes total).
pub const SPKI_ED25519_PREFIX: &str = "302a300506032b6570032100";

// ---------- small helpers ----------

pub fn sha256(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

pub fn hex(b: &[u8]) -> String {
    galion_common::hex::encode(b)
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&sha256(b))
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.bytes().any(|c| c.is_ascii_uppercase()) {
        return Err("hex must be lowercase".into());
    }
    galion_common::hex::decode(s).ok_or_else(|| "not hex".to_string())
}

pub fn b64url(b: &[u8]) -> String {
    galion_common::base64::encode_url(b)
}

/// base64url, no padding (RFC 4648 section 5). The standard alphabet and
/// padding are refused so one value has one spelling.
pub fn unb64url(s: &str) -> Result<Vec<u8>, String> {
    if s.bytes().any(|c| c == b'+' || c == b'/' || c == b'=') {
        return Err("expected base64url without padding".into());
    }
    galion_common::base64::decode(s).map_err(|e| format!("bad base64url: {e:?}"))
}

pub fn is_hex_len(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// `abcd ef01 ...`: the fingerprint as Slavik reads it, groups of four.
pub fn group4(hex: &str) -> String {
    hex.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The signed message bytes: `domain + "\n" + body` (FORMATS section 0).
pub fn message(domain: &str, body: &str) -> Vec<u8> {
    let mut m = Vec::with_capacity(domain.len() + 1 + body.len());
    m.extend_from_slice(domain.as_bytes());
    m.push(b'\n');
    m.extend_from_slice(body.as_bytes());
    m
}

/// The 32 raw bytes a passkey signs as its WebAuthn challenge.
pub fn webauthn_challenge(domain: &str, body: &str) -> [u8; 32] {
    sha256(&message(domain, body))
}

// ---------- owner.pub ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerKey {
    Ed25519 {
        public: [u8; 32],
    },
    Es256 {
        rp_id: String,
        origins: Vec<String>,
        /// Uncompressed SEC1 point, 65 bytes (04 || X || Y).
        sec1: Vec<u8>,
        credential_id: Option<String>,
    },
}

impl OwnerKey {
    /// Parse an owner.pub file: FORMATS v1 JSON, or legacy 64 hex (Ed25519).
    pub fn parse(text: &str) -> Result<OwnerKey, String> {
        let t = text.trim();
        if is_hex_len(t, 64) {
            return Self::ed25519_from_hex(t);
        }
        let v: Value = serde_json::from_str(t).map_err(|_| "owner key: not JSON".to_string())?;
        if v["version"] != OWNER_KEY_V {
            return Err("owner key: unknown version".into());
        }
        match v["scheme"].as_str() {
            Some(SCHEME_ED25519) => {
                Self::ed25519_from_hex(v["public_key_hex"].as_str().unwrap_or_default())
            }
            Some(SCHEME_ES256) => {
                let sec1 = if let Some(h) = v["public_key_sec1_hex"].as_str() {
                    unhex(h)?
                } else if let Some(h) = v["public_key_spki_hex"].as_str() {
                    sec1_from_spki(&unhex(h)?)?
                } else {
                    return Err("owner key: no public key".into());
                };
                check_p256(&sec1)?;
                let rp_id = v["rp_id"].as_str().unwrap_or_default().to_string();
                if rp_id.is_empty() {
                    return Err("owner key: rp_id missing".into());
                }
                let origins: Vec<String> = v["origins"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| o.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                if origins.is_empty() {
                    return Err("owner key: origins missing".into());
                }
                let credential_id = v["credential_id"].as_str().map(str::to_string);
                if let Some(c) = &credential_id {
                    unb64url(c)?;
                }
                Ok(OwnerKey::Es256 {
                    rp_id,
                    origins,
                    sec1,
                    credential_id,
                })
            }
            _ => Err("owner key: unknown scheme".into()),
        }
    }

    fn ed25519_from_hex(h: &str) -> Result<OwnerKey, String> {
        let public: [u8; 32] = unhex(h)?
            .try_into()
            .map_err(|_| "ed25519 public key must be 32 bytes".to_string())?;
        ed25519_dalek::VerifyingKey::from_bytes(&public)
            .map_err(|_| "ed25519 public key is not a valid point".to_string())?;
        Ok(OwnerKey::Ed25519 { public })
    }

    /// The exact owner.pub file text (one JSON line + newline).
    pub fn file_text(&self) -> String {
        let v = match self {
            OwnerKey::Ed25519 { public } => json!({
                "version": OWNER_KEY_V, "scheme": SCHEME_ED25519, "public_key_hex": hex(public),
            }),
            OwnerKey::Es256 {
                rp_id,
                origins,
                sec1,
                credential_id,
            } => {
                let mut v = json!({
                    "version": OWNER_KEY_V, "scheme": SCHEME_ES256, "rp_id": rp_id,
                    "origins": origins, "public_key_sec1_hex": hex(sec1),
                });
                if let Some(c) = credential_id {
                    v["credential_id"] = json!(c);
                }
                v
            }
        };
        format!("{v}\n")
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            OwnerKey::Ed25519 { .. } => SCHEME_ED25519,
            OwnerKey::Es256 { .. } => SCHEME_ES256,
        }
    }

    /// Raw public key bytes (32 for Ed25519, 65-byte SEC1 point for P-256).
    pub fn raw(&self) -> &[u8] {
        match self {
            OwnerKey::Ed25519 { public } => public,
            OwnerKey::Es256 { sec1, .. } => sec1,
        }
    }

    /// FORMATS section 5: sha256(raw public key), lowercase hex.
    pub fn fingerprint(&self) -> String {
        sha256_hex(self.raw())
    }

    /// Ed25519: first 16 hex of the public key (hermes-rs `key_id`).
    /// ES256: first 16 hex of the fingerprint (requested in FORMATS-REQUESTS).
    pub fn key_id(&self) -> String {
        match self {
            OwnerKey::Ed25519 { public } => hex(public)[..16].to_string(),
            OwnerKey::Es256 { .. } => self.fingerprint()[..16].to_string(),
        }
    }
}

fn check_p256(sec1: &[u8]) -> Result<(), String> {
    if sec1.len() != 65 || sec1[0] != 4 {
        return Err("P-256 key must be a 65-byte uncompressed point".into());
    }
    p256::ecdsa::VerifyingKey::from_sec1_bytes(sec1)
        .map(|_| ())
        .map_err(|_| "P-256 key is not a valid point".to_string())
}

/// SEC1 point from a browser's `getPublicKey()` SPKI DER (P-256 only).
pub fn sec1_from_spki(spki: &[u8]) -> Result<Vec<u8>, String> {
    let prefix = unhex(SPKI_P256_PREFIX)?;
    if spki.len() != 91 || spki[..26] != prefix[..] {
        return Err("not a P-256 SubjectPublicKeyInfo (passkey must use ES256)".into());
    }
    let sec1 = spki[26..].to_vec();
    check_p256(&sec1)?;
    Ok(sec1)
}

/// Raw Ed25519 key from WebCrypto `exportKey("spki")` or `("raw")`.
pub fn ed25519_from_spki_or_raw(b: &[u8]) -> Result<[u8; 32], String> {
    let prefix = unhex(SPKI_ED25519_PREFIX)?;
    let raw = if b.len() == 44 && b[..12] == prefix[..] {
        &b[12..]
    } else if b.len() == 32 {
        b
    } else {
        return Err("not an Ed25519 public key".into());
    };
    let public: [u8; 32] = raw.try_into().map_err(|_| "length".to_string())?;
    ed25519_dalek::VerifyingKey::from_bytes(&public)
        .map_err(|_| "ed25519 public key is not a valid point".to_string())?;
    Ok(public)
}

// ---------- signatures ----------

/// Verify the owner's signature over `domain + "\n" + body` in `env`
/// (FORMATS section 4, schemes a and b). `env` carries `scheme` and either
/// `sig` (Ed25519, 128 hex) or the WebAuthn assertion fields.
pub fn verify_assertion(
    key: &OwnerKey,
    domain: &str,
    body: &str,
    env: &Value,
) -> Result<(), String> {
    let scheme = env["scheme"].as_str().unwrap_or_default();
    if scheme != key.scheme() {
        return Err(format!(
            "signed with scheme {scheme:?} but the owner key is {}",
            key.scheme()
        ));
    }
    if let Some(k) = env["key_id"].as_str() {
        if k != key.key_id() {
            return Err("key_id is not the owner key".into());
        }
    }
    let m = message(domain, body);
    match key {
        OwnerKey::Ed25519 { public } => {
            let sig: [u8; 64] = unhex(env["sig"].as_str().ok_or("sig missing")?)?
                .try_into()
                .map_err(|_| "signature must be 64 bytes".to_string())?;
            let vk = ed25519_dalek::VerifyingKey::from_bytes(public).map_err(|e| e.to_string())?;
            vk.verify(&m, &ed25519_dalek::Signature::from_bytes(&sig))
                .map_err(|_| "bad Ed25519 signature (tampered, or not the owner key)".to_string())
        }
        OwnerKey::Es256 {
            rp_id,
            origins,
            sec1,
            credential_id,
        } => {
            let field = |n: &str| -> Result<Vec<u8>, String> {
                unb64url(env[n].as_str().ok_or_else(|| format!("{n} missing"))?)
            };
            let cd = field("client_data_json")?;
            let ad = field("authenticator_data")?;
            let sig = field("signature")?;
            if let Some(want) = credential_id {
                if env["credential_id"].as_str() != Some(want.as_str()) {
                    return Err("credential_id is not the owner passkey".into());
                }
            }
            check_client_data(&cd, "webauthn.get", &sha256(&m), origins)?;
            if ad.len() < 37 {
                return Err("authenticator data too short".into());
            }
            if ad[..32] != sha256(rp_id.as_bytes()) {
                return Err("authenticator data is for another site (rpIdHash)".into());
            }
            if ad[32] & 0x05 != 0x05 {
                return Err("passkey did not confirm presence and verification (UP+UV)".into());
            }
            let vk = p256::ecdsa::VerifyingKey::from_sec1_bytes(sec1).map_err(|e| e.to_string())?;
            let s = p256::ecdsa::Signature::from_der(&sig)
                .map_err(|_| "signature is not DER ECDSA".to_string())?;
            let mut signed = ad.clone();
            signed.extend_from_slice(&sha256(&cd));
            use p256::ecdsa::signature::Verifier as _;
            vk.verify(&signed, &s).map_err(|_| {
                "bad passkey signature (tampered, or not the owner passkey)".to_string()
            })
        }
    }
}

/// clientDataJSON checks shared by `create` (registration) and `get`.
pub fn check_client_data(
    cd: &[u8],
    kind: &str,
    challenge: &[u8],
    origins: &[String],
) -> Result<(), String> {
    let v: Value =
        serde_json::from_slice(cd).map_err(|_| "clientDataJSON is not JSON".to_string())?;
    if v["type"] != kind {
        return Err(format!("clientDataJSON type is not {kind}"));
    }
    if v["challenge"].as_str() != Some(b64url(challenge).as_str()) {
        return Err("passkey signed a different challenge".into());
    }
    let origin = v["origin"].as_str().unwrap_or_default();
    if !origins.iter().any(|o| o == origin) {
        return Err(format!("passkey used from an unknown origin {origin:?}"));
    }
    if v["crossOrigin"] == true {
        return Err("cross-origin passkey use refused".into());
    }
    Ok(())
}

/// Check a body's version, nonce and 30-minute window. Returns the body.
pub fn check_window(body: &str, version: &str, now: i64, window: i64) -> Result<Value, String> {
    let b: Value = serde_json::from_str(body).map_err(|_| "body is not JSON".to_string())?;
    if b["version"] != version {
        return Err(format!("body version is not {version}"));
    }
    if !is_hex_len(b["nonce"].as_str().unwrap_or_default(), 32) {
        return Err("nonce must be 32 lowercase hex".into());
    }
    let issued = b["issued_at"].as_i64().ok_or("issued_at missing")?;
    let expires = b["expires"].as_i64().ok_or("expires missing")?;
    if expires <= issued || expires - issued > MAX_TTL {
        return Err("expiry must be within 30 minutes of issue".into());
    }
    if issued > now + SKEW {
        return Err("issued in the future".into());
    }
    if now > issued + window.max(expires - issued) {
        return Err("expired".into());
    }
    Ok(b)
}

/// The owner approval file (`<entry>/owner/approval.json`): verify and
/// return the body (FORMATS section 4). Binding to the entry is the caller's.
pub fn verify_owner_approval(key: &OwnerKey, env: &Value, now: i64) -> Result<Value, String> {
    let body = env["approval_json"]
        .as_str()
        .ok_or("approval_json missing")?;
    verify_assertion(key, APPROVAL_V, body, env)?;
    let b = check_window(body, APPROVAL_V, now, 0)?;
    for f in [
        "artifact_sha256",
        "test_record_sha256",
        "dryrun_record_sha256",
    ] {
        if !is_hex_len(b[f].as_str().unwrap_or_default(), 64) {
            return Err(format!("{f} must be 64 hex"));
        }
    }
    if !is_hex_len(b["base_commit"].as_str().unwrap_or_default(), 40) {
        return Err("base_commit must be 40 hex".into());
    }
    Ok(b)
}

/// The approval body string, built once (key order fixed).
#[allow(clippy::too_many_arguments)]
pub fn approval_body(
    id: &str,
    artifact_sha256: &str,
    base_commit: &str,
    test_record_sha256: &str,
    dryrun_record_sha256: &str,
    nonce: &str,
    issued_at: i64,
    expires: i64,
) -> String {
    json!({
        "version": APPROVAL_V, "id": id, "artifact_sha256": artifact_sha256,
        "base_commit": base_commit, "test_record_sha256": test_record_sha256,
        "dryrun_record_sha256": dryrun_record_sha256, "nonce": nonce,
        "issued_at": issued_at, "expires": expires,
    })
    .to_string()
}

// ---------- key setup ----------

/// Registration challenge body: the console asks the device to create a key.
pub fn register_body(nonce: &str, rp_id: &str, issued_at: i64, expires: i64) -> String {
    json!({"version": REGISTER_V, "nonce": nonce, "rp_id": rp_id, "issued_at": issued_at, "expires": expires})
        .to_string()
}

/// Activation body, signed by the NEW key: "this key is on my device and I
/// checked that its fingerprint is the one the console shows".
pub fn activate_body(
    owner_pub_text: &str,
    fingerprint: &str,
    nonce: &str,
    issued_at: i64,
    expires: i64,
) -> String {
    json!({
        "version": ACTIVATE_V, "fingerprint": fingerprint,
        "owner_pub_sha256": sha256_hex(owner_pub_text.as_bytes()),
        "confirmed_match": true, "nonce": nonce, "issued_at": issued_at, "expires": expires,
    })
    .to_string()
}

/// Change body, signed by the CURRENT key: "replace me with this key".
pub fn change_body(
    old_fingerprint: &str,
    new_fingerprint: &str,
    new_owner_pub_text: &str,
    nonce: &str,
    issued_at: i64,
    expires: i64,
) -> String {
    json!({
        "version": CHANGE_V, "old_fingerprint": old_fingerprint, "new_fingerprint": new_fingerprint,
        "new_owner_pub_sha256": sha256_hex(new_owner_pub_text.as_bytes()),
        "nonce": nonce, "issued_at": issued_at, "expires": expires,
    })
    .to_string()
}

/// What the console stages for the root helper (never installs it itself).
pub fn install_request(
    owner_pub_text: &str,
    activate: &Value,
    change: Option<&Value>,
    staged_at: i64,
) -> Value {
    let key = OwnerKey::parse(owner_pub_text).ok();
    json!({
        "version": INSTALL_V,
        "owner_pub": owner_pub_text,
        "fingerprint": key.as_ref().map(|k| k.fingerprint()).unwrap_or_default(),
        "activate": activate,
        "change": change.cloned().unwrap_or(Value::Null),
        "staged_at": staged_at,
    })
}

/// Check an install request against the key installed now (`current`, the
/// text of owner.pub if any). Returns the new key.
pub fn check_install_request(
    req: &Value,
    current: Option<&str>,
    now: i64,
) -> Result<OwnerKey, String> {
    if req["version"] != INSTALL_V {
        return Err("install request: unknown version".into());
    }
    let text = req["owner_pub"].as_str().ok_or("owner_pub missing")?;
    let new = OwnerKey::parse(text)?;
    if new.file_text() != text {
        return Err("owner_pub is not in canonical form".into());
    }
    let fp = new.fingerprint();
    if req["fingerprint"].as_str() != Some(fp.as_str()) {
        return Err("fingerprint does not match the key".into());
    }
    let act = &req["activate"];
    let body = act["body_json"].as_str().ok_or("activation missing")?;
    verify_assertion(&new, ACTIVATE_V, body, act)?;
    let b = check_window(body, ACTIVATE_V, now, INSTALL_WINDOW)?;
    if b["fingerprint"] != fp.as_str()
        || b["owner_pub_sha256"] != sha256_hex(text.as_bytes()).as_str()
        || b["confirmed_match"] != true
    {
        return Err("activation is for a different key or was not confirmed".into());
    }
    if let Some(cur_text) = current.filter(|c| !c.trim().is_empty()) {
        let cur = OwnerKey::parse(cur_text)
            .map_err(|e| format!("current owner.pub unreadable ({e}); refusing to replace"))?;
        if cur.fingerprint() == fp {
            return Ok(new);
        }
        let ch = &req["change"];
        let body = ch["body_json"]
            .as_str()
            .ok_or("replacing the owner key needs a signature from the current key")?;
        verify_assertion(&cur, CHANGE_V, body, ch)
            .map_err(|e| format!("current-key signature: {e}"))?;
        let c = check_window(body, CHANGE_V, now, INSTALL_WINDOW)?;
        if c["old_fingerprint"] != cur.fingerprint().as_str()
            || c["new_fingerprint"] != fp.as_str()
            || c["new_owner_pub_sha256"] != sha256_hex(text.as_bytes()).as_str()
        {
            return Err("current-key signature is for a different change".into());
        }
    }
    Ok(new)
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed {
        fingerprint: String,
        backup: Option<PathBuf>,
    },
    AlreadyInstalled {
        fingerprint: String,
    },
    /// `--dry-run`: everything verified, nothing written.
    WouldInstall {
        fingerprint: String,
        replaces: Option<String>,
    },
}

pub struct InstallOpts {
    /// chown root:root (the real helper; tests run unprivileged).
    pub chown_root: bool,
    pub dry_run: bool,
}

/// Refuse the old gate key location and anything that is not a plain path.
pub fn dest_allowed(dest: &Path) -> Result<(), String> {
    let s = dest.to_string_lossy();
    if !dest.is_absolute() || s.contains("/../") || s.ends_with("/..") {
        return Err("destination must be an absolute path without ..".into());
    }
    if dest.starts_with(FORBIDDEN_DIR) {
        return Err(format!(
            "refused: {FORBIDDEN_DIR} is the old gate key area, never the owner key"
        ));
    }
    Ok(())
}

/// Install owner.pub (root helper). Atomic: a new file in the same
/// directory (create_new, 0444), fsync, then rename over the old one. The
/// old key is kept as `owner.pub.off-<unix>` (hard link), never deleted.
/// Symlinks at the destination or its directory are refused.
pub fn install(
    req: &Value,
    dest: &Path,
    now: i64,
    opts: &InstallOpts,
) -> Result<InstallOutcome, String> {
    dest_allowed(dest)?;
    let dir = dest.parent().ok_or("destination has no directory")?;
    let dmeta = fs::symlink_metadata(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if !dmeta.file_type().is_dir() {
        return Err(format!("{} is not a plain directory", dir.display()));
    }
    let current = match fs::symlink_metadata(dest) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err("refused: owner.pub is a symlink".into());
        }
        Ok(m) if !m.file_type().is_file() => {
            return Err("refused: owner.pub is not a regular file".into());
        }
        Ok(_) => Some(fs::read_to_string(dest).map_err(|e| e.to_string())?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let new = check_install_request(req, current.as_deref(), now)?;
    let fp = new.fingerprint();
    let cur_fp = current
        .as_deref()
        .and_then(|c| OwnerKey::parse(c).ok())
        .map(|k| k.fingerprint());
    if cur_fp.as_deref() == Some(fp.as_str()) {
        return Ok(InstallOutcome::AlreadyInstalled { fingerprint: fp });
    }
    if opts.dry_run {
        return Ok(InstallOutcome::WouldInstall {
            fingerprint: fp,
            replaces: cur_fp,
        });
    }
    let name = dest
        .file_name()
        .ok_or("destination has no file name")?
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.new-{now}-{}", std::process::id()));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o444)
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(new.file_text().as_bytes())
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    if opts.chown_root {
        std::os::unix::fs::chown(&tmp, Some(0), Some(0)).map_err(|e| format!("chown: {e}"))?;
    }
    let mut backup = None;
    if current.is_some() {
        let off = dir.join(format!("{name}.off-{now}"));
        fs::hard_link(dest, &off).map_err(|e| format!("keep old key as {}: {e}", off.display()))?;
        backup = Some(off);
    }
    fs::rename(&tmp, dest).map_err(|e| format!("rename: {e}"))?;
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    // Re-read: the file on disk must be exactly the verified key.
    let back = fs::read_to_string(dest).map_err(|e| e.to_string())?;
    if back != new.file_text() {
        return Err("owner.pub on disk differs from the verified key".into());
    }
    Ok(InstallOutcome::Installed {
        fingerprint: fp,
        backup,
    })
}

/// Software owner devices for tests (feature `testkit`): a fake passkey
/// authenticator (ES256, UP+UV set, like Chrome's virtual authenticator) and
/// an Ed25519 key file. They sign exactly what a real device signs.
#[cfg(any(test, feature = "testkit"))]
pub mod testkit {
    use super::*;
    use p256::ecdsa::signature::Signer as _;

    pub struct SoftPasskey {
        pub sk: p256::ecdsa::SigningKey,
        pub rp_id: String,
        pub origin: String,
        pub credential_id: String,
        pub counter: u32,
    }

    impl SoftPasskey {
        pub fn new(seed: u8, rp_id: &str, origin: &str) -> SoftPasskey {
            let mut b = [7u8; 32];
            b[0] = seed.max(1);
            SoftPasskey {
                sk: p256::ecdsa::SigningKey::from_bytes(&b.into()).expect("scalar"),
                rp_id: rp_id.into(),
                origin: origin.into(),
                credential_id: b64url(&[seed; 16]),
                counter: 0,
            }
        }
        pub fn sec1(&self) -> Vec<u8> {
            self.sk
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec()
        }
        pub fn spki(&self) -> Vec<u8> {
            let mut v = unhex(SPKI_P256_PREFIX).unwrap();
            v.extend(self.sec1());
            v
        }
        pub fn owner_key(&self, origins: &[&str]) -> OwnerKey {
            OwnerKey::Es256 {
                rp_id: self.rp_id.clone(),
                origins: origins.iter().map(|s| s.to_string()).collect(),
                sec1: self.sec1(),
                credential_id: Some(self.credential_id.clone()),
            }
        }
        pub fn client_data(&self, kind: &str, challenge: &[u8]) -> Vec<u8> {
            format!(
                r#"{{"type":"{kind}","challenge":"{}","origin":"{}","crossOrigin":false}}"#,
                b64url(challenge),
                self.origin
            )
            .into_bytes()
        }
        /// `navigator.credentials.get` over `domain + "\n" + body`.
        pub fn assert(&mut self, domain: &str, body: &str, key_id: &str) -> Value {
            self.assert_raw(&webauthn_challenge(domain, body), 0x05, key_id)
        }
        pub fn assert_raw(&mut self, challenge: &[u8], flags: u8, key_id: &str) -> Value {
            self.counter += 1;
            let cd = self.client_data("webauthn.get", challenge);
            let mut ad = sha256(self.rp_id.as_bytes()).to_vec();
            ad.push(flags);
            ad.extend_from_slice(&self.counter.to_be_bytes());
            let mut signed = ad.clone();
            signed.extend_from_slice(&sha256(&cd));
            let sig: p256::ecdsa::Signature = self.sk.sign(&signed);
            json!({
                "scheme": SCHEME_ES256, "credential_id": self.credential_id,
                "authenticator_data": b64url(&ad), "client_data_json": b64url(&cd),
                "signature": b64url(sig.to_der().as_bytes()), "key_id": key_id,
            })
        }
    }

    pub struct SoftKeyFile {
        pub sk: ed25519_dalek::SigningKey,
    }

    impl SoftKeyFile {
        pub fn new(seed: u8) -> SoftKeyFile {
            SoftKeyFile {
                sk: ed25519_dalek::SigningKey::from_bytes(&[seed; 32]),
            }
        }
        pub fn owner_key(&self) -> OwnerKey {
            OwnerKey::Ed25519 {
                public: self.sk.verifying_key().to_bytes(),
            }
        }
        pub fn sign(&self, domain: &str, body: &str) -> Value {
            use ed25519_dalek::Signer as _;
            let sig = self.sk.sign(&message(domain, body));
            json!({"scheme": SCHEME_ED25519, "sig": hex(&sig.to_bytes()), "key_id": self.owner_key().key_id()})
        }
    }

    /// A fake self-change entry in `queue/<id>` exactly as A-core FORMATS v1 lays it out
    /// (proposal + patch + stack sig + signed test record + artifact + signed dry run),
    /// signed with test-only keys. Writes `stack.pub` and `sandbox.pub` into `keys`.
    /// For tests and the local headless run only.
    pub fn fake_entry(
        queue: &std::path::Path,
        keys: &std::path::Path,
        id: &str,
        now: i64,
    ) -> std::io::Result<()> {
        use std::fs;
        let stack = SoftKeyFile::new(0x51);
        let sandbox = SoftKeyFile::new(0x5b);
        fs::create_dir_all(keys)?;
        fs::write(
            keys.join("stack.pub"),
            format!("{}\n", hex(stack.owner_key().raw())),
        )?;
        fs::write(
            keys.join("sandbox.pub"),
            format!("{}\n", hex(sandbox.owner_key().raw())),
        )?;
        let e = queue.join(id);
        fs::create_dir_all(e.join("test"))?;
        fs::create_dir_all(e.join("dryrun"))?;
        let base = "6da98fdc5c1e0b7f4c3a2d1e0f9a8b7c6d5e4f30";
        let patch = "diff --git a/crates/hermes-rs/src/reply.rs b/crates/hermes-rs/src/reply.rs\n--- a/crates/hermes-rs/src/reply.rs\n+++ b/crates/hermes-rs/src/reply.rs\n@@ -40,7 +40,9 @@ fn shorten(text: &str) -> String {\n     let limit = 1200;\n-    text.chars().take(limit).collect()\n+    // Cut on a sentence end so Telegram replies never stop mid-word.\n+    let cut: String = text.chars().take(limit).collect();\n+    cut.rfind('.').map(|i| cut[..=i].to_string()).unwrap_or(cut)\n }\n";
        fs::write(e.join("change.patch"), patch)?;
        let proposal = json!({"version": "hermes-self-modify/proposal/v1", "id": id, "base_commit": base,
            "summary": "Cut long replies at the end of a sentence instead of in the middle of a word",
            "reason": "Slavik saw two Telegram answers stop mid-word yesterday",
            "files": ["crates/hermes-rs/src/reply.rs"], "patch_sha256": sha256_hex(patch.as_bytes()),
            "proposed_by": "hermes", "created_at": "2026-10-31T09:40:00+01:00"});
        let pbytes = format!("{proposal:#}\n").into_bytes();
        fs::write(e.join("proposal.json"), &pbytes)?;
        let sb = json!({"version": "galion-sm/proposal-sig/v1", "id": id, "proposal_sha256": sha256_hex(&pbytes),
            "patch_sha256": sha256_hex(patch.as_bytes()), "base_commit": base, "signed_at": now - 900}).to_string();
        let mut s = stack.sign("galion-sm/proposal-sig/v1", &sb);
        s.as_object_mut().unwrap().remove("scheme");
        fs::write(
            e.join("proposal.stack-sig.json"),
            envelope(s, "body_json", &sb).to_string(),
        )?;
        let artifact: Vec<u8> = (0..300_000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 24) as u8)
            .collect();
        fs::write(e.join("test").join("artifact"), &artifact)?;
        let art_sha = sha256_hex(&artifact);
        fs::create_dir_all(e.join("test/logs"))?;
        fs::create_dir_all(e.join("dryrun/logs"))?;
        let mut stages: Vec<Value> = Vec::new();
        for (i, n) in ["fmt", "clippy", "build", "test", "lint"]
            .iter()
            .enumerate()
        {
            let log = format!("stage {n}: ok (fixture {i})\n");
            fs::write(e.join(format!("test/logs/{n}.log")), &log)?;
            stages.push(
                json!({"name": n, "exit": 0, "ok": true, "log": format!("logs/{n}.log"),
                "log_sha256": sha256_hex(log.as_bytes()), "log_bytes": log.len()}),
            );
        }
        let mut checks: Vec<Value> = Vec::new();
        for (n, d) in [
            ("health", "/health 200 in 41 ms"),
            (
                "telegram reply (fake model)",
                "long reply cut after a full stop",
            ),
            ("no real model calls", "fake model service only"),
        ] {
            let f = format!("logs/{}.log", n.split(' ').next().unwrap_or("x"));
            let log = format!("{n}: {d}\n");
            fs::write(e.join("dryrun").join(&f), &log)?;
            checks.push(json!({"name": n, "ok": true, "detail": d, "log": f, "log_sha256": sha256_hex(log.as_bytes())}));
        }
        let tb = json!({"version": "galion-sm/test-record/v1", "id": id, "proposal_sha256": sha256_hex(&pbytes),
            "proposal_sig_sha256": sha256_hex(sb.as_bytes()), "patch_sha256": sha256_hex(patch.as_bytes()), "base_commit": base,
            "artifact": {"name": "artifact", "bytes": artifact.len(), "sha256": art_sha},
            "stages": stages, "verdict": "pass", "why": "all 5 stages exit 0", "signed_at": now - 600, "signer": "galion-sm-sign"}).to_string();
        let mut t = sandbox.sign("galion-sm/test-record/v1", &tb);
        t.as_object_mut().unwrap().remove("scheme");
        fs::write(
            e.join("test").join("record.json"),
            envelope(t, "record_json", &tb).to_string(),
        )?;
        let db = json!({"version": "galion-sm/dryrun-record/v1", "id": id, "test_record_sha256": sha256_hex(tb.as_bytes()),
            "artifact_sha256": art_sha, "base_commit": base, "port": 23714, "model_calls_real": 0,
            "checks": checks,
            "verdict": "pass", "why": "3 of 3 checks ok", "signed_at": now - 300, "signer": "galion-sm-sign"}).to_string();
        let mut d = sandbox.sign("galion-sm/dryrun-record/v1", &db);
        d.as_object_mut().unwrap().remove("scheme");
        fs::write(
            e.join("dryrun").join("record.json"),
            envelope(d, "record_json", &db).to_string(),
        )?;
        Ok(())
    }

    /// Put `body` into an envelope under `field` (approval_json / body_json).
    pub fn envelope(mut sig: Value, field: &str, body: &str) -> Value {
        sig[field] = json!(body);
        sig
    }
}
