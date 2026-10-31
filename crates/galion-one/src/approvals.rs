// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Approvals screen for Hermes self-change requests (approval-system C,
//! 2026-10-31 CET), and the owner (admin) key setup.
//!
//! What this process does: read the self-change queue, show each request as
//! an approval card (plain words, diff, the signed sandbox-test and dry-run
//! results, the artifact fingerprint), hand out a one-time 30-minute
//! challenge `{artifact sha256, base commit, nonce, expiry}`, check Slavik's
//! on-device signature and store it as `<entry>/owner/approval.json` for the
//! ROOT deploy verifier (`galion-sm-deploy`, agent A).
//!
//! What it never does: deploy, restart, install a binary, or write
//! `/etc/hermes-self-modify/owner.pub`. The owner key is staged as an install
//! request for the root helper `galion-owner-key install`. It never holds a
//! private key: devices sign, this checks.
//!
//! Every route is for a paired device only (the galion_admin cookie); every
//! POST also needs a known Origin. Formats: `A-core/FORMATS.md` v1.

use galion_owner_key as ok;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const STACK_SIG_V: &str = "galion-sm/proposal-sig/v1";
pub const TEST_RECORD_V: &str = "galion-sm/test-record/v1";
pub const DRYRUN_RECORD_V: &str = "galion-sm/dryrun-record/v1";
pub const REJECT_V: &str = "galion-one/owner-reject/v1";
const DIFF_CAP: usize = 64 * 1024;
const READ_CAP: u64 = 4 * 1024 * 1024;
const ARTIFACT_CAP: u64 = 256 * 1024 * 1024;

static LOCK: Mutex<()> = Mutex::new(());

/// Where things live. Defaults are the VPS paths; every one is env-overridable.
#[derive(Clone, Debug)]
pub struct Cfg {
    pub queue: PathBuf,
    pub state: PathBuf,
    /// Write approvals to `<out>/<id>/owner/` instead of `<entry>/owner/`.
    pub out: Option<PathBuf>,
    pub owner_pub: PathBuf,
    pub sandbox_pub: PathBuf,
    pub stack_pub: PathBuf,
    pub rp_id: String,
    pub origins: Vec<String>,
}

fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

impl Cfg {
    pub fn from_env() -> Cfg {
        Cfg {
            queue: env_or("ONE_APPROVAL_QUEUE", "/var/lib/openmind-hermes/queue").into(),
            state: env_or("ONE_APPROVAL_STATE", "/var/lib/galion-one/approval").into(),
            out: std::env::var("ONE_APPROVAL_OUT")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from),
            owner_pub: env_or("ONE_OWNER_PUB", ok::OWNER_PUB).into(),
            sandbox_pub: env_or("ONE_SANDBOX_PUB", "/etc/hermes-self-modify/sandbox-v2.pub").into(),
            stack_pub: env_or("ONE_STACK_PUB", "/etc/hermes-self-modify/stack.pub").into(),
            rp_id: env_or("ONE_WEBAUTHN_RP_ID", "galion.app"),
            origins: env_or(
                "ONE_WEBAUTHN_ORIGINS",
                "https://galion.app,https://one.galion.app",
            )
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        }
    }
}

pub struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub origin: &'a str,
    pub paired: bool,
    pub now: i64,
}

pub struct Resp {
    pub status: u16,
    pub kind: &'static str,
    pub body: Vec<u8>,
}

fn j(status: u16, v: Value) -> Resp {
    Resp {
        status,
        kind: "application/json",
        body: v.to_string().into_bytes(),
    }
}

fn err(status: u16, msg: impl Into<String>) -> Resp {
    j(status, json!({"error": msg.into()}))
}

pub fn handles(path: &str) -> bool {
    path == "/approvals" || path.starts_with("/api/approvals") || path.starts_with("/api/owner-key")
}

/// The approvals page: the V2 style sheet plus this screen, built in.
pub const PAGE: &str = concat!(
    "<!doctype html>\n<html lang=\"en\" class=\"g1v2\">\n<head>\n<meta charset=\"utf-8\">\n",
    "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n",
    "<meta name=\"theme-color\" content=\"#05060f\">\n<meta name=\"color-scheme\" content=\"dark\">\n",
    "<meta name=\"referrer\" content=\"no-referrer\">\n<title>Approvals · Galion One</title>\n<style>\n",
    include_str!("../../../apps/galion-one-web/v2/style.css"),
    include_str!("../../../apps/galion-one-web/v2/approvals/approvals.css"),
    "</style>\n</head>\n<body>\n",
    include_str!("../../../apps/galion-one-web/v2/approvals/approvals.html"),
    "<script>\n",
    include_str!("../../../apps/galion-one-web/v2/approvals/approvals.js"),
    "</script>\n</body>\n</html>\n"
);

pub fn route(cfg: &Cfg, req: &Req) -> Resp {
    if req.method == "GET" && req.path == "/approvals" {
        // The page itself holds no data; it asks for pairing like the room.
        return Resp {
            status: 200,
            kind: "text/html; charset=utf-8",
            body: PAGE.as_bytes().to_vec(),
        };
    }
    if !req.paired {
        return err(401, "pair");
    }
    if req.method == "POST" && !cfg.origins.iter().any(|o| o == req.origin) {
        return err(403, "unknown origin");
    }
    let body: Value = if req.method == "POST" {
        match serde_json::from_slice(req.body) {
            Ok(v) => v,
            Err(_) => return err(400, "body must be JSON"),
        }
    } else {
        Value::Null
    };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let r = match (req.method, req.path) {
        ("GET", "/api/approvals") => Ok(list(cfg, req.now)),
        ("GET", "/api/approvals/count") => Ok(count(cfg, req.now)),
        ("POST", "/api/approvals/challenge") => challenge(cfg, &body, req.now),
        ("POST", "/api/approvals/approve") => approve(cfg, &body, req.now),
        ("POST", "/api/approvals/reject") => reject(cfg, &body, req.now),
        ("GET", "/api/owner-key") => Ok(key_status(cfg)),
        ("POST", "/api/owner-key/begin") => key_begin(cfg, &body, req.now),
        ("POST", "/api/owner-key/register") => key_register(cfg, &body, req.now),
        ("POST", "/api/owner-key/activate-challenge") => key_activate_challenge(cfg, req.now),
        ("POST", "/api/owner-key/activate") => key_activate(cfg, &body, req.now),
        _ => return err(404, "not found"),
    };
    match r {
        Ok(v) => j(200, v),
        Err(e) => err(409, e),
    }
}

// ---------- files ----------

/// Read a regular file, never through a symlink (the queue is written by
/// the Hermes account; this process may run as root).
fn read_plain(p: &Path, cap: u64) -> Result<Vec<u8>, String> {
    let m = fs::symlink_metadata(p).map_err(|_| format!("{} missing", name_of(p)))?;
    if !m.file_type().is_file() {
        return Err(format!("{} is not a regular file", name_of(p)));
    }
    if m.len() > cap {
        return Err(format!("{} is too large", name_of(p)));
    }
    let mut f = fs::File::open(p).map_err(|e| e.to_string())?;
    let mut v = Vec::new();
    Read::by_ref(&mut f)
        .take(cap)
        .read_to_end(&mut v)
        .map_err(|e| e.to_string())?;
    Ok(v)
}

