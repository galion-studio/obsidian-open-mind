// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The self-modify switch (owner order t400u, 2026-10-29 CET +01:00).
//!
//! One root-owned JSON file, default [`PATH`], records whether Hermes may
//! change its own persona (`SOUL.md`), config (`config.yaml`) or code.
//!
//! * **Locked** is the default and the fail-safe: no file, an unreadable or
//!   malformed file, an unknown state, or an `allowed` record without a valid
//!   owner signature all read as locked.
//! * **Allowed** counts only when the record carries an Ed25519 signature by
//!   the owner key (the hermes-gate owner key; its public half is
//!   [`OWNER_PUB`]) over [`message`]. Writing the file needs root, and
//!   signing an `allowed` record needs the owner seed, which is never on the
//!   VPS.
//! * Locking needs no signature: anyone with root may lock.
//!
//! While locked, the persona and config files must not be world-writable;
//! [`target_drift`] reports any that are.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{json, Value};
use std::path::Path;

/// Where the switch lives on the VPS.
pub const PATH: &str = "/etc/galion/hermes-self-modify.json";
/// The owner public key (hermes-gate `owner.pub`, hex).
pub const OWNER_PUB: &str = "/var/lib/galion-gate/owner.pub";
/// The files the lock protects.
pub const TARGETS: &[&str] = &[
    "/opt/galion/hermes/SOUL.md",
    "/opt/galion/hermes/config.yaml",
];
pub const VERSION: &str = "galion/self-modify/v1";
/// Largest switch file read.
pub const MAX_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub state: String,
    pub changed_by: String,
    pub changed_at: String,
    pub reason: String,
    pub key_id: String,
    pub signature: String,
}

/// The effective state after checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// `locked` or `allowed`.
    pub state: &'static str,
    /// True only for an owner-signed `allowed` record that verified.
    pub verified: bool,
    pub why: String,
}

/// The exact bytes the owner signs.
pub fn message(state: &str, by: &str, at: &str, reason: &str) -> Vec<u8> {
    format!("{VERSION}\nstate={state}\nchanged_by={by}\nchanged_at={at}\nreason={reason}\n")
        .into_bytes()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("not hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

/// Parse a 32-byte hex public key.
pub fn pubkey(hex_text: &str) -> Result<VerifyingKey, String> {
    let b: [u8; 32] = unhex(hex_text)?
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&b).map_err(|e| e.to_string())
}

/// Parse a 32-byte hex seed.
pub fn seed(hex_text: &str) -> Result<SigningKey, String> {
    let b: [u8; 32] = unhex(hex_text)?
        .try_into()
        .map_err(|_| "seed must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&b))
}

/// Short key id: the first 16 hex chars of the public key.
pub fn key_id(k: &VerifyingKey) -> String {
    hex(k.as_bytes())[..16].to_string()
}

impl Record {
    pub fn locked(by: &str, reason: &str) -> Self {
        Record {
            state: "locked".into(),
            changed_by: by.into(),
            changed_at: zagreb_time::now_rfc3339(),
            reason: reason.into(),
            key_id: String::new(),
            signature: String::new(),
        }
    }

    /// A record signed with the owner seed (the only way to allow).
    pub fn signed(state: &str, by: &str, reason: &str, sk: &SigningKey) -> Result<Self, String> {
        if state != "locked" && state != "allowed" {
            return Err("state must be locked or allowed".into());
        }
        let at = zagreb_time::now_rfc3339();
        let sig = sk.sign(&message(state, by, &at, reason));
        Ok(Record {
            state: state.into(),
            changed_by: by.into(),
            changed_at: at,
            reason: reason.into(),
            key_id: key_id(&sk.verifying_key()),
            signature: hex(&sig.to_bytes()),
        })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "version": VERSION,
            "state": self.state,
            "changed_by": self.changed_by,
            "changed_at": self.changed_at,
            "reason": self.reason,
            "key_id": if self.key_id.is_empty() { Value::Null } else { json!(self.key_id) },
            "signature": if self.signature.is_empty() { Value::Null } else { json!(self.signature) },
            "owner_only": true,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        if v.get("version").and_then(Value::as_str) != Some(VERSION) {
            return Err("unknown version".into());
        }
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Ok(Record {
            state: s("state"),
            changed_by: s("changed_by"),
            changed_at: s("changed_at"),
            reason: s("reason"),
            key_id: s("key_id"),
            signature: s("signature"),
        })
    }

    /// Effective state against the owner key (`None` = key missing).
    pub fn effective(&self, owner: Option<&VerifyingKey>) -> Effective {
        let locked = |why: &str| Effective {
            state: "locked",
            verified: false,
            why: why.into(),
        };
        match self.state.as_str() {
            "locked" => locked("locked by record"),
            "allowed" => {
                let Some(k) = owner else {
                    return locked("allowed record, but the owner public key is missing");
                };
                let sig = match unhex(&self.signature)
                    .ok()
                    .and_then(|b| <[u8; 64]>::try_from(b).ok())
                {
                    Some(b) => Signature::from_bytes(&b),
                    None => return locked("allowed record without a valid signature"),
                };
                let m = message(
                    &self.state,
                    &self.changed_by,
                    &self.changed_at,
                    &self.reason,
                );
                if k.verify(&m, &sig).is_ok() {
                    Effective {
                        state: "allowed",
                        verified: true,
                        why: "owner-signed".into(),
                    }
                } else {
                    locked("allowed record, but the signature is not the owner's")
                }
            }
            _ => locked("unknown state"),
        }
    }
}

/// Read the switch; any failure reads as locked (`Err` says why).
pub fn read(path: &Path) -> Result<Record, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("no switch file: {e}"))?;
    if meta.len() > MAX_BYTES {
        return Err("switch file too large".into());
    }
    let t = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&t).map_err(|e| format!("bad json: {e}"))?;
    Record::from_json(&v)
}

