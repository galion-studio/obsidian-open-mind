// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Pairing payload for enrollment by Hermes/VPS, and `verify`, the fail-closed
//! check a verifier runs on it. v0.1 `pair` is a stub: it prints the
//! signed payload and a terminal QR and does NOT contact the URL.

use crate::config::Config;
use crate::identity::Identity;
use crate::util::{hex, random_bytes};
use crate::{sysinfo, wifi, zagreb};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Payload {
    pub v: u8,
    pub kind: &'static str,
    pub device_id: String,
    pub public_key: String,
    pub fingerprint: String,
    pub network_hash: Option<String>,
    pub network_trusted: bool,
    pub capabilities: Vec<String>,
    pub arch: String,
    pub cores: usize,
    pub ram_mb: u64,
    pub platform: String,
    pub enroll_url: String,
    pub nonce: String,
    pub created: String,
    pub expires: String,
    pub expires_unix: i64,
}

impl Payload {
    /// Canonical, unambiguous string that is signed (fields are '|'-free).
    pub fn canonical(&self) -> String {
        [
            "omnode-pair-v1",
            &self.device_id,
            &self.public_key,
            self.network_hash.as_deref().unwrap_or("-"),
            &self.capabilities.join(","),
            &self.arch,
            &self.cores.to_string(),
            &self.ram_mb.to_string(),
            &self.enroll_url,
            &self.nonce,
            &self.created,
            &self.expires,
        ]
        .join("|")
    }

    /// Short signed string for the QR code (fits a phone screen):
    /// `omp1|device_id|pk_b64url|net|caps|nonce|expires_unix`, then `|sig_b64url`.
    pub fn qr_short(&self) -> String {
        [
            "omp1",
            &self.device_id,
            &crate::util::b64url(&crate::util::unhex(&self.public_key).unwrap_or_default()),
            self.network_hash.as_deref().unwrap_or("-"),
            &self.capabilities.join(","),
            &self.nonce,
            &self.expires_unix.to_string(),
        ]
        .join("|")
    }
}

pub fn validate_url(url: &str) -> Result<Vec<String>, String> {
    let mut warn = vec![];
    if url.contains('|') || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("enroll URL contains invalid characters".into());
    }
    if let Some(rest) = url.strip_prefix("https://") {
        if rest.is_empty() || rest.starts_with('/') {
            return Err("enroll URL has no host".into());
        }
    } else if url.starts_with("http://") {
        warn.push("enroll URL is plain http; use https for real enrollment".into());
    } else {
        return Err("enroll URL must start with https:// (or http:// for a local test)".into());
    }
    Ok(warn)
}

pub fn build(
    cfg: &Config,
    id: &Identity,
    url: &str,
    obs: &wifi::Observation,
) -> Result<(Payload, String, Vec<String>), String> {
    build_at(cfg, id, url, obs, zagreb::now_unix())
}

/// [`build`] with an explicit creation time (`omnode pair --now`, used by the
/// real-life tests to produce an expired payload; a verifier refuses it).
pub fn build_at(
    cfg: &Config,
    id: &Identity,
    url: &str,
    obs: &wifi::Observation,
    now: i64,
) -> Result<(Payload, String, Vec<String>), String> {
    let warns = validate_url(url)?;
    let me = sysinfo::self_info();
    let net = id.current_network(cfg, obs);
    let p = Payload {
        v: 1,
        kind: "openmind-pair",
        device_id: id.state.device_id.clone(),
        public_key: id.state.public_key.clone(),
        fingerprint: id.fingerprint(),
        network_hash: net.as_ref().map(|n| n.0.clone()),
        network_trusted: net.as_ref().map(|n| n.2).unwrap_or(false),
        capabilities: cfg.node.capabilities.clone(),
        arch: me.arch.into(),
        cores: me.cores,
        ram_mb: me.ram_total_mb,
        platform: me.platform.into(),
        enroll_url: url.into(),
        nonce: hex(&random_bytes::<12>().map_err(|e| e.to_string())?),
        created: zagreb::rfc3339(now),
        expires: zagreb::rfc3339(now + PAYLOAD_TTL),
        expires_unix: now + PAYLOAD_TTL,
    };
    let sig = id.sign(p.canonical().as_bytes());
    Ok((p, sig, warns))
}