fn name_of(p: &Path) -> String {
    let parts: Vec<_> = p.iter().rev().take(2).collect();
    parts
        .iter()
        .rev()
        .map(|s| s.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn read_json(p: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read_plain(p, READ_CAP)?)
        .map_err(|_| format!("{} is not JSON", name_of(p)))
}

fn plain_dir(p: &Path) -> bool {
    fs::symlink_metadata(p)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
}

/// Make `dir` (no symlinks) and write `name` atomically: create_new temp,
/// fsync, rename. An existing file is first kept as `<name>.off-<now>`.
fn write_atomic(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    mode: u32,
    now: i64,
) -> Result<PathBuf, String> {
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if !plain_dir(dir) {
        return Err(format!("{} is not a plain directory", dir.display()));
    }
    let dest = dir.join(name);
    if let Ok(m) = fs::symlink_metadata(&dest) {
        if !m.file_type().is_file() {
            return Err(format!("{name} is not a regular file"));
        }
        let mut n = 0;
        let mut off = dir.join(format!("{name}.off-{now}"));
        while off.exists() {
            n += 1;
            off = dir.join(format!("{name}.off-{now}-{n}"));
        }
        fs::rename(&dest, &off).map_err(|e| e.to_string())?;
    }
    let tmp = dir.join(format!(".{name}.new-{now}-{}", std::process::id()));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)
        .map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Move a file aside (one-time codes are used up by renaming, never deleted).
fn retire(from: &Path, to_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(to_dir).map_err(|e| e.to_string())?;
    let name = from.file_name().ok_or("no name")?;
    fs::rename(from, to_dir.join(name))
        .map_err(|_| "this one-time code was already used".to_string())
}

fn nonce() -> Result<String, String> {
    let mut b = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|_| "no randomness".to_string())?;
    Ok(ok::hex(&b))
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        && !id.starts_with('-')
}

fn when(secs: i64) -> String {
    galion_common::time::label(secs)
}

// ---------- the signed envelopes ----------

/// FORMATS section 0 envelope: verify `sig` over `domain\ninner`, parse inner.
fn open_env(
    env: &Value,
    field: &str,
    domain: &str,
    pubkey: Option<&ok::OwnerKey>,
) -> (Option<Value>, String, Result<(), String>) {
    let Some(inner) = env[field].as_str() else {
        return (None, String::new(), Err(format!("{field} missing")));
    };
    let sha = ok::sha256_hex(inner.as_bytes());
    let parsed: Option<Value> = serde_json::from_str(inner).ok();
    let checked = match pubkey {
        None => Err("public key not installed".to_string()),
        Some(k) => {
            let mut e = env.clone();
            e["scheme"] = json!(ok::SCHEME_ED25519);
            ok::verify_assertion(k, domain, inner, &e)
        }
    }
    .and_then(|_| match &parsed {
        Some(v) if v["version"] == domain => Ok(()),
        _ => Err("unknown version".into()),
    });
    (parsed, sha, checked)
}

/// Re-hash the logs a record names (as the root verifier does): plain relative
/// paths only, regular files only, bytes must match the signed sha256.
fn logs_ok(base: &Path, items: &Value) -> Result<(), String> {
    for x in items.as_array().cloned().unwrap_or_default() {
        let Some(rel) = x["log"].as_str() else {
            continue;
        };
        let rp = Path::new(rel);
        if rel.is_empty()
            || !rp
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("unsafe log path {rel:?}"));
        }
        let got = ok::sha256_hex(&read_plain(&base.join(rp), READ_CAP)?);
        if x["log_sha256"] != got.as_str() {
            return Err(format!("log {rel} changed after signing"));
        }
    }
    Ok(())
}

fn load_pub(p: &Path) -> Option<ok::OwnerKey> {
    let text = String::from_utf8(read_plain(p, 64 * 1024).ok()?).ok()?;
    match ok::OwnerKey::parse(&text).ok()? {
        k @ ok::OwnerKey::Ed25519 { .. } => Some(k),
        _ => None,
    }
}

pub fn active_owner_key(cfg: &Cfg) -> Result<Option<(ok::OwnerKey, String)>, String> {
    match fs::symlink_metadata(&cfg.owner_pub) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(_) => {
            let text = String::from_utf8(read_plain(&cfg.owner_pub, 64 * 1024)?)
                .map_err(|_| "owner.pub is not text".to_string())?;
            Ok(Some((ok::OwnerKey::parse(&text)?, text)))
        }
    }
}

// ---------- cards ----------

fn out_dir(cfg: &Cfg, id: &str) -> PathBuf {
    match &cfg.out {
        Some(o) => o.join(id).join("owner"),
        None => cfg.queue.join(id).join("owner"),
    }
}

fn patch_files(patch: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for l in patch.lines() {
        if let Some(f) = l.strip_prefix("+++ b/") {
            if !v.iter().any(|x| x == f) {
                v.push(f.to_string());
            }
        } else if let Some(rest) = l.strip_prefix("diff --git a/") {
            if let Some((_, b)) = rest.split_once(" b/") {
                if !v.iter().any(|x| x == b) {
                    v.push(b.to_string());
                }
            }
        }
    }
    v
}

fn plain_words(summary: &str, reason: &str, files: &[String]) -> String {
    let areas: Vec<&str> = files
        .iter()
        .map(|f| {
            let p: Vec<&str> = f.split('/').collect();
            if p.len() > 1 && p[0] == "crates" {
                p[1]
            } else {
                p[0]
            }
        })
        .fold(Vec::new(), |mut a, x| {
            if !a.contains(&x) {
                a.push(x);
            }
            a
        });
    let n = files.len();
    let mut s = format!(
        "Hermes wants to change {n} file{} in {}.",
        if n == 1 { "" } else { "s" },
        if areas.is_empty() {
            "its own code".to_string()
        } else {
            areas.join(", ")
        }
    );
    let summary = summary.trim();
    if !summary.is_empty() {
        s.push_str(&format!(" What it does: {summary}"));
        if !summary.ends_with('.') {
            s.push('.');
        }
    }
    let reason = reason.trim();
    if !reason.is_empty() {
        s.push_str(&format!(" Why: {reason}"));
        if !reason.ends_with('.') {
            s.push('.');
        }
    }
    s
}

/// Everything the card shows, plus whether Slavik may sign it now.
pub struct Card {
    pub id: String,
    pub json: Value,
    pub ready: bool,
    pub artifact_sha256: String,
    pub base_commit: String,
    pub test_record_sha256: String,
    pub dryrun_record_sha256: String,
}

