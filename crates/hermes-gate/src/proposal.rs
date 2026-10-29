// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The only thing Hermes may hand over: a proposal. A unified diff against a
//! named base commit, plus who/what made it. No branch, no push, no file
//! written outside the proposal outbox.

use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// `[A-Za-z0-9_-]{1,64}`.
    pub id: String,
    /// Full 40-hex commit the patch applies to.
    pub base_commit: String,
    /// `git diff` output (text only; binary patches are refused by checks).
    pub patch: String,
    /// One line from Hermes: what and why.
    pub summary: String,
    /// Model tier that wrote it (see [`crate::tier`]), recorded for audit.
    pub model_tier: String,
    pub created: String,
}

pub fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub fn commit_ok(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

impl Proposal {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "base_commit": self.base_commit,
            "summary": self.summary,
            "model_tier": self.model_tier,
            "created": self.created,
            "patch_sha256": crate::sha256_hex(self.patch.as_bytes()),
            "patch": self.patch,
        })
    }

    pub fn from_json(v: &Value) -> Result<Self, String> {
        let s = |k: &str| {
            v[k].as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("proposal: missing {k}"))
        };
        let p = Proposal {
            id: s("id")?,
            base_commit: s("base_commit")?,
            patch: s("patch")?,
            summary: s("summary")?,
            model_tier: s("model_tier").unwrap_or_else(|_| "unknown".into()),
            created: s("created").unwrap_or_default(),
        };
        if !id_ok(&p.id) {
            return Err("proposal: bad id".into());
        }
        if !commit_ok(&p.base_commit) {
            return Err("proposal: base_commit must be a full lowercase sha".into());
        }
        if let Some(h) = v["patch_sha256"].as_str() {
            if h != crate::sha256_hex(p.patch.as_bytes()) {
                return Err("proposal: patch_sha256 does not match the patch".into());
            }
        }
        Ok(p)
    }

    /// Identity of the proposal: sha256 over id, base and patch.
    pub fn sha256(&self) -> String {
        crate::sha256_hex(
            format!(
                "hermes-gate/proposal/v1\n{}\n{}\n{}",
                self.id, self.base_commit, self.patch
            )
            .as_bytes(),
        )
    }

    /// Paths touched by the patch (`diff --git a/X b/Y` -> X and Y,
    /// `+++`/`---` lines, renames and copies).
    pub fn files(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        let mut add = |p: &str| {
            let p = p.trim();
            if p.is_empty() || p == "/dev/null" {
                return;
            }
            let p = p
                .strip_prefix("a/")
                .or_else(|| p.strip_prefix("b/"))
                .unwrap_or(p)
                .to_string();
            if !out.contains(&p) {
                out.push(p);
            }
        };
        for l in self.patch.lines() {
            if let Some(r) = l.strip_prefix("diff --git ") {
                for part in r.split_whitespace() {
                    add(part);
                }
            } else if let Some(r) = l.strip_prefix("+++ ").or_else(|| l.strip_prefix("--- ")) {
                add(r.split('\t').next().unwrap_or(r));
            } else if let Some(r) = l
                .strip_prefix("rename from ")
                .or_else(|| l.strip_prefix("rename to "))
                .or_else(|| l.strip_prefix("copy from "))
                .or_else(|| l.strip_prefix("copy to "))
            {
                add(r);
            }
        }
        out
    }

    /// Added lines (without the leading `+`), file headers excluded.
    pub fn added_lines(&self) -> impl Iterator<Item = &str> {
        self.patch
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
    }

    /// Build a proposal from patch text (e.g. extracted from a Hermes chat
    /// reply) against a named base commit. Nothing is applied here.
    pub fn from_patch(
        base_commit: &str,
        patch: &str,
        id: &str,
        summary: &str,
        tier: &str,
    ) -> Result<Proposal, String> {
        let mut patch = patch.replace("\r\n", "\n");
        if !patch.ends_with('\n') {
            patch.push('\n');
        }
        if patch.trim().is_empty() {
            return Err("nothing to propose: empty patch".into());
        }
        let p = Proposal {
            id: id.into(),
            base_commit: base_commit.into(),
            patch,
            summary: summary
                .chars()
                .filter(|c| !c.is_control())
                .take(300)
                .collect(),
            model_tier: tier.into(),
            created: crate::now_stamp(),
        };
        if !id_ok(&p.id) {
            return Err("bad proposal id".into());
        }
        if !commit_ok(&p.base_commit) {
            return Err("base_commit must be a full lowercase sha".into());
        }
        Ok(p)
    }

    /// Build a proposal from the uncommitted changes of a work tree (what
    /// Hermes's sandboxed `fs_write` produced), against its HEAD. Files are
    /// not changed; new files are only marked intent-to-add in the index.
    pub fn from_worktree(
        wt: &Path,
        id: &str,
        summary: &str,
        tier: &str,
    ) -> Result<Proposal, String> {
        let base = crate::git::head(wt)?;
        crate::git::git(wt, &["add", "-A", "--intent-to-add"], None)?;
        let patch = crate::git::git(wt, &["diff", "--no-color", "--no-ext-diff", "HEAD"], None)?;
        if patch.trim().is_empty() {
            return Err("nothing to propose: the work tree has no changes".into());
        }
        let p = Proposal {
            id: id.into(),
            base_commit: base,
            patch,
            summary: summary.into(),
            model_tier: tier.into(),
            created: crate::now_stamp(),
        };
        if !id_ok(&p.id) {
            return Err("bad proposal id".into());
        }
        Ok(p)
    }
}
