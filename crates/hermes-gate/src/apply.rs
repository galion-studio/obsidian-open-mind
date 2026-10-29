// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only way a proposal reaches a tree: a valid sandbox pass record AND
//! an owner approval over that exact record, the source's current commit
//! and an expiry. Applied to a fresh staging copy only (never in place),
//! once per approval (replay ledger), and reversed if the result differs
//! from the tree the sandbox tested. Live paths are refused before any
//! command runs.

use crate::proposal::Proposal;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const APPROVE_V: &str = "hermes-gate/approve/v2";

fn approval_msg(
    proposal_sha: &str,
    record_sha: &str,
    target_head: &str,
    expires: i64,
    label: &str,
) -> String {
    format!("{APPROVE_V}\n{proposal_sha}\n{record_sha}\n{target_head}\n{expires}\n{label}")
}

/// Owner side: sign an approval for one record, one target commit, until
/// `expires` (unix seconds). `label` is signed too (e.g. `DEMO approval`).
/// Runs where the owner's key is, never on the host Hermes runs on.
pub fn approve(
    owner: &SigningKey,
    proposal_sha: &str,
    record_sha: &str,
    target_head: &str,
    expires: i64,
) -> Value {
    approve_labeled(owner, proposal_sha, record_sha, target_head, expires, "")
}

pub fn approve_labeled(
    owner: &SigningKey,
    proposal_sha: &str,
    record_sha: &str,
    target_head: &str,
    expires: i64,
    label: &str,
) -> Value {
    let label: String = label
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    let m = approval_msg(proposal_sha, record_sha, target_head, expires, &label);
    json!({
        "version": APPROVE_V,
        "proposal_sha256": proposal_sha,
        "record_sha256": record_sha,
        "target_head": target_head,
        "expires": expires,
        "label": label,
        "owner_key_id": crate::record::key_id(&owner.verifying_key()),
        "sig": crate::hex(&owner.sign(m.as_bytes()).to_bytes()),
    })
}

/// sha256 of an approval's signature (its replay-ledger identity).
pub fn approval_id(approval: &Value) -> String {
    crate::sha256_hex(approval["sig"].as_str().unwrap_or("").as_bytes())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Authorized {
    pub proposal_sha256: String,
    pub base_commit: String,
    pub result_tree: String,
    pub approval_id: String,
    pub label: String,
}

/// Every condition for an apply; any miss is an error naming it.
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
    if approval.is_null() || approval["sig"].as_str().unwrap_or("").is_empty() {
        return Err("no owner approval".into());
    }
    let rec = crate::record::verify(signed_record, sandbox_vk)?;
    if rec["verdict"] != "pass" {
        return Err("record verdict is not pass".into());
    }
    let psha = p.sha256();
    if rec["proposal_sha256"] != psha.as_str() {
        return Err("record is for a different proposal (checksum mismatch)".into());
    }
    if rec["patch_sha256"].is_string()
        && rec["patch_sha256"] != crate::sha256_hex(p.patch.as_bytes()).as_str()
    {
        return Err("patch checksum mismatch".into());
    }
    if rec["base_commit"] != p.base_commit.as_str() {
        return Err("record base differs from the proposal base".into());
    }
    if p.base_commit != target_head {
        return Err(
            "target moved: its commit is not the tested base commit (re-run the sandbox)".into(),
        );
    }
    let rsha = crate::record::record_sha256(signed_record);
    let s = |k: &str| approval[k].as_str().unwrap_or("").to_string();
    if s("proposal_sha256") != psha || s("record_sha256") != rsha || s("target_head") != target_head
    {
        return Err("approval does not cover this proposal, record and target commit".into());
    }
    let expires = approval["expires"]
        .as_i64()
        .ok_or("approval: expires missing")?;
    if now > expires {
        return Err("approval expired".into());
    }
    let label = s("label");
    let sig: [u8; 64] = crate::unhex(&s("sig"))?
        .try_into()
        .map_err(|_| "approval: signature must be 64 bytes")?;
    owner_vk
        .verify_strict(
            approval_msg(&psha, &rsha, target_head, expires, &label).as_bytes(),
            &Signature::from_bytes(&sig),
        )
        .map_err(|_| "approval: bad owner signature (not the owner key)".to_string())?;
    let tree = rec["result_tree"].as_str().unwrap_or("").to_string();
    if tree.len() != 40 {
        return Err("record has no result tree".into());
    }
    Ok(Authorized {
        proposal_sha256: psha,
        base_commit: p.base_commit.clone(),
        result_tree: tree,
        approval_id: approval_id(approval),
        label,
    })
}

