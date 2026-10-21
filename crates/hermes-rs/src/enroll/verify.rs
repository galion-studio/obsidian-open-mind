// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Fail-closed check of an omnode pairing document (docs/hermes-enrollment.md
//! §4, checks 1-9 except the nonce store, which lives in `super`).
//!
//! Copied from `edge/openmind-node/src/pair.rs` (`verify`, `verify_document`,
//! same repository, same MIT licence, branch swarm/edge-boot 753676e). omnode
//! is a standalone, unpublished crate with device-only dependencies (mDNS,
//! landlock, QR), so Hermes carries its own copy instead of depending on it.
//! Keep the two in step: the canonical string, the 600 s window, the 120 s
//! skew and the field list must stay identical (the real-life test
//! crates/hermes-gateway/tests/real_enroll.rs signs with the real omnode binary).

use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Lifetime of a pairing payload, seconds (`expires - created`).
pub const PAYLOAD_TTL: i64 = 600;
/// Allowed clock skew between device and verifier, seconds.
pub const SKEW: i64 = 120;

/// Exactly the fields omnode sends; anything else is refused.
const FIELDS: [&str; 17] = [
    "v",
    "kind",
    "device_id",
    "public_key",
    "fingerprint",
    "network_hash",
    "network_trusted",
    "capabilities",
    "arch",
    "cores",
    "ram_mb",
    "platform",
    "enroll_url",
    "nonce",
    "created",
    "expires",
    "expires_unix",
];

/// What Hermes may rely on after [`verify`] said yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub device_id: String,
    pub fingerprint: String,
    pub public_key: String,
    pub capabilities: Vec<String>,
    pub network_hash: Option<String>,
    pub arch: String,
    pub cores: u64,
    pub ram_mb: u64,
    pub nonce: String,
    pub expires_unix: i64,
    /// Unsigned device claims: shown to the owner, never used for decisions.
    pub claimed_network_trusted: bool,
    pub claimed_platform: String,
}

pub fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

pub fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `SHA256:xxxx-xxxx-...` over the first 16 bytes of sha256(pk) (omnode
/// `identity::fingerprint`).
pub fn fingerprint(vk: &VerifyingKey) -> String {
    let h = hex(&Sha256::digest(vk.as_bytes())[..16]);
    let groups: Vec<&str> = (0..h.len()).step_by(4).map(|i| &h[i..i + 4]).collect();
    format!("SHA256:{}", groups.join("-"))
}

/// `omn-` + 12 hex of sha256(pk) (omnode `identity::device_id`).
pub fn device_id(vk: &VerifyingKey) -> String {
    format!("omn-{}", &hex(&Sha256::digest(vk.as_bytes()))[..12])
}

/// Parse a 32-byte lower-hex ed25519 key that is a valid, non-weak point.
pub fn parse_key(public_key: &str) -> Result<VerifyingKey, String> {
    if !is_lower_hex(public_key, 64) {
        return Err("public_key is not 32 bytes of lower-case hex".into());
    }
    let pk: [u8; 32] = unhex(public_key)
        .and_then(|v| v.try_into().ok())
        .ok_or("public_key is not 32 bytes")?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(|_| "public_key is not an ed25519 point")?;
    if vk.is_weak() {
        return Err("public_key is a weak (small-order) point".into());
    }
    Ok(vk)
}

/// Strict ed25519 check of a 64-byte lower-hex signature over `msg`.
pub fn check_sig(vk: &VerifyingKey, msg: &[u8], signature_hex: &str) -> Result<(), String> {
    if !is_lower_hex(signature_hex, 128) {
        return Err("signature is not 64 bytes of lower-case hex".into());
    }
    let bytes = unhex(signature_hex).ok_or("signature is not hex")?;
    let sig = Signature::from_slice(&bytes).map_err(|_| "signature is malformed")?;
    vk.verify_strict(msg, &sig)
        .map_err(|_| "signature does not verify".to_string())
}

/// Same rules as omnode `pair::validate_url`, without the http warning.
pub fn validate_url(url: &str) -> Result<(), String> {
    if url.contains('|') || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("enroll URL contains invalid characters".into());
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("enroll URL must start with https:// (or http:// for a local test)")?;
    if rest.is_empty() || rest.starts_with('/') {
        return Err("enroll URL has no host".into());
    }
    Ok(())
}

fn str_field<'a>(o: &'a serde_json::Map<String, Value>, k: &str) -> Result<&'a str, String> {
    o.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("payload rejected: {k} missing or not a string"))
}

fn u64_field(o: &serde_json::Map<String, Value>, k: &str) -> Result<u64, String> {
    o.get(k)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("payload rejected: {k} missing or not an unsigned integer"))
}

