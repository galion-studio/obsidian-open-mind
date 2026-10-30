// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Self-modify through the sandbox gate: Hermes proposes, the sandbox checks
//! and signs, the owner approves, a separate deploy step installs.
//!
//! Roles and who holds which key:
//!
//! * **Hermes** (the `propose_self_change` tool, [`SelfModifyTool`]) writes a
//!   change into its OWN git worktree copy of the Hermes source, turns it into
//!   a patch and puts it in the queue as `<queue>/<id>/proposal.json` +
//!   `change.patch`. It holds only PUBLIC keys. It cannot approve, sign or
//!   deploy: those actions are refused, the tool refuses to start when it can
//!   read the gate's signing key, and nothing it writes verifies.
//! * **The sandbox gate runner** ([`gate_entry`], `hermes-self-modify gate`,
//!   run as the gate user) rebuilds base + patch in a fresh copy (never
//!   Hermes's worktree), runs the deny-list checks ([`check_patch`]) and the
//!   plan's stages (build, tests, clippy, deny-list tests) and signs
//!   `gate/record.json` with the gate key.
//! * **The owner (Slavik)** signs `approval.json` ([`approve`]) with the owner
//!   key, bound to the proposal, the signed record and the built binary.
//! * **Deploy** ([`deploy`], run as root by a person) verifies both
//!   signatures, installs the gate-built binary under a new versioned name,
//!   writes the drop-in (saving the previous one) and [`rollback`] undoes it.
//!   It never restarts anything itself.
//!
//! Never editable through this path: persona and config files (SOUL.md,
//! config.yaml), the gate and sandbox crates, the deny list, this module, its
//! CLI and tests, Cargo manifests and lock, build scripts, CI, unit files.

use crate::tools::{Tool, ToolCtx, ToolOutput};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const TOOL_NAME: &str = "propose_self_change";
pub const PROPOSAL_V: &str = "hermes-self-modify/proposal/v1";
pub const RECORD_V: &str = "hermes-self-modify/gate-record/v1";
pub const APPROVAL_V: &str = "hermes-self-modify/approval/v1";
pub const DEPLOY_V: &str = "hermes-self-modify/deploy/v1";
pub const MAX_PATCH_BYTES: usize = 256 * 1024;
pub const MAX_FILES: usize = 32;

/// Only these trees may change (Hermes's own code and docs).
pub const ALLOWED_PREFIXES: &[&str] = &[
    "crates/hermes-rs/",
    "crates/hermes-gateway/",
    "crates/hermes-tools/",
    "docs/",
];
/// Never, even inside an allowed tree: the gate, the sandbox, the guards and
/// this self-modify path itself (so a change cannot weaken what checks it).
pub const PROTECTED_PREFIXES: &[&str] = &[
    ".git/",
    ".github/",
    ".cargo/",
    "deploy/",
    "secrets/",
    "crates/hermes-gate/",
    "crates/hades-gate/",
    "crates/openmind-sandbox/",
    "crates/galion-self-modify/",
    "crates/galion-guard/",
    "crates/hermes-rs/src/denylist/",
    "crates/hermes-rs/src/self_modify.rs",
    "crates/hermes-rs/src/bin/hermes-self-modify.rs",
    "crates/hermes-rs/tests/self_modify.rs",
    "crates/hermes-rs/tests/denylist",
    "crates/hermes-tools/src/guard.rs",
    "crates/hermes-tools/src/config.rs",
    "crates/hermes-tools/src/exec.rs",
];
/// File names that are never changed, in any directory (case-insensitive).
pub const PROTECTED_NAMES: &[&str] = &[
    "soul.md",
    "config.yaml",
    "config.yml",
    "cargo.toml",
    "cargo.lock",
    "build.rs",
    "deny.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
];
pub const PROTECTED_SUFFIXES: &[&str] = &[
    ".service", ".conf", ".timer", ".socket", ".path", ".env", ".key", ".pem", ".seed", ".pub",
];
/// Added lines that reach for the persona, config, keys or this gate.
pub const PROTECTED_ADDED: &[(&str, &str)] = &[
    ("soul.md", "persona file"),
    ("config.yaml", "live config"),
    ("sandbox.seed", "gate signing key"),
    ("owner.pub", "owner key"),
    ("hermes-self-modify", "self-modify gate"),
    ("self_modify::", "self-modify gate"),
    ("hermes-gate", "sandbox gate"),
    ("galion-gate", "sandbox gate"),
    (concat!("-----", "begin"), "key material"),
];

// ---------- small helpers ----------

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("not hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

pub fn signing_key(hex_seed: &str) -> Result<SigningKey, String> {
    let b: [u8; 32] = unhex(hex_seed)?
        .try_into()
        .map_err(|_| "seed must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&b))
}

pub fn verifying_key(hex_pub: &str) -> Result<VerifyingKey, String> {
    let b: [u8; 32] = unhex(hex_pub)?
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&b).map_err(|e| e.to_string())
}

pub fn key_id(k: &VerifyingKey) -> String {
    hex(k.as_bytes())[..16].to_string()
}

/// A fresh key pair as (seed hex, public hex). Used by `keygen` and tests.
pub fn keygen() -> Result<(String, String), String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    let sk = SigningKey::from_bytes(&b);
    Ok((hex(&b), hex(sk.verifying_key().as_bytes())))
}

fn now() -> String {
    crate::time::zagreb_rfc3339(crate::time::now_secs())
}

fn read(p: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn read_json(p: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read(p)?).map_err(|e| format!("{}: {e}", p.display()))
}

