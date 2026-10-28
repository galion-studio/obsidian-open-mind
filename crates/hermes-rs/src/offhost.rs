// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Host-gone continuity for Hermes.
//!
//! This is not on-host probing (openmind-survival, guardian-tick) and it is
//! not the cash lane (survival-cash, cash_usd / lane / when, or the desk
//! volume_used_pct line).
//! Those keep working. This module answers a different failure: the VPS
//! itself is off, so the gateway must leave a snapshot somewhere else and
//! pick it up on the next boot, or a loopback standby elsewhere can hold
//! the same files without calling a model provider or firing cron.
//!
//! The secondary store is for the gap until new Galion hosts exist. Caller
//! note, 2026-10-27 CET: Croatian government-linked infrastructure at
//! Teglenice (Split-Dalmatia waste-to-energy) is about 1.5 years out. This
//! crate does not deploy there and does not invent a store URL.
//!
//! State it copies is the hermes-rs set already on disk: `gateway_state.json`,
//! `cron/jobs.json` (read-only unless the file is missing), the cron state
//! file, the spend file, and the session db or JSONL directory. `config.yaml`
//! is a sha256 only. The legacy Python `state.db` is not part of the bundle.
//! Secrets are not written into the snapshot. `HERMES_OFFHOST_TOKEN` is read
//! at call time, passed to curl on stdin, and never logged.

use crate::config::Config;
use crate::provider::curl_quote;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SCHEMA: u32 = 1;
pub const KIND: &str = "hermes-offhost";
pub const DEFAULT_INTERVAL_S: u64 = 300;
pub const DEFAULT_STALE_AFTER_S: i64 = 6 * 3600;
pub const DEFAULT_MAX_BYTES: usize = 2 * 1024 * 1024;
const HASH_LIMIT: u64 = 32 * 1024 * 1024;
const SKEW_S: i64 = 60;

pub type Status = Arc<Mutex<Value>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Primary,
    Standby,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Primary => "primary",
            Role::Standby => "standby",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "primary" => Ok(Role::Primary),
            "standby" => Ok(Role::Standby),
            _ => Err("HERMES_OFFHOST_ROLE must be primary or standby".into()),
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumePolicy {
    IfLocalMissingOrStale,
    IfMissing,
    Never,
}

impl ResumePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            ResumePolicy::IfLocalMissingOrStale => "if_local_missing_or_stale",
            ResumePolicy::IfMissing => "if_missing",
            ResumePolicy::Never => "never",
        }
    }

    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "if_local_missing_or_stale" => Ok(ResumePolicy::IfLocalMissingOrStale),
            "if_missing" => Ok(ResumePolicy::IfMissing),
            "never" => Ok(ResumePolicy::Never),
            _ => Err(
                "HERMES_OFFHOST_RESUME must be if_local_missing_or_stale, if_missing, or never"
                    .into(),
            ),
        }
    }
}

impl std::fmt::Display for ResumePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    Never,
    IfMissing,
    IfStale,
}

/// Fields this module may write into survival.json. Cash, lane, when, and
/// volume_used_pct are never set here.
#[derive(Debug, Clone, PartialEq)]
pub struct SurvivalOffhost {
    pub enabled: bool,
    pub last_heartbeat: Option<String>,
    pub external_state_uri: Option<String>,
    pub resume_policy: String,
    pub role: String,
    pub last_export_ok: Option<bool>,
}

#[derive(Clone)]
pub struct OffhostCfg {
    pub enabled: bool,
    pub role: Role,
    pub resume: ResumePolicy,
    pub home: PathBuf,
    pub interval_s: u64,
    pub stale_after_s: i64,
    pub max_bytes: usize,
    pub local_path: PathBuf,
    pub external_path: Option<PathBuf>,
    pub external_uri: Option<String>,
    pub survival_path: Option<PathBuf>,
    pub gateway_state: PathBuf,
    pub cron_jobs: PathBuf,
    pub cron_state: PathBuf,
    pub spend_file: PathBuf,
    pub state_db: PathBuf,
    pub config_path: PathBuf,
    pub host: String,
    pub port: u16,
    /// F1: host-side HMAC key (`HERMES_OFFHOST_HMAC_FILE`). Required when a
    /// secondary store is set; every snapshot is sealed and verified with it.
    pub seal: Option<crate::offhost_guard::SealKey>,
    /// Slot 340: previous key of a rotation window
    /// (`HERMES_OFFHOST_HMAC_PREV_FILE` + `HERMES_OFFHOST_HMAC_PREV_UNTIL`).
    /// Verify-only; never seals.
    pub seal_prev: Option<crate::offhost_guard::PrevKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BeatReport {
    pub local_ok: bool,
    pub path_ok: Option<bool>,
    pub uri_ok: Option<bool>,
    pub survival_ok: Option<bool>,
    pub blockers: Vec<String>,
    pub written_at: String,
    pub config_sha256: Option<String>,
    pub session_files: u64,
    pub bytes: usize,
    pub role: String,
    pub resume_policy: String,
    pub external_state_uri: Option<String>,
    /// Snapshot was sealed with the host HMAC key (F1).
    pub sealed: bool,
}

impl BeatReport {
    pub fn summary(&self) -> String {
        format!(
            "offhost beat role={} local_ok={} path_ok={:?} uri_ok={:?} survival_ok={:?} sessions={} bytes={} sealed={} blockers={}",
            self.role,
            self.local_ok,
            self.path_ok,
            self.uri_ok,
            self.survival_ok,
            self.session_files,
            self.bytes,
            self.sealed,
            self.blockers.join("; ")
        )
    }

