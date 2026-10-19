// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Slavik / Galion Studio

//! Pairing payload for enrollment by Hermes/VPS. v0.1 is a stub: it prints the
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
    let warns = validate_url(url)?;
    let me = sysinfo::self_info();
    let net = id.current_network(cfg, obs);
    let now = zagreb::now_unix();
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
        expires: zagreb::rfc3339(now + 600),
        expires_unix: now + 600,
    };
    let sig = id.sign(p.canonical().as_bytes());
    Ok((p, sig, warns))
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
}