/// Write via a temp file + rename so readers never see half a file.
fn write_atomic(p: &Path, body: &[u8]) -> Result<(), String> {
    let tmp = p.with_extension("tmp-write");
    std::fs::write(&tmp, body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("{}: {e}", p.display()))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "git {}: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).to_string())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn new_id() -> Result<String, String> {
    let mut b = [0u8; 3];
    getrandom::getrandom(&mut b).map_err(|e| format!("no randomness: {e}"))?;
    let t: String = now()
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(14)
        .collect();
    Ok(format!("sm-{t}-{}", hex(&b)))
}

// ---------- deny-list checks on a path / patch ----------

/// Why a repo-relative path may not be changed, or None when it may.
pub fn path_refusal(p: &str) -> Option<String> {
    if p.is_empty() || p.starts_with('/') || p.starts_with('~') {
        return Some(format!("{p}: must be a relative path inside the repo"));
    }
    if p.split('/').any(|c| c.is_empty() || c == "." || c == "..")
        || p.bytes().any(|c| c < 0x20 || c == b'\\' || c == b'"')
    {
        return Some(format!("{p}: bad path component"));
    }
    let l = p.to_ascii_lowercase();
    if let Some(d) = PROTECTED_PREFIXES.iter().find(|d| l.starts_with(*d)) {
        return Some(format!("{p}: protected (the gate or a guard: {d})"));
    }
    let name = l.rsplit('/').next().unwrap_or("");
    if PROTECTED_NAMES.contains(&name) {
        return Some(format!("{p}: protected file ({name})"));
    }
    if let Some(s) = PROTECTED_SUFFIXES.iter().find(|s| l.ends_with(*s)) {
        return Some(format!("{p}: protected file type ({s})"));
    }
    if !ALLOWED_PREFIXES.iter().any(|a| l.starts_with(a)) {
        return Some(format!("{p}: outside Hermes's own code"));
    }
    None
}

/// Files a unified git diff touches (both sides of renames).
pub fn patch_files(patch: &str) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    for l in patch.lines() {
        let f = if let Some(r) = l.strip_prefix("diff --git ") {
            r.split(' ')
                .filter_map(|x| x.strip_prefix("a/").or_else(|| x.strip_prefix("b/")))
                .map(String::from)
                .collect::<Vec<_>>()
        } else if let Some(r) = l
            .strip_prefix("+++ b/")
            .or_else(|| l.strip_prefix("--- a/"))
            .or_else(|| l.strip_prefix("rename from "))
            .or_else(|| l.strip_prefix("rename to "))
        {
            vec![r.to_string()]
        } else {
            vec![]
        };
        for x in f {
            if !v.contains(&x) {
                v.push(x);
            }
        }
    }
    v
}

/// The deny-list checks every change must pass (run by the tool for quick
/// feedback and again by the gate runner, which is the one that counts).
pub fn check_patch(patch: &str) -> Vec<Value> {
    let files = patch_files(patch);
    let mut out = vec![];
    let mut push = |name: &str, bad: Vec<String>| {
        out.push(json!({"name": name, "ok": bad.is_empty(),
            "detail": if bad.is_empty() { "ok".to_string() } else { bad.join("; ") }}));
    };
    let mut size = vec![];
    if patch.len() > MAX_PATCH_BYTES {
        size.push(format!("patch {} B > {MAX_PATCH_BYTES} B", patch.len()));
    }
    if files.is_empty() {
        size.push("patch touches no file".into());
    }
    if files.len() > MAX_FILES {
        size.push(format!("{} files > {MAX_FILES}", files.len()));
    }
    push("size", size);
    push(
        "paths",
        files.iter().filter_map(|f| path_refusal(f)).collect(),
    );
    let mut text = vec![];
    for l in patch.lines() {
        // Split literals so this file does not trip the gate's own text_only scan.
        if l.starts_with(concat!("GIT binary", " patch"))
            || l.starts_with(concat!("Binary", " files "))
        {
            text.push("binary change".to_string());
        }
        if (l.starts_with("new mode ") || l.starts_with("new file mode ")) && !l.ends_with("100644")
        {
            text.push(format!("{l} (only plain 644 files)"));
        }
        if l.starts_with("deleted file mode ") {
            text.push("deletes a file (not allowed through self-modify)".into());
        }
    }
    text.dedup();
    push("text_only", text);
    let mut live = vec![];
    for l in patch.lines() {
        if l.starts_with('+') && !l.starts_with("+++") {
            let low = l.to_ascii_lowercase();
            for (m, why) in PROTECTED_ADDED {
                let s = format!("added line reaches for the {why}");
                if low.contains(m) && !live.contains(&s) {
                    live.push(s);
                }
            }
        }
    }
    push("protected_targets", live);
    out
}

pub fn checks_ok(c: &[Value]) -> bool {
    !c.is_empty() && c.iter().all(|x| x["ok"] == true)
}