/// Check a pairing payload exactly as docs/hermes-enrollment.md §4 says.
/// Pure and fail-closed: anything not provably right is an `Err`.
pub fn verify(
    payload: &Value,
    signature_hex: &str,
    expected_url: &str,
    now: i64,
) -> Result<Verified, String> {
    // 1. shape: object, exact field set, types, v, kind.
    let o = payload
        .as_object()
        .ok_or("payload rejected: not an object")?;
    if let Some(k) = o.keys().find(|k| !FIELDS.contains(&k.as_str())) {
        return Err(format!("payload rejected: unknown field `{k}`"));
    }
    let v = u64_field(o, "v")?;
    if v != 1 {
        return Err(format!("unsupported payload version {v}"));
    }
    if str_field(o, "kind")? != "openmind-pair" {
        return Err("payload kind is not openmind-pair".into());
    }
    let device = str_field(o, "device_id")?;
    let public_key = str_field(o, "public_key")?;
    let fp = str_field(o, "fingerprint")?;
    let network_hash = match o.get("network_hash") {
        Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.as_str()),
        _ => return Err("payload rejected: network_hash missing or not a string/null".into()),
    };
    let network_trusted = o
        .get("network_trusted")
        .and_then(Value::as_bool)
        .ok_or("payload rejected: network_trusted missing or not a bool")?;
    let caps: Vec<String> = match o.get("capabilities") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|c| c.as_str().map(String::from))
            .collect::<Option<_>>()
            .ok_or("payload rejected: capabilities are not strings")?,
        _ => return Err("payload rejected: capabilities missing or not a list".into()),
    };
    let arch = str_field(o, "arch")?;
    let cores = u64_field(o, "cores")?;
    let ram_mb = u64_field(o, "ram_mb")?;
    let platform = str_field(o, "platform")?;
    let enroll_url = str_field(o, "enroll_url")?;
    let nonce = str_field(o, "nonce")?;
    let created = str_field(o, "created")?;
    let expires = str_field(o, "expires")?;
    let expires_unix = o
        .get("expires_unix")
        .and_then(Value::as_i64)
        .ok_or("payload rejected: expires_unix missing or not an integer")?;
    // 2. canonical-string injection.
    let signed = [
        device, public_key, arch, enroll_url, nonce, created, expires,
    ];
    if signed.iter().any(|s| s.is_empty() || s.contains('|'))
        || network_hash.is_some_and(|h| h.contains('|'))
    {
        return Err("signed field empty or contains '|'".into());
    }
    if caps
        .iter()
        .any(|c| c.is_empty() || c.contains('|') || c.contains(','))
    {
        return Err("capability empty or contains '|' or ','".into());
    }
    if let Some(h) = network_hash {
        if !is_lower_hex(h, 32) {
            return Err("network_hash is not 32 lower-case hex chars".into());
        }
    }
    // 3. public key.
    let vk = parse_key(public_key)?;
    // 4. identity derived from the key.
    if device != device_id(&vk) {
        return Err("device_id does not match public_key".into());
    }
    if fp != fingerprint(&vk) {
        return Err("fingerprint does not match public_key".into());
    }
    // 5. signature (strict) over the recomputed canonical string.
    let canonical = [
        "omnode-pair-v1",
        device,
        public_key,
        network_hash.unwrap_or("-"),
        &caps.join(","),
        arch,
        &cores.to_string(),
        &ram_mb.to_string(),
        enroll_url,
        nonce,
        created,
        expires,
    ]
    .join("|");
    check_sig(&vk, canonical.as_bytes(), signature_hex.trim())?;
    // 6. unsigned expires_unix must agree with the signed strings.
    let rfc = crate::time::zagreb_rfc3339;
    if rfc(expires_unix) != expires {
        return Err("expires_unix does not match the signed expires".into());
    }
    if rfc(expires_unix - PAYLOAD_TTL) != created {
        return Err("created is not expires - 600 s".into());
    }
    // 7. time window.
    if now > expires_unix + SKEW {
        return Err(format!("payload expired at {expires}"));
    }
    if expires_unix - PAYLOAD_TTL > now + SKEW {
        return Err(format!("payload created in the future ({created})"));
    }
    // 8. bound to this server's URL.
    validate_url(expected_url)?;
    if enroll_url != expected_url {
        return Err("enroll_url is not this server's URL".into());
    }
    // 9. nonce shape (the replay store is the caller's job).
    if !is_lower_hex(nonce, 24) {
        return Err("nonce is not 96-bit lower-case hex".into());
    }
    Ok(Verified {
        device_id: device.into(),
        fingerprint: fp.into(),
        public_key: public_key.into(),
        capabilities: caps,
        network_hash: network_hash.map(String::from),
        arch: arch.into(),
        cores,
        ram_mb,
        nonce: nonce.into(),
        expires_unix,
        claimed_network_trusted: network_trusted,
        claimed_platform: platform.into(),
    })
}

/// The JSON `omnode pair` prints: `{"payload":..,"signature":..,"signed_string":..}`.
/// `signed_string` is informational and ignored.
pub fn verify_document(doc: &Value, expected_url: &str, now: i64) -> Result<Verified, String> {
    let obj = doc.as_object().ok_or("document is not a JSON object")?;
    if obj
        .keys()
        .any(|k| !matches!(k.as_str(), "payload" | "signature" | "signed_string"))
    {
        return Err("document has unknown top-level fields".into());
    }
    let payload = obj.get("payload").ok_or("document has no payload")?;
    let sig = obj
        .get("signature")
        .and_then(Value::as_str)
        .ok_or("document has no signature")?;
    verify(payload, sig, expected_url, now)
}
