// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The sector and agent registry, loaded from `config/hermes/sectors.toml`.
//!
//! Hierarchy: Hermes (the boss, one session) -> one lead agent per sector ->
//! sub-agents, one fresh session per task attempt. Every sector is mapped to
//! Gaia systems and functions from the gaia-land ownership registry
//! ([`gaia_core::System`]); a name Gaia does not know, or a system whose
//! owning function is not listed, refuses the whole file.

use gaia_core::{FunctionId, System};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Top-level file.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub hermes: HermesCfg,
    #[serde(default)]
    pub retry: RetryCfg,
    #[serde(default)]
    pub hades: HadesCfg,
    #[serde(default)]
    pub readiness: ReadinessCfg,
    #[serde(rename = "sector")]
    pub sectors: Vec<Sector>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HermesCfg {
    pub boss_agent: String,
    pub boss_model: String,
    #[serde(default = "default_reasoning")]
    pub boss_reasoning: String,
    pub sub_model: String,
    /// `mock` is the only provider that can run. `openrouter` is refused
    /// until the credit (402) is fixed and Slavik approves spend.
    pub provider: String,
    #[serde(default = "default_cap")]
    pub concurrency_cap: usize,
    #[serde(default = "default_low")]
    pub measured_limit_low: usize,
    #[serde(default = "default_high")]
    pub measured_limit_high: usize,
    #[serde(default = "default_reserved")]
    pub reserved_slots: usize,
    #[serde(default)]
    pub plans_dir: String,
}