fn failed(c: &[Value]) -> String {
    c.iter()
        .filter(|x| x["ok"] != true)
        .map(|x| {
            format!(
                "{}: {}",
                x["name"].as_str().unwrap_or(""),
                x["detail"].as_str().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

// ---------- signed envelopes ----------

fn sign_env(domain: &str, body: &Value, field: &str, sk: &SigningKey) -> Value {
    let text = body.to_string();
    let sig = sk.sign(format!("{domain}\n{text}").as_bytes());
    json!({ field: text, "sig": hex(&sig.to_bytes()), "key_id": key_id(&sk.verifying_key()) })
}

/// Verify an envelope; returns (inner JSON, sha256 of the inner text).
fn verify_env(
    domain: &str,
    env: &Value,
    field: &str,
    vk: &VerifyingKey,
) -> Result<(Value, String), String> {
    let text = env[field].as_str().ok_or(format!("{field} missing"))?;
    let sig: [u8; 64] = unhex(env["sig"].as_str().ok_or("sig missing")?)?
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    vk.verify(
        format!("{domain}\n{text}").as_bytes(),
        &Signature::from_bytes(&sig),
    )
    .map_err(|_| {
        format!("bad signature on {field} (tampered, or not signed by the expected key)")
    })?;
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if v["version"] != domain {
        return Err(format!("{field}: unknown version"));
    }
    Ok((v, sha256_hex(text.as_bytes())))
}

// ---------- queue entry ----------

/// One queued change: `<queue>/<id>/`.
pub struct Entry {
    pub dir: PathBuf,
}

impl Entry {
    pub fn open(dir: &Path) -> Entry {
        Entry {
            dir: dir.to_path_buf(),
        }
    }
    pub fn proposal_path(&self) -> PathBuf {
        self.dir.join("proposal.json")
    }
    pub fn patch_path(&self) -> PathBuf {
        self.dir.join("change.patch")
    }
    pub fn record_path(&self) -> PathBuf {
        self.dir.join("gate").join("record.json")
    }
    pub fn approval_path(&self) -> PathBuf {
        self.dir.join("approval.json")
    }
    pub fn deploy_path(&self) -> PathBuf {
        self.dir.join("deploy.json")
    }
    pub fn proposal(&self) -> Result<(Value, String), String> {
        let b = read(&self.proposal_path())?;
        let v: Value = serde_json::from_slice(&b).map_err(|e| e.to_string())?;
        if v["version"] != PROPOSAL_V {
            return Err("proposal: unknown version".into());
        }
        Ok((v, sha256_hex(&b)))
    }
    /// The gate record, verified against the sandbox public key and bound to
    /// this entry's proposal and patch bytes.
    pub fn verified_record(&self, sandbox: &VerifyingKey) -> Result<(Value, String), String> {
        let (rec, rsha) = verify_env(
            RECORD_V,
            &read_json(&self.record_path())?,
            "record_json",
            sandbox,
        )?;
        let (_, psha) = self.proposal()?;
        if rec["proposal_sha256"] != psha.as_str() {
            return Err("gate record is for a different proposal".into());
        }
        if rec["patch_sha256"] != sha256_hex(&read(&self.patch_path())?).as_str() {
            return Err("change.patch changed after the gate ran".into());
        }
        Ok((rec, rsha))
    }

    /// Plain-English state, derived only from what verifies.
    pub fn state(&self, sandbox: &VerifyingKey, owner: &VerifyingKey) -> String {
        if self.proposal().is_err() {
            return "invalid: no readable proposal".into();
        }
        if !self.record_path().exists() {
            return "waiting for the sandbox gate".into();
        }
        let (rec, _) = match self.verified_record(sandbox) {
            Ok(r) => r,
            Err(e) => return format!("rejected: gate record does not verify ({e})"),
        };
        if rec["verdict"] != "pass" {
            return format!(
                "rejected by the sandbox gate: {}",
                rec["why"].as_str().unwrap_or("fail")
            );
        }
        if let Ok(d) = read_json(&self.deploy_path()) {
            return format!(
                "{} ({})",
                d["state"].as_str().unwrap_or("?"),
                d["binary"].as_str().unwrap_or("")
            );
        }
        match check_approval(self, sandbox, owner) {
            Ok(_) => "approved by the owner, waiting for deploy".into(),
            Err(_) if self.approval_path().exists() => {
                "pending owner approval (the approval file present does not verify)".into()
            }
            Err(_) => "gate PASS (signed); pending Slavik's approval".into(),
        }
    }
}

// ---------- Hermes side: propose ----------

#[derive(Clone, Debug)]
pub struct Cfg {
    /// Hermes's own clone of the Hermes source (worktrees are made from it).
    pub repo: PathBuf,
    pub queue: PathBuf,
    /// Base revision changes are made against.
    pub base: String,
    pub sandbox_pub: VerifyingKey,
    /// None until Slavik installs his public key: nothing can be approved.
    pub owner_pub: Option<VerifyingKey>,
    /// The gate's signing key path: Hermes must NOT be able to read it.
    pub sandbox_key_path: PathBuf,
    /// Command that hands the entry to the gate (a sudo rule or a request
    /// file for a path unit); `--entry <dir>` is appended. None = the entry
    /// waits for the runner.
    pub gate_cmd: Option<Vec<String>>,
    /// When Hermes runs as root: do the propose work as this unprivileged
    /// uid:gid through `<cli> propose` (HERMES_RS_SELF_MODIFY_RUN_AS).
    pub run_as: Option<(u32, u32)>,
    /// The hermes-self-modify CLI (HERMES_RS_SELF_MODIFY_CLI), needed with run_as.
    pub cli: Option<PathBuf>,
    /// HERMES_RS_SELF_MODIFY_* settings handed to the run-as child.
    pub child_env: Vec<(String, String)>,
}

fn parse_run_as(s: &str) -> Result<(u32, u32), String> {
    let (u, g) = s.split_once(':').ok_or("RUN_AS must be uid:gid")?;
    let n = |x: &str| {
        x.trim()
            .parse::<u32>()
            .map_err(|_| "RUN_AS must be uid:gid")
    };
    let (u, g) = (n(u)?, n(g)?);
    if u == 0 || g == 0 {
        return Err("RUN_AS must not be root".into());
    }
    Ok((u, g))
}

impl Cfg {
    /// Off unless HERMES_RS_SELF_MODIFY_REPO is set. Errors fail closed (the
    /// tool is not registered).
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Result<Option<Cfg>, String> {
        let Some(repo) = env("HERMES_RS_SELF_MODIFY_REPO").filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let need = |k: &str| {
            env(k)
                .filter(|s| !s.is_empty())
                .ok_or(format!("{k} is required"))
        };
        let pubk = |p: &str| -> Result<VerifyingKey, String> {
            verifying_key(&String::from_utf8_lossy(&read(Path::new(p))?))
        };
        let owner_pub = match env("HERMES_RS_SELF_MODIFY_OWNER_PUB").filter(|s| !s.is_empty()) {
            Some(p) if Path::new(&p).exists() => Some(pubk(&p)?),
            _ => None,
        };
        let child_env = [
            "REPO",
            "QUEUE",
            "BASE",
            "SANDBOX_PUB",
            "OWNER_PUB",
            "SANDBOX_KEY",
            "GATE_CMD",
        ]
        .iter()
        .map(|k| format!("HERMES_RS_SELF_MODIFY_{k}"))
        .filter_map(|k| env(&k).map(|v| (k, v)))
        .collect();
        let cfg = Cfg {
            repo: PathBuf::from(repo),
            queue: PathBuf::from(need("HERMES_RS_SELF_MODIFY_QUEUE")?),
            base: env("HERMES_RS_SELF_MODIFY_BASE").unwrap_or_else(|| "HEAD".into()),
            sandbox_pub: pubk(&need("HERMES_RS_SELF_MODIFY_SANDBOX_PUB")?)?,
            owner_pub,
            sandbox_key_path: PathBuf::from(need("HERMES_RS_SELF_MODIFY_SANDBOX_KEY")?),
            gate_cmd: env("HERMES_RS_SELF_MODIFY_GATE_CMD")
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.split_whitespace().map(String::from).collect()),
            run_as: match env("HERMES_RS_SELF_MODIFY_RUN_AS").filter(|s| !s.is_empty()) {
                Some(s) => Some(parse_run_as(&s)?),
                None => None,
            },
            cli: env("HERMES_RS_SELF_MODIFY_CLI")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            child_env,
        };
        cfg.preflight()?;
        Ok(Some(cfg))
    }

    /// Refuse to run when Hermes (or its run-as user) could sign for itself.
    pub fn preflight(&self) -> Result<(), String> {
        if std::fs::File::open(&self.sandbox_key_path).is_ok() {
            return Err(format!(
                "refusing: this process can read the gate signing key {} (it must belong to the gate user only)",
                self.sandbox_key_path.display()
            ));
        }
        if self.owner_pub == Some(self.sandbox_pub) {
            return Err("refusing: the gate key and the owner key are the same".into());
        }
        if let Some((u, g)) = self.run_as {
            let st = self
                .child(u, g, &["can-read"])?
                .arg(&self.sandbox_key_path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|e| format!("refusing: run-as check failed: {e}"))?;
            match st.code() {
                Some(3) => {}
                Some(0) => {
                    return Err(format!(
                        "refusing: the run-as user {u}:{g} can read the gate signing key {}",
                        self.sandbox_key_path.display()
                    ))
                }
                c => return Err(format!("refusing: run-as key check exit {c:?}")),
            }
        }
        Ok(())
    }

    /// `<cli> args..` as uid:gid with a clean environment.
    fn child(&self, u: u32, g: u32, args: &[&str]) -> Result<Command, String> {
        use std::os::unix::process::CommandExt;
        let cli = self
            .cli
            .as_ref()
            .ok_or("HERMES_RS_SELF_MODIFY_CLI is required with RUN_AS")?;
        let mut c = Command::new(cli);
        c.args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.queue)
            .uid(u)
            .gid(g)
            .stdin(Stdio::null());
        Ok(c)
    }

    /// Entry state; with no owner key installed nothing counts as approved.
    pub fn state(&self, e: &Entry) -> String {
        match &self.owner_pub {
            Some(o) => e.state(&self.sandbox_pub, o),
            None => {
                let s = e.state(&self.sandbox_pub, &self.sandbox_pub);
                if s.starts_with("gate PASS")
                    || s.starts_with("approved")
                    || s.starts_with("pending owner")
                {
                    "gate PASS (signed); pending Slavik's approval (owner key not installed yet)"
                        .into()
                } else {
                    s
                }
            }
        }
    }
}

pub struct SelfModifyTool {
    pub cfg: Cfg,
}

impl SelfModifyTool {
    pub fn new(cfg: Cfg) -> Self {
        SelfModifyTool { cfg }
    }

    fn entry(&self, id: &str) -> Result<Entry, String> {
        if !valid_id(id) {
            return Err("bad id".into());
        }
        Ok(Entry::open(&self.cfg.queue.join(id)))
    }

    /// Write the change in a fresh worktree, enqueue it, run the gate.
    pub fn propose(&self, args: &Value) -> Result<Value, String> {
        let summary = args["summary"].as_str().unwrap_or("").trim();
        if summary.is_empty() || summary.len() > 500 {
            return Err("summary is required (max 500 chars)".into());
        }
        let files = args["files"]
            .as_array()
            .ok_or("files: [{path, content}] is required")?;
        if files.is_empty() || files.len() > MAX_FILES {
            return Err(format!("files: 1..={MAX_FILES} entries"));
        }
        let mut edits = vec![];
        for f in files {
            let p = f["path"].as_str().ok_or("each file needs a path")?;
            let c = f["content"]
                .as_str()
                .ok_or("each file needs content (full new text)")?;
            if let Some(why) = path_refusal(p) {
                return Err(format!("refused: {why}"));
            }
            edits.push((p.to_string(), c.to_string()));
        }
        let base = git(
            &self.cfg.repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", self.cfg.base),
            ],
        )?
        .trim()
        .to_string();
        let id = new_id()?;
        let e = self.entry(&id)?;
        std::fs::create_dir_all(&self.cfg.queue).map_err(|x| x.to_string())?;
        std::fs::create_dir(&e.dir).map_err(|x| format!("{}: {x}", e.dir.display()))?;
        let wt = e.dir.join("worktree");
        let wts = wt.to_string_lossy().to_string();
        git(
            &self.cfg.repo,
            &["worktree", "add", "-q", "--detach", &wts, &base],
        )?;
        let root = wt.canonicalize().map_err(|x| x.to_string())?;
        for (p, c) in &edits {
            let dst = wt.join(p);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|x| x.to_string())?;
                let cp = parent.canonicalize().map_err(|x| x.to_string())?;
                if !cp.starts_with(&root) {
                    return Err(format!("refused: {p} resolves outside the worktree"));
                }
            }
            if dst
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                return Err(format!("refused: {p} is a symlink"));
            }
            std::fs::write(&dst, c).map_err(|x| format!("{p}: {x}"))?;
        }
        git(&wt, &["add", "-A"])?;
        let patch = git(
            &wt,
            &[
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "--full-index",
                "--binary",
                &base,
            ],
        )?;
        if patch.trim().is_empty() {
            return Err("the change is empty (files already have that content)".into());
        }
        let checks = check_patch(&patch);
        if !checks_ok(&checks) {
            return Err(format!(
                "refused by the deny-list checks: {}",
                failed(&checks)
            ));
        }
        let prop = json!({
            "version": PROPOSAL_V, "id": id, "base_commit": base, "summary": summary,
            "reason": args["reason"].as_str().unwrap_or(""),
            "files": patch_files(&patch), "patch_sha256": sha256_hex(patch.as_bytes()),
            "proposed_by": "hermes", "created_at": now(),
        });
        std::fs::write(e.patch_path(), &patch).map_err(|x| x.to_string())?;
        write_atomic(&e.proposal_path(), format!("{prop}\n").as_bytes())?;
        let gate_note = match &self.cfg.gate_cmd {
            None => "queued; the sandbox gate runner will pick it up".to_string(),
            Some(argv) if !argv.is_empty() => {
                let o = Command::new(&argv[0])
                    .args(&argv[1..])
                    .arg("--entry")
                    .arg(&e.dir)
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|x| format!("gate command: {x}"))?;
                format!("gate exit {}", o.status.code().unwrap_or(-1))
            }
            Some(_) => "queued".to_string(),
        };
        Ok(json!({
            "id": id, "base_commit": base, "files": patch_files(&patch), "gate": gate_note,
            "state": self.cfg.state(&e),
            "next": "Only Slavik can approve; deploy is a separate owner step. Hermes cannot approve, sign or deploy.",
        }))
    }

    /// propose as the unprivileged run-as user: `<cli> propose` gets the
    /// same settings and the arguments on stdin, and prints the result.
    pub fn propose_as(&self, u: u32, g: u32, args: &Value) -> Result<Value, String> {
        use std::io::Write;
        let mut c = self.cfg.child(u, g, &["propose"])?;
        c.envs(self.cfg.child_env.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut ch = c.spawn().map_err(|e| format!("propose as {u}:{g}: {e}"))?;
        if let Some(mut i) = ch.stdin.take() {
            i.write_all(args.to_string().as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let o = ch.wait_with_output().map_err(|e| e.to_string())?;
        if !o.status.success() {
            let e = String::from_utf8_lossy(&o.stderr);
            return Err(e
                .trim()
                .trim_start_matches("hermes-self-modify: ")
                .to_string());
        }
        serde_json::from_slice(&o.stdout).map_err(|e| format!("propose output: {e}"))
    }

    fn status(&self, id: &str) -> Result<Value, String> {
        let e = self.entry(id)?;
        let (p, _) = e.proposal()?;
        let mut v = json!({"id": id, "summary": p["summary"], "files": p["files"],
            "state": self.cfg.state(&e)});
        if let Ok((rec, _)) = e.verified_record(&self.cfg.sandbox_pub) {
            v["gate"] = json!({"verdict": rec["verdict"], "why": rec["why"], "checks": rec["checks"], "stages": rec["stages"]});
        }
        Ok(v)
    }

    fn list(&self) -> Result<Value, String> {
        let mut out = vec![];
        if let Ok(rd) = std::fs::read_dir(&self.cfg.queue) {
            let mut names: Vec<String> = rd
                .flatten()
                .map(|d| d.file_name().to_string_lossy().to_string())
                .filter(|n| valid_id(n))
                .collect();
            names.sort();
            for n in names {
                let e = Entry::open(&self.cfg.queue.join(&n));
                out.push(json!({"id": n, "state": self.cfg.state(&e)}));
            }
        }
        Ok(json!({"changes": out}))
    }
}

impl Tool for SelfModifyTool {
    fn name(&self) -> String {
        TOOL_NAME.into()
    }
    fn description(&self) -> String {
        "Propose a change to your own Hermes source code (to adapt to the user). action=propose \
         writes the files (full new content, repo-relative paths) into a separate worktree copy, \
         then the sandbox gate builds, tests, runs clippy and the deny-list checks and signs the \
         result. A passing change waits for Slavik's approval; you cannot approve, sign or deploy \
         it. Never allowed: SOUL.md, config.yaml, the gate/sandbox/deny-list code, Cargo files. \
         action=status {id} or action=list shows where changes are."
            .into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "action": {"type": "string", "enum": ["propose", "status", "list"]},
            "summary": {"type": "string", "description": "What the change does and why (propose)."},
            "reason": {"type": "string", "description": "What the user needed that led to it."},
            "files": {"type": "array", "items": {"type": "object", "properties": {
                "path": {"type": "string"}, "content": {"type": "string"}},
                "required": ["path", "content"]}},
            "id": {"type": "string", "description": "Change id (status)."}
        }, "required": ["action"]})
    }
    fn call(&self, args: &Value, _ctx: &ToolCtx) -> ToolOutput {
        let action = args["action"].as_str().unwrap_or("");
        for k in [
            "approval",
            "signature",
            "sig",
            "record",
            "record_json",
            "approve",
            "deploy",
        ] {
            if !args[k].is_null() {
                return ToolOutput::err(format!(
                    "refused: Hermes cannot supply a {k}; approval and signing belong to Slavik and the sandbox gate"
                ));
            }
        }
        let r = match action {
            "propose" => match self.cfg.run_as {
                Some((u, g)) => self.propose_as(u, g, args),
                None => self.propose(args),
            },
            "status" => self.status(args["id"].as_str().unwrap_or("")),
            "list" => self.list(),
            "approve" | "sign" | "deploy" | "rollback" | "install" | "merge" => Err(format!(
                "refused: Hermes can never {action} its own change. Only Slavik approves (owner key), \
                 only the sandbox gate signs, and deploy is a separate owner step."
            )),
            other => Err(format!("unknown action {other:?} (propose, status, list)")),
        };
        match r {
            Ok(v) => ToolOutput::ok(v.to_string()),
            Err(e) => ToolOutput::err(e),
        }
    }
}

