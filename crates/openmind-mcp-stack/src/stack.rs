// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Read-only views of the rest of the OpenMind / Galion stack.
//!
//! Every part is optional; a part that is not configured does not list its
//! tools. All HTTP goes to **loopback only** (see [`MemoryClient::new`]) and
//! only `GET` is used. Files are only read (spend ledger, readiness
//! snapshot) or only `stat`ed (backup archives: names, sizes and times,
//! never contents). Nothing here restarts, deploys, writes or reads a
//! secret file, and no key is sent to these services.
//! Galion One is out of scope (deferred by Mate, t452u): no tool reads it.
//!
//! Sources (live VPS, 2026-10-30):
//! - galion-nerve `:8670` `GET /health`, `/state`, `/alerts`, `/incidents`
//! - gaia-cortex `:8674` `GET /health`, `/heals`, `/escalations`, `/alerts`
//! - openmind-queue `:19120` `GET /healthz`, `/stats`, `/gaia/status`
//! - `/var/lib/hermes-rs/rs-spend.json` (OpenRouter spend ledger)
//! - `/var/backups/galion` (galion-backup archives) and its `offsite/` folder

use crate::memory::MemoryClient;
use crate::vault::Vault;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Largest spend ledger / readiness file read.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Most archives listed by `backup_status`.
pub const BACKUP_LIST_MAX: usize = 10;

/// Spend thresholds (USD) for the fresh OpenRouter budget.
#[derive(Debug, Clone, PartialEq)]
pub struct SpendRules {
    /// Ledger total when the fresh budget started.
    pub baseline_usd: f64,
    /// Fresh budget limit.
    pub limit_usd: f64,
    /// Stop line below the limit (agents stop before it).
    pub stop_usd: f64,
}

impl Default for SpendRules {
    fn default() -> Self {
        SpendRules {
            baseline_usd: 3.449,
            limit_usd: 5.0,
            stop_usd: 4.5,
        }
    }
}

/// Which stack parts are reachable.
#[derive(Debug, Clone, Default)]
pub struct Stack {
    /// galion-nerve.
    pub nerve: Option<MemoryClient>,
    /// gaia-cortex.
    pub cortex: Option<MemoryClient>,
    /// openmind-queue.
    pub queue: Option<MemoryClient>,
    /// Spend ledger file.
    pub spend_file: Option<PathBuf>,
    /// Spend thresholds.
    pub spend: SpendRules,
    /// galion-backup archive folder.
    pub backup_dir: Option<PathBuf>,
    /// Whole docs tree (read-only; contains the vault).
    pub docs: Option<Vault>,
    /// hermes-core readiness snapshot (JSON).
    pub readiness_file: Option<PathBuf>,
}

fn need<'a, T>(o: &'a Option<T>, what: &str) -> Result<&'a T, String> {
    o.as_ref()
        .ok_or_else(|| format!("{what} is not configured"))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn mtime(m: &fs::Metadata) -> u64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn zagreb(unix: u64) -> String {
    zagreb_time::from_unix(i64::try_from(unix).unwrap_or(0))
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    let mut m = Map::new();
    for k in keys {
        if let Some(x) = v.get(*k) {
            m.insert((*k).to_string(), x.clone());
        }
    }
    Value::Object(m)
}

fn read_small(p: &PathBuf) -> Result<(String, u64), String> {
    let meta = fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err(format!("{} is not a small regular file", p.display()));
    }
    let text = fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((text, mtime(&meta)))
}

impl Stack {
    /// Tool names this configuration lists.
    #[must_use]
    pub fn tool_names(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.nerve.is_some() {
            out.extend(["stack_health", "stack_alerts"]);
        }
        if self.cortex.is_some() {
            out.push("cortex_heals");
        }
        if self.spend_file.is_some() {
            out.push("spend_status");
        }
        if self.backup_dir.is_some() {
            out.push("backup_status");
        }
        if self.queue.is_some() {
            out.push("queue_status");
        }
        if self.docs.is_some() {
            out.extend(["docs_list", "docs_read", "docs_search"]);
        }
        if self.readiness_file.is_some() {
            out.push("hermes_readiness");
        }
        out
    }

