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

/// How hermes-rs talks to an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpTransport {
    /// Child process, newline-delimited JSON-RPC on stdin/stdout.
    #[default]
    Stdio,
    /// MCP "Streamable HTTP" (2025-03-26+): one POST per message, JSON or SSE reply.
    Http,
    /// Legacy HTTP+SSE (2024-11-05): GET event stream + POST to the `endpoint` event URL.
    Sse,
}

/// What a server's tools can do, as far as the deny list is concerned.
/// `Read` uses the tool-name hints only; `Write` makes every tool a write
/// tool (paths are write targets, vault writes refused); `Shell` makes every
/// tool a shell tool (needs an inspectable command argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpClass {
    #[default]
    Read,
    Write,
    Shell,
}

/// How write tools of a server with writes off are exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpWriteTools {
    #[default]
    Hidden,
    Gated,
}

/// One MCP server. Secrets never live here: only the path of a per-server
/// env file (mode 0600, or a systemd credential) whose values are read at
/// connect time and handed to that server alone.
#[derive(Debug, Clone, PartialEq)]
pub struct McpServerCfg {
    pub name: String,
    /// stdio: program to run (empty for url servers).
    pub command: String,
    pub args: Vec<String>,
    /// http / sse: server URL.
    pub url: Option<String>,
    pub transport: McpTransport,
    pub timeout_s: u64,
    pub connect_timeout_s: u64,
    /// Lazy: discover tools at startup (in parallel), then stop the server;
    /// it is started again on the first call.
    pub lazy: bool,
    /// Literal env for the child; `${NAME}` is replaced from `env_file`.
    pub env: BTreeMap<String, String>,
    /// `KEY=VALUE` secrets file (mode 0600/0400, owner root or us), or
    /// `credential:<name>` for `$CREDENTIALS_DIRECTORY/<name>` (LoadCredential=).
    pub env_file: Option<String>,
    /// Names from `env_file` passed to a stdio child. None = all of them.
    pub pass_env: Option<Vec<String>>,
    /// HTTP headers (url servers); `${NAME}` is replaced from `env_file`.
    pub headers: BTreeMap<String, String>,
    /// Secrets that must be present, else the server waits (not started).
    pub require_env: Vec<String>,
    /// Only these tools are registered (exact names, or `prefix*`). None = all.
    pub allow_tools: Option<Vec<String>>,
    pub deny_tools: Vec<String>,
    pub class: McpClass,
    /// Writes are OFF unless set: tools that are not read-only (MCP
    /// `readOnlyHint`, or no write hint in the name) are not registered.
    pub allow_writes: bool,
    /// What "writes off" means for a write tool: `hidden` (default, not
    /// registered) or `gated` (registered with its exact name and schema,
    /// but every call is refused until `allow_writes: true`).
    pub write_tools: McpWriteTools,
    /// Our own vetted server (e.g. github-mcp-rs): descriptions are passed
    /// through as written (still capped and control-stripped) instead of
    /// being labelled untrusted. Tool OUTPUT is always labelled untrusted.
    pub trusted_descriptions: bool,
    /// Cap on the text a tool call returns to the model.
    pub max_result_chars: usize,
    /// Cap on each tool description (server text is untrusted).
    pub max_description_chars: usize,
    pub max_tools: usize,
    /// `uid:gid` the stdio child (and curl) runs as when the gateway is
    /// root; `root` keeps root (logged loudly). None = HERMES_RS_MCP_RUN_AS
    /// or 65534:65534.
    pub run_as: Option<String>,
    /// Working directory of a stdio child (default `/`).
    pub cwd: Option<String>,
    /// `oauth` is not implemented yet (TODO): such servers are not started.
    pub auth: Option<String>,
    /// Where the entry came from (config.yaml or the MCP servers file).
    pub source: String,
}

impl Default for McpServerCfg {
    fn default() -> Self {
        McpServerCfg {
            name: String::new(),
            command: String::new(),
            args: Vec::new(),
            url: None,
            transport: McpTransport::Stdio,
            timeout_s: 30,
            connect_timeout_s: 15,
            lazy: false,
            env: BTreeMap::new(),
            env_file: None,
            pass_env: None,
            headers: BTreeMap::new(),
            require_env: Vec::new(),
            allow_tools: None,
            deny_tools: Vec::new(),
            class: McpClass::Read,
            allow_writes: false,
            write_tools: McpWriteTools::Hidden,
            trusted_descriptions: false,
            max_result_chars: 50_000,
            max_description_chars: 1_024,
            max_tools: 128,
            run_as: None,
            cwd: None,
            auth: None,
            source: String::new(),
        }
    }
}