pub fn card(cfg: &Cfg, id: &str, owner: Option<&ok::OwnerKey>, now: i64) -> Result<Card, String> {
    if !safe_id(id) {
        return Err("bad id".into());
    }
    let dir = cfg.queue.join(id);
    if !plain_dir(&dir) {
        return Err("no such request".into());
    }
    let mut blockers: Vec<String> = Vec::new();
    let proposal_bytes = read_plain(&dir.join("proposal.json"), READ_CAP)?;
    let proposal: Value = serde_json::from_slice(&proposal_bytes)
        .map_err(|_| "proposal.json is not JSON".to_string())?;
    let patch_bytes = read_plain(&dir.join("change.patch"), READ_CAP).unwrap_or_default();
    let patch = String::from_utf8_lossy(&patch_bytes).into_owned();
    let files = {
        let f = patch_files(&patch);
        if f.is_empty() {
            proposal["files"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            f
        }
    };
    let summary = proposal["summary"].as_str().unwrap_or_default();
    let reason = proposal["reason"].as_str().unwrap_or_default();
    let base_commit = proposal["base_commit"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let proposal_sha = ok::sha256_hex(&proposal_bytes);
    let patch_sha = ok::sha256_hex(&patch_bytes);

    // 1. Stack signature (origin proof from openmind-hermes).
    let stack_key = load_pub(&cfg.stack_pub);
    let mut stack_sig_sha = String::new();
    let stack = match read_json(&dir.join("proposal.stack-sig.json")) {
        Err(e) => json!({"ok": false, "why": e}),
        Ok(env) => {
            let (b, ssha, chk) = open_env(&env, "body_json", STACK_SIG_V, stack_key.as_ref());
            stack_sig_sha = ssha;
            let b = b.unwrap_or(Value::Null);
            let bound = b["proposal_sha256"] == proposal_sha.as_str()
                && b["patch_sha256"] == patch_sha.as_str()
                && b["id"] == id
                && b["base_commit"] == base_commit.as_str();
            match chk {
                Err(e) => json!({"ok": false, "why": format!("stack signature: {e}")}),
                Ok(()) if !bound => {
                    json!({"ok": false, "why": "stack signature is for different files"})
                }
                Ok(()) => {
                    json!({"ok": true, "why": "signed by the OpenMind stack key", "key_id": env["key_id"]})
                }
            }
        }
    };
    if stack["ok"] != true {
        blockers.push(stack["why"].as_str().unwrap_or("stack").to_string());
    }

    // 2. Sandbox test record.
    let sandbox_key = load_pub(&cfg.sandbox_pub);
    let mut test_sha = String::new();
    let mut artifact_sha = String::new();
    let mut artifact_bytes = 0u64;
    let test = match read_json(&dir.join("test").join("record.json")) {
        Err(_) => {
            json!({"state": "waiting", "ok": false, "why": "the sandbox test has not finished"})
        }
        Ok(env) => {
            let (b, sha, chk) = open_env(&env, "record_json", TEST_RECORD_V, sandbox_key.as_ref());
            test_sha = sha;
            let b = b.unwrap_or(Value::Null);
            let chk = chk.and_then(|_| logs_ok(&dir.join("test"), &b["stages"]));
            artifact_sha = b["artifact"]["sha256"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            artifact_bytes = b["artifact"]["bytes"].as_u64().unwrap_or(0);
            let bound = b["id"] == id
                && b["proposal_sha256"] == proposal_sha.as_str()
                && b["proposal_sig_sha256"] == stack_sig_sha.as_str()
                && b["patch_sha256"] == patch_sha.as_str()
                && b["base_commit"] == base_commit.as_str();
            let stages: Vec<Value> = b["stages"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|s| json!({"name": s["name"], "ok": s["ok"], "exit": s["exit"]}))
                .collect();
            let pass = b["verdict"] == "pass";
            let (state, okk, why) = match (&chk, bound, pass) {
                (Err(e), _, _) => ("bad", false, format!("test record signature: {e}")),
                (Ok(()), false, _) => (
                    "bad",
                    false,
                    "test record is for a different proposal".into(),
                ),
                (Ok(()), true, false) => (
                    "fail",
                    false,
                    format!("tests failed: {}", b["why"].as_str().unwrap_or("")),
                ),
                (Ok(()), true, true) => (
                    "pass",
                    true,
                    "passed in the sandbox, signed by the sandbox key".into(),
                ),
            };
            json!({"state": state, "ok": okk, "why": why, "stages": stages,
                   "signed_at": b["signed_at"], "when": b["signed_at"].as_i64().map(when)})
        }
    };
    if test["ok"] != true {
        blockers.push(test["why"].as_str().unwrap_or("test").to_string());
    }

    // 3. Dry-run record.
    let mut dry_sha = String::new();
    let dryrun = match read_json(&dir.join("dryrun").join("record.json")) {
        Err(_) => json!({"state": "waiting", "ok": false, "why": "the dry run has not finished"}),
        Ok(env) => {
            let (b, sha, chk) =
                open_env(&env, "record_json", DRYRUN_RECORD_V, sandbox_key.as_ref());
            dry_sha = sha;
            let b = b.unwrap_or(Value::Null);
            let chk = chk.and_then(|_| logs_ok(&dir.join("dryrun"), &b["checks"]));
            let bound = b["id"] == id
                && b["test_record_sha256"] == test_sha.as_str()
                && b["artifact_sha256"] == artifact_sha.as_str()
                && b["base_commit"] == base_commit.as_str();
            let pass = b["verdict"] == "pass";
            let real = b["model_calls_real"].as_i64().unwrap_or(-1);
            let checks: Vec<Value> = b["checks"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|c| json!({"name": c["name"], "ok": c["ok"], "detail": c["detail"]}))
                .collect();
            let (state, okk, why) = match (&chk, bound, pass, real) {
                (Err(e), _, _, _) => ("bad", false, format!("dry-run record signature: {e}")),
                (Ok(()), false, _, _) => ("bad", false, "dry run is for a different build".into()),
                (Ok(()), true, _, r) if r != 0 => {
                    ("bad", false, "dry run made real model calls".into())
                }
                (Ok(()), true, false, _) => (
                    "fail",
                    false,
                    format!("dry run failed: {}", b["why"].as_str().unwrap_or("")),
                ),
                _ => (
                    "pass",
                    true,
                    "the new build ran on a copy of the stack, signed by the sandbox key".into(),
                ),
            };
            json!({"state": state, "ok": okk, "why": why, "port": b["port"], "model_calls_real": b["model_calls_real"],
                   "checks": checks, "signed_at": b["signed_at"], "when": b["signed_at"].as_i64().map(when)})
        }
    };
    if dryrun["ok"] != true {
        blockers.push(dryrun["why"].as_str().unwrap_or("dry run").to_string());
    }

    // 4. The artifact on disk must still be the tested one.
    let art_path = dir.join("test").join("artifact");
    let rehash = read_plain(&art_path, ARTIFACT_CAP).map(|b| ok::sha256_hex(&b));
    let artifact_ok = matches!(&rehash, Ok(h) if !artifact_sha.is_empty() && *h == artifact_sha);
    if !artifact_sha.is_empty() && !artifact_ok {
        blockers.push("the built file on disk is not the tested one".into());
    }

    // 5. Where it stands.
    let od = out_dir(cfg, id);
    let mut approval = Value::Null;
    let mut state = "ready";
    if read_plain(&dir.join("deploy.json"), READ_CAP).is_ok() {
        state = "deployed";
    } else if let Ok(r) = read_json(&od.join("rejected.json")) {
        if r["artifact_sha256"] == artifact_sha.as_str() {
            state = "rejected";
            approval = json!({"rejected_at": r["at"], "when": r["at"].as_i64().map(when), "note": r["note"]});
        }
    }
    if state == "ready" {
        if let (Some(k), Ok(env)) = (owner, read_json(&od.join("approval.json"))) {
            if let Ok(b) = ok::verify_owner_approval(k, &env, now) {
                if b["artifact_sha256"] == artifact_sha.as_str() && b["id"] == id {
                    state = "approved";
                    approval = json!({"expires": b["expires"], "when": b["expires"].as_i64().map(when), "nonce": b["nonce"]});
                }
            }
        }
    }
    if owner.is_none() {
        blockers.push("no admin key installed yet (set it up below)".into());
    }
    let ready = blockers.is_empty() && state == "ready";
    if state == "ready" && !blockers.is_empty() {
        state = if test["state"] == "waiting" || dryrun["state"] == "waiting" {
            "waiting"
        } else {
            "blocked"
        };
    }
    let (diff, cut) = if patch.len() > DIFF_CAP {
        let mut end = DIFF_CAP;
        while !patch.is_char_boundary(end) {
            end -= 1;
        }
        (patch[..end].to_string(), true)
    } else {
        (patch.clone(), false)
    };
    let created = proposal["created_at"].clone();
    let json = json!({
        "id": id, "state": state, "ready": ready, "blockers": blockers,
        "plain": plain_words(summary, reason, &files), "summary": summary, "reason": reason,
        "files": files, "diff": diff, "diff_truncated": cut,
        "base_commit": base_commit, "proposal_sha256": proposal_sha,
        "stack": stack, "test": test, "dryrun": dryrun,
        "artifact": {"sha256": artifact_sha, "bytes": artifact_bytes, "on_disk_matches": artifact_ok,
                     "name": art_path.file_name().map(|s| s.to_string_lossy().into_owned())},
        "approval": approval, "created": created,
    });
    Ok(Card {
        id: id.to_string(),
        json,
        ready,
        artifact_sha256: artifact_sha,
        base_commit,
        test_record_sha256: test_sha,
        dryrun_record_sha256: dry_sha,
    })
}

fn ids(cfg: &Cfg) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(&cfg.queue)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| safe_id(n) && !n.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

fn owner_now(cfg: &Cfg) -> Option<ok::OwnerKey> {
    active_owner_key(cfg).ok().flatten().map(|(k, _)| k)
}

pub fn list(cfg: &Cfg, now: i64) -> Value {
    let owner = owner_now(cfg);
    let items: Vec<Value> = ids(cfg)
        .iter()
        .filter(|id| plain_dir(&cfg.queue.join(id)) && cfg.queue.join(id).join("proposal.json").exists())
        .map(|id| match card(cfg, id, owner.as_ref(), now) {
            Ok(c) => c.json,
            Err(e) => json!({"id": id, "state": "broken", "ready": false, "blockers": [e], "plain": "This request could not be read."}),
        })
        .collect();
    json!({"now": now, "owner_key": key_status(cfg), "items": items, "never_deploys": true})
}

pub fn count(cfg: &Cfg, now: i64) -> Value {
    let l = list(cfg, now);
    let n = l["items"]
        .as_array()
        .map(|a| a.iter().filter(|i| i["state"] == "ready").count())
        .unwrap_or(0);
    json!({"ready": n})
}

// ---------- approval challenge, approve, reject ----------

fn chal_dir(cfg: &Cfg) -> PathBuf {
    cfg.state.join("challenges")
}

pub fn challenge(cfg: &Cfg, body: &Value, now: i64) -> Result<Value, String> {
    let id = body["id"].as_str().unwrap_or_default();
    let owner = owner_now(cfg).ok_or("no admin key installed yet")?;
    let c = card(cfg, id, Some(&owner), now)?;
    if !c.ready {
        return Err(format!(
            "not ready to sign: {}",
            c.json["blockers"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or(c.json["state"].as_str().unwrap_or("?"))
        ));
    }
    let dir = chal_dir(cfg);
    // Reuse an open code for the same build (one per card, not one per refresh).
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            if let Ok(v) = read_json(&e.path()) {
                let b: Value =
                    serde_json::from_str(v["body"].as_str().unwrap_or("")).unwrap_or(Value::Null);
                if v["id"] == id
                    && b["artifact_sha256"] == c.artifact_sha256.as_str()
                    && b["test_record_sha256"] == c.test_record_sha256.as_str()
                    && b["dryrun_record_sha256"] == c.dryrun_record_sha256.as_str()
                    && b["expires"].as_i64().unwrap_or(0) > now + 60
                {
                    return Ok(chal_view(&owner, &v, now));
                }
            }
        }
    }
    let n = nonce()?;
    let text = ok::approval_body(
        id,
        &c.artifact_sha256,
        &c.base_commit,
        &c.test_record_sha256,
        &c.dryrun_record_sha256,
        &n,
        now,
        now + ok::MAX_TTL,
    );
    let rec = json!({"id": id, "nonce": n, "body": text, "issued_at": now});
    write_atomic(
        &dir,
        &format!("{n}.json"),
        format!("{rec}\n").as_bytes(),
        0o600,
        now,
    )?;
    Ok(chal_view(&owner, &rec, now))
}

fn chal_view(owner: &ok::OwnerKey, rec: &Value, now: i64) -> Value {
    let text = rec["body"].as_str().unwrap_or_default();
    let b: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let cred = match owner {
        ok::OwnerKey::Es256 {
            credential_id,
            rp_id,
            ..
        } => json!({"credential_id": credential_id, "rp_id": rp_id}),
        _ => Value::Null,
    };
    json!({
        "id": rec["id"], "nonce": rec["nonce"], "domain": ok::APPROVAL_V, "body": text,
        "webauthn_challenge": ok::b64url(&ok::webauthn_challenge(ok::APPROVAL_V, text)),
        "expires": b["expires"], "expires_in": b["expires"].as_i64().unwrap_or(0) - now,
        "when": b["expires"].as_i64().map(when),
        "scheme": owner.scheme(), "key_id": owner.key_id(), "passkey": cred,
    })
}

/// Pick the device signature fields out of a request (never trust others).
fn assertion_of(a: &Value) -> Value {
    let mut out = json!({});
    for f in [
        "scheme",
        "sig",
        "key_id",
        "credential_id",
        "authenticator_data",
        "client_data_json",
        "signature",
    ] {
        if let Some(s) = a[f].as_str().filter(|s| !s.is_empty()) {
            out[f] = json!(s);
        }
    }
    out
}

pub fn approve(cfg: &Cfg, body: &Value, now: i64) -> Result<Value, String> {
    let n = body["nonce"].as_str().unwrap_or_default();
    if !ok::is_hex_len(n, 32) {
        return Err("bad code".into());
    }
    let path = chal_dir(cfg).join(format!("{n}.json"));
    let rec = read_json(&path)
        .map_err(|_| "this one-time code is unknown or already used".to_string())?;
    let text = rec["body"].as_str().ok_or("bad code record")?;
    let id = rec["id"].as_str().unwrap_or_default();
    let owner = owner_now(cfg).ok_or("no admin key installed")?;
    let mut env = assertion_of(&body["assertion"]);
    env["approval_json"] = json!(text);
    if env.get("key_id").is_none() {
        env["key_id"] = json!(owner.key_id());
    }
    let b = ok::verify_owner_approval(&owner, &env, now)?;
    // The build must still be exactly what was signed.
    let c = card(cfg, id, Some(&owner), now)?;
    if !c.ready
        || b["artifact_sha256"] != c.artifact_sha256.as_str()
        || b["base_commit"] != c.base_commit.as_str()
        || b["test_record_sha256"] != c.test_record_sha256.as_str()
        || b["dryrun_record_sha256"] != c.dryrun_record_sha256.as_str()
        || b["id"] != c.id.as_str()
    {
        return Err("the request changed since the code was issued; ask for a new code".into());
    }
    retire(&path, &cfg.state.join("challenges-used"))?;
    // Field order as in FORMATS section 4.
    let file = match owner {
        ok::OwnerKey::Ed25519 { .. } => json!({
            "scheme": ok::SCHEME_ED25519, "approval_json": text, "sig": env["sig"], "key_id": owner.key_id(),
        }),
        ok::OwnerKey::Es256 { .. } => json!({
            "scheme": ok::SCHEME_ES256, "approval_json": text, "credential_id": env["credential_id"],
            "authenticator_data": env["authenticator_data"], "client_data_json": env["client_data_json"],
            "signature": env["signature"], "key_id": owner.key_id(),
        }),
    };
    let dest = write_atomic(
        &out_dir(cfg, id),
        "approval.json",
        format!("{file}\n").as_bytes(),
        0o644,
        now,
    )?;
    Ok(json!({
        "ok": true, "id": id, "stored": dest.display().to_string(),
        "expires": b["expires"], "when": b["expires"].as_i64().map(when),
        "next": "Signed and stored. The root deploy step (galion-sm-deploy) checks all four signatures before anything is installed; this console never deploys.",
    }))
}

pub fn reject(cfg: &Cfg, body: &Value, now: i64) -> Result<Value, String> {
    let id = body["id"].as_str().unwrap_or_default();
    let owner = owner_now(cfg);
    let c = card(cfg, id, owner.as_ref(), now)?;
    let note: String = body["note"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(500)
        .collect();
    let rec = json!({"version": REJECT_V, "id": id, "artifact_sha256": c.artifact_sha256, "note": note, "at": now});
    write_atomic(
        &out_dir(cfg, id),
        "rejected.json",
        format!("{rec}\n").as_bytes(),
        0o644,
        now,
    )?;
    // A rejection also withdraws an earlier approval of this request, so the
    // deploy step can no longer find it (kept aside, never deleted).
    let ap = out_dir(cfg, id).join("approval.json");
    if fs::symlink_metadata(&ap)
        .map(|m| m.file_type().is_file())
        .unwrap_or(false)
    {
        fs::rename(
            &ap,
            out_dir(cfg, id).join(format!("approval.json.withdrawn-{now}")),
        )
        .map_err(|e| e.to_string())?;
    }
    // Open codes for this request are used up too.
    if let Ok(rd) = fs::read_dir(chal_dir(cfg)) {
        for e in rd.flatten() {
            if read_json(&e.path()).map(|v| v["id"] == id).unwrap_or(false) {
                let _ = retire(&e.path(), &cfg.state.join("challenges-used"));
            }
        }
    }
    Ok(json!({"ok": true, "id": id, "state": "rejected"}))
}

// ---------- admin (owner) key setup ----------

fn key_dir(cfg: &Cfg) -> PathBuf {
    cfg.state.join("owner-key")
}

fn key_view(k: &ok::OwnerKey) -> Value {
    let fp = k.fingerprint();
    let cred = match k {
        ok::OwnerKey::Es256 {
            credential_id,
            rp_id,
            ..
        } => json!({"credential_id": credential_id, "rp_id": rp_id}),
        _ => Value::Null,
    };
    json!({"scheme": k.scheme(), "fingerprint": fp, "grouped": ok::group4(&fp), "short": &fp[..16], "key_id": k.key_id(), "passkey": cred})
}

pub fn key_status(cfg: &Cfg) -> Value {
    let active = match active_owner_key(cfg) {
        Ok(Some((k, _))) => key_view(&k),
        Ok(None) => Value::Null,
        Err(e) => json!({"error": e}),
    };
    let kd = key_dir(cfg);
    let pending = read_json(&kd.join("pending.json")).ok().and_then(|p| {
        let k = ok::OwnerKey::parse(p["owner_pub"].as_str()?).ok()?;
        let mut v = key_view(&k);
        v["created"] = p["created"].clone();
        v["when"] = json!(p["created"].as_i64().map(when));
        Some(v)
    });
    let staged = read_json(&kd.join("install-request.json"))
        .ok()
        .and_then(|r| {
            let k = ok::OwnerKey::parse(r["owner_pub"].as_str()?).ok()?;
            let mut v = key_view(&k);
            v["staged_at"] = r["staged_at"].clone();
            v["when"] = json!(r["staged_at"].as_i64().map(when));
            Some(v)
        });
    let staged_live = match (&staged, &active) {
        (Some(s), a) if !a.is_null() && a["fingerprint"] == s["fingerprint"] => Value::Null,
        (Some(s), _) => s.clone(),
        _ => Value::Null,
    };
    let state = if !staged_live.is_null() {
        "staged"
    } else if pending.is_some() {
        "pending"
    } else if !active.is_null() && active.get("error").is_none() {
        "active"
    } else {
        "none"
    };
    let req = kd.join("install-request.json");
    json!({
        "state": state, "active": active, "pending": pending, "staged": staged_live,
        "owner_pub": cfg.owner_pub.display().to_string(),
        "install_command": format!("galion-owner-key install --request {} --dry-run   # then again without --dry-run", req.display()),
        "rp_id": cfg.rp_id,
    })
}

pub fn key_begin(cfg: &Cfg, body: &Value, now: i64) -> Result<Value, String> {
    let scheme = body["scheme"].as_str().unwrap_or(ok::SCHEME_ES256);
    if scheme != ok::SCHEME_ES256 && scheme != ok::SCHEME_ED25519 {
        return Err("unknown key type".into());
    }
    let n = nonce()?;
    let text = ok::register_body(&n, &cfg.rp_id, now, now + ok::MAX_TTL);
    let rec = json!({"nonce": n, "scheme": scheme, "body": text, "issued_at": now});
    write_atomic(
        &key_dir(cfg),
        &format!("reg-{n}.json"),
        format!("{rec}\n").as_bytes(),
        0o600,
        now,
    )?;
    let replacing = owner_now(cfg).map(|k| key_view(&k));
    Ok(json!({
        "nonce": n, "scheme": scheme, "domain": ok::REGISTER_V, "body": text,
        "webauthn_challenge": ok::b64url(&ok::webauthn_challenge(ok::REGISTER_V, &text)),
        "rp_id": cfg.rp_id, "user_id": ok::b64url(b"galion-owner-slavik"),
        "expires_in": ok::MAX_TTL, "replacing": replacing,
    }))
}

pub fn key_register(cfg: &Cfg, body: &Value, now: i64) -> Result<Value, String> {
    let n = body["nonce"].as_str().unwrap_or_default();
    if !ok::is_hex_len(n, 32) {
        return Err("bad code".into());
    }
    let kd = key_dir(cfg);
    let path = kd.join(format!("reg-{n}.json"));
    let rec =
        read_json(&path).map_err(|_| "this setup code is unknown or already used".to_string())?;
    let text = rec["body"].as_str().unwrap_or_default();
    ok::check_window(text, ok::REGISTER_V, now, 0)?;
    let scheme = rec["scheme"].as_str().unwrap_or_default();
    let key = if scheme == ok::SCHEME_ES256 {
        let spki = ok::unhex(body["public_key_spki_hex"].as_str().unwrap_or_default())?;
        let sec1 = ok::sec1_from_spki(&spki)?;
        let cd = ok::unb64url(body["client_data_json"].as_str().unwrap_or_default())?;
        ok::check_client_data(
            &cd,
            "webauthn.create",
            &ok::webauthn_challenge(ok::REGISTER_V, text),
            &cfg.origins,
        )?;
        let cred = body["credential_id"].as_str().unwrap_or_default();
        if ok::unb64url(cred).map(|c| c.is_empty()).unwrap_or(true) {
            return Err("credential id missing".into());
        }
        ok::OwnerKey::Es256 {
            rp_id: cfg.rp_id.clone(),
            origins: cfg.origins.clone(),
            sec1,
            credential_id: Some(cred.to_string()),
        }
    } else {
        let raw = ok::unhex(body["public_key_hex"].as_str().unwrap_or_default())?;
        let public = ok::ed25519_from_spki_or_raw(&raw)?;
        let k = ok::OwnerKey::Ed25519 { public };
        // Proof the device holds the private half: it signs the setup code.
        let mut e = json!({"scheme": ok::SCHEME_ED25519, "sig": body["sig"]});
        e["key_id"] = json!(k.key_id());
        ok::verify_assertion(&k, ok::REGISTER_V, text, &e)
            .map_err(|e| format!("key proof: {e}"))?;
        k
    };
    // FORMATS 6: all four keys must be distinct.
    for p in [&cfg.stack_pub, &cfg.sandbox_pub] {
        if load_pub(p)
            .map(|k| k.fingerprint() == key.fingerprint())
            .unwrap_or(false)
        {
            return Err(
                "this key is already used by the stack or the sandbox signer; make a new one"
                    .into(),
            );
        }
    }
    retire(&path, &kd.join("used"))?;
    let owner_pub = key.file_text();
    let rec = json!({"owner_pub": owner_pub, "fingerprint": key.fingerprint(), "scheme": key.scheme(), "created": now});
    write_atomic(
        &kd,
        "pending.json",
        format!("{rec}\n").as_bytes(),
        0o600,
        now,
    )?;
    let mut v = key_view(&key);
    v["state"] = json!("pending");
    v["next"] = json!(
        "Compare this fingerprint with the one your device shows. Activate only if they match."
    );
    Ok(v)
}

pub fn key_activate_challenge(cfg: &Cfg, now: i64) -> Result<Value, String> {
    let kd = key_dir(cfg);
    let p = read_json(&kd.join("pending.json")).map_err(|_| "no new key is waiting".to_string())?;
    let text = p["owner_pub"].as_str().ok_or("pending key unreadable")?;
    let new = ok::OwnerKey::parse(text)?;
    let n = nonce()?;
    let act = ok::activate_body(text, &new.fingerprint(), &n, now, now + ok::MAX_TTL);
    let cur = owner_now(cfg);
    let change = cur
        .as_ref()
        .filter(|c| c.fingerprint() != new.fingerprint())
        .map(|c| {
            ok::change_body(
                &c.fingerprint(),
                &new.fingerprint(),
                text,
                &n,
                now,
                now + ok::MAX_TTL,
            )
        });
    let rec =
        json!({"nonce": n, "owner_pub": text, "activate": act, "change": change, "issued_at": now});
    write_atomic(
        &kd,
        &format!("act-{n}.json"),
        format!("{rec}\n").as_bytes(),
        0o600,
        now,
    )?;
    let side = |domain: &str, b: &str, k: &ok::OwnerKey| json!({"domain": domain, "body": b, "webauthn_challenge": ok::b64url(&ok::webauthn_challenge(domain, b)), "key": key_view(k)});
    Ok(json!({
        "nonce": n, "expires_in": ok::MAX_TTL,
        "activate": side(ok::ACTIVATE_V, &act, &new),
        "change": match (&change, &cur) { (Some(b), Some(c)) => side(ok::CHANGE_V, b, c), _ => Value::Null },
    }))
}

pub fn key_activate(cfg: &Cfg, body: &Value, now: i64) -> Result<Value, String> {
    if body["confirmed"] != true {
        return Err("confirm first that the fingerprints match".into());
    }
    let n = body["nonce"].as_str().unwrap_or_default();
    if !ok::is_hex_len(n, 32) {
        return Err("bad code".into());
    }
    let kd = key_dir(cfg);
    let path = kd.join(format!("act-{n}.json"));
    let rec = read_json(&path)
        .map_err(|_| "this activation code is unknown or already used".to_string())?;
    let text = rec["owner_pub"].as_str().unwrap_or_default();
    let pend =
        read_json(&kd.join("pending.json")).map_err(|_| "no new key is waiting".to_string())?;
    if pend["owner_pub"].as_str() != Some(text) {
        return Err("a different key is waiting now; start activation again".into());
    }
    let new = ok::OwnerKey::parse(text)?;
    let act_body = rec["activate"].as_str().unwrap_or_default();
    let mut act = assertion_of(&body["activate"]);
    act["body_json"] = json!(act_body);
    if act.get("key_id").is_none() {
        act["key_id"] = json!(new.key_id());
    }
    ok::verify_assertion(&new, ok::ACTIVATE_V, act_body, &act)
        .map_err(|e| format!("new key: {e}"))?;
    ok::check_window(act_body, ok::ACTIVATE_V, now, 0)?;
    let mut change = None;
    if let Some(ch_body) = rec["change"].as_str() {
        let cur = owner_now(cfg).ok_or("the installed key changed; start again")?;
        let mut ch = assertion_of(&body["change"]);
        ch["body_json"] = json!(ch_body);
        if ch.get("key_id").is_none() {
            ch["key_id"] = json!(cur.key_id());
        }
        ok::verify_assertion(&cur, ok::CHANGE_V, ch_body, &ch)
            .map_err(|e| format!("replacing needs your CURRENT key: {e}"))?;
        change = Some(ch);
    }
    let req = ok::install_request(text, &act, change.as_ref(), now);
    // Same checks the root helper will run, against the key installed now.
    let current = active_owner_key(cfg)?.map(|(_, t)| t);
    ok::check_install_request(&req, current.as_deref(), now)?;
    retire(&path, &kd.join("used"))?;
    write_atomic(&kd, "owner.pub.staged", text.as_bytes(), 0o644, now)?;
    let dest = write_atomic(
        &kd,
        "install-request.json",
        format!("{req:#}\n").as_bytes(),
        0o644,
        now,
    )?;
    let _ = fs::rename(
        kd.join("pending.json"),
        kd.join(format!("pending.json.staged-{now}")),
    );
    Ok(json!({
        "ok": true, "state": "staged", "key": key_view(&new), "request": dest.display().to_string(),
        "install_command": format!("galion-owner-key install --request {} --dry-run", dest.display()),
        "next": "Staged for the root helper. The key becomes active only after root runs galion-owner-key install; this console never writes owner.pub.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use galion_owner_key::testkit::{fake_entry, SoftKeyFile, SoftPasskey};

    const NOW: i64 = 1_793_436_000; // 2026-10-31 ~09:40 CET (+01:00)
    const ORIGIN: &str = "https://one.galion.app";
    const ID: &str = "sm-20261031094000-c0ffee";

    fn setup(tag: &str) -> Cfg {
        let root = std::env::temp_dir()
            .join("galion-one-approvals-tests")
            .join(format!("approvals-{tag}-{}", std::process::id()));
        if root.exists() {
            let _ = fs::rename(&root, root.with_extension(format!("old-{}", NOW)));
        }
        let keys = root.join("etc");
        fake_entry(&root.join("queue"), &keys, ID, NOW).unwrap();
        Cfg {
            queue: root.join("queue"),
            state: root.join("state"),
            out: None,
            owner_pub: keys.join("owner.pub"),
            sandbox_pub: keys.join("sandbox.pub"),
            stack_pub: keys.join("stack.pub"),
            rp_id: "galion.app".into(),
            origins: vec!["https://galion.app".into(), ORIGIN.into()],
        }
    }

    fn call(
        cfg: &Cfg,
        method: &str,
        path: &str,
        body: Value,
        origin: &str,
        paired: bool,
        now: i64,
    ) -> (u16, Value) {
        let b = body.to_string();
        let r = route(
            cfg,
            &Req {
                method,
                path,
                body: b.as_bytes(),
                origin,
                paired,
                now,
            },
        );
        (
            r.status,
            serde_json::from_slice(&r.body).unwrap_or(Value::Null),
        )
    }
    fn post(cfg: &Cfg, path: &str, body: Value, now: i64) -> (u16, Value) {
        call(cfg, "POST", path, body, ORIGIN, true, now)
    }
    fn get(cfg: &Cfg, path: &str, now: i64) -> Value {
        let (s, v) = call(cfg, "GET", path, Value::Null, "", true, now);
        assert_eq!(s, 200, "{v}");
        v
    }

    /// Stand-in for the root helper: install the staged request into cfg.owner_pub.
    fn root_install(cfg: &Cfg, now: i64) {
        let req = read_json(&cfg.state.join("owner-key/install-request.json")).unwrap();
        let out = ok::install(
            &req,
            &cfg.owner_pub,
            now,
            &ok::InstallOpts {
                chown_root: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert!(
            matches!(out, ok::InstallOutcome::Installed { .. }),
            "{out:?}"
        );
    }

    fn set_up_passkey(cfg: &Cfg, pk: &mut SoftPasskey, now: i64) {
        let (s, b) = post(
            cfg,
            "/api/owner-key/begin",
            json!({"scheme": "webauthn-es256"}),
            now,
        );
        assert_eq!(s, 200, "{b}");
        let chal = ok::unb64url(b["webauthn_challenge"].as_str().unwrap()).unwrap();
        let cd = pk.client_data("webauthn.create", &chal);
        let (s, r) = post(
            cfg,
            "/api/owner-key/register",
            json!({
            "nonce": b["nonce"], "public_key_spki_hex": ok::hex(&pk.spki()),
            "credential_id": pk.credential_id, "client_data_json": ok::b64url(&cd)}),
            now,
        );
        assert_eq!(s, 200, "{r}");
        assert_eq!(
            r["fingerprint"],
            ok::sha256_hex(&pk.sec1()),
            "server shows the device's fingerprint"
        );
        assert_eq!(get(cfg, "/api/owner-key", now)["state"], "pending");
    }

    #[test]
    fn auth_and_origin() {
        let cfg = setup("auth");
        let (s, _) = call(&cfg, "GET", "/api/approvals", Value::Null, "", false, NOW);
        assert_eq!(s, 401);
        let (s, _) = call(
            &cfg,
            "POST",
            "/api/approvals/challenge",
            json!({"id": ID}),
            "https://evil.example",
            true,
            NOW,
        );
        assert_eq!(s, 403);
        let (s, _) = call(
            &cfg,
            "POST",
            "/api/approvals/challenge",
            json!({"id": ID}),
            "",
            true,
            NOW,
        );
        assert_eq!(s, 403);
        let r = route(
            &cfg,
            &Req {
                method: "GET",
                path: "/approvals",
                body: b"",
                origin: "",
                paired: false,
                now: NOW,
            },
        );
        assert_eq!(r.status, 200);
        assert!(String::from_utf8_lossy(&r.body).contains("Hermes self-changes"));
    }

    #[test]
    fn card_shows_verified_records_and_blocks_without_key() {
        let cfg = setup("card");
        let l = get(&cfg, "/api/approvals", NOW);
        let c = &l["items"][0];
        assert_eq!(c["id"], ID);
        assert_eq!(c["stack"]["ok"], true, "{c}");
        assert_eq!(c["test"]["state"], "pass", "{c}");
        assert_eq!(c["dryrun"]["state"], "pass", "{c}");
        assert_eq!(c["artifact"]["on_disk_matches"], true);
        assert!(c["plain"]
            .as_str()
            .unwrap()
            .starts_with("Hermes wants to change 1 file in hermes-rs."));
        assert_eq!(c["state"], "blocked");
        assert_eq!(c["ready"], false);
        assert_eq!(l["owner_key"]["state"], "none");
        let (s, e) = post(&cfg, "/api/approvals/challenge", json!({"id": ID}), NOW);
        assert_eq!(s, 409);
        assert!(e["error"].as_str().unwrap().contains("no admin key"));
        // Tamper: the built file on disk changes after the test.
        fs::write(cfg.queue.join(ID).join("test/artifact"), b"other build").unwrap();
        let c = get(&cfg, "/api/approvals", NOW)["items"][0].clone();
        assert_eq!(c["artifact"]["on_disk_matches"], false);
        assert!(c["blockers"].to_string().contains("not the tested one"));
        // Tamper: a signed test log changes.
        fs::write(cfg.queue.join(ID).join("test/logs/build.log"), b"edited").unwrap();
        let c = get(&cfg, "/api/approvals", NOW)["items"][0].clone();
        assert_eq!(c["test"]["state"], "bad", "{c}");
        assert!(c["test"]["why"]
            .as_str()
            .unwrap()
            .contains("changed after signing"));
    }

    #[test]
    fn passkey_setup_approve_replay_and_tamper() {
        let cfg = setup("passkey");
        let mut pk = SoftPasskey::new(3, "galion.app", ORIGIN);
        set_up_passkey(&cfg, &mut pk, NOW);
        // Activation needs the box ticked AND a signature from the new key.
        let (_, ac) = post(&cfg, "/api/owner-key/activate-challenge", json!({}), NOW);
        assert!(ac["change"].is_null(), "first key: no current key to sign");
        let a = &ac["activate"];
        let sig = pk.assert(
            a["domain"].as_str().unwrap(),
            a["body"].as_str().unwrap(),
            "",
        );
        let (s, e) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": false, "activate": sig}),
            NOW,
        );
        assert_eq!(s, 409, "{e}");
        let other = SoftPasskey::new(9, "galion.app", ORIGIN).assert(
            a["domain"].as_str().unwrap(),
            a["body"].as_str().unwrap(),
            "",
        );
        let (s, _) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": other}),
            NOW,
        );
        assert_eq!(s, 409, "another passkey cannot activate this key");
        let (s, r) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": sig}),
            NOW + 5,
        );
        assert_eq!(s, 200, "{r}");
        assert_eq!(get(&cfg, "/api/owner-key", NOW + 5)["state"], "staged");
        assert!(!cfg.owner_pub.exists(), "console never writes owner.pub");
        let (s, _) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": sig}),
            NOW + 6,
        );
        assert_eq!(s, 409, "activation code is one-time");
        root_install(&cfg, NOW + 10);
        let k = get(&cfg, "/api/owner-key", NOW + 10);
        assert_eq!(k["state"], "active");
        assert_eq!(k["active"]["fingerprint"], ok::sha256_hex(&pk.sec1()));

        // Approve the card.
        let t = NOW + 60;
        let c = get(&cfg, "/api/approvals", t)["items"][0].clone();
        assert_eq!(c["state"], "ready", "{c}");
        let (s, ch) = post(&cfg, "/api/approvals/challenge", json!({"id": ID}), t);
        assert_eq!(s, 200, "{ch}");
        assert_eq!(ch["expires_in"], 1800);
        let (_, ch2) = post(&cfg, "/api/approvals/challenge", json!({"id": ID}), t + 30);
        assert_eq!(
            ch2["nonce"], ch["nonce"],
            "same open code for the same build"
        );
        let body: Value = serde_json::from_str(ch["body"].as_str().unwrap()).unwrap();
        assert_eq!(body["artifact_sha256"], c["artifact"]["sha256"]);
        assert_eq!(body["base_commit"], c["base_commit"]);
        let dom = ch["domain"].as_str().unwrap();
        let text = ch["body"].as_str().unwrap();
        // Wrong origin inside clientData (phishing page) is refused.
        let mut phish = SoftPasskey::new(3, "galion.app", "https://galion-app.example");
        let (s, _) = post(
            &cfg,
            "/api/approvals/approve",
            json!({"nonce": ch["nonce"], "assertion": phish.assert(dom, text, "")}),
            t + 40,
        );
        assert_eq!(s, 409);
        // Signed a different body (tampered artifact hash) is refused.
        let bad = text.replace(&c["artifact"]["sha256"].as_str().unwrap()[..8], "00000000");
        let (s, _) = post(
            &cfg,
            "/api/approvals/approve",
            json!({"nonce": ch["nonce"], "assertion": pk.assert(dom, &bad, "")}),
            t + 40,
        );
        assert_eq!(s, 409);
        // Expired code is refused.
        let (s, _) = post(
            &cfg,
            "/api/approvals/approve",
            json!({"nonce": ch["nonce"], "assertion": pk.assert(dom, text, "")}),
            t + 1900,
        );
        assert_eq!(s, 409);
        let (s, r) = post(
            &cfg,
            "/api/approvals/approve",
            json!({"nonce": ch["nonce"], "assertion": pk.assert(dom, text, "")}),
            t + 50,
        );
        assert_eq!(s, 200, "{r}");
        let file = read_json(&cfg.queue.join(ID).join("owner/approval.json")).unwrap();
        assert_eq!(file["scheme"], "webauthn-es256");
        let keys: Vec<&String> = file.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "scheme",
                "approval_json",
                "credential_id",
                "authenticator_data",
                "client_data_json",
                "signature",
                "key_id"
            ]
        );
        let owner = ok::OwnerKey::parse(&fs::read_to_string(&cfg.owner_pub).unwrap()).unwrap();
        ok::verify_owner_approval(&owner, &file, t + 60).unwrap();
        let (s, _) = post(
            &cfg,
            "/api/approvals/approve",
            json!({"nonce": ch["nonce"], "assertion": pk.assert(dom, text, "")}),
            t + 55,
        );
        assert_eq!(s, 409, "replay of a used code");
        assert_eq!(
            get(&cfg, "/api/approvals", t + 60)["items"][0]["state"],
            "approved"
        );
        assert!(
            !cfg.queue.join(ID).join("deploy.json").exists(),
            "console never deploys"
        );
    }

    #[test]
    fn keyfile_approve_reject_and_replace_needs_current_key() {
        let cfg = setup("keyfile");
        let kf = SoftKeyFile::new(0x21);
        let (_, b) = post(
            &cfg,
            "/api/owner-key/begin",
            json!({"scheme": "ed25519"}),
            NOW,
        );
        let pubhex = ok::hex(kf.owner_key().raw());
        // Proof of possession: a key that does not sign the code cannot register.
        let wrong =
            SoftKeyFile::new(0x22).sign(b["domain"].as_str().unwrap(), b["body"].as_str().unwrap());
        let (s, _) = post(
            &cfg,
            "/api/owner-key/register",
            json!({"nonce": b["nonce"], "public_key_hex": pubhex, "sig": wrong["sig"]}),
            NOW,
        );
        assert_eq!(s, 409);
        let good = kf.sign(b["domain"].as_str().unwrap(), b["body"].as_str().unwrap());
        let (s, r) = post(
            &cfg,
            "/api/owner-key/register",
            json!({"nonce": b["nonce"], "public_key_hex": pubhex, "sig": good["sig"]}),
            NOW,
        );
        assert_eq!(s, 200, "{r}");
        let (_, ac) = post(&cfg, "/api/owner-key/activate-challenge", json!({}), NOW);
        let a = &ac["activate"];
        let sig = kf.sign(a["domain"].as_str().unwrap(), a["body"].as_str().unwrap());
        let (s, r) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": sig}),
            NOW,
        );
        assert_eq!(s, 200, "{r}");
        root_install(&cfg, NOW + 1);

        // Approve with the key file.
        let (_, ch) = post(
            &cfg,
            "/api/approvals/challenge",
            json!({"id": ID}),
            NOW + 10,
        );
        let sig = kf.sign(ch["domain"].as_str().unwrap(), ch["body"].as_str().unwrap());
        let (s, r) = post(
            &cfg,
            "/api/approvals/approve",
            json!({"nonce": ch["nonce"], "assertion": sig}),
            NOW + 20,
        );
        assert_eq!(s, 200, "{r}");
        let file = read_json(&cfg.queue.join(ID).join("owner/approval.json")).unwrap();
        let keys: Vec<&String> = file.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["scheme", "approval_json", "sig", "key_id"]);

        // Reject: the card says rejected, old approval file kept, no code can be issued.
        let (s, _) = post(
            &cfg,
            "/api/approvals/reject",
            json!({"id": ID, "note": "not now"}),
            NOW + 30,
        );
        assert_eq!(s, 200);
        assert_eq!(
            get(&cfg, "/api/approvals", NOW + 31)["items"][0]["state"],
            "rejected"
        );
        assert!(
            !cfg.queue.join(ID).join("owner/approval.json").exists(),
            "rejecting withdraws the earlier approval"
        );
        let (s, _) = post(
            &cfg,
            "/api/approvals/challenge",
            json!({"id": ID}),
            NOW + 32,
        );
        assert_eq!(s, 409);

        // Replace with a passkey: the current key file must sign the change.
        let mut pk = SoftPasskey::new(4, "galion.app", ORIGIN);
        set_up_passkey(&cfg, &mut pk, NOW + 40);
        let (_, ac) = post(
            &cfg,
            "/api/owner-key/activate-challenge",
            json!({}),
            NOW + 41,
        );
        assert!(!ac["change"].is_null());
        let a = &ac["activate"];
        let c = &ac["change"];
        let new_sig = pk.assert(
            a["domain"].as_str().unwrap(),
            a["body"].as_str().unwrap(),
            "",
        );
        let (s, e) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": new_sig}),
            NOW + 42,
        );
        assert_eq!(s, 409, "missing current-key signature: {e}");
        assert!(e["error"].as_str().unwrap().contains("CURRENT key"));
        let thief =
            SoftKeyFile::new(0x66).sign(c["domain"].as_str().unwrap(), c["body"].as_str().unwrap());
        let (s, _) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": new_sig, "change": thief}),
            NOW + 42,
        );
        assert_eq!(s, 409);
        let cur = kf.sign(c["domain"].as_str().unwrap(), c["body"].as_str().unwrap());
        let (s, r) = post(
            &cfg,
            "/api/owner-key/activate",
            json!({"nonce": ac["nonce"], "confirmed": true, "activate": new_sig, "change": cur}),
            NOW + 43,
        );
        assert_eq!(s, 200, "{r}");
        let old = fs::read_to_string(&cfg.owner_pub).unwrap();
        root_install(&cfg, NOW + 50);
        let now_key = ok::OwnerKey::parse(&fs::read_to_string(&cfg.owner_pub).unwrap()).unwrap();
        assert_eq!(now_key.fingerprint(), ok::sha256_hex(&pk.sec1()));
        assert!(
            fs::read_dir(cfg.owner_pub.parent().unwrap())
                .unwrap()
                .flatten()
                .any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("owner.pub.off-")
                    && fs::read_to_string(e.path()).unwrap() == old),
            "old key kept, not deleted"
        );
    }

    #[test]
    fn symlinked_queue_files_are_not_followed() {
        let cfg = setup("symlink");
        let p = cfg.queue.join(ID).join("change.patch");
        fs::rename(&p, p.with_extension("real")).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", &p).unwrap();
        let c = get(&cfg, "/api/approvals", NOW)["items"][0].clone();
        assert_eq!(
            c["stack"]["ok"], false,
            "patch via symlink must not verify: {c}"
        );
    }
}