/// Where an apply may happen.
#[derive(Clone, Debug)]
pub struct ApplyTarget {
    /// Non-live git repository holding the base commit (read for its HEAD
    /// and to make the staging copy; its own tree is never changed).
    pub source: PathBuf,
    /// Dir under which a fresh staging copy is created.
    pub staging_root: PathBuf,
    /// Append-only list of used approval ids (replay refusal).
    pub ledger: PathBuf,
}

/// Pure path refusal, before any command: live roots for every path.
pub fn refuse_live(t: &ApplyTarget) -> Result<(), String> {
    for (what, p) in [
        ("source", &t.source),
        ("staging root", &t.staging_root),
        ("ledger", &t.ledger),
    ] {
        if !p.is_absolute() {
            return Err(format!("{what} {}: must be absolute", p.display()));
        }
        if crate::stage::under_live_root(p) {
            return Err(format!(
                "{what} {}: live path refused (never applied in place, no git run there)",
                p.display()
            ));
        }
    }
    let s = std::fs::canonicalize(&t.source).unwrap_or_else(|_| t.source.clone());
    let r = std::fs::canonicalize(&t.staging_root).unwrap_or_else(|_| t.staging_root.clone());
    if r.starts_with(&s) || s.starts_with(&r) {
        return Err("staging root and source must be separate trees (no in-place apply)".into());
    }
    Ok(())
}

fn ledger_has(ledger: &Path, id: &str) -> Result<bool, String> {
    match std::fs::read_to_string(ledger) {
        Ok(t) => Ok(t.lines().any(|l| l.split_whitespace().next() == Some(id))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("ledger: {e}")),
    }
}

fn ledger_add(ledger: &Path, a: &Authorized) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(ledger)
        .map_err(|e| format!("ledger: {e}"))?;
    writeln!(
        f,
        "{} {} {} {}",
        a.approval_id,
        a.proposal_sha256,
        crate::now_stamp(),
        if a.label.is_empty() { "-" } else { "labeled" }
    )
    .map_err(|e| format!("ledger: {e}"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub staging_copy: PathBuf,
    pub tree: String,
}

/// Full apply: refusals first (live paths, replay), then authorization
/// against the source's current commit, then a fresh staging copy at that
/// commit, `git apply --index`, and a tree comparison (reversed on
/// mismatch). The source tree itself is never modified.
#[allow(clippy::too_many_arguments)]
pub fn apply_staged(
    t: &ApplyTarget,
    signed_record: &Value,
    sandbox_vk: &VerifyingKey,
    p: &Proposal,
    approval: &Value,
    owner_vk: &VerifyingKey,
    now: i64,
) -> Result<Applied, String> {
    refuse_live(t)?; // no process has been started before this line
    if approval.is_null() || approval["sig"].as_str().unwrap_or("").is_empty() {
        return Err("no owner approval".into());
    }
    if ledger_has(&t.ledger, &approval_id(approval))? {
        return Err("replay refused: this approval was already used".into());
    }
    let head = crate::git::head(&t.source)?;
    let auth = authorize(signed_record, sandbox_vk, p, approval, owner_vk, &head, now)?;
    ledger_add(&t.ledger, &auth)?; // consumed even if the apply fails below
    apply_to_copy(t, &auth, p)
}

/// Make the staging copy and apply there. Public for tests.
pub fn apply_to_copy(t: &ApplyTarget, auth: &Authorized, p: &Proposal) -> Result<Applied, String> {
    refuse_live(t)?;
    if auth.proposal_sha256 != p.sha256() {
        return Err("authorization is for a different proposal".into());
    }
    std::fs::create_dir_all(&t.staging_root).map_err(|e| format!("staging root: {e}"))?;
    let mut r = [0u8; 4];
    getrandom::getrandom(&mut r).map_err(|e| e.to_string())?;
    let copy = t.staging_root.join(format!("{}-{}", p.id, crate::hex(&r)));
    let cs = copy.display().to_string();
    crate::git::git(
        &t.source,
        &["worktree", "add", "--detach", &cs, &auth.base_commit],
        None,
    )?;
    let head = crate::git::head(&copy)?;
    if head != auth.base_commit {
        return Err("staging copy is not at the approved commit".into());
    }
    let patch = p.patch.as_bytes();
    crate::git::git(&copy, &["apply", "--check", "--index", "-"], Some(patch))?;
    crate::git::git(&copy, &["apply", "--index", "-"], Some(patch))?;
    let tree = crate::git::git(&copy, &["write-tree"], None)?
        .trim()
        .to_string();
    if tree != auth.result_tree {
        let _ = crate::git::git(&copy, &["apply", "-R", "--index", "-"], Some(patch));
        return Err(format!(
            "applied tree differs from the sandbox-tested tree; reversed in {}",
            copy.display()
        ));
    }
    Ok(Applied {
        staging_copy: copy,
        tree,
    })
}