    /// `stack_health`: nerve system verdict + one line per component, or
    /// the full record of one component.
    ///
    /// # Errors
    /// Nerve not configured or unreachable; unknown component.
    pub fn health(&self, component: Option<&str>) -> Result<Value, String> {
        let nerve = need(&self.nerve, "nerve")?;
        let health = nerve.get("/health")?;
        let state = nerve.get("/state")?;
        let comps = state["components"].as_array().cloned().unwrap_or_default();
        if let Some(name) = component.map(str::trim).filter(|s| !s.is_empty()) {
            let c = comps
                .iter()
                .find(|c| c["name"].as_str() == Some(name) || c["unit"].as_str() == Some(name))
                .ok_or_else(|| {
                    let names: Vec<&str> =
                        comps.iter().filter_map(|c| c["name"].as_str()).collect();
                    format!("unknown component {name}; known: {}", names.join(", "))
                })?;
            return Ok(json!({ "component": c, "state_ts": state["ts"] }));
        }
        let list: Vec<Value> = comps
            .iter()
            .map(|c| {
                pick(
                    c,
                    &[
                        "name",
                        "status",
                        "unit",
                        "unit_state",
                        "unit_sub",
                        "port",
                        "http",
                        "restarts_1h",
                        "reasons",
                        "last_error",
                        "status_since",
                    ],
                )
            })
            .collect();
        let cortex = self.cortex.as_ref().map(|c| match c.get("/health") {
            Ok(h) => pick(
                &h,
                &[
                    "ok",
                    "mode",
                    "commit",
                    "incidents_open",
                    "escalations_open",
                    "heals_pending",
                    "heal_verify",
                    "nerve_contact",
                ],
            ),
            Err(e) => json!({ "error": e }),
        });
        Ok(json!({
            "system": state["system"],
            "nerve": pick(&health, &["status", "score", "commit", "state_ts", "state_age_s", "state_fresh"]),
            "cortex": cortex,
            "components": list,
            "hint": "call stack_health with component=<name> for the full record of one service",
        }))
    }

    /// `stack_alerts`: newest nerve alerts, open/recent nerve incidents and
    /// cortex alerts.
    ///
    /// # Errors
    /// Nerve not configured or unreachable.
    pub fn alerts(&self, limit: usize) -> Result<Value, String> {
        let nerve = need(&self.nerve, "nerve")?;
        let mut alerts = nerve
            .get("/alerts")?
            .as_array()
            .cloned()
            .unwrap_or_default();
        alerts.sort_by(|a, b| b["ts"].as_str().cmp(&a["ts"].as_str()));
        alerts.truncate(limit);
        let inc = nerve
            .get("/incidents")
            .unwrap_or_else(|e| json!({ "error": e }));
        let incidents = json!({
            "open": inc.get("open").cloned().unwrap_or(json!([])),
            "error": inc.get("error").cloned().unwrap_or(Value::Null),
        });
        let cortex = self.cortex.as_ref().map(|c| match c.get("/alerts") {
            Ok(Value::Array(mut a)) => {
                a.sort_by(|x, y| y["ts"].as_str().cmp(&x["ts"].as_str()));
                a.truncate(limit);
                Value::Array(a)
            }
            Ok(other) => other,
            Err(e) => json!({ "error": e }),
        });
        Ok(json!({ "nerve_alerts": alerts, "nerve_incidents": incidents, "cortex_alerts": cortex }))
    }