// ---------- gate runner side (gate user only) ----------

/// The sandbox stages, as JSON:
/// `{"stages":[{"name","argv":[..],"timeout_s"}], "wrapper":[..], "env":{..},
///   "pass_env":[..], "artifact": "target/debug/hermes-gateway", "target_dir": ".."}`
pub struct Plan {
    pub stages: Vec<(String, Vec<String>, u64)>,
    pub wrapper: Vec<String>,
    pub env: Vec<(String, String)>,
    pub pass_env: Vec<String>,
    pub artifact: String,
    pub target_dir: Option<PathBuf>,
}

impl Plan {
    pub fn from_json(v: &Value) -> Result<Plan, String> {
        let strs = |x: &Value| -> Vec<String> {
            x.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut stages = vec![];
        for s in v["stages"].as_array().ok_or("plan: stages missing")? {
            let argv = strs(&s["argv"]);
            if argv.is_empty() {
                return Err("plan: empty argv".into());
            }
            stages.push((
                s["name"].as_str().ok_or("plan: stage name")?.to_string(),
                argv,
                s["timeout_s"].as_u64().unwrap_or(1800),
            ));
        }
        if stages.is_empty() {
            return Err("plan: no stages".into());
        }
        Ok(Plan {
            stages,
            wrapper: strs(&v["wrapper"]),
            env: v["env"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, x)| x.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
                .unwrap_or_default(),
            pass_env: strs(&v["pass_env"]),
            artifact: v["artifact"]
                .as_str()
                .ok_or("plan: artifact missing")?
                .to_string(),
            target_dir: v["target_dir"].as_str().map(PathBuf::from),
        })
    }
}

fn run_stage(
    argv: &[String],
    plan: &Plan,
    cwd: &Path,
    target: &Path,
    log: &Path,
    timeout_s: u64,
) -> (i32, f64) {
    let t0 = crate::time::now_f64();
    let full: Vec<&String> = plan.wrapper.iter().chain(argv.iter()).collect();
    let Ok(out) = std::fs::File::create(log) else {
        return (-2, 0.0);
    };
    let err = out.try_clone();
    let mut c = Command::new(full[0]);
    c.args(&full[1..]).current_dir(cwd).env_clear();
    for k in &plan.pass_env {
        if let Ok(v) = std::env::var(k) {
            c.env(k, v);
        }
    }
    for (k, v) in &plan.env {
        c.env(k, v);
    }
    c.env("CARGO_TARGET_DIR", target)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out));
    if let Ok(e) = err {
        c.stderr(Stdio::from(e));
    }
    let Ok(mut ch) = c.spawn() else {
        return (-3, 0.0);
    };
    loop {
        match ch.try_wait() {
            Ok(Some(st)) => return (st.code().unwrap_or(-1), crate::time::now_f64() - t0),
            Ok(None) if crate::time::now_f64() - t0 > timeout_s as f64 => {
                let _ = ch.kill();
                let _ = ch.wait();
                return (-9, crate::time::now_f64() - t0);
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return (-4, crate::time::now_f64() - t0),
        }
    }
}