fn str_list(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::Array(a) => Some(a.iter().filter_map(s).collect()),
        Value::String(x) => Some(
            x.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
        ),
        _ => None,
    }
}

fn str_map(v: &Value) -> BTreeMap<String, String> {
    v.as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, x)| match x {
                    Value::String(t) => Some((k.clone(), t.clone())),
                    Value::Number(n) => Some((k.clone(), n.to_string())),
                    Value::Bool(b) => Some((k.clone(), b.to_string())),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

impl McpServerCfg {
    /// Parse one `mcp_servers:` entry. `Ok(None)` = disabled. `Err` = a
    /// broken entry (logged loudly by the caller, never silently dropped).
    pub fn from_value(name: &str, sv: &Value, source: &str) -> Result<Option<Self>, String> {
        if !sv.is_object() {
            return Err(format!("mcp server {name} ({source}): entry is not a map"));
        }
        if sv["enabled"].as_bool() == Some(false) {
            return Ok(None);
        }
        let d = McpServerCfg::default();
        let command = s(&sv["command"]).unwrap_or_default();
        let url = s(&sv["url"]);
        let transport = match s(&sv["transport"]).as_deref().map(str::to_ascii_lowercase) {
            None => {
                if url.is_some() && command.is_empty() {
                    McpTransport::Http
                } else {
                    McpTransport::Stdio
                }
            }
            Some(t) => match t.as_str() {
                "stdio" => McpTransport::Stdio,
                "http" | "streamable_http" | "streamable-http" | "streamablehttp" => {
                    McpTransport::Http
                }
                "sse" => McpTransport::Sse,
                other => {
                    return Err(format!(
                        "mcp server {name} ({source}): unknown transport {other:?}"
                    ))
                }
            },
        };
        match transport {
            McpTransport::Stdio if command.is_empty() => {
                return Err(format!(
                    "mcp server {name} ({source}): stdio needs `command:` (got neither command nor a usable url)"
                ))
            }
            McpTransport::Http | McpTransport::Sse if url.is_none() => {
                return Err(format!("mcp server {name} ({source}): needs `url:`"))
            }
            _ => {}
        }
        if let Some(u) = &url {
            if !(u.starts_with("https://") || u.starts_with("http://")) {
                return Err(format!(
                    "mcp server {name} ({source}): url must be http(s)://"
                ));
            }
        }
        let class = match s(&sv["class"]).as_deref().map(str::to_ascii_lowercase) {
            None => McpClass::Read,
            Some(c) => match c.as_str() {
                "read" | "read_only" | "readonly" => McpClass::Read,
                "write" => McpClass::Write,
                "shell" | "exec" => McpClass::Shell,
                other => {
                    return Err(format!(
                        "mcp server {name} ({source}): unknown class {other:?} (read, write, shell)"
                    ))
                }
            },
        };
        // Old Python-Hermes key: `env:` inline. Values that look like real
        // secrets are refused here; put them in `env_file`.
        let env = str_map(&sv["env"]);
        for (k, v) in &env {
            if !v.contains("${") && crate::redact(&format!("{k}={v}")) != format!("{k}={v}") {
                return Err(format!(
                    "mcp server {name} ({source}): env {k} looks like an inline secret; move it to env_file"
                ));
            }
        }
        let headers = str_map(&sv["headers"]);
        for (k, v) in &headers {
            if k.eq_ignore_ascii_case("authorization") && !v.contains("${") {
                return Err(format!(
                    "mcp server {name} ({source}): inline Authorization header; use \"Bearer ${{NAME}}\" with env_file"
                ));
            }
        }
        let write_tools = match s(&sv["write_tools"]).as_deref() {
            None | Some("hidden") => McpWriteTools::Hidden,
            Some("gated") => McpWriteTools::Gated,
            Some(o) => {
                return Err(format!(
                    "mcp server {name} ({source}): write_tools must be hidden or gated, not {o:?}"
                ))
            }
        };
        let u64_or = |k: &str, dflt: u64| sv[k].as_u64().unwrap_or(dflt);
        Ok(Some(McpServerCfg {
            name: name.to_string(),
            command,
            args: sv["args"]
                .as_array()
                .map(|a| a.iter().filter_map(s).collect())
                .unwrap_or_default(),
            url,
            transport,
            timeout_s: u64_or("timeout", d.timeout_s),
            connect_timeout_s: u64_or("connect_timeout", d.connect_timeout_s),
            lazy: sv["lazy"].as_bool().unwrap_or(false),
            env,
            env_file: s(&sv["env_file"]).or_else(|| s(&sv["secrets_file"])),
            pass_env: str_list(&sv["pass_env"]),
            headers,
            require_env: str_list(&sv["require_env"]).unwrap_or_default(),
            allow_tools: str_list(&sv["allow_tools"]).or_else(|| str_list(&sv["tools"])),
            deny_tools: str_list(&sv["deny_tools"]).unwrap_or_default(),
            class,
            allow_writes: sv["allow_writes"].as_bool().unwrap_or(false),
            write_tools,
            trusted_descriptions: sv["trusted_descriptions"].as_bool().unwrap_or(false),
            max_result_chars: u64_or("max_result_chars", d.max_result_chars as u64) as usize,
            max_description_chars: u64_or("max_description_chars", d.max_description_chars as u64)
                as usize,
            max_tools: u64_or("max_tools", d.max_tools as u64) as usize,
            run_as: s(&sv["run_as"]),
            cwd: s(&sv["cwd"]),
            auth: s(&sv["auth"]),
            source: source.to_string(),
        }))
    }
}

/// Merge `mcp_servers:` from `v` into `list` (same name = replaced).
/// Returns loud warnings for broken entries.
pub fn merge_mcp_servers(list: &mut Vec<McpServerCfg>, v: &Value, source: &str) -> Vec<String> {
    let mut warn = Vec::new();
    let Some(obj) = v["mcp_servers"].as_object() else {
        if !v["mcp_servers"].is_null() {
            warn.push(format!("{source}: mcp_servers is not a map; ignored"));
        }
        return warn;
    };
    for (name, sv) in obj {
        match McpServerCfg::from_value(name, sv, source) {
            Ok(Some(c)) => {
                list.retain(|x| x.name != *name);
                list.push(c);
            }
            Ok(None) => list.retain(|x| x.name != *name),
            Err(e) => warn.push(format!("NOT LOADED: {e}")),
        }
    }
    warn
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
    /// Broken / skipped MCP entries, logged loudly at startup.
    pub mcp_warnings: Vec<String>,
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
    /// Hard model lock from `HERMES_RS_MODEL_LOCK` (env only, never
    /// config.yaml). When set, every provider call uses this one model.
    pub model_lock: Option<crate::model_lock::ModelLock>,
    /// What the lock changed in the loaded config (logged at startup).
    pub model_lock_notes: Vec<String>,
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
            mcp_warnings: Vec::new(),
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
            model_lock: None,
            model_lock_notes: Vec::new(),
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
        if v["mcp_servers"].is_object() {
            self.mcp_servers.clear();
            let w = merge_mcp_servers(&mut self.mcp_servers, v, "config.yaml");
            self.mcp_warnings.extend(w);
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
        // Extra MCP servers file (config.yaml stays untouched): same
        // `mcp_servers:` shape; an entry with the same name replaces the
        // config.yaml one, `enabled: false` removes it.
        if let Some(x) = get("HERMES_RS_MCP_SERVERS_FILE") {
            match std::fs::read_to_string(&x) {
                Ok(text) => match yaml::parse(&text) {
                    Ok(v) => {
                        let w = merge_mcp_servers(&mut self.mcp_servers, &v, &x);
                        self.mcp_warnings.extend(w);
                    }
                    Err(e) => self
                        .mcp_warnings
                        .push(format!("NOT LOADED: MCP servers file {x}: parse: {e}")),
                },
                Err(e) => self
                    .mcp_warnings
                    .push(format!("NOT LOADED: MCP servers file {x}: {e}")),
            }
        }
        // Default low-privilege user for MCP children when the gateway is root.
        let run_as = get("HERMES_RS_MCP_RUN_AS");
        for m in &mut self.mcp_servers {
            if m.run_as.is_none() {
                m.run_as = run_as.clone();
            }
        }
        // HERMES_RS_MCP_ONLY=a,b keeps only the named servers.
        if let Some(x) = get("HERMES_RS_MCP_ONLY") {
            let keep: Vec<&str> = x.split(',').map(str::trim).collect();
            self.mcp_servers.retain(|m| keep.contains(&m.name.as_str()));
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
        // Last, so it wins over every model setting above (incl.
        // HERMES_RS_DELEGATION_MODEL) and over config.yaml.
        if let Some(l) = crate::model_lock::ModelLock::from_env(&get) {
            let notes = l.enforce(self);
            self.model_lock_notes.extend(notes);
            self.model_lock = Some(l);
        }
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
            ..McpServerCfg::default()
        });
        d.apply_env(&env).unwrap();
        assert!(d.mcp_servers.is_empty());
    }
}