    /// `cortex_heals`: heal-verify stats, recent heals, open escalations.
    ///
    /// # Errors
    /// Cortex not configured or unreachable.
    pub fn heals(&self, limit: usize) -> Result<Value, String> {
        let c = need(&self.cortex, "cortex")?;
        let health = c.get("/health")?;
        let mut heals = c.get("/heals")?;
        if let Some(r) = heals.get_mut("recent").and_then(Value::as_array_mut) {
            r.truncate(limit);
        }
        let esc = c
            .get("/escalations")
            .unwrap_or_else(|e| json!({ "error": e }));
        let open: Vec<Value> = esc["open"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(limit)
                    .map(|e| {
                        pick(
                            e,
                            &[
                                "id", "ts", "rule", "severity", "subject", "incident", "reason",
                                "events", "handoff",
                            ],
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "cortex": pick(&health, &["ok", "mode", "commit", "heal_verify", "heals_pending", "incidents_open", "escalations_open", "hermes_handoff", "ts"]),
            "heals": heals,
            "escalations_open": open,
            "escalations_closed_total": esc["closed_total"],
            "note": "read-only; the cortex itself decides and asks the nerve for restarts",
        }))
    }

    /// `spend_status`: OpenRouter spend ledger vs the fresh budget.
    ///
    /// # Errors
    /// Ledger not configured, unreadable or not JSON.
    pub fn spend(&self) -> Result<Value, String> {
        let p = need(&self.spend_file, "spend ledger")?;
        let (text, mt) = read_small(p)?;
        let v: Value =
            serde_json::from_str(&text).map_err(|_| "spend ledger is not JSON".to_string())?;
        let sessions = v["sessions"].as_object().cloned().unwrap_or_default();
        let total: f64 = sessions.values().filter_map(Value::as_f64).sum();
        let mut top: Vec<(String, f64)> = sessions
            .iter()
            .filter_map(|(k, x)| x.as_f64().map(|u| (k.clone(), u)))
            .filter(|(_, u)| *u > 0.0)
            .collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        top.truncate(5);
        let r = &self.spend;
        let fresh = total - r.baseline_usd;
        let state = if fresh >= r.limit_usd {
            "over limit: stop all paid calls"
        } else if fresh >= r.stop_usd {
            "past the stop line: no new paid calls"
        } else if fresh >= r.stop_usd - 0.25 {
            "close to the stop line"
        } else {
            "ok"
        };
        Ok(json!({
            "ledger_total_usd": round4(total),
            "fresh_usd": round4(fresh),
            "baseline_usd": r.baseline_usd,
            "limit_usd": r.limit_usd,
            "stop_line_usd": r.stop_usd,
            "left_to_stop_line_usd": round4((r.stop_usd - fresh).max(0.0)),
            "left_to_limit_usd": round4((r.limit_usd - fresh).max(0.0)),
            "state": state,
            "day": v["day"],
            "day_usd": v["day_usd"].as_f64().map(round4),
            "day_calls": v["day_calls"],
            "sessions": sessions.len(),
            "top_sessions": top.into_iter().map(|(k, u)| json!({ "session": k, "usd": round4(u) })).collect::<Vec<_>>(),
            "ledger_updated": zagreb(mt),
            "source": "hermes-rs spend ledger (OpenRouter + River calls made through Hermes)",
        }))
    }

    /// `backup_status`: newest galion-backup archives (metadata only).
    ///
    /// # Errors
    /// Backup folder not configured or unreadable.
    pub fn backup(&self) -> Result<Value, String> {
        let dir = need(&self.backup_dir, "backup folder")?;
        let now = now_unix();
        let archives = list_files(dir, |n| n.starts_with("galion-") && n.ends_with(".tar.gz"))?;
        let offsite = list_files(&dir.join("offsite"), |n| n.ends_with(".age")).unwrap_or_default();
        let newest = archives.first();
        let age_h = newest.map(|a| (now.saturating_sub(a.2)) as f64 / 3600.0);
        let mut problems = Vec::new();
        match newest {
            None => problems.push("no archive found".to_string()),
            Some(a) => {
                if age_h.unwrap_or(0.0) > 26.0 {
                    problems.push(format!(
                        "newest archive is {:.1} h old",
                        age_h.unwrap_or(0.0)
                    ));
                }
                if a.1 < 1024 * 1024 {
                    problems.push(format!("newest archive is only {} bytes", a.1));
                }
            }
        }
        if offsite.first().map(|o| o.2) < newest.map(|a| a.2) {
            problems.push("newest archive has no newer offsite (.age) copy".into());
        }
        let show = |v: &[(String, u64, u64)]| -> Vec<Value> {
            v.iter()
                .take(BACKUP_LIST_MAX)
                .map(|(n, b, m)| json!({ "name": n, "bytes": b, "modified": zagreb(*m) }))
                .collect()
        };
        Ok(json!({
            "state": if problems.is_empty() { "ok" } else { "check" },
            "problems": problems,
            "archives": archives.len(),
            "newest_age_h": age_h.map(|h| (h * 10.0).round() / 10.0),
            "recent": show(&archives),
            "offsite_copies": offsite.len(),
            "offsite_recent": show(&offsite[..offsite.len().min(3)]),
            "note": "metadata only; archives are not opened",
        }))
    }

    /// `queue_status`: openmind-queue (POSEIDON) health, depth and agents.
    ///
    /// # Errors
    /// Queue not configured or unreachable.
    pub fn queue(&self) -> Result<Value, String> {
        let q = need(&self.queue, "queue")?;
        let health = q.get("/healthz")?;
        let stats = q.get("/stats").unwrap_or_else(|e| json!({ "error": e }));
        let gaia = q
            .get("/gaia/status")
            .unwrap_or_else(|e| json!({ "error": e }));
        Ok(json!({
            "health": health,
            "depth": stats["depth"],
            "load": stats["load"],
            "global": stats["global"],
            "agents": stats["agents"],
            "started": stats["started"],
            "gaia": pick(&gaia, &["function", "health", "systems"]),
            "note": "the VPS work queue; the coding swarm's own task list lives on the build box, not here",
        }))
    }

    /// `hermes_readiness`: the hermes-core G1..G15 snapshot file.
    ///
    /// # Errors
    /// Not configured, missing, or not JSON.
    pub fn readiness(&self) -> Result<Value, String> {
        let p = need(&self.readiness_file, "readiness snapshot")?;
        let (text, mt) = read_small(p).map_err(|e| format!("no readiness snapshot yet ({e})"))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| "readiness snapshot is not JSON".to_string())?;
        Ok(json!({
            "live": false,
            "note": "hermes-core is not deployed as a service; this is the last recorded run of its 15 readiness checks",
            "file_modified": zagreb(mt),
            "snapshot": v,
        }))
    }
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// Regular files in `dir` whose name passes `keep`, newest first:
/// (name, bytes, modified unix). Symlinks are skipped.
fn list_files(
    dir: &PathBuf,
    keep: impl Fn(&str) -> bool,
) -> Result<Vec<(String, u64, u64)>, String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut v: Vec<(String, u64, u64)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let m = fs::symlink_metadata(e.path()).ok()?;
            (m.is_file() && keep(&name)).then(|| (name, m.len(), mtime(&m)))
        })
        .collect();
    v.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.0.cmp(&a.0)));
    Ok(v)
}
