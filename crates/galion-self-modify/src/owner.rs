// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Signature 4: the owner (Slavik) approves one exact artifact, once, for
//! at most 30 minutes. Two schemes:
//!
//! * `webauthn-es256`: a passkey assertion from galion.app. The WebAuthn
//!   challenge is sha256(domain + "\n" + body); the authenticator signs
//!   `authenticatorData || sha256(clientDataJSON)` with P-256.
//! * `ed25519`: a plain key file on Slavik's device signs
//!   domain + "\n" + body (strict Ed25519).

use crate::util::{b64url, is_hex, unb64url};
use ed25519_dalek::Signer as _;
use hermes_rs::self_modify::{hex, sha256_hex, unhex};
use p256::ecdsa::signature::Verifier as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const APPROVAL_V: &str = "galion-sm/owner-approval/v1";
pub const OWNER_KEY_V: &str = "galion-sm/owner-key/v1";
/// Longest allowed approval window (Slavik: 30 minutes).
pub const MAX_TTL_S: i64 = 30 * 60;
/// Clock skew tolerated for `issued_at` in the future.
pub const MAX_SKEW_S: i64 = 120;
/// SPKI DER prefix of an uncompressed P-256 public key.
pub const P256_SPKI_PREFIX: &str = "3059301306072a8648ce3d020106082a8648ce3d030107034200";

/// The owner's public key, from `/etc/hermes-self-modify/owner.pub`.
#[derive(Clone, Debug)]
pub enum OwnerKey {
    Ed25519(ed25519_dalek::VerifyingKey),
    WebAuthn {
        rp_id: String,
        origins: Vec<String>,
        key: p256::ecdsa::VerifyingKey,
        sec1: Vec<u8>,
        credential_id: Option<Vec<u8>>,
    },
}

impl OwnerKey {
    pub fn parse(text: &str) -> Result<OwnerKey, String> {
        let t = text.trim();
        if is_hex(t, 64) {
            // Legacy hermes-self-modify owner.pub: bare Ed25519 hex.
            return Ok(OwnerKey::Ed25519(hermes_rs::self_modify::verifying_key(t)?));
        }
        let v: Value = serde_json::from_str(t).map_err(|e| format!("owner.pub: {e}"))?;
        if v["version"] != OWNER_KEY_V {
            return Err("owner.pub: unknown version".into());
        }
        match v["scheme"].as_str() {
            Some("ed25519") => Ok(OwnerKey::Ed25519(hermes_rs::self_modify::verifying_key(
                v["public_key_hex"].as_str().unwrap_or(""),
            )?)),
            Some("webauthn-es256") => {
                let sec1 = if let Some(h) = v["public_key_sec1_hex"].as_str() {
                    unhex(h)?
                } else if let Some(h) = v["public_key_spki_hex"].as_str() {
                    let h = h.trim().to_ascii_lowercase();
                    let rest = h
                        .strip_prefix(P256_SPKI_PREFIX)
                        .ok_or("owner.pub: SPKI is not an uncompressed P-256 key")?;
                    unhex(rest)?
                } else {
                    return Err("owner.pub: no public key".into());
                };
                if sec1.len() != 65 || sec1[0] != 4 {
                    return Err("owner.pub: P-256 key must be 65-byte uncompressed SEC1".into());
                }
                let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1)
                    .map_err(|_| "owner.pub: not a valid P-256 point".to_string())?;
                let rp_id = v["rp_id"].as_str().unwrap_or("").to_string();
                if rp_id.is_empty() || rp_id.contains('/') || rp_id.contains(':') {
                    return Err("owner.pub: rp_id must be a bare domain".into());
                }
                let origins: Vec<String> = v["origins"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if origins.is_empty() || origins.iter().any(|o| !o.starts_with("https://")) {
                    return Err(
                        "owner.pub: origins must be a non-empty list of https:// origins".into(),
                    );
                }
                let credential_id = match v["credential_id"].as_str() {
                    Some(c) => Some(unb64url(c)?),
                    None => None,
                };
                Ok(OwnerKey::WebAuthn {
                    rp_id,
                    origins,
                    key,
                    sec1,
                    credential_id,
                })
            }
            _ => Err("owner.pub: unknown scheme".into()),
        }
    }