/// Re-check, rebuild base + patch in a fresh copy, run the stages, sign.
/// `repo` is the gate's own clone; Hermes's worktree is never built.
pub fn gate_entry(
    entry: &Path,
    repo: &Path,
    scratch: &Path,
    plan: &Plan,
    key: &SigningKey,
) -> Result<Value, String> {
    let e = Entry::open(entry);
    let (p, psha) = e.proposal()?;
    let patch_bytes = read(&e.patch_path())?;
    let patch =
        String::from_utf8(patch_bytes.clone()).map_err(|_| "patch is not UTF-8".to_string())?;
    let id = p["id"].as_str().unwrap_or("").to_string();
    let base = p["base_commit"].as_str().unwrap_or("").to_string();
    let mut checks = check_patch(&patch);
    let mut why = vec![];
    if !valid_id(&id) || base.len() != 40 || !base.bytes().all(|c| c.is_ascii_hexdigit()) {
        why.push("proposal id/base malformed".to_string());
    }
    if p["patch_sha256"] != sha256_hex(&patch_bytes).as_str() {
        why.push("patch does not match the proposal".into());
    }
    if p["proposed_by"] != "hermes" {
        why.push("proposal not from hermes".into());
    }
    let mut stages = vec![];
    let mut artifact = Value::Null;
    if checks_ok(&checks) && why.is_empty() {
        let copy = scratch.join(format!("{id}-{}", crate::time::now_secs()));
        std::fs::create_dir_all(scratch).map_err(|x| x.to_string())?;
        let cs = copy.to_string_lossy().to_string();
        git(repo, &["worktree", "add", "-q", "--detach", &cs, &base])?;
        let pf = scratch.join(format!("{id}.patch"));
        std::fs::write(&pf, &patch_bytes).map_err(|x| x.to_string())?;
        let applied = git(
            &copy,
            &["apply", "--whitespace=nowarn", &pf.to_string_lossy()],
        );
        checks.push(json!({"name": "applies_to_base", "ok": applied.is_ok(),
            "detail": applied.err().unwrap_or_else(|| "ok".into())}));
        if checks_ok(&checks) {
            let logs = e.dir.join("gate").join("logs");
            std::fs::create_dir_all(&logs).map_err(|x| x.to_string())?;
            let target = plan
                .target_dir
                .clone()
                .unwrap_or_else(|| copy.join("target"));
            for (name, argv, t) in &plan.stages {
                let log = logs.join(format!("{name}.log"));
                let (code, secs) = run_stage(argv, plan, &copy, &target, &log, *t);
                let lsha = read(&log).map(|b| sha256_hex(&b)).unwrap_or_default();
                stages.push(json!({"name": name, "exit": code, "ok": code == 0,
                    "secs": (secs * 10.0).round() / 10.0, "log_sha256": lsha}));
                if code != 0 {
                    why.push(format!("stage {name} failed (exit {code})"));
                    break;
                }
            }
            if why.is_empty() {
                let src = target.join(plan.artifact.trim_start_matches("target/"));
                let src = if src.exists() {
                    src
                } else {
                    copy.join(&plan.artifact)
                };
                let dst = e.dir.join("gate").join("artifact");
                match std::fs::copy(&src, &dst) {
                    Ok(n) => {
                        let sha = sha256_hex(&read(&dst)?);
                        artifact = json!({"name": plan.artifact, "bytes": n, "sha256": sha});
                    }
                    Err(x) => why.push(format!("artifact {}: {x}", src.display())),
                }
            }
        }
    }
    if !checks_ok(&checks) {
        why.insert(0, format!("checks failed: {}", failed(&checks)));
    }
    let verdict = if why.is_empty() { "pass" } else { "fail" };
    let rec = json!({
        "version": RECORD_V, "id": id, "proposal_sha256": psha,
        "patch_sha256": sha256_hex(&patch_bytes), "base_commit": base,
        "checks": checks, "stages": stages, "artifact": artifact,
        "verdict": verdict, "why": why.join("; "), "when": now(),
    });
    let signed = sign_env(RECORD_V, &rec, "record_json", key);
    std::fs::create_dir_all(e.dir.join("gate")).map_err(|x| x.to_string())?;
    write_atomic(&e.record_path(), format!("{signed}\n").as_bytes())?;
    Ok(rec)
}