    pub fn view(&self) -> Value {
        json!({
            "enabled": true,
            "kind": KIND,
            "role": self.role,
            "resume_policy": self.resume_policy,
            "external_state_uri": self.external_state_uri,
            "last_heartbeat": self.written_at,
            "local_ok": self.local_ok,
            "path_ok": self.path_ok,
            "uri_ok": self.uri_ok,
            "survival_ok": self.survival_ok,
            "config_sha256": self.config_sha256,
            "session_files": self.session_files,
            "bytes": self.bytes,
            "sealed": self.sealed,
            "blockers": self.blockers,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeReport {
    pub loaded: bool,
    pub source: String,
    pub restored: Vec<String>,
    pub kept: Vec<String>,
    pub blockers: Vec<String>,
    pub role: String,
    pub resume_policy: String,
    pub external_state_uri: Option<String>,
    pub written_at: Option<String>,
    /// "hmac-sha256" when the snapshot seal verified with the current key,
    /// "hmac-sha256+previous-key" inside a rotation window (slot 340),
    /// "unsigned-local" when
    /// no key is set (local-only mode), "none" when nothing was applied.
    pub auth: String,
}

impl ResumeReport {
    pub fn summary(&self) -> String {
        format!(
            "offhost resume role={} loaded={} source={} auth={} restored={} kept={} blockers={}",
            self.role,
            self.loaded,
            self.source,
            self.auth,
            self.restored.join(","),
            self.kept.join(","),
            self.blockers.join("; ")
        )
    }

    pub fn view(&self) -> Value {
        json!({
            "enabled": true,
            "kind": KIND,
            "role": self.role,
            "resume_policy": self.resume_policy,
            "external_state_uri": self.external_state_uri,
            "last_heartbeat": self.written_at,
            "auth": self.auth,
            "loaded": self.loaded,
            "source": self.source,
            "restored": self.restored,
            "kept": self.kept,
            "blockers": self.blockers,
        })
    }
}

struct Piece {
    role: &'static str,
    name: String,
    restore: &'static str,
    raw_len: u64,
    source_sha: Option<String>,
    body: Option<Vec<u8>>,
    text: bool,
    redacted: bool,
    /// Values redacted inside the sanitised session DB (F2).
    redacted_values: usize,
    omit_reason: Option<&'static str>,
}

fn env_get(env: &dyn Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    env(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn flag(v: &str, key: &str) -> Result<bool, String> {
    match v {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("{key} must be 1/0, true/false, yes/no, or on/off")),
    }
}

fn sha256_hex(data: &[u8]) -> String {
    crate::mcp::sha256(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Locator safe to write into survival.json and logs. Drops userinfo, query,
/// and fragment. The process still uses the raw URI for the transfer.
pub fn public_locator(raw: &str) -> String {
    let s = raw.trim();
    let Some((scheme, rest)) = s.split_once("://") else {
        return s.to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or("");
    let hostpath = rest.rsplit_once('@').map(|(_, h)| h).unwrap_or(rest);
    format!("{scheme}://{hostpath}")
}

fn uri_host(uri: &str) -> String {
    let rest = uri.split_once("://").map(|(_, r)| r).unwrap_or(uri);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    host.split(':').next().unwrap_or(host).to_ascii_lowercase()
}

/// Host part of an http(s) URI is 127.0.0.0/8, ::1 or localhost.
fn loopback_uri(uri: &str) -> bool {
    let rest = uri.split_once("://").map(|(_, r)| r).unwrap_or("");
    let hostport = rest.split(['/', '?', '#']).next().unwrap_or("");
    if hostport.contains('@') {
        return false;
    }
    let host = if let Some(v6) = hostport.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host == "::1" {
        return true;
    }
    match host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => false,
    }
}

fn check_uri(uri: &str) -> Result<(), String> {
    if !(uri.starts_with("https://") || uri.starts_with("http://")) {
        return Err("HERMES_OFFHOST_URI must be http or https".into());
    }
    // F4: the bearer token and the snapshot never cross the network in clear.
    if uri.starts_with("http://") && !loopback_uri(uri) {
        return Err("HERMES_OFFHOST_URI must be https (plain http only for loopback)".into());
    }
    if uri.contains([' ', '\n', '\r', '"', '\\']) {
        return Err("HERMES_OFFHOST_URI has unusable characters".into());
    }
    let host = uri_host(uri);
    if host.is_empty() {
        return Err("HERMES_OFFHOST_URI is missing a host".into());
    }
    let rest = uri.split_once("://").map(|(_, r)| r).unwrap_or("");
    let hostport = rest.split(['/', '?', '#']).next().unwrap_or("");
    if hostport.contains('@') {
        return Err("HERMES_OFFHOST_URI must not embed userinfo; set HERMES_OFFHOST_TOKEN".into());
    }
    if host == "openrouter.ai" || host.ends_with(".openrouter.ai") {
        return Err("HERMES_OFFHOST_URI must be a state store, not a model host".into());
    }
    Ok(())
}

fn parse_u64(v: &str, key: &str, min: u64, max: u64) -> Result<u64, String> {
    let n: u64 = v.parse().map_err(|_| format!("bad {key}"))?;
    if n < min || n > max {
        return Err(format!("bad {key}"));
    }
    Ok(n)
}

fn parse_i64(v: &str, key: &str, min: i64, max: i64) -> Result<i64, String> {
    let n: i64 = v.parse().map_err(|_| format!("bad {key}"))?;
    if n < min || n > max {
        return Err(format!("bad {key}"));
    }
    Ok(n)
}

impl OffhostCfg {
    pub fn load(cfg: &Config, env: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        let enabled = match env_get(env, "HERMES_OFFHOST") {
            Some(v) => flag(&v, "HERMES_OFFHOST")?,
            None => false,
        };
        let role = match env_get(env, "HERMES_OFFHOST_ROLE") {
            Some(v) => Role::parse(&v)?,
            None => Role::Primary,
        };
        let resume = match env_get(env, "HERMES_OFFHOST_RESUME") {
            Some(v) => ResumePolicy::parse(&v)?,
            None => ResumePolicy::IfLocalMissingOrStale,
        };
        let interval_s = match env_get(env, "HERMES_OFFHOST_INTERVAL_S") {
            Some(v) => parse_u64(&v, "HERMES_OFFHOST_INTERVAL_S", 30, 86_400)?,
            None => DEFAULT_INTERVAL_S,
        };
        let stale_after_s = match env_get(env, "HERMES_OFFHOST_STALE_S") {
            Some(v) => parse_i64(&v, "HERMES_OFFHOST_STALE_S", 0, 31_536_000)?,
            None => DEFAULT_STALE_AFTER_S,
        };
        let max_bytes = match env_get(env, "HERMES_OFFHOST_MAX_BYTES") {
            Some(v) => parse_u64(&v, "HERMES_OFFHOST_MAX_BYTES", 65_536, 16_777_216)? as usize,
            None => DEFAULT_MAX_BYTES,
        };
        let external_uri = match env_get(env, "HERMES_OFFHOST_URI") {
            Some(v) => {
                check_uri(&v)?;
                Some(v)
            }
            None => None,
        };
        let external_path = env_get(env, "HERMES_OFFHOST_PATH").map(|p| {
            let pb = PathBuf::from(&p);
            if pb.extension().and_then(|e| e.to_str()) == Some("json") {
                pb
            } else {
                pb.join("continuity.json")
            }
        });
        let parent = cfg
            .spend_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let local_path = env_get(env, "HERMES_OFFHOST_LOCAL")
            .map(PathBuf::from)
            .unwrap_or_else(|| parent.join("offhost").join("continuity.json"));
        let config_path = env_get(env, "HERMES_RS_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| cfg.home.join("config.yaml"));
        let seal = match env_get(env, "HERMES_OFFHOST_HMAC_FILE") {
            Some(p) => Some(crate::offhost_guard::SealKey::load(
                Path::new(&p),
                external_path.as_deref(),
            )?),
            None => None,
        };
        if seal.is_none() && (external_path.is_some() || external_uri.is_some()) {
            return Err(
                "HERMES_OFFHOST_HMAC_FILE is required when HERMES_OFFHOST_PATH or HERMES_OFFHOST_URI is set (unsigned off-host snapshots are refused)"
                    .into(),
            );
        }
        let seal_prev = Self::load_prev(env, seal.as_ref(), external_path.as_deref())?;
        Ok(OffhostCfg {
            enabled,
            role,
            resume,
            home: cfg.home.clone(),
            interval_s,
            stale_after_s,
            max_bytes,
            local_path,
            external_path,
            external_uri,
            survival_path: env_get(env, "HERMES_OFFHOST_SURVIVAL").map(PathBuf::from),
            gateway_state: cfg.home.join("gateway_state.json"),
            cron_jobs: cfg.home.join("cron").join("jobs.json"),
            cron_state: cfg.cron_state_file.clone(),
            spend_file: cfg.spend_file.clone(),
            state_db: cfg.state_db.clone(),
            config_path,
            host: cfg.host.clone(),
            port: cfg.port,
            seal,
            seal_prev,
        })
    }

    pub fn external_state_uri(&self) -> Option<String> {
        if let Some(u) = &self.external_uri {
            return Some(public_locator(u));
        }
        self.external_path.as_ref().map(|p| p.display().to_string())
    }

    /// Slot 340: the previous key of a rotation window. Both settings come
    /// together, the current key must be set, and the two keys must differ.
    fn load_prev(
        env: &dyn Fn(&str) -> Option<String>,
        cur: Option<&crate::offhost_guard::SealKey>,
        store: Option<&Path>,
    ) -> Result<Option<crate::offhost_guard::PrevKey>, String> {
        let file = env_get(env, "HERMES_OFFHOST_HMAC_PREV_FILE");
        let until = env_get(env, "HERMES_OFFHOST_HMAC_PREV_UNTIL");
        let (file, until) = match (file, until) {
            (None, None) => return Ok(None),
            (Some(_), None) => {
                return Err(
                    "HERMES_OFFHOST_HMAC_PREV_UNTIL is required with HERMES_OFFHOST_HMAC_PREV_FILE (a rotation window must end)"
                        .into(),
                )
            }
            (None, Some(_)) => {
                return Err(
                    "HERMES_OFFHOST_HMAC_PREV_UNTIL is set but HERMES_OFFHOST_HMAC_PREV_FILE is not"
                        .into(),
                )
            }
            (Some(f), Some(u)) => (f, u),
        };
        let Some(cur) = cur else {
            return Err(
                "HERMES_OFFHOST_HMAC_PREV_FILE needs HERMES_OFFHOST_HMAC_FILE (the new key)".into(),
            );
        };
        let until = crate::offhost_guard::parse_window_end(&until, crate::time::now_secs())?;
        let key = crate::offhost_guard::SealKey::load_as(
            Path::new(&file),
            store,
            "HERMES_OFFHOST_HMAC_PREV_FILE",
        )?;
        if key.id == cur.id {
            return Err(
                "HERMES_OFFHOST_HMAC_PREV_FILE holds the same key as HERMES_OFFHOST_HMAC_FILE"
                    .into(),
            );
        }
        Ok(Some(crate::offhost_guard::PrevKey { key, until }))
    }

    /// One-line key summary for `--check-config`: public key ids and the
    /// window end only, never key bytes.
    pub fn seal_summary(&self, now: i64) -> String {
        let cur = self.seal.as_ref().map(|k| k.id.as_str()).unwrap_or("no");
        match &self.seal_prev {
            None => format!("sealed={cur} prev=no"),
            Some(p) => format!(
                "sealed={cur} prev={} prev_until={} prev_window={}",
                p.key.id,
                crate::time::zagreb_rfc3339(p.until),
                if now > p.until { "closed" } else { "open" }
            ),
        }
    }

    pub fn beat_at(
        &self,
        now: i64,
        mut put: impl FnMut(&str, &[u8]) -> Result<(), String>,
    ) -> BeatReport {
        let snap = self.build_snapshot(now);
        // F1: sealed with the host key when one is set (always, when a
        // secondary store is configured).
        let bytes = match &self.seal {
            Some(k) => k.seal(&snap),
            None => serde_json::to_vec_pretty(&snap).unwrap_or_default(),
        };
        let mut blockers = string_list(&snap["blockers"]);
        if self.external_path.is_none() && self.external_uri.is_none() {
            blockers.push(
                "no secondary store configured (set HERMES_OFFHOST_PATH or HERMES_OFFHOST_URI); the local continuity file does not survive loss of this disk"
                    .into(),
            );
        }
        let local_ok = match write_private(&self.local_path, &bytes) {
            Ok(()) => true,
            Err(e) => {
                blockers.push(format!("local continuity write failed: {}", scrub_err(&e)));
                false
            }
        };
        let path_ok = if let Some(p) = &self.external_path {
            if p == &self.local_path {
                Some(local_ok)
            } else {
                match write_private(p, &bytes) {
                    Ok(()) => Some(true),
                    Err(e) => {
                        blockers.push(format!("external path write failed: {}", scrub_err(&e)));
                        Some(false)
                    }
                }
            }
        } else {
            None
        };
        let uri_ok = if let Some(uri) = &self.external_uri {
            match put(uri, &bytes) {
                Ok(()) => Some(true),
                Err(e) => {
                    blockers.push(format!("external uri export failed: {}", scrub_err(&e)));
                    Some(false)
                }
            }
        } else {
            None
        };
        let export_ok = match (path_ok, uri_ok) {
            (None, None) => None,
            (Some(true), None) | (None, Some(true)) | (Some(true), Some(true)) => Some(true),
            _ => Some(false),
        };
        let survival_ok = self.write_survival(now, export_ok, &mut blockers);
        BeatReport {
            local_ok,
            path_ok,
            uri_ok,
            survival_ok,
            blockers,
            written_at: snap["written_at"].as_str().unwrap_or("").to_string(),
            config_sha256: snap["config_sha256"].as_str().map(str::to_string),
            session_files: snap["session_files"].as_u64().unwrap_or(0),
            bytes: bytes.len(),
            role: self.role.as_str().to_string(),
            resume_policy: self.resume.as_str().to_string(),
            external_state_uri: self.external_state_uri(),
            sealed: self.seal.is_some(),
        }
    }

    pub fn resume_at(
        &self,
        now: i64,
        mut get: impl FnMut(&str) -> Result<Vec<u8>, String>,
    ) -> ResumeReport {
        let mut blockers = Vec::new();
        let (bytes, source) = self.read_snapshot(&mut get, &mut blockers);
        let Some(bytes) = bytes else {
            if self.role == Role::Standby {
                let _ = self.write_survival(now, None, &mut blockers);
            }
            return self.resume_report(false, "none", Vec::new(), Vec::new(), blockers, None);
        };
        let doc: Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => {
                blockers.push("snapshot is not JSON; local state was not overwritten".into());
                return self.resume_report(false, &source, Vec::new(), Vec::new(), blockers, None);
            }
        };
        // F1: verify the seal before anything is parsed further or written.
        let mut verified = None;
        let snap = match (&self.seal, crate::offhost_guard::is_sealed(&doc)) {
            (Some(k), _) => {
                match crate::offhost_guard::open_rotating(k, self.seal_prev.as_ref(), &doc, now) {
                    Ok((s, v)) => {
                        verified = Some(v);
                        s
                    }
                    Err(e) => {
                        blockers.push(format!("{e}; local state was not overwritten"));
                        if self.role == Role::Standby {
                            let _ = self.write_survival(now, None, &mut blockers);
                        }
                        return self.resume_report(
                            false,
                            &source,
                            Vec::new(),
                            Vec::new(),
                            blockers,
                            None,
                        );
                    }
                }
            }
            (None, true) => {
                blockers.push(
                    "snapshot is sealed but HERMES_OFFHOST_HMAC_FILE is not set; local state was not overwritten"
                        .into(),
                );
                return self.resume_report(false, &source, Vec::new(), Vec::new(), blockers, None);
            }
            (None, false) => doc,
        };
        let written_at = snap["written_at"].as_str().map(str::to_string);
        let mut report = self.apply_snapshot(&snap, now);
        report.source = source;
        report.loaded = true;
        report.auth = match verified {
            Some(crate::offhost_guard::Verified::Current) => crate::offhost_guard::ALG.into(),
            // Slot 340: accepted inside a rotation window; the next heartbeat
            // re-seals the store with the current key.
            Some(crate::offhost_guard::Verified::Previous) => {
                format!("{}+previous-key", crate::offhost_guard::ALG)
            }
            None => "unsigned-local".into(),
        };
        report.written_at = written_at;
        report.blockers.splice(0..0, blockers);
        if self.role == Role::Standby {
            let _ = self.write_survival(now, None, &mut report.blockers);
        }
        report
    }

    fn resume_report(
        &self,
        loaded: bool,
        source: &str,
        restored: Vec<String>,
        kept: Vec<String>,
        blockers: Vec<String>,
        written_at: Option<String>,
    ) -> ResumeReport {
        ResumeReport {
            loaded,
            source: source.to_string(),
            restored,
            kept,
            blockers,
            role: self.role.as_str().to_string(),
            resume_policy: self.resume.as_str().to_string(),
            external_state_uri: self.external_state_uri(),
            written_at,
            auth: "none".into(),
        }
    }

    fn read_snapshot(
        &self,
        get: &mut impl FnMut(&str) -> Result<Vec<u8>, String>,
        blockers: &mut Vec<String>,
    ) -> (Option<Vec<u8>>, String) {
        if let Some(p) = &self.external_path {
            match fs::read(p) {
                Ok(b) if !b.is_empty() => return (Some(b), "external_path".into()),
                Ok(_) => blockers.push("external path snapshot is empty".into()),
                Err(_) => blockers.push("external path snapshot is missing".into()),
            }
        }
        if let Some(uri) = &self.external_uri {
            match get(uri) {
                Ok(b) if !b.is_empty() => return (Some(b), "uri".into()),
                Ok(_) => blockers.push("external uri snapshot is empty".into()),
                Err(e) => blockers.push(format!("external uri read failed: {}", scrub_err(&e))),
            }
        }
        match fs::read(&self.local_path) {
            Ok(b) if !b.is_empty() => (Some(b), "local".into()),
            _ => {
                blockers.push("no snapshot to resume from".into());
                (None, "none".into())
            }
        }
    }

    fn apply_snapshot(&self, snap: &Value, now: i64) -> ResumeReport {
        let mut blockers = Vec::new();
        let mut restored = Vec::new();
        let mut kept = Vec::new();
        if snap["schema"].as_u64() != Some(SCHEMA as u64) || snap["kind"].as_str() != Some(KIND) {
            blockers.push(
                "snapshot schema is not hermes-offhost v1; local state was not overwritten".into(),
            );
            return self.resume_report(true, "", restored, kept, blockers, None);
        }
        let snap_role = snap["role"].as_str().unwrap_or("primary");
        if self.role == Role::Primary && snap_role == "standby" {
            blockers.push("snapshot role is standby; primary state was not overwritten".into());
            return self.resume_report(true, "", restored, kept, blockers, None);
        }
        let snapshot_unix = snap["written_unix"].as_i64().unwrap_or(0);
        let Some(files) = snap["files"].as_array() else {
            blockers.push("snapshot has no files array".into());
            return self.resume_report(true, "", restored, kept, blockers, None);
        };
        for entry in files {
            let role = entry["role"].as_str().unwrap_or("");
            let name = entry["name"].as_str().unwrap_or("");
            let label = if name.is_empty() {
                role.to_string()
            } else {
                format!("{role}:{name}")
            };
            if !entry["included"].as_bool().unwrap_or(false) {
                if let Some(reason) = entry["reason"].as_str() {
                    if reason != "hash_only" {
                        kept.push(format!("{label}:{reason}"));
                    }
                }
                continue;
            }
            let dest = match self.dest(role, name) {
                Ok(p) => p,
                Err(e) => {
                    blockers.push(format!("{label}: {e}"));
                    continue;
                }
            };
            let body = match decode_body(entry) {
                Ok(b) => b,
                Err(e) => {
                    blockers.push(format!("{label}: {e}"));
                    continue;
                }
            };
            let sha = entry["sha256"].as_str().unwrap_or("");
            if sha.is_empty() || sha256_hex(&body) != sha {
                blockers.push(format!("{label} hash mismatch; left untouched"));
                continue;
            }
            let mode = match entry["restore"].as_str() {
                Some("never") => RestoreMode::Never,
                Some("if_missing") => RestoreMode::IfMissing,
                _ => RestoreMode::IfStale,
            };
            let (present, local_unix) = file_state(&dest);
            if !should_restore(
                self.resume,
                mode,
                present,
                local_unix,
                snapshot_unix,
                now,
                self.stale_after_s,
            ) {
                kept.push(label);
                continue;
            }
            if role == "spend" && present {
                if let Ok(local) = fs::read_to_string(&dest) {
                    if let Ok(remote) = std::str::from_utf8(&body) {
                        if prefer_local_spend(&local, remote) {
                            kept.push("spend:cap".into());
                            continue;
                        }
                    }
                }
            }
            match write_private(&dest, &body) {
                Ok(()) => restored.push(label),
                Err(e) => blockers.push(format!("{label} write failed: {}", scrub_err(&e))),
            }
        }
        self.resume_report(true, "", restored, kept, blockers, None)
    }

    fn dest(&self, role: &str, name: &str) -> Result<PathBuf, String> {
        match role {
            "gateway_state" => Ok(self.gateway_state.clone()),
            "cron_jobs" => Ok(self.cron_jobs.clone()),
            "cron_state" => Ok(self.cron_state.clone()),
            "spend" => Ok(self.spend_file.clone()),
            "session_db" => Ok(self.state_db.clone()),
            "session_file" => {
                let name = safe_file_name(name)?;
                let dir = if self.state_db.is_dir() {
                    self.state_db.clone()
                } else {
                    self.state_db.with_extension("sessions")
                };
                Ok(dir.join(name))
            }
            "config" => Err("config is hash only".into()),
            _ => Err("unknown role".into()),
        }
    }

    #[allow(clippy::vec_init_then_push)]
    fn build_snapshot(&self, now: i64) -> Value {
        let mut budget = self.max_bytes.saturating_sub(4_096);
        // Sequential pushes share `budget`. A vec![] literal would borrow it
        // more than once.
        let mut pieces = Vec::new();
        pieces.push(consider(
            &self.gateway_state,
            "gateway_state",
            "",
            "if_stale",
            &mut budget,
            true,
        ));
        pieces.push(consider(
            &self.cron_state,
            "cron_state",
            "",
            "if_stale",
            &mut budget,
            true,
        ));
        pieces.push(consider(
            &self.spend_file,
            "spend",
            "",
            "if_stale",
            &mut budget,
            true,
        ));
        pieces.push(consider(
            &self.cron_jobs,
            "cron_jobs",
            "",
            "if_missing",
            &mut budget,
            true,
        ));
        pieces.push(consider(
            &self.config_path,
            "config",
            "",
            "never",
            &mut budget,
            false,
        ));
        if self.state_db.is_file() {
            let tmp_dir = self
                .local_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            pieces.push(consider_db(&self.state_db, &tmp_dir, &mut budget));
        }
        if let Some(dir) = session_dir(&self.state_db) {
            for (name, path) in session_files(&dir) {
                pieces.push(consider(
                    &path,
                    "session_file",
                    &name,
                    "if_stale",
                    &mut budget,
                    false,
                ));
            }
        }
        let mut blockers = Vec::new();
        for p in &pieces {
            if matches!(
                p.omit_reason,
                Some("over_cap")
                    | Some("secret_like_bytes")
                    | Some("too_large")
                    | Some("unreadable")
                    | Some("unsanitizable")
            ) {
                blockers.push(format!(
                    "{} omitted: {}",
                    p.role,
                    p.omit_reason.unwrap_or("unknown")
                ));
            }
        }
        let session_files = pieces
            .iter()
            .filter(|p| p.role == "session_file" && p.name.ends_with(".jsonl") && p.body.is_some())
            .count() as u64;
        let config_sha = pieces
            .iter()
            .find(|p| p.role == "config")
            .and_then(|p| p.source_sha.clone());
        let session_db_included = pieces
            .iter()
            .any(|p| p.role == "session_db" && p.body.is_some());
        json!({
            "schema": SCHEMA,
            "kind": KIND,
            "role": self.role.as_str(),
            "written_at": crate::time::zagreb_rfc3339(now),
            "written_unix": now,
            "resume_policy": self.resume.as_str(),
            "stale_after_s": self.stale_after_s,
            "node": {
                "host": self.host,
                "port": self.port,
                "implementation": "hermes-rs",
                "version": crate::VERSION,
            },
            "config_sha256": config_sha,
            "external_state_uri": self.external_state_uri(),
            "external_path": self.external_path.as_ref().map(|p| p.display().to_string()),
            "session_files": session_files,
            "session_db_included": session_db_included,
            "blockers": blockers,
            "files": pieces.iter().map(piece_json).collect::<Vec<_>>(),
        })
    }

    fn write_survival(
        &self,
        now: i64,
        export_ok: Option<bool>,
        blockers: &mut Vec<String>,
    ) -> Option<bool> {
        let path = self.survival_path.as_ref()?;
        let patch = SurvivalOffhost {
            enabled: self.enabled,
            last_heartbeat: Some(crate::time::zagreb_rfc3339(now)),
            external_state_uri: self.external_state_uri(),
            resume_policy: self.resume.as_str().to_string(),
            role: self.role.as_str().to_string(),
            last_export_ok: export_ok,
        };
        match patch_survival(path, &patch) {
            Ok(()) => Some(true),
            Err(e) => {
                blockers.push(format!("survival.json left unchanged: {}", scrub_err(&e)));
                Some(false)
            }
        }
    }
}

fn string_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn session_dir(state_db: &Path) -> Option<PathBuf> {
    if state_db.is_dir() {
        return Some(state_db.to_path_buf());
    }
    let d = state_db.with_extension("sessions");
    d.is_dir().then_some(d)
}

fn session_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut rows: Vec<(String, PathBuf, u64)> = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    for ent in rd.flatten() {
        let Some(name) = ent.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.ends_with(".tmp") || safe_file_name(&name).is_err() {
            continue;
        }
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let len = path.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
        rows.push((name, path, len));
    }
    rows.sort_by(|a, b| {
        let ia = a.0 == "index.json";
        let ib = b.0 == "index.json";
        ib.cmp(&ia).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0))
    });
    rows.into_iter().map(|(n, p, _)| (n, p)).collect()
}

fn consider(
    path: &Path,
    role: &'static str,
    name: &str,
    restore: &'static str,
    budget: &mut usize,
    force: bool,
) -> Piece {
    let mut p = Piece {
        role,
        name: name.to_string(),
        restore,
        raw_len: 0,
        source_sha: None,
        body: None,
        text: false,
        redacted: false,
        redacted_values: 0,
        omit_reason: None,
    };
    let meta = match fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => return p,
    };
    p.raw_len = meta.len();
    if meta.len() == 0 {
        p.omit_reason = Some("empty");
        return p;
    }
    if meta.len() > HASH_LIMIT {
        p.omit_reason = Some("too_large");
        return p;
    }
    let raw = match fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            p.omit_reason = Some("unreadable");
            return p;
        }
    };
    p.source_sha = Some(sha256_hex(&raw));
    if role == "config" {
        p.omit_reason = Some("hash_only");
        return p;
    }
    if !is_utf8_text(&raw) {
        if secret_like(&raw) {
            p.omit_reason = Some("secret_like_bytes");
            return p;
        }
        if !take_budget(raw.len(), budget, force) {
            p.omit_reason = Some("over_cap");
            return p;
        }
        p.body = Some(raw);
        return p;
    }
    let text = String::from_utf8(raw).unwrap_or_default();
    let (exported, redacted) = export_text(&text);
    let bytes = exported.into_bytes();
    if secret_like(&bytes) {
        p.omit_reason = Some("secret_like_bytes");
        p.redacted = true;
        return p;
    }
    if !take_budget(bytes.len(), budget, force) {
        p.omit_reason = Some("over_cap");
        return p;
    }
    p.redacted = redacted;
    p.text = true;
    p.body = Some(bytes);
    p
}

/// F2: the SQLite session DB leaves the host only as a sanitised copy
/// (offhost_guard::export_sqlite): freed pages dropped, every TEXT/BLOB value
/// redacted with the strict list, then a final strict byte scan. One secret
/// now costs one value, not the whole DB (also softens F3). A file that is not
/// SQLite falls back to the strict raw scan; an export error omits the DB.
fn consider_db(path: &Path, tmp_dir: &Path, budget: &mut usize) -> Piece {
    #[cfg(feature = "sqlite")]
    {
        let is_sqlite = fs::File::open(path)
            .and_then(|mut f| {
                let mut head = [0u8; 16];
                f.read_exact(&mut head).map(|_| head)
            })
            .map(|h| &h == b"SQLite format 3\0")
            .unwrap_or(false);
        if is_sqlite {
            let mut p = Piece {
                role: "session_db",
                name: String::new(),
                restore: "if_stale",
                raw_len: 0,
                source_sha: None,
                body: None,
                text: false,
                redacted: false,
                redacted_values: 0,
                omit_reason: None,
            };
            let meta = match fs::metadata(path) {
                Ok(m) if m.is_file() => m,
                _ => return p,
            };
            p.raw_len = meta.len();
            if meta.len() > HASH_LIMIT {
                p.omit_reason = Some("too_large");
                return p;
            }
            match fs::read(path) {
                Ok(raw) => p.source_sha = Some(sha256_hex(&raw)),
                Err(_) => {
                    p.omit_reason = Some("unreadable");
                    return p;
                }
            }
            match crate::offhost_guard::export_sqlite(path, tmp_dir) {
                Ok(x) => {
                    if secret_like(&x.bytes) {
                        p.omit_reason = Some("secret_like_bytes");
                        p.redacted = true;
                        return p;
                    }
                    if !take_budget(x.bytes.len(), budget, false) {
                        p.omit_reason = Some("over_cap");
                        return p;
                    }
                    p.redacted = x.redacted_values > 0 || x.dropped_virtual > 0;
                    p.redacted_values = x.redacted_values;
                    p.body = Some(x.bytes);
                }
                Err(_) => p.omit_reason = Some("unsanitizable"),
            }
            return p;
        }
    }
    let _ = tmp_dir;
    consider(path, "session_db", "", "if_stale", budget, false)
}

fn take_budget(len: usize, budget: &mut usize, force: bool) -> bool {
    if len <= *budget {
        *budget = budget.saturating_sub(len);
        return true;
    }
    if force && len <= 65_536 {
        *budget = 0;
        return true;
    }
    false
}

fn is_utf8_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0)
}