    pub fn scheme(&self) -> &'static str {
        match self {
            OwnerKey::Ed25519(_) => "ed25519",
            OwnerKey::WebAuthn { .. } => "webauthn-es256",
        }
    }

    /// Raw public key bytes (32 for Ed25519, 65-byte SEC1 point for P-256).
    pub fn raw(&self) -> Vec<u8> {
        match self {
            OwnerKey::Ed25519(k) => k.as_bytes().to_vec(),
            OwnerKey::WebAuthn { sec1, .. } => sec1.clone(),
        }
    }

    /// sha256 of the raw public key: what the console shows and Slavik
    /// compares with his device before activation.
    pub fn fingerprint(&self) -> String {
        sha256_hex(&self.raw())
    }

    /// Same bytes as another key (any scheme)?
    pub fn same_as_ed25519(&self, k: &ed25519_dalek::VerifyingKey) -> bool {
        matches!(self, OwnerKey::Ed25519(o) if o == k)
    }
}

/// owner.pub text for a plain Ed25519 key.
pub fn ed25519_owner_pub(k: &ed25519_dalek::VerifyingKey) -> String {
    json!({"version": OWNER_KEY_V, "scheme": "ed25519", "public_key_hex": hex(k.as_bytes())})
        .to_string()
}

/// 16 random bytes as 32 lowercase hex.
pub fn new_nonce() -> Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    Ok(hex(&b))
}

/// What the owner approves. Built ONCE as a string; that exact string is
/// signed and stored.
pub struct ApprovalFacts<'a> {
    pub id: &'a str,
    pub artifact_sha256: &'a str,
    pub base_commit: &'a str,
    pub test_record_sha256: &'a str,
    pub dryrun_record_sha256: &'a str,
    pub nonce: &'a str,
    pub issued_at: i64,
    pub ttl_s: i64,
}

pub fn approval_body(f: &ApprovalFacts) -> Result<String, String> {
    if f.ttl_s <= 0 || f.ttl_s > MAX_TTL_S {
        return Err(format!("ttl must be 1..={MAX_TTL_S} s"));
    }
    if !is_hex(f.nonce, 32) {
        return Err("nonce must be 32 lowercase hex".into());
    }
    Ok(json!({
        "version": APPROVAL_V, "id": f.id, "artifact_sha256": f.artifact_sha256,
        "base_commit": f.base_commit, "test_record_sha256": f.test_record_sha256,
        "dryrun_record_sha256": f.dryrun_record_sha256, "nonce": f.nonce,
        "issued_at": f.issued_at, "expires": f.issued_at + f.ttl_s,
    })
    .to_string())
}

/// Bytes the owner signs (Ed25519) or hashes into the WebAuthn challenge.
pub fn message(body: &str) -> Vec<u8> {
    format!("{APPROVAL_V}\n{body}").into_bytes()
}

/// WebAuthn challenge (raw 32 bytes) for `navigator.credentials.get`.
pub fn webauthn_challenge(body: &str) -> [u8; 32] {
    Sha256::digest(message(body)).into()
}

/// Plain key-file approval envelope.
pub fn sign_ed25519(body: &str, sk: &ed25519_dalek::SigningKey) -> Value {
    let sig = sk.sign(&message(body));
    json!({"scheme": "ed25519", "approval_json": body, "sig": hex(&sig.to_bytes()),
        "key_id": hermes_rs::self_modify::key_id(&sk.verifying_key())})
}