// ---------- owner side ----------

/// Owner approval. Refused for Hermes, for the gate key, and for anything
/// but a verified gate PASS.
pub fn approve(
    entry: &Path,
    owner: &SigningKey,
    sandbox: &VerifyingKey,
    by: &str,
    ttl_s: i64,
) -> Result<Value, String> {
    let by = by.trim();
    if by.is_empty() || by.to_ascii_lowercase().contains("hermes") {
        return Err(
            "refused: only the owner approves; Hermes can never approve its own change".into(),
        );
    }
    if owner.verifying_key() == *sandbox {
        return Err(
            "refused: the gate key cannot approve (signing a pass is not an approval)".into(),
        );
    }
    let e = Entry::open(entry);
    let (rec, rsha) = e.verified_record(sandbox)?;
    if rec["verdict"] != "pass" {
        return Err(format!(
            "refused: the gate did not pass ({})",
            rec["why"].as_str().unwrap_or("")
        ));
    }
    let now_s = crate::time::now_secs();
    let body = json!({
        "version": APPROVAL_V, "id": rec["id"], "proposal_sha256": rec["proposal_sha256"],
        "record_sha256": rsha, "artifact_sha256": rec["artifact"]["sha256"],
        "approved_by": by, "approved_at": now(), "expires": now_s + ttl_s.clamp(60, 7 * 86_400),
    });
    let signed = sign_env(APPROVAL_V, &body, "approval_json", owner);
    write_atomic(&e.approval_path(), format!("{signed}\n").as_bytes())?;
    Ok(body)
}