fn piece_json(p: &Piece) -> Value {
    let mut v = json!({
        "role": p.role,
        "name": p.name,
        "restore": p.restore,
        "present": p.source_sha.is_some(),
        "bytes": p.raw_len,
        "included": p.body.is_some(),
        "redacted": p.redacted,
    });
    if let Some(body) = &p.body {
        v["sha256"] = json!(sha256_hex(body));
        if p.text {
            let text = String::from_utf8(body.clone()).unwrap_or_default();
            v["text"] = json!(text);
        } else {
            v["b64"] = json!(b64_encode(body));
        }
    } else if let Some(s) = &p.source_sha {
        v["sha256"] = json!(s);
    }
    if let Some(s) = &p.source_sha {
        v["source_sha256"] = json!(s);
    }
    if p.redacted_values > 0 {
        v["redacted_values"] = json!(p.redacted_values);
    }
    if let Some(reason) = p.omit_reason {
        v["reason"] = json!(reason);
    }
    v
}

fn decode_body(entry: &Value) -> Result<Vec<u8>, String> {
    if let Some(t) = entry["text"].as_str() {
        return Ok(t.as_bytes().to_vec());
    }
    if let Some(b) = entry["b64"].as_str() {
        return b64_decode(b);
    }
    Err("missing body".into())
}