/// Verify an approval envelope against the owner key. Returns the parsed
/// body (time/nonce/binding checks are the deploy verifier's job).
pub fn verify(env: &Value, owner: &OwnerKey) -> Result<Value, String> {
    let body = env["approval_json"]
        .as_str()
        .ok_or("owner approval: approval_json missing")?;
    let scheme = env["scheme"].as_str().unwrap_or("");
    if scheme != owner.scheme() {
        return Err(format!(
            "owner approval scheme {scheme:?} does not match owner.pub ({})",
            owner.scheme()
        ));
    }
    match owner {
        OwnerKey::Ed25519(vk) => {
            let sig: [u8; 64] = unhex(env["sig"].as_str().ok_or("owner approval: sig missing")?)?
                .try_into()
                .map_err(|_| "owner approval: signature must be 64 bytes".to_string())?;
            vk.verify_strict(&message(body), &ed25519_dalek::Signature::from_bytes(&sig))
                .map_err(|_| {
                    "owner approval: bad signature (forged, tampered or wrong key)".to_string()
                })?;
        }
        OwnerKey::WebAuthn {
            rp_id,
            origins,
            key,
            credential_id,
            ..
        } => verify_webauthn(env, body, rp_id, origins, key, credential_id.as_deref())?,
    }
    let v: Value = serde_json::from_str(body).map_err(|e| format!("owner approval: {e}"))?;
    if v["version"] != APPROVAL_V {
        return Err("owner approval: unknown version".into());
    }
    Ok(v)
}

fn field_b64(env: &Value, k: &str) -> Result<Vec<u8>, String> {
    unb64url(
        env[k]
            .as_str()
            .ok_or(format!("owner approval: {k} missing"))?,
    )
    .map_err(|e| format!("owner approval: {k}: {e}"))
}

fn verify_webauthn(
    env: &Value,
    body: &str,
    rp_id: &str,
    origins: &[String],
    key: &p256::ecdsa::VerifyingKey,
    cred: Option<&[u8]>,
) -> Result<(), String> {
    let auth = field_b64(env, "authenticator_data")?;
    let cdj = field_b64(env, "client_data_json")?;
    let sig_der = field_b64(env, "signature")?;
    if let Some(c) = cred {
        if field_b64(env, "credential_id")? != c {
            return Err("owner approval: different passkey (credential id)".into());
        }
    }
    let cd: Value =
        serde_json::from_slice(&cdj).map_err(|e| format!("owner approval: clientDataJSON: {e}"))?;
    if cd["type"] != "webauthn.get" {
        return Err("owner approval: clientData.type is not webauthn.get".into());
    }
    if cd["challenge"] != b64url(&webauthn_challenge(body)).as_str() {
        return Err("owner approval: WebAuthn challenge does not match this approval".into());
    }
    let origin = cd["origin"].as_str().unwrap_or("");
    if !origins.iter().any(|o| o == origin) {
        return Err(format!("owner approval: origin {origin:?} not allowed"));
    }
    if cd["crossOrigin"] == true {
        return Err("owner approval: cross-origin assertion".into());
    }
    if auth.len() < 37 {
        return Err("owner approval: authenticatorData too short".into());
    }
    if auth[..32] != Sha256::digest(rp_id.as_bytes())[..] {
        return Err("owner approval: rpIdHash does not match owner.pub rp_id".into());
    }
    let flags = auth[32];
    if flags & 0x01 == 0 || flags & 0x04 == 0 {
        return Err("owner approval: user presence + user verification required".into());
    }
    let sig = p256::ecdsa::Signature::from_der(&sig_der)
        .map_err(|_| "owner approval: signature is not DER ECDSA".to_string())?;
    let sig = sig.normalize_s().unwrap_or(sig);
    let mut msg = auth.clone();
    msg.extend_from_slice(&Sha256::digest(&cdj));
    key.verify(&msg, &sig).map_err(|_| {
        "owner approval: bad passkey signature (forged, tampered or wrong key)".to_string()
    })
}

/// Authenticator sign counter (bytes 33..37), for monotonic checks.
pub fn webauthn_sign_count(env: &Value) -> Option<u32> {
    let a = unb64url(env["authenticator_data"].as_str()?).ok()?;
    Some(u32::from_be_bytes(a.get(33..37)?.try_into().ok()?))
}