/// Lifetime of a pairing payload, seconds (`expires - created`).
pub const PAYLOAD_TTL: i64 = 600;
/// Allowed clock skew between device and verifier, seconds.
pub const SKEW: i64 = 120;

/// The payload as received (owned strings, unknown fields rejected). Only the
/// fields in [`Payload::canonical`] are signed; `fingerprint`,
/// `network_trusted`, `platform` and `expires_unix` are checked against the
/// signed ones or treated as device claims.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incoming {
    pub v: u8,
    pub kind: String,
    pub device_id: String,
    pub public_key: String,
    pub fingerprint: String,
    pub network_hash: Option<String>,
    pub network_trusted: bool,
    pub capabilities: Vec<String>,
    pub arch: String,
    pub cores: usize,
    pub ram_mb: u64,
    pub platform: String,
    pub enroll_url: String,
    pub nonce: String,
    pub created: String,
    pub expires: String,
    pub expires_unix: i64,
}

/// What a verifier may rely on after [`verify`] said yes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verified {
    pub device_id: String,
    pub fingerprint: String,
    pub public_key: String,
    pub capabilities: Vec<String>,
    pub network_hash: Option<String>,
    pub nonce: String,
    pub expires: String,
    /// Unsigned device claims: shown to the owner, never used for decisions.
    pub claimed_network_trusted: bool,
    pub claimed_platform: String,
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Check a pairing payload exactly as Hermes must (docs/hermes-enrollment.md
/// §4, all checks except the Hermes-side nonce store). Pure and fail-closed:
/// anything that is not provably right is an `Err` with a short reason, and
/// there is no "unknown" or "skip" outcome.
pub fn verify(
    payload: &serde_json::Value,
    signature_hex: &str,
    expected_url: &str,
    now: i64,
) -> Result<Verified, String> {
    use ed25519_dalek::{Signature, VerifyingKey};

    // 1. shape: v, kind, no unknown fields.
    let p: Incoming =
        serde_json::from_value(payload.clone()).map_err(|e| format!("payload rejected: {e}"))?;
    if p.v != 1 {
        return Err(format!("unsupported payload version {}", p.v));
    }
    if p.kind != "openmind-pair" {
        return Err("payload kind is not openmind-pair".into());
    }
    // 2. canonical-string injection.
    let signed = [
        &p.device_id,
        &p.public_key,
        &p.arch,
        &p.enroll_url,
        &p.nonce,
        &p.created,
        &p.expires,
    ];
    if signed.iter().any(|s| s.is_empty() || s.contains('|'))
        || p.network_hash.as_deref().is_some_and(|h| h.contains('|'))
    {
        return Err("signed field empty or contains '|'".into());
    }
    if p.capabilities
        .iter()
        .any(|c| c.is_empty() || c.contains('|') || c.contains(','))
    {
        return Err("capability empty or contains '|' or ','".into());
    }
    if let Some(h) = &p.network_hash {
        // `-` is the canonical placeholder for "no network"; a real hash is hex.
        if !is_lower_hex(h, 32) {
            return Err("network_hash is not 32 lower-case hex chars".into());
        }
    }
    // 3. public key.
    if !is_lower_hex(&p.public_key, 64) {
        return Err("public_key is not 32 bytes of lower-case hex".into());
    }
    let pk: [u8; 32] = crate::util::unhex(&p.public_key)
        .and_then(|v| v.try_into().ok())
        .ok_or("public_key is not 32 bytes")?;
    let vk = VerifyingKey::from_bytes(&pk).map_err(|_| "public_key is not an ed25519 point")?;
    if vk.is_weak() {
        return Err("public_key is a weak (small-order) point".into());
    }
    // 4. identity derived from the key.
    if p.device_id != crate::identity::device_id(&vk) {
        return Err("device_id does not match public_key".into());
    }
    if p.fingerprint != crate::identity::fingerprint(&vk) {
        return Err("fingerprint does not match public_key".into());
    }
    // 5. signature (strict).
    if !is_lower_hex(signature_hex.trim(), 128) {
        return Err("signature is not 64 bytes of lower-case hex".into());
    }
    let sig_bytes = crate::util::unhex(signature_hex.trim()).ok_or("signature is not hex")?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| "signature is malformed")?;
    let canonical = [
        "omnode-pair-v1",
        &p.device_id,
        &p.public_key,
        p.network_hash.as_deref().unwrap_or("-"),
        &p.capabilities.join(","),
        &p.arch,
        &p.cores.to_string(),
        &p.ram_mb.to_string(),
        &p.enroll_url,
        &p.nonce,
        &p.created,
        &p.expires,
    ]
    .join("|");
    vk.verify_strict(canonical.as_bytes(), &sig)
        .map_err(|_| "signature does not verify")?;
    // 6. unsigned expires_unix must agree with the signed strings.
    if zagreb::rfc3339(p.expires_unix) != p.expires {
        return Err("expires_unix does not match the signed expires".into());
    }
    if zagreb::rfc3339(p.expires_unix - PAYLOAD_TTL) != p.created {
        return Err("created is not expires - 600 s".into());
    }
    // 7. time window.
    if now > p.expires_unix + SKEW {
        return Err(format!("payload expired at {}", p.expires));
    }
    if p.expires_unix - PAYLOAD_TTL > now + SKEW {
        return Err(format!("payload created in the future ({})", p.created));
    }
    // 8. bound to this verifier's URL.
    validate_url(expected_url)?;
    if p.enroll_url != expected_url {
        return Err("enroll_url is not this server's URL".into());
    }
    // 9. nonce shape (replay store is Hermes-side).
    if !is_lower_hex(&p.nonce, 24) {
        return Err("nonce is not 96-bit lower-case hex".into());
    }
    Ok(Verified {
        device_id: p.device_id,
        fingerprint: p.fingerprint,
        public_key: p.public_key,
        capabilities: p.capabilities,
        network_hash: p.network_hash,
        nonce: p.nonce,
        expires: p.expires,
        claimed_network_trusted: p.network_trusted,
        claimed_platform: p.platform,
    })
}