fn default_reasoning() -> String {
    "high".into()
}
fn default_cap() -> usize {
    16
}
fn default_low() -> usize {
    18
}
fn default_high() -> usize {
    20
}
fn default_reserved() -> usize {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RetryCfg {
    pub max_attempts: u32,
    pub backoff_base_secs: u64,
    pub backoff_factor: u64,
    pub backoff_max_secs: u64,
    pub max_error_attempts: u32,
}

impl Default for RetryCfg {
    fn default() -> Self {
        RetryCfg {
            max_attempts: 8,
            backoff_base_secs: 30,
            backoff_factor: 2,
            backoff_max_secs: 600,
            max_error_attempts: 2,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HadesCfg {
    /// Path of a hades-gate config file. Empty = no HADES = refuse gated work.
    #[serde(default)]
    pub config: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessCfg {
    #[serde(default)]
    pub require_merged: Vec<RequireMerged>,
    #[serde(default)]
    pub owner: Vec<OwnerItem>,
}

/// A decision only Slavik can make; the gate fails until `decided = true`.
///
/// `closed` is for an item that no longer needs a decision because events
/// settled it (for example the branch it was about is already on main). It
/// holds the evidence, is shown in the G14 evidence, and never counts as
/// "decided": only Slavik sets `decided`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerItem {
    pub item: String,
    #[serde(default)]
    pub decided: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<String>,
}

impl OwnerItem {
    /// Still waiting for Slavik: not decided and not closed by events.
    pub fn is_open(&self) -> bool {
        let closed = self.closed.as_deref().is_some_and(|c| !c.trim().is_empty());
        !self.decided && !closed
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequireMerged {
    pub name: String,
    pub commit: String,
    pub into: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sector {
    pub id: String,
    pub name: String,
    /// Plan file name, relative to `hermes.plans_dir`.
    pub plan: String,
    pub lead: String,
    pub max_concurrent: usize,
    pub gaia_functions: Vec<String>,
    pub gaia_systems: Vec<String>,
    #[serde(default)]
    pub planner_sessions: usize,
    #[serde(default)]
    pub planner_tasks: usize,
    #[serde(default)]
    pub planner_target: usize,
    #[serde(default)]
    pub planner_useful: usize,
    #[serde(default)]
    pub note: String,
}

/// The kinds of agent Hermes knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Boss,
    Lead,
    Sub,
}

/// One agent row (boss, lead, or a sub-agent session from the ledger).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Agent {
    pub id: String,
    pub role: Role,
    pub sector: Option<String>,
    pub model: String,
    pub reports_to: Option<String>,
}

impl Registry {
    /// Parse and validate TOML text.
    ///
    /// # Errors
    /// Bad TOML, unknown keys, or a validation failure.
    pub fn parse(text: &str) -> Result<Registry, String> {
        let reg: Registry = toml::from_str(text).map_err(|e| format!("sectors.toml: {e}"))?;
        reg.validate()?;
        Ok(reg)
    }

    /// Load from a file.
    ///
    /// # Errors
    /// I/O or [`Registry::parse`] errors.
    pub fn load(path: &Path) -> Result<Registry, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Registry::parse(&text)
    }

    /// Every rule the file must satisfy.
    ///
    /// # Errors
    /// The first broken rule, as a sentence.
    pub fn validate(&self) -> Result<(), String> {
        let h = &self.hermes;
        if h.concurrency_cap == 0 {
            return Err("hermes.concurrency_cap must be at least 1".into());
        }
        if h.reserved_slots >= h.concurrency_cap {
            return Err("hermes.reserved_slots must be below concurrency_cap".into());
        }
        if h.measured_limit_low > h.measured_limit_high {
            return Err("hermes.measured_limit_low is above measured_limit_high".into());
        }
        if !matches!(h.provider.as_str(), "mock" | "openrouter") {
            return Err(format!(
                "hermes.provider {:?}: use mock or openrouter",
                h.provider
            ));
        }
        if self.retry.max_attempts == 0 || self.retry.backoff_factor == 0 {
            return Err("retry.max_attempts and retry.backoff_factor must be at least 1".into());
        }
        if self.sectors.is_empty() {
            return Err("no [[sector]] entries".into());
        }
        let mut ids = BTreeSet::new();
        for s in &self.sectors {
            if !valid_id(&s.id) {
                return Err(format!("sector id {:?}: use a-z, 0-9 and -", s.id));
            }
            if !ids.insert(s.id.as_str()) {
                return Err(format!("sector {} is listed twice", s.id));
            }
            if s.max_concurrent == 0 {
                return Err(format!(
                    "sector {}: max_concurrent must be at least 1",
                    s.id
                ));
            }
            if s.plan.contains('/') || s.plan.contains("..") || !s.plan.ends_with(".md") {
                return Err(format!(
                    "sector {}: plan must be a bare *.md file name",
                    s.id
                ));
            }
            if s.lead == h.boss_agent || !s.lead.starts_with(&format!("{}.", s.id)) {
                return Err(format!(
                    "sector {}: lead must be named {}.<name>",
                    s.id, s.id
                ));
            }
            let mut funcs = BTreeSet::new();
            for f in &s.gaia_functions {
                let fid = FunctionId::parse(f)
                    .ok_or_else(|| format!("sector {}: {f:?} is not a Gaia function", s.id))?;
                funcs.insert(fid);
            }
            if s.gaia_systems.is_empty() {
                return Err(format!("sector {}: maps to no Gaia system", s.id));
            }
            let mut used = BTreeSet::new();
            for name in &s.gaia_systems {
                let sys = System::parse(name)
                    .ok_or_else(|| format!("sector {}: {name:?} is not a Gaia system", s.id))?;
                if !funcs.contains(&sys.function()) {
                    return Err(format!(
                        "sector {}: system {name} is owned by {}, which the sector does not list",
                        s.id,
                        sys.function().name()
                    ));
                }
                used.insert(sys.function());
            }
            if let Some(extra) = funcs.difference(&used).next() {
                return Err(format!(
                    "sector {}: function {} is listed but none of its systems is",
                    s.id,
                    extra.name()
                ));
            }
        }
        Ok(())
    }

    pub fn sector(&self, id: &str) -> Option<&Sector> {
        self.sectors.iter().find(|s| s.id == id)
    }

    /// Worker slots: the cap minus the always-on sessions.
    pub fn worker_slots(&self) -> usize {
        self.hermes.concurrency_cap - self.hermes.reserved_slots
    }

    pub fn plans_dir(&self, override_dir: Option<&Path>) -> PathBuf {
        override_dir
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(&self.hermes.plans_dir))
    }

    /// Boss plus one lead per sector (sub-agents come from the ledger).
    pub fn standing_agents(&self) -> Vec<Agent> {
        let mut out = vec![Agent {
            id: self.hermes.boss_agent.clone(),
            role: Role::Boss,
            sector: None,
            model: self.hermes.boss_model.clone(),
            reports_to: None,
        }];
        for s in &self.sectors {
            out.push(Agent {
                id: s.lead.clone(),
                role: Role::Lead,
                sector: Some(s.id.clone()),
                model: self.hermes.sub_model.clone(),
                reports_to: Some(self.hermes.boss_agent.clone()),
            });
        }
        out
    }

    /// Systems covered by at least one sector, and the Gaia systems none covers.
    pub fn coverage(&self) -> (BTreeSet<&'static str>, Vec<&'static str>) {
        let mut covered = BTreeSet::new();
        for s in &self.sectors {
            for name in &s.gaia_systems {
                if let Some(sys) = System::parse(name) {
                    covered.insert(sys.name());
                }
            }
        }
        let uncovered = System::ALL
            .iter()
            .map(|s| s.name())
            .filter(|n| !covered.contains(n))
            .collect();
        (covered, uncovered)
    }
}

/// `a-z 0-9 -`, 1..=32 chars.
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: &str = r#"
[hermes]
boss_agent = "hermes"
boss_model = "m-opus"
sub_model = "m-sonnet"
provider = "mock"

[[sector]]
id = "docs"
name = "Docs"
plan = "docs.md"
lead = "docs.lead"
max_concurrent = 2
gaia_functions = ["APOLLO"]
gaia_systems = ["teacher-docs"]
"#;

    #[test]
    fn minimal_file_loads_with_defaults() {
        let r = Registry::parse(MIN).unwrap();
        assert_eq!(r.hermes.concurrency_cap, 16);
        assert_eq!(r.worker_slots(), 15);
        assert_eq!(r.retry, RetryCfg::default());
        assert_eq!(r.standing_agents().len(), 2);
    }

    #[test]
    fn unknown_gaia_system_is_refused() {
        let bad = MIN.replace("\"teacher-docs\"", "\"ares-arena\"");
        let e = Registry::parse(&bad).unwrap_err();
        assert!(e.contains("not a Gaia system"), "{e}");
    }

    #[test]
    fn system_owned_by_unlisted_function_is_refused() {
        let bad = MIN.replace("\"teacher-docs\"", "\"nerve\"");
        let e = Registry::parse(&bad).unwrap_err();
        assert!(e.contains("owned by AETHER"), "{e}");
    }

    #[test]
    fn unknown_key_and_bad_lead_are_refused() {
        assert!(Registry::parse(&MIN.replace("max_concurrent", "max_conc")).is_err());
        let e = Registry::parse(&MIN.replace("docs.lead", "hermes")).unwrap_err();
        assert!(e.contains("lead"), "{e}");
    }

    #[test]
    fn plan_path_traversal_is_refused() {
        let e = Registry::parse(&MIN.replace("\"docs.md\"", "\"../x.md\"")).unwrap_err();
        assert!(e.contains("bare"), "{e}");
    }

    #[test]
    fn repo_sectors_toml_loads_and_has_seven_sectors() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/hermes/sectors.toml");
        let r = Registry::load(&path).unwrap();
        let ids: Vec<_> = r.sectors.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "gaia",
                "nerve",
                "cortex",
                "vps-ops",
                "integration-ci",
                "security",
                "docs-whitepaper"
            ]
        );
        assert_eq!(r.hermes.concurrency_cap, 16);
        let tasks: usize = r.sectors.iter().map(|s| s.planner_tasks).sum();
        assert_eq!(tasks, 200);
    }

    #[test]
    fn owner_item_closed_by_events_is_not_open_and_not_decided() {
        let t: ReadinessCfg = toml::from_str(
            r#"
[[owner]]
item = "a"
[[owner]]
item = "b"
decided = true
[[owner]]
item = "c"
closed = "moot: branch already on main"
[[owner]]
item = "d"
closed = "  "
"#,
        )
        .unwrap();
        let open: Vec<&str> = t
            .owner
            .iter()
            .filter(|o| o.is_open())
            .map(|o| o.item.as_str())
            .collect();
        assert_eq!(open, ["a", "d"]);
        assert!(!t.owner[2].decided);
    }
}