pub fn should_restore(
    policy: ResumePolicy,
    mode: RestoreMode,
    local_present: bool,
    local_unix: Option<i64>,
    snapshot_unix: i64,
    now: i64,
    stale_after_s: i64,
) -> bool {
    if mode == RestoreMode::Never || policy == ResumePolicy::Never {
        return false;
    }
    if !local_present {
        return true;
    }
    if mode == RestoreMode::IfMissing || policy == ResumePolicy::IfMissing {
        return false;
    }
    let Some(local_unix) = local_unix else {
        return false;
    };
    if snapshot_unix + SKEW_S < local_unix {
        return false;
    }
    now.saturating_sub(local_unix) >= stale_after_s.max(0)
}

/// Keep the local spend file when it is not a lower ledger than the snapshot.
/// Same Zagreb day: the higher USD (then the higher call count) wins, so a
/// stale local file cannot be replaced by a smaller number and reopen the cap.
pub fn prefer_local_spend(local: &str, remote: &str) -> bool {
    let Ok(l) = serde_json::from_str::<Value>(local) else {
        return false;
    };
    let Ok(r) = serde_json::from_str::<Value>(remote) else {
        return true;
    };
    let ld = l["day"].as_str().unwrap_or("");
    let rd = r["day"].as_str().unwrap_or("");
    if ld > rd {
        return true;
    }
    if ld < rd {
        return false;
    }
    let lusd = l["day_usd"].as_f64().unwrap_or(0.0);
    let rusd = r["day_usd"].as_f64().unwrap_or(0.0);
    if lusd > rusd {
        return true;
    }
    if lusd < rusd {
        return false;
    }
    let lcalls = l["day_calls"].as_u64().unwrap_or(0);
    let rcalls = r["day_calls"].as_u64().unwrap_or(0);
    lcalls >= rcalls
}

pub fn patch_survival(path: &Path, patch: &SurvivalOffhost) -> Result<(), String> {
    let existing = if path.exists() {
        let text = fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
        if text.trim().is_empty() {
            json!({})
        } else {
            let v: Value = serde_json::from_str(&text)
                .map_err(|_| "survival.json is not JSON; left untouched".to_string())?;
            if !v.is_object() {
                return Err("survival.json is not a JSON object; left untouched".into());
            }
            v
        }
    } else {
        json!({})
    };
    let mut obj = existing;
    {
        let map = obj
            .as_object_mut()
            .ok_or_else(|| "survival.json is not a JSON object; left untouched".to_string())?;
        map.insert("offhost_enabled".into(), json!(patch.enabled));
        map.insert(
            "last_heartbeat".into(),
            match &patch.last_heartbeat {
                Some(s) => json!(s),
                None => Value::Null,
            },
        );
        map.insert(
            "external_state_uri".into(),
            match &patch.external_state_uri {
                Some(s) => json!(s),
                None => Value::Null,
            },
        );
        map.insert("resume_policy".into(), json!(patch.resume_policy));
        map.insert("offhost_role".into(), json!(patch.role));
        map.insert("continuity_schema".into(), json!(SCHEMA));
        map.insert(
            "last_export_ok".into(),
            match patch.last_export_ok {
                Some(b) => json!(b),
                None => Value::Null,
            },
        );
    }
    let body = serde_json::to_vec_pretty(&obj).map_err(|e| e.to_string())?;
    write_private(path, &body)
}

/// Notes for a survival.json that predates off-host fields. Does not rewrite
/// the file. A cash figure is never invented here.
pub fn survival_migration_notes(doc: &Value) -> Vec<&'static str> {
    let mut n = Vec::new();
    if !doc.is_object() {
        n.push("survival.json is not a JSON object; offhost will not replace it");
        return n;
    }
    if doc.get("offhost_enabled").is_none() {
        n.push(
            "offhost_enabled is absent, so off-host continuity stays off until a heartbeat writes it",
        );
    }
    if doc.get("cash_usd").is_some() || doc.get("lane").is_some() || doc.get("when").is_some() {
        n.push("cash_usd, lane, and when belong to the cash lane; offhost does not write them");
    }
    if doc.get("volume_used_pct").is_some() {
        n.push("volume_used_pct is the desk disk line; offhost does not write it");
    }
    n
}

fn secret_key(k: &str) -> bool {
    let n = k.to_ascii_lowercase().replace('-', "_");
    const PARTS: &[&str] = &[
        "api_key",
        "apikey",
        "token",
        "secret",
        "password",
        "authorization",
        "private_key",
        "access_token",
        "refresh_token",
    ];
    PARTS
        .iter()
        .any(|p| n == *p || n.ends_with(&format!("_{p}")) || n.starts_with(&format!("{p}_")))
}

fn scrub_value(v: &mut Value) -> bool {
    let mut changed = false;
    match v {
        Value::Object(map) => {
            let before = map.len();
            map.retain(|k, _| !secret_key(k));
            if map.len() != before {
                changed = true;
            }
            for val in map.values_mut() {
                changed |= scrub_value(val);
            }
        }
        Value::Array(a) => {
            for x in a {
                changed |= scrub_value(x);
            }
        }
        Value::String(s) => {
            let n = crate::offhost_guard::strict_redact(s);
            if n != *s {
                *s = n;
                changed = true;
            }
        }
        _ => {}
    }
    changed
}