/// Read the owner public key file.
pub fn read_owner(path: &Path) -> Option<VerifyingKey> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| pubkey(&t).ok())
}

/// Full status: effective state plus target drift.
pub fn status(path: &Path, owner_pub: &Path, targets: &[&Path]) -> Value {
    let owner = read_owner(owner_pub);
    let (rec, eff) = match read(path) {
        Ok(r) => {
            let e = r.effective(owner.as_ref());
            (Some(r), e)
        }
        Err(why) => (
            None,
            Effective {
                state: "locked",
                verified: false,
                why: format!("default locked ({why})"),
            },
        ),
    };
    let drift = target_drift(targets);
    json!({
        "ok": true,
        "effective": eff.state,
        "verified": eff.verified,
        "why": eff.why,
        "record": rec.map(|r| r.to_json()),
        "owner_key": owner.map(|k| key_id(&k)),
        "targets_world_writable": drift,
        "lock_holds": eff.state == "allowed" || drift.is_empty(),
    })
}

/// Targets that are world-writable (only meaningful while locked).
pub fn target_drift(targets: &[&Path]) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    targets
        .iter()
        .filter(|p| {
            std::fs::metadata(p)
                .map(|m| m.permissions().mode() & 0o002 != 0)
                .unwrap_or(false)
        })
        .map(|p| p.display().to_string())
        .collect()
}

/// Atomic write (temp + rename), mode 0644.
pub fn write(path: &Path, r: &Record) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(&r.to_json()).map_err(|e| e.to_string())? + "\n";
    f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    #[test]
    fn default_and_bad_files_are_locked() {
        let d = std::env::temp_dir().join(format!("gsm-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("sw.json");
        let st = status(&p, &d.join("none.pub"), &[]);
        assert_eq!(st["effective"], "locked");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(status(&p, &d.join("none.pub"), &[])["effective"], "locked");
        std::fs::write(
            &p,
            r#"{"version":"galion/self-modify/v1","state":"allowed"}"#,
        )
        .unwrap();
        assert_eq!(status(&p, &d.join("none.pub"), &[])["effective"], "locked");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn only_owner_signature_allows() {
        let owner = key(7);
        let other = key(9);
        let ok = Record::signed("allowed", "Mate", "demo", &owner).unwrap();
        let e = ok.effective(Some(&owner.verifying_key()));
        assert_eq!((e.state, e.verified), ("allowed", true));
        // Signed by someone else.
        let bad = Record::signed("allowed", "Mate", "demo", &other).unwrap();
        assert_eq!(bad.effective(Some(&owner.verifying_key())).state, "locked");
        // Tampered field.
        let mut t = ok.clone();
        t.changed_by = "hermes".into();
        assert_eq!(t.effective(Some(&owner.verifying_key())).state, "locked");
        // No owner key on the machine.
        assert_eq!(ok.effective(None).state, "locked");
        // Unsigned allow.
        let mut u = ok.clone();
        u.signature.clear();
        assert_eq!(u.effective(Some(&owner.verifying_key())).state, "locked");
        // Round trip through JSON.
        let back = Record::from_json(&ok.to_json()).unwrap();
        assert_eq!(back, ok);
        assert!(Record::signed("open", "x", "", &owner).is_err());
    }

    #[test]
    fn drift_and_write() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("gsm-w-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("SOUL.md");
        std::fs::write(&f, "x").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(target_drift(&[&f]).len(), 1);
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(target_drift(&[&f]).is_empty());
        let p = d.join("sw.json");
        write(&p, &Record::locked("test", "demo")).unwrap();
        let m = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o644);
        assert_eq!(read(&p).unwrap().state, "locked");
        std::fs::remove_dir_all(&d).ok();
    }
}
