// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only way a proposal reaches a target tree: a valid sandbox pass
//! record AND an owner approval over that exact record, at the same base
//! commit. The applied tree must equal the tree the sandbox tested, or the
//! apply is reversed. This prototype refuses every live root outright.

use crate::proposal::Proposal;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::{json, Value};
use std::path::Path;

pub const APPROVE_V: &str = "hermes-gate/approve/v1";

fn approval_msg(proposal_sha: &str, record_sha: &str, target_head: &str, expires: i64) -> String {
    format!("{APPROVE_V}\n{proposal_sha}\n{record_sha}\n{target_head}\n{expires}")
}

/// Owner side: sign an approval for one record, one target head, until
/// `expires` (unix seconds). Runs where the owner's key is, never on the
/// host Hermes runs on.
pub fn approve(
    owner: &SigningKey,
    proposal_sha: &str,
    record_sha: &str,
    target_head: &str,
    expires: i64,
) -> Value {
    let m = approval_msg(proposal_sha, record_sha, target_head, expires);
    json!({
        "proposal_sha256": proposal_sha,
        "record_sha256": record_sha,
        "target_head": target_head,
        "expires": expires,
        "sig": crate::hex(&owner.sign(m.as_bytes()).to_bytes()),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Authorized {
    pub proposal_sha256: String,
    pub base_commit: String,
    pub result_tree: String,
}

/// Every condition for a live apply; any miss is an error naming it.
pub fn authorize(
    signed_record: &Value,
    sandbox_vk: &VerifyingKey,
    p: &Proposal,
    approval: &Value,
    owner_vk: &VerifyingKey,
    target_head: &str,
    now: i64,
) -> Result<Authorized, String> {
    if sandbox_vk.as_bytes() == owner_vk.as_bytes() {
        return Err("the sandbox key and the owner key must differ".into());
    }
    let rec = crate::record::verify(signed_record, sandbox_vk)?;
    if rec["verdict"] != "pass" {
        return Err("record verdict is not pass".into());
    }
    let psha = p.sha256();
    if rec["proposal_sha256"] != psha.as_str() {
        return Err("record is for a different proposal".into());
    }
    if rec["base_commit"] != p.base_commit.as_str() {
        return Err("record base differs from the proposal base".into());
    }
    if p.base_commit != target_head {
        return Err(
            "target moved: its HEAD is not the tested base commit (re-run the sandbox)".into(),
        );
    }
    let rsha = crate::record::record_sha256(signed_record);
    let s = |k: &str| approval[k].as_str().unwrap_or("").to_string();
    if s("proposal_sha256") != psha || s("record_sha256") != rsha || s("target_head") != target_head
    {
        return Err("approval does not cover this proposal, record and target".into());
    }
    let expires = approval["expires"]
        .as_i64()
        .ok_or("approval: expires missing")?;
    if now > expires {
        return Err("approval expired".into());
    }
    let sig: [u8; 64] = crate::unhex(&s("sig"))?
        .try_into()
        .map_err(|_| "approval: signature must be 64 bytes")?;
    owner_vk
        .verify_strict(
            approval_msg(&psha, &rsha, target_head, expires).as_bytes(),
            &Signature::from_bytes(&sig),
        )
        .map_err(|_| "approval: bad owner signature".to_string())?;
    let tree = rec["result_tree"].as_str().unwrap_or("").to_string();
    if tree.len() != 40 {
        return Err("record has no result tree".into());
    }
    Ok(Authorized {
        proposal_sha256: psha,
        base_commit: p.base_commit.clone(),
        result_tree: tree,
    })
}

/// Apply an authorized proposal to `target` (index + work tree, no commit:
/// the owner commits). Refuses live roots in this prototype. If the result
/// is not the tree the sandbox tested, the apply is reversed.
pub fn apply(target: &Path, auth: &Authorized, p: &Proposal) -> Result<String, String> {
    if crate::stage::under_live_root(target) {
        return Err(format!(
            "{}: live apply is not enabled in this prototype (owner rule: nothing live changes)",
            target.display()
        ));
    }
    if auth.proposal_sha256 != p.sha256() {
        return Err("authorization is for a different proposal".into());
    }
    let head = crate::git::head(target)?;
    if head != auth.base_commit {
        return Err("target HEAD moved since authorization".into());
    }
    let dirty = crate::git::git(
        target,
        &["status", "--porcelain", "--untracked-files=no"],
        None,
    )?;
    if !dirty.trim().is_empty() {
        return Err("target has uncommitted changes".into());
    }
    let patch = p.patch.as_bytes();
    crate::git::git(target, &["apply", "--check", "--index", "-"], Some(patch))?;
    crate::git::git(target, &["apply", "--index", "-"], Some(patch))?;
    let tree = crate::git::git(target, &["write-tree"], None)?
        .trim()
        .to_string();
    if tree != auth.result_tree {
        let _ = crate::git::git(target, &["apply", "-R", "--index", "-"], Some(patch));
        return Err("applied tree differs from the sandbox-tested tree; reversed".into());
    }
    Ok(tree)
}