/// Cross-check with agent A's root verifier (`galion-sm-deploy verify`), run by
/// hand: `APPROVAL_CROSSCHECK_DIR=<dir> cargo test -p galion-one crosscheck -- --ignored`.
/// Real clock and an https origin, because A only accepts https origins.
#[cfg(test)]
mod crosscheck {
    use super::*;
    use galion_owner_key::testkit::{fake_entry, SoftPasskey};

    #[test]
    #[ignore]
    fn crosscheck_dir_for_a_verifier() {
        let Ok(root) = std::env::var("APPROVAL_CROSSCHECK_DIR") else {
            return;
        };
        let root = PathBuf::from(root);
        let now = std::time::SystemTime::now() // repo-check: allow (unix epoch number for signed bodies in a test; never printed)
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let id = "sm-20261031-crosscheck";
        fake_entry(&root.join("queue"), &root.join("etc"), id, now).unwrap();
        let origin = "https://one.galion.app";
        let cfg = Cfg {
            queue: root.join("queue"),
            state: root.join("state"),
            out: None,
            owner_pub: root.join("etc/owner.pub"),
            sandbox_pub: root.join("etc/sandbox.pub"),
            stack_pub: root.join("etc/stack.pub"),
            rp_id: "galion.app".into(),
            origins: vec!["https://galion.app".into(), origin.into()],
        };
        let mut pk = SoftPasskey::new(5, "galion.app", origin);
        let b = key_begin(&cfg, &json!({"scheme": "webauthn-es256"}), now).unwrap();
        let chal = ok::unb64url(b["webauthn_challenge"].as_str().unwrap()).unwrap();
        let cd = pk.client_data("webauthn.create", &chal);
        key_register(
            &cfg,
            &json!({"nonce": b["nonce"], "public_key_spki_hex": ok::hex(&pk.spki()),
            "credential_id": pk.credential_id, "client_data_json": ok::b64url(&cd)}),
            now,
        )
        .unwrap();
        let ac = key_activate_challenge(&cfg, now).unwrap();
        let a = &ac["activate"];
        let s = pk.assert(
            a["domain"].as_str().unwrap(),
            a["body"].as_str().unwrap(),
            "",
        );
        key_activate(
            &cfg,
            &json!({"nonce": ac["nonce"], "confirmed": true, "activate": s}),
            now,
        )
        .unwrap();
        let req = read_json(&cfg.state.join("owner-key/install-request.json")).unwrap();
        ok::install(
            &req,
            &cfg.owner_pub,
            now,
            &ok::InstallOpts {
                chown_root: false,
                dry_run: false,
            },
        )
        .unwrap();
        let ch = challenge(&cfg, &json!({"id": id}), now).unwrap();
        let s = pk.assert(
            ch["domain"].as_str().unwrap(),
            ch["body"].as_str().unwrap(),
            "",
        );
        approve(&cfg, &json!({"nonce": ch["nonce"], "assertion": s}), now).unwrap();
        println!("crosscheck entry {}", root.join("queue").join(id).display());
    }
}
