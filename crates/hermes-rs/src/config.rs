// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Gateway configuration: read the existing Hermes `config.yaml` (subset),
//! then env overrides. Loopback defaults. Secrets are never stored here,
//! only the *names* of env vars that hold them, resolved at call time.

use crate::yaml;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCfg {
    pub name: String,
    pub base_url: String,
    /// Env var holding the key. Inline keys in config.yaml are honoured via
    /// `inline_key` but never logged or echoed.
    pub key_env: Option<String>,
    pub inline_key: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub model: String,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub key_env: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DelegationCfg {
    pub max_children: usize,
    pub max_depth: usize,
    pub max_concurrent: usize,
    pub max_iterations: usize,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub reasoning_effort: Option<String>,
    /// Galion patch (run_agent.py `_user_asked_for_helper`): only delegate
    /// when the latest user message asked for a helper.
    pub require_user_ask: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpServerCfg {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub timeout_s: u64,
    pub connect_timeout_s: u64,
    pub lazy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpendCfg {
    /// USD per session (parent + its subagents). 0 = no cap.
    pub session_usd_cap: f64,
    /// USD per Europe/Zagreb day for the whole node. 0 = no cap.
    pub daily_usd_cap: f64,
    /// Provider calls per day for the whole node. 0 = no cap.
    pub daily_call_cap: u64,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub home: PathBuf,
    pub host: String,
    pub port: u16,
    pub default_model: String,
    pub default_provider: String,
    pub context_length: usize,
    pub providers: BTreeMap<String, ProviderCfg>,
    pub aliases: BTreeMap<String, Route>,
    pub model_routes: BTreeMap<String, Route>,
    pub reasoning_effort: String,
    pub reasoning_overrides: BTreeMap<String, String>,
    pub system_prompt: Option<String>,
    pub max_turns: usize,
    pub delegation: DelegationCfg,
    pub max_concurrent_runs: usize,
    pub free_only_background: bool,
    pub background_model: Option<String>,
    pub cron_enabled: bool,
    pub cron_model: Option<String>,
    pub cron_provider: Option<String>,
    pub cron_state_file: PathBuf,
    pub cron_output_dir: PathBuf,
    pub script_timeout_s: u64,
    pub mcp_servers: Vec<McpServerCfg>,
    pub spend: SpendCfg,
    pub spend_file: PathBuf,
    /// Own session DB (read-write).
    pub state_db: PathBuf,
    /// Optional legacy Python state.db, opened read-only for history.
    pub legacy_state_db: Option<PathBuf>,
    pub history_tool_output_max_chars: usize,
    pub tool_hard_stop_after: usize,
    pub api_key_env: String,
    pub persona_file: Option<PathBuf>,
    /// Optional openmind-memory client (slot 403). Off unless
    /// `HERMES_RS_MEMORY=1` or `hermes_rs.memory.enabled: true`.
    pub memory: crate::memory::MemoryCfg,
    /// Skills (SKILL.md playbooks) on unless `HERMES_RS_SKILLS=0`.
    pub skills_enabled: bool,
    /// `$HERMES_HOME/skills`, then config `skills.external_dirs` (missing dirs are skipped at scan time).
    pub skills_dirs: Vec<PathBuf>,
    /// Rust ports of skill scripts (arxiv_search, citations) on unless `HERMES_RS_SKILL_TOOLS=0`.
    pub skill_tools_enabled: bool,
    /// Citation ledgers (one per session). Default: next to the spend file.
    pub citations_dir: Option<PathBuf>,
}

fn s(v: &Value) -> Option<String> {
    match v {
        Value::String(x) if !x.trim().is_empty() => Some(x.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn u(v: &Value, d: usize) -> usize {
    v.as_u64().map(|x| x as usize).unwrap_or(d)
}

fn route_of(v: &Value) -> Option<Route> {
    Some(Route {
        model: s(&v["model"])?,
        provider: s(&v["provider"]),
        base_url: s(&v["base_url"]),
        key_env: s(&v["key_env"]).or_else(|| s(&v["api_key_env"])),
    })
}

impl Config {
    /// Defaults only (no file, no env). Loopback, port 8642, home ".".
    pub fn defaults(home: &Path) -> Config {
        let mut providers = BTreeMap::new();
        providers.insert(
            "openrouter".to_string(),
            ProviderCfg {
                name: "openrouter".into(),
                base_url: DEFAULT_OPENROUTER_BASE.into(),
                key_env: Some("OPENROUTER_API_KEY".into()),
                inline_key: None,
                default_model: None,
            },
        );
        Config {
            home: home.to_path_buf(),
            host: "127.0.0.1".into(),
            port: 8642,
            default_model: "anthropic/claude-opus-5.5".into(),
            default_provider: "openrouter".into(),
            context_length: 65_536,
            providers,
            aliases: BTreeMap::new(),
            model_routes: BTreeMap::new(),
            reasoning_effort: "medium".into(),
            reasoning_overrides: BTreeMap::new(),
            system_prompt: None,
            max_turns: 90,
            delegation: DelegationCfg {
                max_children: 3,
                max_depth: 1,
                max_concurrent: 3,
                max_iterations: 50,
                model: None,
                provider: None,
                reasoning_effort: None,
                require_user_ask: true,
            },
            max_concurrent_runs: 3,
            free_only_background: true,
            background_model: None,
            cron_enabled: false,
            cron_model: None,
            cron_provider: None,
            cron_state_file: home.join("rs-cron-state.json"),
            cron_output_dir: home.join("rs-cron-output"),
            script_timeout_s: 120,
            mcp_servers: Vec::new(),
            spend: SpendCfg {
                session_usd_cap: 1.0,
                daily_usd_cap: 2.0,
                daily_call_cap: 500,
            },
            spend_file: home.join("rs-spend.json"),
            state_db: home.join("rs-state.db"),
            legacy_state_db: None,
            history_tool_output_max_chars: 4000,
            tool_hard_stop_after: 5,
            api_key_env: "API_SERVER_KEY".into(),
            persona_file: None,
            memory: crate::memory::MemoryCfg::default(),
            skills_enabled: true,
            skills_dirs: vec![home.join("skills")],
            skill_tools_enabled: true,
            citations_dir: None,
        }
    }

    /// Apply a parsed config.yaml (as JSON value) on top of defaults.
    pub fn apply_yaml(&mut self, v: &Value) {
        let m = &v["model"];
        if let Some(x) = s(&m["default"]) {
            self.default_model = x;
        }
        if let Some(x) = s(&m["provider"]) {
            self.default_provider = x;
        }
        if let Some(n) = m["context_length"].as_u64() {
            self.context_length = n as usize;
        }
        if let Some(base) = s(&m["base_url"]) {
            let p = self.default_provider.clone();
            let e = self
                .providers
                .entry(p.clone())
                .or_insert_with(|| ProviderCfg {
                    name: p.clone(),
                    base_url: base.clone(),
                    key_env: Some(format!(
                        "{}_API_KEY",
                        p.to_ascii_uppercase().replace('-', "_")
                    )),
                    inline_key: None,
                    default_model: None,
                });
            e.base_url = base;
        }
        if let Some(obj) = v["providers"].as_object() {
            for (name, pv) in obj {
                let base = s(&pv["api"]).or_else(|| s(&pv["base_url"]));
                let Some(base) = base else { continue };
                let key_env = s(&pv["key_env"])
                    .or_else(|| s(&pv["api_key_env"]))
                    .or_else(|| {
                        Some(format!(
                            "{}_API_KEY",
                            name.to_ascii_uppercase().replace('-', "_")
                        ))
                    });
                self.providers.insert(
                    name.clone(),
                    ProviderCfg {
                        name: name.clone(),
                        base_url: base,
                        key_env,
                        inline_key: s(&pv["api_key"]),
                        default_model: s(&pv["default_model"]),
                    },
                );
            }
        }
        if let Some(obj) = v["model_aliases"].as_object() {
            for (k, rv) in obj {
                if let Some(r) = route_of(rv) {
                    self.aliases.insert(k.clone(), r);
                }
            }
        }
        if let Some(obj) = v["platforms"]["api_server"]["extra"]["model_routes"].as_object() {
            for (k, rv) in obj {
                if let Some(r) = route_of(rv) {
                    self.model_routes.insert(k.clone(), r);
                }
            }
        }
        let a = &v["agent"];
        if let Some(x) = s(&a["reasoning_effort"]) {
            self.reasoning_effort = x;
        }
        if let Some(obj) = a["reasoning_overrides"].as_object() {
            for (k, ov) in obj {
                if let Some(x) = s(ov) {
                    self.reasoning_overrides.insert(k.clone(), x);
                }
            }
        }
        self.system_prompt = s(&a["system_prompt"]).or(self.system_prompt.take());
        self.max_turns = u(&a["max_turns"], self.max_turns);
        for d in v["skills"]["external_dirs"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(p) = s(d) {
                let p = PathBuf::from(p);
                if !self.skills_dirs.contains(&p) {
                    self.skills_dirs.push(p);
                }
            }
        }
        let d = &v["delegation"];
        self.delegation.max_children =
            u(&d["max_concurrent_children"], self.delegation.max_children);
        self.delegation.max_concurrent = self.delegation.max_children;
        self.delegation.max_depth = u(&d["max_spawn_depth"], self.delegation.max_depth);
        self.delegation.max_iterations = u(&d["max_iterations"], self.delegation.max_iterations);
        self.delegation.model = s(&d["model"]).or(self.delegation.model.take());
        self.delegation.provider = s(&d["provider"]).or(self.delegation.provider.take());
        self.delegation.reasoning_effort =
            s(&d["reasoning_effort"]).or(self.delegation.reasoning_effort.take());
        let api = &v["gateway"]["api_server"];
        self.max_concurrent_runs = u(&api["max_concurrent_runs"], self.max_concurrent_runs);
        self.history_tool_output_max_chars = u(
            &api["history_tool_output_max_chars"],
            self.history_tool_output_max_chars,
        );
        if let Some(b) = v["auxiliary"]["free_only"].as_bool() {
            self.free_only_background = b;
        }
        self.background_model =
            s(&v["auxiliary"]["openrouter_model"]).or(self.background_model.take());
        self.cron_model = s(&v["cron"]["model"]);
        self.cron_provider = s(&v["cron"]["model_provider"]);
        if let Some(n) = v["tool_loop_guardrails"]["hard_stop_after"]["exact_failure"].as_u64() {
            self.tool_hard_stop_after = n as usize;
        }
        if let Some(obj) = v["mcp_servers"].as_object() {
            self.mcp_servers.clear();
            for (name, sv) in obj {
                let Some(command) = s(&sv["command"]) else {
                    continue;
                };
                if sv["enabled"].as_bool() == Some(false) {
                    continue;
                }
                let args = sv["args"]
                    .as_array()
                    .map(|a| a.iter().filter_map(s).collect())
                    .unwrap_or_default();
                self.mcp_servers.push(McpServerCfg {
                    name: name.clone(),
                    command,
                    args,
                    timeout_s: sv["timeout"].as_u64().unwrap_or(30),
                    connect_timeout_s: sv["connect_timeout"].as_u64().unwrap_or(15),
                    lazy: sv["lazy"].as_bool().unwrap_or(false),
                });
            }
        }
        // hermes-rs only keys (ignored by Python Hermes)
        let rs = &v["hermes_rs"];
        if let Some(x) = rs["session_usd_cap"].as_f64() {
            self.spend.session_usd_cap = x;
        }
        if let Some(x) = rs["daily_usd_cap"].as_f64() {
            self.spend.daily_usd_cap = x;
        }
        if let Some(x) = rs["daily_call_cap"].as_u64() {
            self.spend.daily_call_cap = x;
        }
        // The Python `memory.provider` key is ignored on purpose: only the
        // hermes-rs key (or HERMES_RS_MEMORY=1) turns the client on.
        self.memory.apply_yaml(&rs["memory"]);
    }

    /// Apply env overrides. `env` is injected so tests do not touch the
    /// process environment.
    pub fn apply_env(&mut self, env: &dyn Fn(&str) -> Option<String>) -> Result<(), String> {
        let get = |k: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        if let Some(h) = get("HERMES_RS_HOST") {
            self.host = h;
        }
        if let Some(p) = get("HERMES_RS_PORT").or_else(|| get("API_SERVER_PORT")) {
            self.port = p.parse().map_err(|_| format!("bad port: {p}"))?;
        }
        if let Some(x) = get("HERMES_RS_STATE_DB") {
            self.state_db = PathBuf::from(x);
        }
        if let Some(x) = get("HERMES_RS_LEGACY_STATE_DB") {
            self.legacy_state_db = Some(PathBuf::from(x));
        }
        if let Some(x) = get("HERMES_RS_SPEND_FILE") {
            self.spend_file = PathBuf::from(x);
        }
        if let Some(x) = get("HERMES_RS_SESSION_USD_CAP") {
            self.spend.session_usd_cap = x
                .parse()
                .map_err(|_| "bad HERMES_RS_SESSION_USD_CAP".to_string())?;
        }
        if let Some(x) = get("HERMES_RS_DAILY_USD_CAP") {
            self.spend.daily_usd_cap = x
                .parse()
                .map_err(|_| "bad HERMES_RS_DAILY_USD_CAP".to_string())?;
        }
        if let Some(x) = get("HERMES_RS_DAILY_CALL_CAP") {
            self.spend.daily_call_cap = x
                .parse()
                .map_err(|_| "bad HERMES_RS_DAILY_CALL_CAP".to_string())?;
        }
        if let Some(x) = get("HERMES_RS_CRON") {
            self.cron_enabled = matches!(x.as_str(), "1" | "true" | "yes" | "on");
        }
        if let Some(x) = get("HERMES_RS_CRON_STATE") {
            self.cron_state_file = PathBuf::from(x);
        }
        if let Some(x) = get("HERMES_RS_CRON_OUTPUT") {
            self.cron_output_dir = PathBuf::from(x);
        }
        // HERMES_RS_MCP=0 skips every configured MCP server (shadow runs).
        if get("HERMES_RS_MCP").is_some_and(|x| matches!(x.as_str(), "0" | "false" | "no" | "off"))
        {
            self.mcp_servers.clear();
        }
        if let Some(x) = get("HERMES_RS_PERSONA") {
            self.persona_file = Some(PathBuf::from(x));
        }
        if let Some(x) = get("OPENROUTER_BASE_URL") {
            if let Some(p) = self.providers.get_mut("openrouter") {
                p.base_url = x;
            }
        }
        if let Some(x) = get("HERMES_RS_SKILLS") {
            self.skills_enabled = !matches!(x.as_str(), "0" | "false" | "off");
        }
        if let Some(x) = get("HERMES_RS_SKILLS_DIRS") {
            self.skills_dirs = x
                .split(':')
                .filter(|p| !p.trim().is_empty())
                .map(PathBuf::from)
                .collect();
        }
        if let Some(x) = get("HERMES_RS_SKILL_TOOLS") {
            self.skill_tools_enabled = !matches!(x.as_str(), "0" | "false" | "off");
        }
        // Helper (delegate_task child) model without editing config.yaml,
        // e.g. HERMES_RS_DELEGATION_MODEL=river-flash (an alias keeps its own provider).
        if let Some(x) = get("HERMES_RS_DELEGATION_MODEL").filter(|x| !x.trim().is_empty()) {
            self.delegation.model = Some(x.trim().to_string());
        }
        if let Some(x) = get("HERMES_RS_DELEGATION_PROVIDER").filter(|x| !x.trim().is_empty()) {
            self.delegation.provider = Some(x.trim().to_string());
        }
        if let Some(x) = get("HERMES_RS_DELEGATION_REASONING").filter(|x| !x.trim().is_empty()) {
            self.delegation.reasoning_effort = Some(x.trim().to_string());
        }
        if let Some(x) = get("HERMES_RS_CITATIONS_DIR") {
            self.citations_dir = Some(PathBuf::from(x));
        }
        self.memory.apply_env(&get)?;
        Ok(())
    }

    /// Full load: defaults, `$HERMES_HOME/config.yaml` if present, env.
    pub fn load(env: &dyn Fn(&str) -> Option<String>) -> Result<Config, String> {
        let home = env("HERMES_HOME")
            .filter(|h| !h.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut cfg = Config::defaults(&home);
        let file = env("HERMES_RS_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("config.yaml"));
        if file.exists() {
            let text = std::fs::read_to_string(&file)
                .map_err(|e| format!("read {}: {e}", file.display()))?;
            let v = yaml::parse(&text).map_err(|e| format!("parse {}: {e}", file.display()))?;
            cfg.apply_yaml(&v);
        }
        cfg.apply_env(env)?;
        Ok(cfg)
    }

    /// The gateway API key from env (never from a file the console reads).
    pub fn api_key(&self, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        env(&self.api_key_env)
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
    }

    pub fn is_loopback(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "::1" | "localhost")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GALION: &str = r#"
model:
  provider: openrouter
  default: anthropic/claude-opus-5.5
  context_length: 65536
  base_url: https://openrouter.ai/api/v1
model_aliases:
  boss:
    model: anthropic/claude-opus-5.5
    provider: openrouter
  worker:
    model: anthropic/claude-sonnet-5.5
    provider: openrouter
providers:
  river:
    api: https://api.river.ai/v1
    default_model: deepseek-ai/DeepSeek-V4.1-Flash
agent:
  max_turns: 500
  reasoning_effort: "high"
  reasoning_overrides:
    "anthropic/claude-haiku-5.5": "none"
gateway:
  api_server:
    max_concurrent_runs: 3
delegation:
  max_iterations: 250
  max_concurrent_children: 3
  max_spawn_depth: 1
  model: "anthropic/claude-sonnet-5.5"
  provider: "openrouter"
  reasoning_effort: "medium"
auxiliary:
  free_only: true
platforms:
  api_server:
    extra:
      model_routes:
        galion:
          model: "anthropic/claude-haiku-5.5"
          provider: "openrouter"
mcp_servers:
  galion-stack:
    command: /usr/bin/stack
    args: ["/x.py"]
    timeout: 30
    lazy: true
"#;

    #[test]
    fn galion_config_maps() {
        let mut c = Config::defaults(Path::new("/h"));
        c.apply_yaml(&yaml::parse(GALION).unwrap());
        assert_eq!(c.default_model, "anthropic/claude-opus-5.5");
        assert_eq!(c.context_length, 65_536);
        assert_eq!(c.model_routes["galion"].model, "anthropic/claude-haiku-5.5");
        assert_eq!(c.aliases["worker"].model, "anthropic/claude-sonnet-5.5");
        assert_eq!(
            c.providers["river"].key_env.as_deref(),
            Some("RIVER_API_KEY")
        );
        assert_eq!(c.delegation.max_children, 3);
        assert_eq!(c.delegation.max_depth, 1);
        assert_eq!(c.delegation.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(c.reasoning_overrides["anthropic/claude-haiku-5.5"], "none");
        assert_eq!(c.max_turns, 500);
        assert_eq!(c.mcp_servers[0].name, "galion-stack");
        assert!(c.free_only_background);
    }

    /// Slot 403: the live config.yaml still names the Python memory plugin;
    /// that alone must never switch the hermes-rs client on.
    #[test]
    fn memory_client_off_unless_hermes_rs_key_or_env() {
        let live = "memory:\n  provider: memory_tencentdb\n  memory_enabled: true\n";
        let mut c = Config::defaults(Path::new("/h"));
        c.apply_yaml(&yaml::parse(live).unwrap());
        c.apply_env(&|_k: &str| None).unwrap();
        assert!(!c.memory.enabled);
        let on = "hermes_rs:\n  memory:\n    enabled: true\n    recall_timeout_ms: 1500\n";
        let mut d = Config::defaults(Path::new("/h"));
        d.apply_yaml(&yaml::parse(on).unwrap());
        assert!(d.memory.enabled);
        assert_eq!(d.memory.recall_timeout.as_millis(), 1500);
        let env = |k: &str| (k == "HERMES_RS_MEMORY").then(|| "0".to_string());
        d.apply_env(&env).unwrap();
        assert!(!d.memory.enabled, "env wins over yaml");
        let env = |k: &str| (k == "HERMES_RS_MEMORY").then(|| "1".to_string());
        c.apply_env(&env).unwrap();
        assert!(c.memory.enabled);
        assert_eq!(c.memory.url, "http://127.0.0.1:8420");
    }

    #[test]
    fn env_overrides_and_loopback_default() {
        let mut c = Config::defaults(Path::new("/h"));
        assert!(c.is_loopback());
        let env = |k: &str| match k {
            "HERMES_RS_PORT" => Some("8652".to_string()),
            "API_SERVER_PORT" => Some("8642".to_string()),
            "HERMES_RS_CRON" => Some("1".to_string()),
            "HERMES_RS_MCP" => Some("off".to_string()),
            _ => None,
        };
        c.apply_env(&env).unwrap();
        assert_eq!(c.port, 8652);
        assert!(c.cron_enabled);
        let mut d = Config::defaults(Path::new("/h"));
        d.mcp_servers.push(McpServerCfg {
            name: "x".into(),
            command: "true".into(),
            args: vec![],
            timeout_s: 1,
            connect_timeout_s: 1,
            lazy: false,
        });
        d.apply_env(&env).unwrap();
        assert!(d.mcp_servers.is_empty());
    }
}