fn export_text(raw: &str) -> (String, bool) {
    let trimmed = raw.trim_start_matches('\u{feff}');
    if let Ok(mut v) = serde_json::from_str::<Value>(trimmed) {
        if v.is_object() || v.is_array() {
            let changed = scrub_value(&mut v);
            return (v.to_string(), changed);
        }
    }
    let mut changed = false;
    let mut lines_out = Vec::new();
    for line in raw.lines() {
        if let Ok(mut v) = serde_json::from_str::<Value>(line) {
            if v.is_object() || v.is_array() {
                changed |= scrub_value(&mut v);
                lines_out.push(v.to_string());
                continue;
            }
        }
        let n = crate::offhost_guard::strict_redact(line);
        changed |= n != line;
        lines_out.push(n);
    }
    let mut out = lines_out.join("\n");
    if raw.ends_with('\n') {
        out.push('\n');
    }
    (out, changed)
}

/// F2: one strict pattern list for bytes and text (see offhost_guard).
fn secret_like(bytes: &[u8]) -> bool {
    crate::offhost_guard::secret_like(bytes)
}

fn safe_file_name(name: &str) -> Result<&str, String> {
    if name.is_empty()
        || name.len() > 200
        || name.contains(['/', '\\', '\0'])
        || name.contains("..")
        || name.starts_with('.')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err("bad session file name".into());
    }
    Ok(name)
}

fn file_state(path: &Path) -> (bool, Option<i64>) {
    let Ok(meta) = fs::metadata(path) else {
        return (false, None);
    };
    if !meta.is_file() || meta.len() == 0 {
        return (false, None);
    }
    let unix = meta.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    });
    (true, unix)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    let tmp = tmp_path(path);
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))
}

fn scrub_err(e: &str) -> String {
    let r = crate::redact(e);
    let mut out: String = r.chars().take(200).collect();
    if r.chars().count() > 200 {
        out.push_str("...");
    }
    out
}

fn hide(text: &str, token: Option<&str>) -> String {
    let mut s = crate::redact(text);
    if let Some(t) = token {
        if t.len() >= 8 {
            s = s.replace(t, "[REDACTED]");
        }
    }
    scrub_err(&s)
}