/// Both signatures, bound together. Returns (record, approval).
pub fn check_approval(
    e: &Entry,
    sandbox: &VerifyingKey,
    owner: &VerifyingKey,
) -> Result<(Value, Value), String> {
    if owner == sandbox {
        return Err("owner key equals gate key".into());
    }
    let (rec, rsha) = e.verified_record(sandbox)?;
    if rec["verdict"] != "pass" {
        return Err("the gate did not pass".into());
    }
    let env = read_json(&e.approval_path()).map_err(|_| "no owner approval yet".to_string())?;
    let (ap, _) = verify_env(APPROVAL_V, &env, "approval_json", owner)?;
    if ap["record_sha256"] != rsha.as_str()
        || ap["proposal_sha256"] != rec["proposal_sha256"]
        || ap["artifact_sha256"] != rec["artifact"]["sha256"]
        || ap["id"] != rec["id"]
    {
        return Err("approval is for a different change".into());
    }
    if ap["expires"].as_i64().unwrap_or(0) < crate::time::now_secs() {
        return Err("approval expired".into());
    }
    if ap["approved_by"]
        .as_str()
        .unwrap_or("hermes")
        .to_ascii_lowercase()
        .contains("hermes")
    {
        return Err("approval names Hermes as approver".into());
    }
    Ok((rec, ap))
}