/// `omnode pair verify FILE`: FILE holds the JSON `omnode pair` printed
/// (`{"payload":..,"signature":..,"signed_string":..}`). `signed_string` is
/// informational and ignored; the canonical string is recomputed.
pub fn verify_document(text: &str, expected_url: &str, now: i64) -> Result<Verified, String> {
    let doc: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
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
        .and_then(|s| s.as_str())
        .ok_or("document has no signature")?;
    verify(payload, sig, expected_url, now)
}

pub fn qr_text(data: &str) -> Result<String, String> {
    let code = qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::L)
        .map_err(|e| e.to_string())?;
    Ok(code
        .render::<qrcode::render::unicode::Dense1x2>()
        .dark_color(qrcode::render::unicode::Dense1x2::Light)
        .light_color(qrcode::render::unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

/// QR text = short string + `|` + base64url signature over the short string.
pub fn qr_payload(id: &Identity, p: &Payload) -> String {
    let short = p.qr_short();
    let sig = crate::util::unhex(&id.sign(short.as_bytes())).unwrap_or_default();
    format!("{short}|{}", crate::util::b64url(&sig))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::unhex;
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    #[test]
    fn url_rules() {
        assert!(validate_url("https://hermes.example.invalid/enroll")
            .unwrap()
            .is_empty());
        assert_eq!(
            validate_url("http://192.168.1.2:8080/enroll")
                .unwrap()
                .len(),
            1
        );
        assert!(validate_url("ftp://x").is_err());
        assert!(validate_url("https://").is_err());
        assert!(validate_url("https://a|b").is_err());
    }

    #[test]
    fn payload_signature_verifies_and_qr_renders() {
        let d = std::env::temp_dir().join(format!(
            "omnode-test-pair-{}",
            hex(&random_bytes::<6>().unwrap())
        ));
        let id = Identity::init(&d).unwrap();
        let (p, sig, _) = build(
            &Config::default(),
            &id,
            "https://hermes.example.invalid/enroll",
            &wifi::Observation::default(),
        )
        .unwrap();
        let vk =
            VerifyingKey::from_bytes(&unhex(&p.public_key).unwrap().try_into().unwrap()).unwrap();
        let s = Signature::from_slice(&unhex(&sig).unwrap()).unwrap();
        assert!(vk.verify(p.canonical().as_bytes(), &s).is_ok());
        let short = qr_payload(&id, &p);
        assert!(short.len() < 260, "{}", short.len());
        let qr = qr_text(&short).unwrap();
        assert!(qr.lines().next().unwrap().chars().count() <= 66);
        assert!(qr.lines().count() > 10);
        // No secret material in the payload.
        let seed = std::fs::read_to_string(d.join("node.key")).unwrap();
        assert!(!serde_json::to_string(&p).unwrap().contains(seed.trim()));
        std::fs::remove_dir_all(&d).ok();
    }

    const URL: &str = "https://hermes.example.invalid/openmind/enroll";

    fn fresh(tag: &str) -> (std::path::PathBuf, Identity, serde_json::Value, String, i64) {
        let d = std::env::temp_dir().join(format!(
            "omnode-test-verify-{tag}-{}",
            hex(&random_bytes::<6>().unwrap())
        ));
        let id = Identity::init(&d).unwrap();
        let (p, sig, _) =
            build(&Config::default(), &id, URL, &wifi::Observation::default()).unwrap();
        let now = p.expires_unix - PAYLOAD_TTL;
        (d, id, serde_json::to_value(&p).unwrap(), sig, now)
    }

    /// Re-sign an edited payload with the device key (a "valid" signature
    /// over bad content), so the check under test is not the signature.
    fn resign(id: &Identity, v: &serde_json::Value) -> String {
        let p: Incoming = serde_json::from_value(v.clone()).unwrap();
        let canonical = [
            "omnode-pair-v1",
            &p.device_id,
            &p.public_key,
            p.network_hash.as_deref().unwrap_or("-"),
            &p.capabilities.join(","),
            &p.arch,
            &p.cores.to_string(),
            &p.ram_mb.to_string(),
            &p.enroll_url,
            &p.nonce,
            &p.created,
            &p.expires,
        ]
        .join("|");
        id.sign(canonical.as_bytes())
    }

    #[test]
    fn verify_accepts_a_fresh_payload() {
        let (d, id, v, sig, now) = fresh("ok");
        let ok = verify(&v, &sig, URL, now).unwrap();
        assert_eq!(ok.device_id, id.state.device_id);
        assert_eq!(ok.fingerprint, id.fingerprint());
        // within skew on both ends
        assert!(verify(&v, &sig, URL, now + PAYLOAD_TTL + SKEW).is_ok());
        assert!(verify(&v, &sig, URL, now - SKEW).is_ok());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn verify_refuses_expired_and_future() {
        let (d, _id, v, sig, now) = fresh("time");
        let e = verify(&v, &sig, URL, now + PAYLOAD_TTL + SKEW + 1).unwrap_err();
        assert!(e.contains("expired"), "{e}");
        let e = verify(&v, &sig, URL, now - SKEW - 1).unwrap_err();
        assert!(e.contains("future"), "{e}");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn verify_refuses_tampering() {
        let (d, id, v, sig, now) = fresh("tamper");
        let edit = |k: &str, val: serde_json::Value| {
            let mut w = v.clone();
            w[k] = val;
            w
        };
        // tampered capabilities, original signature
        let w = edit(
            "capabilities",
            serde_json::json!(["agent-sandbox", "money"]),
        );
        assert!(verify(&w, &sig, URL, now)
            .unwrap_err()
            .contains("signature"));
        // edited unsigned expiry
        let w = edit(
            "expires_unix",
            serde_json::json!(v["expires_unix"].as_i64().unwrap() + 86400),
        );
        assert!(verify(&w, &sig, URL, now)
            .unwrap_err()
            .contains("expires_unix"));
        // wrong fingerprint
        let w = edit("fingerprint", serde_json::json!("SHA256:0000-0000"));
        assert!(verify(&w, &sig, URL, now)
            .unwrap_err()
            .contains("fingerprint"));
        // payload meant for another server
        let e = verify(&v, &sig, "https://other.example.invalid/enroll", now).unwrap_err();
        assert!(e.contains("enroll_url"), "{e}");
        // unknown field
        let w = edit("admin", serde_json::json!(true));
        assert!(verify(&w, &sig, URL, now).unwrap_err().contains("rejected"));
        // wrong version / kind
        assert!(verify(&edit("v", serde_json::json!(2)), &sig, URL, now).is_err());
        assert!(verify(&edit("kind", serde_json::json!("x")), &sig, URL, now).is_err());
        // canonical-string injection, even when re-signed by the device
        let w = edit("capabilities", serde_json::json!(["a,b"]));
        assert!(verify(&w, &resign(&id, &w), URL, now)
            .unwrap_err()
            .contains("capability"));
        let w = edit("arch", serde_json::json!("x86_64|evil"));
        assert!(verify(&w, &resign(&id, &w), URL, now)
            .unwrap_err()
            .contains("'|'"));
        let w = edit("network_hash", serde_json::json!("-"));
        assert!(verify(&w, &resign(&id, &w), URL, now)
            .unwrap_err()
            .contains("network_hash"));
        let w = edit("nonce", serde_json::json!("00"));
        assert!(verify(&w, &resign(&id, &w), URL, now)
            .unwrap_err()
            .contains("nonce"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn verify_refuses_foreign_key_and_bad_signatures() {
        let (d1, _a, v, sig, now) = fresh("keyA");
        let (d2, b, _, sig_b, _) = fresh("keyB");
        // B's key swapped in: device_id no longer matches
        let mut w = v.clone();
        w["public_key"] = serde_json::json!(b.state.public_key);
        assert!(verify(&w, &sig, URL, now)
            .unwrap_err()
            .contains("device_id"));
        // B's whole identity claimed but A's signature
        w["device_id"] = serde_json::json!(b.state.device_id);
        w["fingerprint"] = serde_json::json!(b.fingerprint());
        assert!(verify(&w, &sig, URL, now)
            .unwrap_err()
            .contains("signature"));
        // another payload's signature
        assert!(verify(&v, &sig_b, URL, now).is_err());
        // malformed signatures
        for s in ["", "zz", &sig[..126], &sig.to_uppercase(), &"0".repeat(128)] {
            assert!(verify(&v, s, URL, now).is_err(), "accepted sig {s:?}");
        }
        // not an object / missing fields
        assert!(verify(&serde_json::json!(null), &sig, URL, now).is_err());
        assert!(verify(&serde_json::json!({}), &sig, URL, now).is_err());
        std::fs::remove_dir_all(&d1).ok();
        std::fs::remove_dir_all(&d2).ok();
    }

    #[test]
    fn verify_document_roundtrip_and_strict_top_level() {
        let (d, _id, v, sig, now) = fresh("doc");
        let doc = serde_json::json!({"payload": v, "signature": sig, "signed_string": "ignored"});
        assert!(verify_document(&doc.to_string(), URL, now).is_ok());
        let mut bad = doc.clone();
        bad["extra"] = serde_json::json!(1);
        assert!(verify_document(&bad.to_string(), URL, now).is_err());
        assert!(verify_document("not json", URL, now).is_err());
        assert!(verify_document(r#"{"payload":{}}"#, URL, now).is_err());
        std::fs::remove_dir_all(&d).ok();
    }
}