pub fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    let rest = data.len() - i;
    if rest == 1 {
        let n = (data[i] as u32) << 16;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if !clean.len().is_multiple_of(4) {
        return Err("bad base64".into());
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0u32;
        for &c in chunk {
            n <<= 6;
            if c == b'=' {
                pad += 1;
            } else if pad > 0 {
                return Err("bad base64".into());
            } else {
                n |= u32::from(b64_val(c).ok_or_else(|| "bad base64".to_string())?);
            }
        }
        if pad > 2 {
            return Err("bad base64".into());
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

fn b64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn token_from_env() -> Option<String> {
    std::env::var("HERMES_OFFHOST_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub fn put_from_process(uri: &str, body: &[u8]) -> Result<(), String> {
    let token = token_from_env();
    curl_put("curl", uri, body, token.as_deref())
}

pub fn get_from_process(uri: &str) -> Result<Vec<u8>, String> {
    let token = token_from_env();
    curl_get("curl", uri, token.as_deref())
}

pub fn curl_put(curl: &str, uri: &str, body: &[u8], token: Option<&str>) -> Result<(), String> {
    curl_exchange(curl, "PUT", uri, Some(body), token).map(|_| ())
}

pub fn curl_get(curl: &str, uri: &str, token: Option<&str>) -> Result<Vec<u8>, String> {
    curl_exchange(curl, "GET", uri, None, token)
}

fn curl_exchange(
    curl: &str,
    method: &str,
    uri: &str,
    body: Option<&[u8]>,
    token: Option<&str>,
) -> Result<Vec<u8>, String> {
    check_uri(uri)?;
    if token.unwrap_or("").contains(['"', '\n', '\r', '\\']) {
        return Err("offhost token has unusable characters".into());
    }
    let body_str = match body {
        Some(b) => {
            Some(std::str::from_utf8(b).map_err(|_| "offhost body is not utf-8".to_string())?)
        }
        None => None,
    };
    let mut child = Command::new(curl)
        .args([
            "-sS",
            "--fail-with-body",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "-K",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    {
        let mut si = child.stdin.take().ok_or("curl stdin")?;
        let mut cfg = format!("request = \"{method}\"\nurl = {}\n", curl_quote(uri));
        cfg.push_str("header = \"Content-Type: application/json\"\n");
        if let Some(t) = token {
            cfg.push_str(&format!("header = \"Authorization: Bearer {t}\"\n"));
        }
        if let Some(b) = body_str {
            cfg.push_str(&format!("data-binary = {}\n", curl_quote(b)));
        }
        si.write_all(cfg.as_bytes())
            .map_err(|e| format!("curl stdin: {e}"))?;
    }
    let mut out = Vec::new();
    if let Some(mut so) = child.stdout.take() {
        so.read_to_end(&mut out).map_err(|e| e.to_string())?;
    }
    if out.len() > 16 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        return Err("offhost response is too large".into());
    }
    let status = child.wait().map_err(|e| format!("curl: {e}"))?;
    if !status.success() {
        let mut err = String::new();
        if let Some(mut se) = child.stderr.take() {
            let _ = se.read_to_string(&mut err);
        }
        return Err(hide(&err, token));
    }
    Ok(out)
}

pub fn spawn_periodic(cfg: OffhostCfg, status: Status) -> Result<(), String> {
    std::thread::Builder::new()
        .name("hermes-offhost".into())
        .spawn(move || loop {
            let now = crate::time::now_secs();
            let view = if cfg.role == Role::Standby {
                cfg.resume_at(now, get_from_process).view()
            } else {
                cfg.beat_at(now, put_from_process).view()
            };
            {
                let mut g = status.lock().unwrap_or_else(|e| e.into_inner());
                *g = view;
            }
            for _ in 0..cfg.interval_s {
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn spawn_standby(addr: &str, status: Status) -> std::io::Result<std::net::SocketAddr> {
    if !(addr.starts_with("127.0.0.1:")
        || addr.starts_with("[::1]:")
        || addr.starts_with("localhost:"))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "standby is loopback only",
        ));
    }
    let listener = std::net::TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let handler: crate::http::Handler = Arc::new(move |req| standby_handle(&status, req));
    std::thread::Builder::new()
        .name("hermes-standby".into())
        .spawn(move || {
            let _ = crate::http::serve(listener, handler, 16);
        })?;
    Ok(local)
}

fn standby_handle(status: &Status, req: crate::http::Request) -> crate::http::Response {
    let ok = req.method == "GET"
        && matches!(
            req.path.as_str(),
            "/health" | "/v1/health" | "/health/detailed"
        );
    if !ok {
        return crate::http::error(
            404,
            "standby serves health only and does not call a model provider",
            "invalid_request_error",
            Some("standby"),
        );
    }
    let snap = status.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut body = json!({
        "status": "ok",
        "platform": "hermes-agent",
        "version": crate::UPSTREAM_COMPAT,
        "implementation": "hermes-rs",
        "implementation_version": crate::VERSION,
        "gateway_state": "standby",
        "offhost": snap.clone(),
    });
    if let Some(map) = body.as_object_mut() {
        if let Some(obj) = snap.as_object() {
            for (k, v) in obj {
                map.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
    }
    crate::http::Response::json(200, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "hermes-offhost-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Host-side HMAC key in its own dir (never inside the store).
    fn key_file() -> (Tmp, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let p = dir.join("offhost.key");
        fs::write(&p, b"unit-test-offhost-hmac-key-0123456789abcdef\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        (Tmp(dir), p.display().to_string())
    }

    fn home() -> (Tmp, Config) {
        let dir = scratch();
        let cfg = Config::defaults(&dir);
        (Tmp(dir), cfg)
    }

    fn now() -> i64 {
        crate::time::parse_rfc3339("2026-10-27T18:00:00+01:00").unwrap()
    }

    fn age_file(path: &Path) {
        let st = std::process::Command::new("touch")
            .args(["-d", "@1000000000"])
            .arg(path)
            .status()
            .expect("touch");
        assert!(st.success());
    }

    #[test]
    fn defaults_are_off_and_do_not_write() {
        let (tmp, cfg) = home();
        let off = OffhostCfg::load(&cfg, &|_| None).unwrap();
        assert!(!off.enabled);
        assert_eq!(off.role, Role::Primary);
        assert_eq!(off.resume, ResumePolicy::IfLocalMissingOrStale);
        assert_eq!(off.interval_s, 300);
        assert_eq!(off.stale_after_s, 21_600);
        assert_eq!(off.max_bytes, 2 * 1024 * 1024);
        assert!(off.local_path.ends_with("offhost/continuity.json"));
        assert!(off.external_uri.is_none());
        assert!(!tmp.0.join("offhost").exists());
    }

    #[test]
    fn bad_interval_and_model_host_and_userinfo_are_refused() {
        let (_tmp, cfg) = home();
        let err = match OffhostCfg::load(&cfg, &|k| {
            (k == "HERMES_OFFHOST_INTERVAL_S").then(|| "5".into())
        }) {
            Err(e) => e,
            Ok(_) => panic!("expected a bad interval"),
        };
        assert!(err.contains("HERMES_OFFHOST_INTERVAL_S"));
        assert!(!err.contains("5"));
        let err = match OffhostCfg::load(&cfg, &|k| {
            (k == "HERMES_OFFHOST_URI").then(|| "https://api.openrouter.ai/state".into())
        }) {
            Err(e) => e,
            Ok(_) => panic!("expected a refused host"),
        };
        assert!(err.contains("model host"));
        assert!(!err.contains("openrouter"));
        let err = match OffhostCfg::load(&cfg, &|k| {
            (k == "HERMES_OFFHOST_URI").then(|| "https://demo:demo@example.com/snap".into())
        }) {
            Err(e) => e,
            Ok(_) => panic!("expected a refused uri"),
        };
        assert!(err.contains("userinfo"));
        assert!(!err.contains("demo:demo"));
    }

    #[test]
    fn public_locator_strips_query() {
        assert_eq!(
            public_locator("https://example.com/snap?sig=abc"),
            "https://example.com/snap"
        );
        assert_eq!(
            public_locator("https://demo:demo@example.com/snap"),
            "https://example.com/snap"
        );
    }

    #[test]
    fn restore_matrix() {
        let stale = ResumePolicy::IfLocalMissingOrStale;
        assert!(should_restore(
            stale,
            RestoreMode::IfStale,
            false,
            None,
            10,
            100,
            50
        ));
        assert!(!should_restore(
            stale,
            RestoreMode::IfStale,
            true,
            Some(90),
            100,
            100,
            50
        ));
        assert!(should_restore(
            stale,
            RestoreMode::IfStale,
            true,
            Some(10),
            20,
            100,
            50
        ));
        assert!(!should_restore(
            stale,
            RestoreMode::IfMissing,
            true,
            Some(10),
            20,
            100,
            50
        ));
        assert!(!should_restore(
            ResumePolicy::Never,
            RestoreMode::IfStale,
            false,
            None,
            20,
            100,
            50
        ));
        assert!(!should_restore(
            ResumePolicy::IfMissing,
            RestoreMode::IfStale,
            true,
            Some(10),
            20,
            100,
            50
        ));
    }

    #[test]
    fn b64_roundtrip() {
        for s in [b"".as_slice(), b"a", b"ab", b"abc", &[0, 255, 10]] {
            let enc = b64_encode(s);
            assert_eq!(b64_decode(&enc).unwrap(), s);
        }
    }

    #[test]
    fn snapshot_hashes_config_redacts_session_and_hides_query() {
        let (tmp, mut cfg) = home();
        let secret = concat!("s", "k-", "abcdefghijklmnopqrstuvwxyz");
        fs::write(tmp.0.join("config.yaml"), "marker INLINE-KEY-MARKER\n").unwrap();
        fs::create_dir_all(tmp.0.join("cron")).unwrap();
        fs::write(
            tmp.0.join("cron/jobs.json"),
            r#"{"jobs":[{"id":"survival-cash"}]}"#,
        )
        .unwrap();
        fs::write(
            tmp.0.join("gateway_state.json"),
            r#"{"gateway_state":"running","active_agents":1}"#,
        )
        .unwrap();
        fs::write(
            tmp.0.join("rs-spend.json"),
            r#"{"day":"2026-10-27","day_usd":1.25,"day_calls":3,"sessions":{}}"#,
        )
        .unwrap();
        fs::write(
            tmp.0.join("rs-cron-state.json"),
            r#"{"survival-cash":{"last_run_at":1}}"#,
        )
        .unwrap();
        let sdir = tmp.0.join("rs-state.sessions");
        fs::create_dir_all(&sdir).unwrap();
        fs::write(
            sdir.join("index.json"),
            r#"[{"id":"sess-1","source":"api_server"}]"#,
        )
        .unwrap();
        fs::write(
            sdir.join("sess-1.jsonl"),
            format!(r#"{{"role":"user","content":"{secret}"}}"#),
        )
        .unwrap();
        cfg.state_db = tmp.0.join("rs-state.db");
        let (_kt, kf) = key_file();
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_URI" => Some("https://example.com/snap?sig=abc".into()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            "HERMES_OFFHOST_SURVIVAL" => Some(tmp.0.join("survival.json").display().to_string()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        let mut put_uri = None;
        let mut put_body = None;
        let report = off.beat_at(now(), |uri, body| {
            put_uri = Some(uri.to_string());
            put_body = Some(body.to_vec());
            Ok(())
        });
        assert!(report.local_ok);
        assert_eq!(report.uri_ok, Some(true));
        assert!(report.written_at.ends_with("+01:00"));
        assert!(!report.written_at.contains('Z'));
        let raw = fs::read_to_string(&off.local_path).unwrap();
        assert!(!raw.contains("INLINE-KEY-MARKER"));
        assert!(!raw.contains(secret));
        assert!(!raw.contains("sig=abc"));
        assert!(raw.contains("[REDACTED]"));
        assert!(raw.contains("survival-cash"));
        assert_eq!(put_uri.as_deref(), Some("https://example.com/snap?sig=abc"));
        let sent = String::from_utf8(put_body.unwrap()).unwrap();
        assert!(!sent.contains("sig=abc"));
        assert!(!sent.contains(secret));
        let env_doc: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(env_doc["kind"], crate::offhost_guard::SEALED_KIND);
        let snap = off.seal.as_ref().unwrap().open(&env_doc).unwrap();
        let cfg_bytes = fs::read(tmp.0.join("config.yaml")).unwrap();
        assert_eq!(snap["config_sha256"], sha256_hex(&cfg_bytes));
        assert_eq!(report.session_files, 1);
        let survival: Value =
            serde_json::from_str(&fs::read_to_string(tmp.0.join("survival.json")).unwrap())
                .unwrap();
        assert_eq!(survival["offhost_enabled"], true);
        assert_eq!(survival["external_state_uri"], "https://example.com/snap");
        assert_eq!(survival["resume_policy"], "if_local_missing_or_stale");
        assert!(survival.get("cash_usd").is_none());
        assert!(survival["last_heartbeat"]
            .as_str()
            .unwrap()
            .ends_with("+01:00"));
    }

    #[test]
    fn resume_restores_missing_keeps_fresh_and_jobs() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("ext").join("continuity.json");
        fs::create_dir_all(tmp.0.join("cron")).unwrap();
        fs::write(tmp.0.join("cron/jobs.json"), r#"{"jobs":[{"id":"keep"}]}"#).unwrap();
        fs::write(
            tmp.0.join("rs-spend.json"),
            r#"{"day":"2026-10-27","day_usd":2.0,"day_calls":4,"note":"new"}"#,
        )
        .unwrap();
        fs::write(tmp.0.join("rs-cron-state.json"), r#"{"job":1}"#).unwrap();
        let (_kt, kf) = key_file();
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            "HERMES_OFFHOST_STALE_S" => Some("60".into()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        let beat = off.beat_at(crate::time::now_secs(), |_u, _b| panic!("uri unset"));
        assert!(beat.local_ok);
        assert_eq!(beat.path_ok, Some(true));
        fs::write(tmp.0.join("cron/jobs.json"), r#"{"jobs":[{"id":"local"}]}"#).unwrap();
        age_file(&tmp.0.join("cron/jobs.json"));
        fs::write(
            tmp.0.join("rs-spend.json"),
            r#"{"day":"2026-10-27","day_usd":1.0,"day_calls":1,"note":"old"}"#,
        )
        .unwrap();
        age_file(&tmp.0.join("rs-spend.json"));
        fs::remove_file(tmp.0.join("rs-cron-state.json")).unwrap();
        let clock = crate::time::now_secs();
        let resumed = off.resume_at(clock, |_u| panic!("path is present"));
        assert!(resumed.loaded);
        assert_eq!(resumed.source, "external_path");
        let jobs = fs::read_to_string(tmp.0.join("cron/jobs.json")).unwrap();
        assert!(jobs.contains("local"));
        let spend = fs::read_to_string(tmp.0.join("rs-spend.json")).unwrap();
        assert!(spend.contains("new"), "{spend}");
        let cron = fs::read_to_string(tmp.0.join("rs-cron-state.json")).unwrap();
        assert!(cron.contains("job"));
        assert!(resumed.restored.iter().any(|r| r == "cron_state"));
    }

    #[test]
    fn higher_local_spend_survives_a_stale_restore() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("ext.json");
        fs::write(
            tmp.0.join("rs-spend.json"),
            r#"{"day":"2026-10-27","day_usd":1.0,"day_calls":2}"#,
        )
        .unwrap();
        let (_kt, kf) = key_file();
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            "HERMES_OFFHOST_STALE_S" => Some("60".into()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        assert!(
            off.beat_at(crate::time::now_secs(), |_u, _b| Ok(()))
                .local_ok
        );
        fs::write(
            tmp.0.join("rs-spend.json"),
            r#"{"day":"2026-10-27","day_usd":5.0,"day_calls":9}"#,
        )
        .unwrap();
        age_file(&tmp.0.join("rs-spend.json"));
        let resumed = off.resume_at(crate::time::now_secs(), |_u| panic!("no"));
        let spend = fs::read_to_string(tmp.0.join("rs-spend.json")).unwrap();
        assert!(spend.contains("5.0"), "{spend} {:?}", resumed.restored);
        assert!(resumed.kept.iter().any(|k| k.contains("spend")));
    }

    #[test]
    fn fresh_local_is_kept() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("ext.json");
        fs::write(tmp.0.join("rs-cron-state.json"), r#"{"v":"snap"}"#).unwrap();
        let (_kt, kf) = key_file();
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        let clock = crate::time::now_secs();
        off.beat_at(clock, |_u, _b| Ok(()));
        fs::write(tmp.0.join("rs-cron-state.json"), r#"{"v":"live"}"#).unwrap();
        off.resume_at(clock + 30, |_u| panic!("no"));
        let text = fs::read_to_string(tmp.0.join("rs-cron-state.json")).unwrap();
        assert!(text.contains("live"));
    }

    #[test]
    fn survival_patch_preserves_cash_and_disk_and_does_not_invent_them() {
        let (tmp, cfg) = home();
        let path = tmp.0.join("survival.json");
        let when = "2026-10-18T18:59:00+02:00";
        fs::write(
            &path,
            format!(
                r#"{{"cash_usd":3.5,"lane":"plan","when":"{when}","volume_used_pct":40.0,"note":"keep"}}"#
            ),
        )
        .unwrap();
        let notes = survival_migration_notes(
            &serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap(),
        );
        assert!(notes.iter().any(|n| n.contains("cash_usd")));
        assert!(notes
            .iter()
            .any(|n| n.contains("offhost_enabled is absent")));
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_SURVIVAL" => Some(path.display().to_string()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        let report = off.beat_at(now(), |_u, _b| Ok(()));
        assert_eq!(report.survival_ok, Some(true));
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["cash_usd"], 3.5);
        assert_eq!(v["lane"], "plan");
        assert_eq!(v["when"], when);
        assert_eq!(v["volume_used_pct"], 40.0);
        assert_eq!(v["note"], "keep");
        assert_eq!(v["offhost_enabled"], true);
        assert_eq!(v["continuity_schema"], 1);

        let bad = tmp.0.join("bad.json");
        fs::write(&bad, "not-json").unwrap();
        let err = patch_survival(
            &bad,
            &SurvivalOffhost {
                enabled: true,
                last_heartbeat: None,
                external_state_uri: None,
                resume_policy: "never".into(),
                role: "primary".into(),
                last_export_ok: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("left untouched"));
        assert_eq!(fs::read_to_string(&bad).unwrap(), "not-json");
    }

    #[test]
    fn over_cap_session_is_not_restored_as_empty() {
        let (tmp, mut cfg) = home();
        let sdir = tmp.0.join("rs-state.sessions");
        fs::create_dir_all(&sdir).unwrap();
        let mut big = b"BIG-SESSION-MARKER".to_vec();
        big.extend(std::iter::repeat_n(b'a', 80_000));
        fs::write(sdir.join("sess-1.jsonl"), &big).unwrap();
        cfg.state_db = tmp.0.join("rs-state.db");
        let (_kt, kf) = key_file();
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_MAX_BYTES" => Some("65536".into()),
            "HERMES_OFFHOST_PATH" => Some(tmp.0.join("ext.json").display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        off.beat_at(now(), |_u, _b| Ok(()));
        let raw = fs::read_to_string(&off.local_path).unwrap();
        assert!(!raw.contains("BIG-SESSION-MARKER"));
        assert!(raw.contains("over_cap"));
        fs::remove_file(sdir.join("sess-1.jsonl")).unwrap();
        off.resume_at(now() + 86_400, |_u| panic!("no"));
        assert!(!sdir.join("sess-1.jsonl").exists());
    }

    #[test]
    fn secret_like_session_db_is_omitted() {
        let (tmp, mut cfg) = home();
        let secret = concat!("s", "k-", "abcdefghijklmnopqrstuvwxyz");
        let mut bytes = vec![0u8];
        bytes.extend(secret.as_bytes());
        let db = tmp.0.join("rs-state.db");
        fs::write(&db, &bytes).unwrap();
        cfg.state_db = db.clone();
        let env = |k: &str| (k == "HERMES_OFFHOST").then(|| "1".into());
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        off.beat_at(now(), |_u, _b| Ok(()));
        let raw = fs::read_to_string(off.local_path).unwrap();
        assert!(!raw.contains(secret));
        assert!(raw.contains("secret_like_bytes"));
    }

    #[test]
    fn primary_ignores_standby_snapshot_and_bad_names() {
        let (_tmp, cfg) = home();
        let off = OffhostCfg::load(&cfg, &|k| (k == "HERMES_OFFHOST").then(|| "1".into())).unwrap();
        let snap = json!({
            "schema": 1,
            "kind": KIND,
            "role": "standby",
            "written_unix": now(),
            "files": []
        });
        let report = off.apply_snapshot(&snap, now());
        assert!(report.blockers.iter().any(|b| b.contains("standby")));
        assert!(report.restored.is_empty());

        let evil = json!({
            "schema": 1,
            "kind": KIND,
            "role": "primary",
            "written_unix": now(),
            "files": [{
                "role": "session_file",
                "name": "../evil.json",
                "included": true,
                "restore": "if_stale",
                "text": "nope",
                "sha256": sha256_hex(b"nope")
            }]
        });
        let report = off.apply_snapshot(&evil, now());
        assert!(report.blockers.iter().any(|b| b.contains("bad session")));
        assert!(!cfg.home.join("evil.json").exists());
        assert!(!cfg.home.parent().unwrap().join("evil.json").exists());
    }

    #[test]
    fn no_secondary_store_is_a_blocker_but_local_still_writes() {
        let (tmp, cfg) = home();
        fs::write(
            tmp.0.join("rs-spend.json"),
            r#"{"day":"2026-10-27","day_usd":0.0,"day_calls":0}"#,
        )
        .unwrap();
        let off = OffhostCfg::load(&cfg, &|k| (k == "HERMES_OFFHOST").then(|| "1".into())).unwrap();
        let report = off.beat_at(now(), |_u, _b| panic!("no uri"));
        assert!(report.local_ok);
        assert!(report
            .blockers
            .iter()
            .any(|b| b.contains("no secondary store")));
        assert!(off.local_path.is_file());
    }

    #[test]
    fn example_survival_json_keeps_cash_keys_null() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/survival/survival.example.json");
        let v: Value = serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap();
        assert!(v["cash_usd"].is_null());
        assert!(v["lane"].is_null());
        assert!(v["volume_used_pct"].is_null());
        assert_eq!(v["when"], "2026-10-18T18:59:00+02:00");
        assert_eq!(v["offhost_enabled"], false);
        assert_eq!(v["resume_policy"], "if_local_missing_or_stale");
        assert!(v["external_state_uri"].is_null());
        let notes = survival_migration_notes(&v);
        assert!(notes.iter().any(|n| n.contains("cash_usd")));
        assert!(notes
            .iter()
            .all(|n| !n.contains("offhost_enabled is absent")));
    }

    #[test]
    fn standby_health_is_loopback_and_not_the_agent_api() {
        let status = Arc::new(Mutex::new(json!({
            "enabled": true,
            "role": "standby",
            "resume_policy": "if_local_missing_or_stale",
            "blockers": ["no snapshot to resume from"]
        })));
        let addr = spawn_standby("127.0.0.1:0", status).unwrap();
        let health = http_get(addr, "/health");
        assert!(health.contains("200"));
        assert!(health.contains("\"gateway_state\":\"standby\""));
        assert!(!health.contains("day_usd"));
        let denied = http_get(addr, concat!("/v1", "/chat", "/completions"));
        assert!(denied.contains("404"));
        assert!(denied.contains("standby"));
        let err = spawn_standby("0.0.0.0:9", Arc::new(Mutex::new(json!({})))).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrNotAvailable);
    }

    fn http_get(addr: std::net::SocketAddr, path: &str) -> String {
        use std::net::TcpStream;
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let req = format!("GET {path} HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = String::new();
        let _ = s.read_to_string(&mut buf);
        buf
    }

    // ---- slot 314: F1 / F2 / F4 ----

    #[test]
    fn f1_secondary_store_requires_a_host_key() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("store").join("continuity.json");
        let err = match OffhostCfg::load(&cfg, &|k| match k {
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            _ => None,
        }) {
            Err(e) => e,
            Ok(_) => panic!("unsigned store must be refused"),
        };
        assert!(err.contains("HERMES_OFFHOST_HMAC_FILE is required"));
        let err = match OffhostCfg::load(&cfg, &|k| match k {
            "HERMES_OFFHOST_URI" => Some("https://store.example/snap".into()),
            _ => None,
        }) {
            Err(e) => e,
            Ok(_) => panic!("unsigned store must be refused"),
        };
        assert!(err.contains("HERMES_OFFHOST_HMAC_FILE is required"));
        // Key kept inside the store is refused (whoever writes the store
        // would hold the key).
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(ext.parent().unwrap()).unwrap();
        let inside = ext.parent().unwrap().join("k");
        fs::write(&inside, [b'q'; 40]).unwrap();
        fs::set_permissions(&inside, fs::Permissions::from_mode(0o600)).unwrap();
        let err = match OffhostCfg::load(&cfg, &|k| match k {
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(inside.display().to_string()),
            _ => None,
        }) {
            Err(e) => e,
            Ok(_) => panic!("key in store must be refused"),
        };
        assert!(err.contains("secondary store"));
        assert!(!err.contains("qqqq"));
    }

    fn tampered_job_snapshot(off: &OffhostCfg, ext: &Path) -> Value {
        // What an attacker with store write access does: open the envelope
        // (no key needed to read), add a cron job with a matching sha256,
        // and put it back (keeping the old mac, or as a plain document).
        let env_doc: Value = serde_json::from_slice(&fs::read(ext).unwrap()).unwrap();
        let mut snap: Value = serde_json::from_str(env_doc["payload"].as_str().unwrap()).unwrap();
        let evil = r#"{"jobs":[{"id":"evil314","prompt":"spend"}]}"#;
        let files = snap["files"].as_array_mut().unwrap();
        for f in files.iter_mut() {
            if f["role"] == "cron_jobs" {
                f["included"] = json!(true);
                f["text"] = json!(evil);
                f["sha256"] = json!(sha256_hex(evil.as_bytes()));
            }
        }
        let _ = off;
        let mut forged = env_doc.clone();
        forged["payload"] = json!(serde_json::to_string(&snap).unwrap());
        forged
    }

    #[test]
    fn f1_tampered_or_unsigned_snapshot_is_refused() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("ext").join("continuity.json");
        fs::create_dir_all(tmp.0.join("cron")).unwrap();
        fs::write(tmp.0.join("cron/jobs.json"), r#"{"jobs":[{"id":"good"}]}"#).unwrap();
        let (_kt, kf) = key_file();
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        let beat = off.beat_at(now(), |_u, _b| panic!("no uri"));
        assert!(beat.sealed && beat.path_ok == Some(true));
        // Genuine snapshot restores a missing jobs.json.
        fs::remove_file(tmp.0.join("cron/jobs.json")).unwrap();
        let ok = off.resume_at(now() + 10, |_u| panic!("no"));
        assert_eq!(ok.auth, "hmac-sha256");
        assert!(ok.restored.iter().any(|r| r == "cron_jobs"), "{ok:?}");
        // 1) payload edited, old mac kept -> refused, nothing written.
        let forged = tampered_job_snapshot(&off, &ext);
        fs::write(&ext, serde_json::to_vec(&forged).unwrap()).unwrap();
        fs::remove_file(tmp.0.join("cron/jobs.json")).unwrap();
        let r = off.resume_at(now() + 20, |_u| panic!("no"));
        assert!(!r.loaded);
        assert!(
            r.blockers.iter().any(|b| b.contains("MAC mismatch")),
            "{r:?}"
        );
        assert!(!tmp.0.join("cron/jobs.json").exists());
        // 2) unsigned plain document (valid per-file sha256) -> refused.
        let plain: Value = serde_json::from_str(forged["payload"].as_str().unwrap()).unwrap();
        fs::write(&ext, serde_json::to_vec(&plain).unwrap()).unwrap();
        let r = off.resume_at(now() + 30, |_u| panic!("no"));
        assert!(!r.loaded);
        assert!(r.blockers.iter().any(|b| b.contains("unsigned")), "{r:?}");
        assert!(!tmp.0.join("cron/jobs.json").exists());
        // 3) sealed with another key -> refused.
        let other = crate::offhost_guard::SealKey::from_bytes(&[b'x'; 48]).unwrap();
        fs::write(&ext, other.seal(&plain)).unwrap();
        let r = off.resume_at(now() + 40, |_u| panic!("no"));
        assert!(
            r.blockers.iter().any(|b| b.contains("different key")),
            "{r:?}"
        );
        assert!(!tmp.0.join("cron/jobs.json").exists());
    }

    /// Slot 340: a second throwaway key file in its own dir.
    fn key_file_b() -> (Tmp, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let p = dir.join("offhost-new.key");
        fs::write(&p, b"unit-test-offhost-hmac-key-NEW-fedcba9876543210\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        (Tmp(dir), p.display().to_string())
    }

    #[test]
    fn keyrot_dual_key_window_restores_then_reseals() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("ext").join("continuity.json");
        fs::create_dir_all(tmp.0.join("cron")).unwrap();
        fs::write(tmp.0.join("cron/jobs.json"), r#"{"jobs":[{"id":"good"}]}"#).unwrap();
        let (_ka, ka) = key_file();
        let (_kb, kb) = key_file_b();
        let ext_s = ext.display().to_string();
        let open_until = crate::time::zagreb_rfc3339(crate::time::now_secs() + 3600);
        // 1) Old primary (key A) seals the store.
        let old = OffhostCfg::load(&cfg, &|k| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext_s.clone()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(ka.clone()),
            _ => None,
        })
        .unwrap();
        assert!(old.seal_prev.is_none());
        assert!(old.seal_summary(now()).ends_with("prev=no"));
        assert!(old.beat_at(now(), |_u, _b| panic!("no uri")).sealed);
        let id_a = old.seal.as_ref().unwrap().id.clone();
        // 2) New key B alone (no window): refused, local state untouched.
        let b_only = OffhostCfg::load(&cfg, &|k| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext_s.clone()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kb.clone()),
            _ => None,
        })
        .unwrap();
        fs::remove_file(tmp.0.join("cron/jobs.json")).unwrap();
        let r = b_only.resume_at(now() + 10, |_u| panic!("no"));
        assert!(!r.loaded && r.blockers.iter().any(|b| b.contains("different key")));
        assert!(!tmp.0.join("cron/jobs.json").exists());
        // 3) B current + A previous, window open: restores, auth says so.
        let win = OffhostCfg::load(&cfg, &|k| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext_s.clone()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kb.clone()),
            "HERMES_OFFHOST_HMAC_PREV_FILE" => Some(ka.clone()),
            "HERMES_OFFHOST_HMAC_PREV_UNTIL" => Some(open_until.clone()),
            _ => None,
        })
        .unwrap();
        let s = win.seal_summary(now());
        assert!(
            s.contains(&format!("prev={id_a}")) && s.ends_with("prev_window=open"),
            "{s}"
        );
        let r = win.resume_at(now() + 20, |_u| panic!("no"));
        assert!(r.loaded, "{r:?}");
        assert_eq!(r.auth, "hmac-sha256+previous-key");
        assert!(r.restored.iter().any(|x| x == "cron_jobs"), "{r:?}");
        // 4) Next heartbeat re-seals the store with B.
        assert!(win.beat_at(now() + 30, |_u, _b| panic!("no uri")).sealed);
        let doc: Value = serde_json::from_slice(&fs::read(&ext).unwrap()).unwrap();
        assert_eq!(doc["key_id"], json!(win.seal.as_ref().unwrap().id));
        // 5) After the window, B alone reads it; A alone no longer does.
        fs::remove_file(tmp.0.join("cron/jobs.json")).unwrap();
        let r = b_only.resume_at(now() + 40, |_u| panic!("no"));
        assert_eq!((r.loaded, r.auth.as_str()), (true, "hmac-sha256"), "{r:?}");
        fs::remove_file(tmp.0.join("cron/jobs.json")).unwrap();
        let r = old.resume_at(now() + 50, |_u| panic!("no"));
        assert!(!r.loaded && r.blockers.iter().any(|b| b.contains("different key")));
    }

    #[test]
    fn keyrot_closed_window_and_bad_settings_are_refused() {
        let (tmp, cfg) = home();
        let ext = tmp.0.join("ext").join("continuity.json");
        fs::create_dir_all(tmp.0.join("cron")).unwrap();
        fs::write(tmp.0.join("cron/jobs.json"), r#"{"jobs":[{"id":"good"}]}"#).unwrap();
        let (_ka, ka) = key_file();
        let (_kb, kb) = key_file_b();
        let ext_s = ext.display().to_string();
        let t = |d: i64| crate::time::zagreb_rfc3339(crate::time::now_secs() + d);
        let load = |pairs: Vec<(&'static str, String)>| {
            OffhostCfg::load(&cfg, &move |k: &str| {
                pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
            })
        };
        let base = || {
            vec![
                ("HERMES_OFFHOST", "1".to_string()),
                ("HERMES_OFFHOST_PATH", ext_s.clone()),
            ]
        };
        let with = |extra: Vec<(&'static str, String)>| {
            let mut v = base();
            v.extend(extra);
            v
        };
        // Seal with A.
        let a = load(with(vec![("HERMES_OFFHOST_HMAC_FILE", ka.clone())])).unwrap();
        a.beat_at(now(), |_u, _b| panic!("no uri"));
        // Window already closed: config loads, check-config says closed,
        // resume refuses the A snapshot and leaves local state alone.
        let closed = load(with(vec![
            ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
            ("HERMES_OFFHOST_HMAC_PREV_FILE", ka.clone()),
            ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(-60)),
        ]))
        .unwrap();
        assert!(closed
            .seal_summary(crate::time::now_secs())
            .ends_with("prev_window=closed"));
        fs::remove_file(tmp.0.join("cron/jobs.json")).unwrap();
        let r = closed.resume_at(crate::time::now_secs(), |_u| panic!("no"));
        assert!(!r.loaded);
        assert!(
            r.blockers
                .iter()
                .any(|b| b.contains("rotation window closed")),
            "{r:?}"
        );
        assert!(!tmp.0.join("cron/jobs.json").exists());
        // Bad settings: each one is a config error (exit 2 in the binary).
        let cases: Vec<(Vec<(&'static str, String)>, &str)> = vec![
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_FILE", ka.clone()),
                ],
                "PREV_UNTIL is required",
            ),
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(3600)),
                ],
                "PREV_FILE is not",
            ),
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_PREV_FILE", ka.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(3600)),
                ],
                "HERMES_OFFHOST_HMAC_FILE is required",
            ),
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_FILE", kb.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(3600)),
                ],
                "same key",
            ),
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_FILE", ka.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(31 * 86_400)),
                ],
                "30 days",
            ),
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
                    ("HERMES_OFFHOST_HMAC_PREV_FILE", ka.clone()),
                    (
                        "HERMES_OFFHOST_HMAC_PREV_UNTIL",
                        crate::time::utc_rfc3339(crate::time::now_secs() + 3600),
                    ),
                ],
                "Zagreb",
            ),
            (
                vec![
                    ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
                    (
                        "HERMES_OFFHOST_HMAC_PREV_FILE",
                        tmp.0.join("missing.key").display().to_string(),
                    ),
                    ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(3600)),
                ],
                "HERMES_OFFHOST_HMAC_PREV_FILE is missing",
            ),
        ];
        for (extra, want) in cases {
            let e = load(with(extra.clone())).err().expect("config error");
            assert!(e.contains(want), "{extra:?}: {e}");
            assert!(!e.contains("unit-test-offhost"), "key bytes in error: {e}");
        }
        // Previous key inside the store is refused like the current one.
        let inside = ext.parent().unwrap().join("old.key");
        fs::copy(&ka, &inside).unwrap();
        let e = load(with(vec![
            ("HERMES_OFFHOST_HMAC_FILE", kb.clone()),
            (
                "HERMES_OFFHOST_HMAC_PREV_FILE",
                inside.display().to_string(),
            ),
            ("HERMES_OFFHOST_HMAC_PREV_UNTIL", t(3600)),
        ]))
        .err()
        .expect("config error");
        assert!(
            e.contains("PREV_FILE must not live in the secondary store"),
            "{e}"
        );
    }

    #[test]
    fn f4_plain_http_only_for_loopback() {
        let (_tmp, cfg) = home();
        let (_kt, kf) = key_file();
        for (uri, ok) in [
            ("http://store.example/snap", false),
            ("http://10.0.0.5:8080/snap", false),
            ("http://127.0.0.1.evil.example/snap", false),
            ("http://127.0.0.1:21842/snap", true),
            ("http://localhost:9/snap", true),
            ("http://[::1]:9/snap", true),
            ("https://store.example/snap", true),
        ] {
            let r = OffhostCfg::load(&cfg, &|k| match k {
                "HERMES_OFFHOST_URI" => Some(uri.into()),
                "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
                _ => None,
            });
            assert_eq!(r.is_ok(), ok, "{uri}");
            if let Err(e) = r {
                assert!(e.contains("https"), "{e}");
            }
        }
    }

    #[test]
    fn f2_review_shapes_never_leave_in_session_files() {
        let (tmp, mut cfg) = home();
        let pat = concat!(
            "github",
            "_pat_",
            "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz"
        );
        let aws = concat!("AK", "IA", "ABCDEFGHIJKLMNOP");
        let sdir = tmp.0.join("rs-state.sessions");
        fs::create_dir_all(&sdir).unwrap();
        fs::write(
            sdir.join("s.jsonl"),
            format!("{{\"role\":\"user\",\"content\":\"a {pat} b {aws}\"}}\nraw line {pat}\n"),
        )
        .unwrap();
        cfg.state_db = tmp.0.join("rs-state.db");
        let off = OffhostCfg::load(&cfg, &|k| (k == "HERMES_OFFHOST").then(|| "1".into())).unwrap();
        off.beat_at(now(), |_u, _b| Ok(()));
        let raw = fs::read_to_string(&off.local_path).unwrap();
        assert!(!raw.contains(pat) && !raw.contains(aws));
        assert!(raw.contains("[REDACTED]"));
        assert!(raw.contains("raw line"));
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn f2_sqlite_session_db_is_sanitised_not_dropped() {
        let (tmp, mut cfg) = home();
        let pat = concat!(
            "github",
            "_pat_",
            "11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyz"
        );
        let db = tmp.0.join("rs-state.db");
        {
            let store = crate::session_sqlite::SqliteStore::open_rw(&db).unwrap();
            drop(store);
            let c = rusqlite::Connection::open(&db).unwrap();
            c.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('s1','api_server',1.0)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO messages (session_id, role, content, timestamp) VALUES ('s1','user',?1,1.0)",
                [format!("my token {pat} ok")],
            )
            .unwrap();
            c.execute(
                "INSERT INTO messages (session_id, role, content, timestamp) VALUES ('s1','assistant','fine',2.0)",
                [],
            )
            .unwrap();
        }
        cfg.state_db = db.clone();
        let (_kt, kf) = key_file();
        let ext = tmp.0.join("ext").join("c.json");
        let env = |k: &str| match k {
            "HERMES_OFFHOST" => Some("1".into()),
            "HERMES_OFFHOST_PATH" => Some(ext.display().to_string()),
            "HERMES_OFFHOST_HMAC_FILE" => Some(kf.clone()),
            _ => None,
        };
        let off = OffhostCfg::load(&cfg, &env).unwrap();
        let beat = off.beat_at(now(), |_u, _b| Ok(()));
        assert!(
            beat.blockers.iter().all(|b| !b.contains("session_db")),
            "{beat:?}"
        );
        let env_doc: Value = serde_json::from_slice(&fs::read(&ext).unwrap()).unwrap();
        let snap = off.seal.as_ref().unwrap().open(&env_doc).unwrap();
        let entry = snap["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["role"] == "session_db")
            .unwrap()
            .clone();
        assert_eq!(entry["included"], true, "{entry}");
        assert_eq!(entry["redacted_values"], 1);
        let body = b64_decode(entry["b64"].as_str().unwrap()).unwrap();
        assert!(!secret_like(&body));
        // Disk loss -> resume brings the sessions back, redacted.
        fs::remove_file(&db).unwrap();
        let r = off.resume_at(now() + 5, |_u| panic!("no"));
        assert!(r.restored.iter().any(|x| x == "session_db"), "{r:?}");
        let c = rusqlite::Connection::open(&db).unwrap();
        let rows: Vec<String> = c
            .prepare("SELECT content FROM messages ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(
            rows,
            vec!["my token [REDACTED] ok".to_string(), "fine".into()]
        );
    }

    #[test]
    fn source_does_not_build_a_chat_call() {
        let src = include_str!("offhost.rs");
        assert!(!src.contains(concat!("/chat", "/completions")));
        assert!(!src.contains(concat!("OPENROUTER", "_API_KEY")));
    }
}
