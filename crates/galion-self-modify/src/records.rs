// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Signatures 1-3: stack proposal signature, sandbox test record and sandbox
//! dry-run record. Envelope = hermes-rs `self_modify::{sign_env, verify_env}`.
//!
//! This module (the signer side) never starts a process: it only reads,
//! hashes, verifies and writes. The test `signer_never_spawns` checks that.

use crate::util::{
    hash_file, is_hex, read_file, read_json, safe_join, write_new, MAX_ARTIFACT, MAX_SMALL,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hermes_rs::self_modify::{sha256_hex, sign_env, verify_env, PROPOSAL_V};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const STACK_SIG_V: &str = "galion-sm/proposal-sig/v1";
pub const TEST_RESULT_V: &str = "galion-sm/test-result/v1";
pub const TEST_RECORD_V: &str = "galion-sm/test-record/v1";
pub const DRYRUN_RESULT_V: &str = "galion-sm/dryrun-result/v1";
pub const DRYRUN_RECORD_V: &str = "galion-sm/dryrun-record/v1";
/// Spare test ports for the dry run (GAIA_TEST_PORTS).
pub const DRYRUN_PORTS: std::ops::RangeInclusive<i64> = 19000..=23999;
pub const SIGNER: &str = "galion-sm-sign";

/// Paths inside one queue entry `<queue>/<id>/`.
pub struct Entry {
    pub dir: PathBuf,
}

impl Entry {
    pub fn new(dir: &Path) -> Entry {
        Entry {
            dir: dir.to_path_buf(),
        }
    }
    pub fn proposal(&self) -> PathBuf {
        self.dir.join("proposal.json")
    }
    pub fn patch(&self) -> PathBuf {
        self.dir.join("change.patch")
    }
    pub fn stack_sig(&self) -> PathBuf {
        self.dir.join("proposal.stack-sig.json")
    }
    pub fn test_dir(&self) -> PathBuf {
        self.dir.join("test")
    }
    pub fn test_result(&self) -> PathBuf {
        self.test_dir().join("result.json")
    }
    pub fn artifact(&self) -> PathBuf {
        self.test_dir().join("artifact")
    }
    pub fn test_record(&self) -> PathBuf {
        self.test_dir().join("record.json")
    }
    pub fn dryrun_dir(&self) -> PathBuf {
        self.dir.join("dryrun")
    }
    pub fn dryrun_result(&self) -> PathBuf {
        self.dryrun_dir().join("result.json")
    }
    pub fn dryrun_record(&self) -> PathBuf {
        self.dryrun_dir().join("record.json")
    }
    pub fn owner_approval(&self) -> PathBuf {
        self.dir.join("owner").join("approval.json")
    }
    pub fn deploy_record(&self) -> PathBuf {
        self.dir.join("deploy.json")
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 40
        && n.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

// ---------- 1. stack signature ----------

/// What a verified stack signature vouches for.
#[derive(Debug, Clone)]
pub struct StackOk {
    pub id: String,
    pub base_commit: String,
    pub proposal_sha256: String,
    pub patch_sha256: String,
    /// sha256 of the inner body_json string.
    pub sig_sha256: String,
}

fn proposal_facts(e: &Entry) -> Result<(Value, String, String), String> {
    let pb = read_file(&e.proposal(), MAX_SMALL)?;
    let p: Value = serde_json::from_slice(&pb).map_err(|x| format!("proposal.json: {x}"))?;
    if p["version"] != PROPOSAL_V {
        return Err("proposal.json: unknown version".into());
    }
    let patch_sha = hash_file(&e.patch(), MAX_SMALL)?.0;
    if s(&p, "patch_sha256") != patch_sha {
        return Err("change.patch does not match proposal.json".into());
    }
    if !valid_id(s(&p, "id")) || !is_hex(s(&p, "base_commit"), 40) {
        return Err("proposal id/base_commit malformed".into());
    }
    Ok((p, sha256_hex(&pb), patch_sha))
}

/// Hermes (openmind-hermes, STACK key) signs its own proposal. Origin proof
/// only: this key never signs a test or dry-run pass.
pub fn stack_sign(entry: &Path, stack: &SigningKey, now: i64) -> Result<Value, String> {
    let e = Entry::new(entry);
    let (p, psha, patch_sha) = proposal_facts(&e)?;
    let body = json!({
        "version": STACK_SIG_V, "id": s(&p, "id"), "proposal_sha256": psha,
        "patch_sha256": patch_sha, "base_commit": s(&p, "base_commit"), "signed_at": now,
    });
    let env = sign_env(STACK_SIG_V, &body, "body_json", stack);
    write_new(&e.stack_sig(), format!("{env}\n").as_bytes(), 0o644)?;
    Ok(body)
}

/// Verify the stack signature against the CURRENT proposal.json and patch.
pub fn verify_stack(entry: &Path, stack: &VerifyingKey) -> Result<StackOk, String> {
    let e = Entry::new(entry);
    let env = read_json(&e.stack_sig()).map_err(|x| format!("stack signature missing: {x}"))?;
    let (b, sig_sha) = verify_env(STACK_SIG_V, &env, "body_json", stack)
        .map_err(|x| format!("stack signature: {x}"))?;
    let (p, psha, patch_sha) = proposal_facts(&e)?;
    if s(&b, "proposal_sha256") != psha
        || s(&b, "patch_sha256") != patch_sha
        || s(&b, "id") != s(&p, "id")
        || s(&b, "base_commit") != s(&p, "base_commit")
    {
        return Err("stack signature is for a different proposal or patch".into());
    }
    Ok(StackOk {
        id: s(&p, "id").into(),
        base_commit: s(&p, "base_commit").into(),
        proposal_sha256: psha,
        patch_sha256: patch_sha,
        sig_sha256: sig_sha,
    })
}

fn distinct(signer: &SigningKey, stack: &VerifyingKey) -> Result<(), String> {
    if signer.verifying_key() == *stack {
        return Err("refused: the sandbox key must not be the stack key".into());
    }
    Ok(())
}

// ---------- 2. test record ----------

/// The signer (galion-sm-sign) turns the test account's UNSIGNED result into
/// a signed test record. It trusts no hash from the test side: artifact and
/// logs are re-hashed here. The stack signature is verified first.
pub fn sign_test(
    entry: &Path,
    stack: &VerifyingKey,
    key: &SigningKey,
    now: i64,
) -> Result<Value, String> {
    distinct(key, stack)?;
    let e = Entry::new(entry);
    let st = verify_stack(entry, stack)?;
    if e.test_record().exists() {
        return Err("a test record already exists for this entry (re-test = new entry)".into());
    }
    let r = read_json(&e.test_result())?;
    if r["version"] != TEST_RESULT_V {
        return Err("test/result.json: unknown version".into());
    }
    if s(&r, "id") != st.id || s(&r, "base_commit") != st.base_commit {
        return Err("test/result.json is for a different proposal or base".into());
    }
    let tdir = e.test_dir();
    let art_path = safe_join(&tdir, r["artifact"].as_str().unwrap_or("artifact"))?;
    if art_path != e.artifact() {
        return Err("artifact must be test/artifact".into());
    }
    let (art_sha, art_bytes) = hash_file(&art_path, MAX_ARTIFACT)?;
    let mut stages = vec![];
    let mut why = vec![];
    let raw = r["stages"].as_array().cloned().unwrap_or_default();
    if raw.is_empty() || raw.len() > 32 {
        why.push("no stages (or more than 32)".to_string());
    }
    for x in raw.iter().take(32) {
        let name = s(x, "name");
        if !valid_name(name) {
            return Err(format!("bad stage name {name:?}"));
        }
        let exit = x["exit"]
            .as_i64()
            .ok_or(format!("stage {name}: exit missing"))?;
        let (lsha, lbytes) = hash_file(&safe_join(&tdir, s(x, "log"))?, MAX_SMALL)?;
        if exit != 0 {
            why.push(format!("stage {name} failed (exit {exit})"));
        }
        stages.push(json!({"name": name, "exit": exit, "ok": exit == 0,
            "log": s(x, "log"), "log_sha256": lsha, "log_bytes": lbytes}));
    }
    let verdict = if why.is_empty() { "pass" } else { "fail" };
    let rec = json!({
        "version": TEST_RECORD_V, "id": st.id, "proposal_sha256": st.proposal_sha256,
        "proposal_sig_sha256": st.sig_sha256, "patch_sha256": st.patch_sha256,
        "base_commit": st.base_commit,
        "artifact": {"name": "artifact", "bytes": art_bytes, "sha256": art_sha},
        "stages": stages, "verdict": verdict, "why": why.join("; "),
        "signed_at": now, "signer": SIGNER,
    });
    let env = sign_env(TEST_RECORD_V, &rec, "record_json", key);
    write_new(&e.test_record(), format!("{env}\n").as_bytes(), 0o644)?;
    Ok(rec)
}

/// Verify the test record: sandbox signature, bound to the stack-signed
/// proposal, artifact and logs unchanged since signing. Returns (record,
/// sha256 of the inner record string).
pub fn verify_test(
    entry: &Path,
    stack: &VerifyingKey,
    sandbox: &VerifyingKey,
) -> Result<(Value, String), String> {
    let e = Entry::new(entry);
    let st = verify_stack(entry, stack)?;
    let env = read_json(&e.test_record()).map_err(|x| format!("test record missing: {x}"))?;
    let (rec, rsha) = verify_env(TEST_RECORD_V, &env, "record_json", sandbox)
        .map_err(|x| format!("test record: {x}"))?;
    if s(&rec, "proposal_sha256") != st.proposal_sha256
        || s(&rec, "proposal_sig_sha256") != st.sig_sha256
        || s(&rec, "patch_sha256") != st.patch_sha256
        || s(&rec, "id") != st.id
        || s(&rec, "base_commit") != st.base_commit
    {
        return Err("test record is for a different proposal".into());
    }
    let (art_sha, _) = hash_file(&e.artifact(), MAX_ARTIFACT)?;
    if rec["artifact"]["sha256"] != art_sha.as_str() {
        return Err("test/artifact changed after it was signed (fingerprint mismatch)".into());
    }
    for x in rec["stages"].as_array().cloned().unwrap_or_default() {
        let (lsha, _) = hash_file(&safe_join(&e.test_dir(), s(&x, "log"))?, MAX_SMALL)?;
        if s(&x, "log_sha256") != lsha {
            return Err(format!("test log {} changed after signing", s(&x, "log")));
        }
    }
    Ok((rec, rsha))
}

// ---------- 3. dry-run record ----------

/// The signer signs the dry-run harness's UNSIGNED result, bound to its own
/// test record (must be a pass) and the same artifact fingerprint.
pub fn sign_dryrun(
    entry: &Path,
    stack: &VerifyingKey,
    key: &SigningKey,
    now: i64,
) -> Result<Value, String> {
    distinct(key, stack)?;
    let e = Entry::new(entry);
    let (trec, trsha) = verify_test(entry, stack, &key.verifying_key())?;
    if trec["verdict"] != "pass" {
        return Err("refused: the test record is not a pass".into());
    }
    if e.dryrun_record().exists() {
        return Err("a dry-run record already exists for this entry".into());
    }
    let art_sha = s(&trec["artifact"], "sha256").to_string();
    let r = read_json(&e.dryrun_result())?;
    if r["version"] != DRYRUN_RESULT_V {
        return Err("dryrun/result.json: unknown version".into());
    }
    if s(&r, "id") != s(&trec, "id") {
        return Err("dryrun/result.json is for a different entry".into());
    }
    if s(&r, "artifact_sha256") != art_sha {
        return Err("refused: the dry run ran a different binary than the tested one".into());
    }
    let mut why = vec![];
    let port = r["port"].as_i64().unwrap_or(0);
    if !DRYRUN_PORTS.contains(&port) {
        why.push(format!("port {port} outside the test range 19000-23999"));
    }
    let calls = r["model_calls_real"].as_i64().unwrap_or(-1);
    if calls != 0 {
        why.push(format!(
            "real model calls {calls} (must be 0: fake model service only)"
        ));
    }
    let raw = r["checks"].as_array().cloned().unwrap_or_default();
    if raw.is_empty() || raw.len() > 64 {
        why.push("no checks (or more than 64)".into());
    }
    let mut checks = vec![];
    for x in raw.iter().take(64) {
        let name = s(x, "name");
        if !valid_name(name) {
            return Err(format!("bad check name {name:?}"));
        }
        let ok = x["ok"].as_bool() == Some(true);
        let detail: String = s(x, "detail").chars().take(500).collect();
        let log_sha = match x["log"].as_str() {
            Some(l) => json!(hash_file(&safe_join(&e.dryrun_dir(), l)?, MAX_SMALL)?.0),
            None => Value::Null,
        };
        if !ok {
            why.push(format!("check {name} failed"));
        }
        checks.push(json!({"name": name, "ok": ok, "detail": detail,
            "log": x["log"].clone(), "log_sha256": log_sha}));
    }
    let verdict = if why.is_empty() { "pass" } else { "fail" };
    let rec = json!({
        "version": DRYRUN_RECORD_V, "id": s(&trec, "id"), "test_record_sha256": trsha,
        "artifact_sha256": art_sha, "base_commit": s(&trec, "base_commit"),
        "port": port, "model_calls_real": calls, "checks": checks,
        "verdict": verdict, "why": why.join("; "), "signed_at": now, "signer": SIGNER,
    });
    let env = sign_env(DRYRUN_RECORD_V, &rec, "record_json", key);
    std::fs::create_dir_all(e.dryrun_dir()).map_err(|x| x.to_string())?;
    write_new(&e.dryrun_record(), format!("{env}\n").as_bytes(), 0o644)?;
    Ok(rec)
}

/// Verify the dry-run record against the sandbox key and the test record.
pub fn verify_dryrun(
    entry: &Path,
    sandbox: &VerifyingKey,
    test_rec: &Value,
    test_rsha: &str,
) -> Result<(Value, String), String> {
    let e = Entry::new(entry);
    let env = read_json(&e.dryrun_record()).map_err(|x| format!("dry-run record missing: {x}"))?;
    let (rec, rsha) = verify_env(DRYRUN_RECORD_V, &env, "record_json", sandbox)
        .map_err(|x| format!("dry-run record: {x}"))?;
    if s(&rec, "test_record_sha256") != test_rsha
        || rec["artifact_sha256"] != test_rec["artifact"]["sha256"]
        || rec["id"] != test_rec["id"]
        || rec["base_commit"] != test_rec["base_commit"]
    {
        return Err("dry-run record is for a different test record or binary".into());
    }
    for x in rec["checks"].as_array().cloned().unwrap_or_default() {
        if let Some(l) = x["log"].as_str() {
            let (lsha, _) = hash_file(&safe_join(&e.dryrun_dir(), l)?, MAX_SMALL)?;
            if s(&x, "log_sha256") != lsha {
                return Err(format!("dry-run log {l} changed after signing"));
            }
        }
    }
    Ok((rec, rsha))
}

#[cfg(test)]
mod tests {
    /// The signer side must never execute anything (tested code included).
    #[test]
    fn signer_never_spawns() {
        for (name, src) in [
            ("records.rs", include_str!("records.rs")),
            ("util.rs", include_str!("util.rs")),
            ("galion-sm-sign.rs", include_str!("bin/galion-sm-sign.rs")),
        ] {
            let needle = concat!("process", "::", "Command");
            assert!(!src.contains(needle), "{name} must not spawn processes");
            assert!(!src.contains(concat!("Command", "::new")), "{name}");
            assert!(!src.contains(concat!("exec", "v")), "{name}");
        }
    }
}
