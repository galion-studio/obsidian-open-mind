// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The signed pass record. The record is kept as the exact JSON text that
//! was signed (`record_json`), so verification never depends on
//! re-serialisation. Ed25519 (ed25519-dalek, strict verify), the same crate
//! hermes-rs enrollment already uses.

use crate::checks::Check;
use crate::proposal::Proposal;
use crate::stage::{DeployReport, Plan};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::{json, Value};

pub const VERSION: &str = "hermes-gate/pass-record/v1";

/// A new key pair as (seed hex, public key hex).
pub fn keygen() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| e.to_string())?;
    let sk = SigningKey::from_bytes(&seed);
    Ok((crate::hex(&seed), crate::hex(sk.verifying_key().as_bytes())))
}

pub fn signing_key(seed_hex: &str) -> Result<SigningKey, String> {
    let b: [u8; 32] = crate::unhex(seed_hex.trim())?
        .try_into()
        .map_err(|_| "signing key must be 32 bytes")?;
    Ok(SigningKey::from_bytes(&b))
}

pub fn verifying_key(pub_hex: &str) -> Result<VerifyingKey, String> {
    let b: [u8; 32] = crate::unhex(pub_hex.trim())?
        .try_into()
        .map_err(|_| "public key must be 32 bytes")?;
    VerifyingKey::from_bytes(&b).map_err(|e| e.to_string())
}

pub fn key_id(vk: &VerifyingKey) -> String {
    crate::sha256_hex(vk.as_bytes())[..16].to_string()
}

/// The record body. `verdict` is `pass` only if every static check passed,
/// the patch applied and every sandbox stage passed with its layers.
pub fn build(p: &Proposal, plan: &Plan, checks: &[Check], rep: Option<&DeployReport>) -> Value {
    let checks_ok = crate::checks::all_ok(checks);
    let deploy_ok = rep.is_some_and(DeployReport::ok);
    let f = openmind_sandbox::features();
    json!({
        "version": VERSION,
        "proposal_id": p.id,
        "proposal_sha256": p.sha256(),
        "patch_sha256": crate::sha256_hex(p.patch.as_bytes()),
        "base_commit": p.base_commit,
        "model_tier": p.model_tier,
        "plan_sha256": plan.sha256(),
        "result_tree": rep.map(|r| r.result_tree.clone()).unwrap_or_default(),
        "checks": checks.iter().map(Check::to_json).collect::<Vec<_>>(),
        "applied": rep.is_some_and(|r| r.applied),
        "apply_error": rep.and_then(|r| r.apply_error.clone()),
        "stages": rep.map(|r| r.stages.iter().map(|s| s.to_json()).collect::<Vec<_>>()).unwrap_or_default(),
        "checks_passed": checks.iter().filter(|c| c.ok).count(),
        "checks_total": checks.len(),
        "sandbox": {"landlock": "not used", "kernel_landlock_abi": f.landlock_abi, "seccomp": "required", "userns": f.userns, "required_layers": plan.require_layers},
        "verdict": if checks_ok && deploy_ok { "pass" } else { "fail" },
        "when": crate::now_stamp(),
    })
}

pub fn sign(record: &Value, sk: &SigningKey) -> Value {
    let text = record.to_string();
    let sig = sk.sign(text.as_bytes());
    json!({
        "record_json": text,
        "sig": crate::hex(&sig.to_bytes()),
        "key_id": key_id(&sk.verifying_key()),
    })
}

/// Verify a signed record against the trusted sandbox key; returns the
/// record body.
pub fn verify(signed: &Value, vk: &VerifyingKey) -> Result<Value, String> {
    let text = signed["record_json"]
        .as_str()
        .ok_or("record: record_json missing")?;
    let sig: [u8; 64] = crate::unhex(signed["sig"].as_str().ok_or("record: sig missing")?)?
        .try_into()
        .map_err(|_| "record: signature must be 64 bytes")?;
    vk.verify_strict(text.as_bytes(), &Signature::from_bytes(&sig))
        .map_err(|_| "record: bad signature (not from the sandbox key)".to_string())?;
    let v: Value = serde_json::from_str(text).map_err(|e| format!("record: {e}"))?;
    if v["version"] != VERSION {
        return Err("record: unknown version".into());
    }
    Ok(v)
}

/// sha256 of the signed record text (what the owner approves).
pub fn record_sha256(signed: &Value) -> String {
    crate::sha256_hex(signed["record_json"].as_str().unwrap_or("").as_bytes())
}