// ---------- deploy / rollback (root, by a person) ----------

pub struct DeployCfg {
    pub bin_dir: PathBuf,
    pub dropin_dir: PathBuf,
    /// File name of the drop-in, e.g. `99s-self-modify.conf`.
    pub dropin_name: String,
    /// Extra `[Service]` lines kept in the drop-in (Environment=...).
    pub extra_lines: Vec<String>,
}

/// Verify everything again, install the gate-built binary under a new name,
/// save the current drop-in and write the new one. Never restarts.
pub fn deploy(
    entry: &Path,
    sandbox: &VerifyingKey,
    owner: &VerifyingKey,
    d: &DeployCfg,
) -> Result<Value, String> {
    let e = Entry::open(entry);
    let (rec, ap) = check_approval(&e, sandbox, owner)?;
    if e.deploy_path().exists() {
        return Err("already deployed once; propose again for a new deploy".into());
    }
    let art = read(&e.dir.join("gate").join("artifact"))?;
    if rec["artifact"]["sha256"] != sha256_hex(&art).as_str() {
        return Err("the built binary changed after the gate signed it".into());
    }
    if d.dropin_name.contains('/') || !d.dropin_name.ends_with(".conf") {
        return Err("bad drop-in name".into());
    }
    let id = rec["id"].as_str().unwrap_or("x");
    let short = &rec["artifact"]["sha256"].as_str().unwrap_or("")[..12];
    let bin = d.bin_dir.join(format!("hermes-gateway-{id}-{short}"));
    std::fs::create_dir_all(&d.bin_dir).map_err(|x| x.to_string())?;
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&bin)
            .map_err(|x| format!("{}: {x}", bin.display()))?;
        f.write_all(&art).map_err(|x| x.to_string())?;
    }
    std::fs::write(
        bin.with_extension("sha256"),
        format!(
            "{}  {}\n",
            sha256_hex(&art),
            bin.file_name().unwrap_or_default().to_string_lossy()
        ),
    )
    .map_err(|x| x.to_string())?;
    let dropin = d.dropin_dir.join(&d.dropin_name);
    let rb = e.dir.join("rollback");
    std::fs::create_dir_all(&rb).map_err(|x| x.to_string())?;
    let had_prev = dropin.exists();
    if had_prev {
        std::fs::copy(&dropin, rb.join("previous-dropin")).map_err(|x| x.to_string())?;
    }
    let mut body = format!(
        "# hermes self-modify {id}: approved by {} at {}; gate record {}\n# Rollback: hermes-self-modify rollback --entry {}\n[Service]\nExecStart=\nExecStart={}\n",
        ap["approved_by"].as_str().unwrap_or(""),
        ap["approved_at"].as_str().unwrap_or(""),
        ap["record_sha256"].as_str().unwrap_or(""),
        e.dir.display(),
        bin.display()
    );
    for l in &d.extra_lines {
        body.push_str(l.trim());
        body.push('\n');
    }
    std::fs::create_dir_all(&d.dropin_dir).map_err(|x| x.to_string())?;
    write_atomic(&dropin, body.as_bytes())?;
    let rec_out = json!({
        "version": DEPLOY_V, "id": id, "state": "deployed", "binary": bin, "sha256": rec["artifact"]["sha256"],
        "dropin": dropin, "previous_dropin_saved": had_prev, "deployed_at": now(),
        "approved_by": ap["approved_by"],
        "next": "reload units and restart hermes-rs, then check /health (not done by this tool)",
    });
    write_atomic(&e.deploy_path(), format!("{rec_out}\n").as_bytes())?;
    Ok(rec_out)
}

/// Put the previous drop-in back (or move ours aside). The binary stays.
pub fn rollback(entry: &Path) -> Result<Value, String> {
    let e = Entry::open(entry);
    let mut d = read_json(&e.deploy_path())?;
    if d["state"] != "deployed" {
        return Err(format!("nothing to roll back (state {})", d["state"]));
    }
    let dropin = PathBuf::from(d["dropin"].as_str().ok_or("deploy record has no drop-in")?);
    let prev = e.dir.join("rollback").join("previous-dropin");
    if d["previous_dropin_saved"] == true {
        write_atomic(&dropin, &read(&prev)?)?;
    } else {
        let aside = dropin.with_extension(format!("conf.rolled-back-{}", crate::time::now_secs()));
        std::fs::rename(&dropin, &aside).map_err(|x| x.to_string())?;
    }
    d["state"] = json!("rolled_back");
    d["rolled_back_at"] = json!(now());
    write_atomic(&e.deploy_path(), format!("{d}\n").as_bytes())?;
    Ok(d)
}
