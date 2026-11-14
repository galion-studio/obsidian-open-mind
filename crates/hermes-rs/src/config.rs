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

/// Which tools from one MCP server the model may call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolAllow {
    /// Every tool the server lists. This is the default when `tools` is omitted.
    All,
    /// Only these tool names. An empty list allows nothing.
    Only(Vec<String>),
}

impl ToolAllow {
    pub fn permits(&self, name: &str) -> bool {
        match self {
            ToolAllow::All => true,
            ToolAllow::Only(names) => names.iter().any(|n| n == name),
        }
    }
}

/// One MCP server. `command` is stdio. `url` is streamable HTTP
/// (protocol 2025-03-26). Exactly one of them is set.
///
/// `token_env` is the name of an environment variable. The token itself is
/// read when a request is sent and is never stored on this struct.
#[derive(Debug, Clone, PartialEq)]
pub struct McpServerCfg {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub timeout_s: u64,
    pub connect_timeout_s: u64,
    pub lazy: bool,
    pub allow: ToolAllow,
    pub token_env: Option<String>,
    /// Static request headers for streamable HTTP. Not a place for secrets:
    /// `Authorization` is rejected (use `token_env`).
    pub headers: Vec<(String, String)>,
}

impl McpServerCfg {
    pub fn stdio(name: impl Into<String>, command: impl Into<String>, args: Vec<String>) -> Self {
        McpServerCfg {
            name: name.into(),
            command: command.into(),
            args,
            url: None,
            timeout_s: 30,
            connect_timeout_s: 15,
            lazy: false,
            allow: ToolAllow::All,
            token_env: None,
            headers: Vec::new(),
        }
    }

    pub fn http(name: impl Into<String>, url: impl Into<String>) -> Self {
        McpServerCfg {
            name: name.into(),
            command: String::new(),
            args: Vec::new(),
            url: Some(url.into()),
            timeout_s: 30,
            connect_timeout_s: 15,
            lazy: false,
            allow: ToolAllow::All,
            token_env: None,
            headers: Vec::new(),
        }
    }

    pub fn is_http(&self) -> bool {
        self.url.is_some()
    }
}

/// Remote GitHub MCP server, read-only toolset, all toolsets.
/// Documented at https://github.com/github/github-mcp-server (`/x/all/readonly`).
pub const GITHUB_MCP_URL: &str = "https://api.githubcopilot.com/mcp/x/all/readonly";

/// Environment variable the GitHub entry reads. The value is never copied
/// into config.
pub const GITHUB_TOKEN_ENV: &str = "GITHUB_TOKEN";

/// Read-only tool names from the GitHub MCP server README `## Tools` headings
/// (github/github-mcp-server, fetched 2026-11-15). A heading is included when
/// its summary is get, list, search, or read. Write, merge, comment, star,
/// trigger, and assign tools are not included. `mark_all_notifications_read`
/// is a write, so it is not included.
///
/// A server named `github`, or any URL on `api.githubcopilot.com`, is clamped
/// to this list.
pub const GITHUB_READONLY_TOOLS: &[&str] = &[
    "actions_get",
    "actions_list",
    "custom_properties_read",
    "get_code_quality_finding",
    "get_code_scanning_alert",
    "get_commit",
    "get_copilot_space",
    "get_dependabot_alert",
    "get_discussion",
    "get_discussion_comments",
    "get_file_contents",
    "get_gist",
    "get_global_security_advisory",
    "get_job_logs",
    "get_label",
    "get_latest_release",
    "get_me",
    "get_notification_details",
    "get_release_by_tag",
    "get_repository_tree",
    "get_secret_scanning_alert",
    "get_tag",
    "get_team_members",
    "get_teams",
    "github_support_docs_search",
    "issue_read",
    "list_branches",
    "list_code_scanning_alerts",
    "list_commits",
    "list_copilot_spaces",
    "list_dependabot_alerts",
    "list_discussion_categories",
    "list_discussions",
    "list_gists",
    "list_global_security_advisories",
    "list_issue_fields",
    "list_issue_types",
    "list_issues",
    "list_label",
    "list_notifications",
    "list_org_repository_security_advisories",
    "list_pull_requests",
    "list_releases",
    "list_repository_collaborators",
    "list_repository_security_advisories",
    "list_secret_scanning_alerts",
    "list_starred_repositories",
    "list_tags",
    "projects_get",
    "projects_list",
    "pull_request_read",
    "repository_ruleset_read",
    "search_code",
    "search_commits",
    "search_issues",
    "search_orgs",
    "search_pull_requests",
    "search_repositories",
    "search_users",
    "ui_get",
];

/// The built-in GitHub server: streamable HTTP, read-only header, token from
/// [`GITHUB_TOKEN_ENV`] at request time. Not added unless
/// `HERMES_RS_MCP_GITHUB` is on (see [`Config::apply_env`]).
pub fn github_readonly_server() -> McpServerCfg {
    let mut cfg = McpServerCfg {
        name: "github".into(),
        command: String::new(),
        args: Vec::new(),
        url: Some(GITHUB_MCP_URL.into()),
        timeout_s: 60,
        connect_timeout_s: 15,
        lazy: false,
        allow: ToolAllow::All,
        token_env: Some(GITHUB_TOKEN_ENV.into()),
        headers: vec![("X-MCP-Readonly".into(), "true".into())],
    };
    clamp_github_readonly(&mut cfg);
    cfg
}

pub(crate) fn looks_like_secret(value: &str) -> bool {
    let v = value.trim();
    let lower = v.to_ascii_lowercase();
    lower.starts_with("bearer ")
        || (v.len() >= 16
            && (v.starts_with(concat!("gh", "p_"))
                || v.starts_with(concat!("gh", "o_"))
                || v.starts_with(concat!("github", "_pat_"))
                || v.starts_with(concat!("s", "k-"))))
}

pub(crate) fn valid_env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    match bytes.next() {
        Some(c) if c.is_ascii_alphabetic() || c == b'_' => {}
        _ => return false,
    }
    name.len() <= 64
        && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && !looks_like_secret(name)
}

pub(crate) fn header_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value
            .bytes()
            .all(|c| (0x20..=0x7e).contains(&c) && c != b'"' && c != b'\\')
}

fn http_token(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
}

fn header_pair_ok(name: &str, value: &str) -> bool {
    http_token(name)
        && name.len() <= 64
        && !name.eq_ignore_ascii_case("authorization")
        && !name.eq_ignore_ascii_case("host")
        && !name.eq_ignore_ascii_case("content-length")
        && !name.eq_ignore_ascii_case("connection")
        && header_safe(value)
        && !looks_like_secret(value)
}

pub(crate) fn url_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    if rest.is_empty() || rest.contains('@') {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']')?.0
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if port.is_empty() || !port.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        host
    } else {
        authority
    };
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn github_mcp_host(url: &str) -> bool {
    url_host(url).is_some_and(|h| h.eq_ignore_ascii_case("api.githubcopilot.com"))
}

fn is_github_entry(cfg: &McpServerCfg) -> bool {
    cfg.name == "github" || cfg.url.as_deref().is_some_and(github_mcp_host)
}

/// Force a GitHub entry down to [`GITHUB_READONLY_TOOLS`]. An explicit
/// `tools` list keeps only the intersection, so a write tool in config
/// cannot be registered. An omitted list becomes the full read-only set.
pub fn clamp_github_readonly(cfg: &mut McpServerCfg) {
    if !is_github_entry(cfg) {
        return;
    }
    let allowed: Vec<String> = match &cfg.allow {
        ToolAllow::All => GITHUB_READONLY_TOOLS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        ToolAllow::Only(names) => names
            .iter()
            .filter(|n| GITHUB_READONLY_TOOLS.contains(&n.as_str()))
            .cloned()
            .collect(),
    };
    cfg.allow = ToolAllow::Only(allowed);
    if cfg.url.is_some() {
        cfg.headers.retain(|(k, _)| {
            !k.eq_ignore_ascii_case("authorization") && !k.eq_ignore_ascii_case("x-mcp-readonly")
        });
        cfg.headers.push(("X-MCP-Readonly".into(), "true".into()));
        if cfg.token_env.is_none() {
            cfg.token_env = Some(GITHUB_TOKEN_ENV.into());
        }
    }
}

fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

fn string_list(v: &Value) -> Option<Vec<String>> {
    let arr = v.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let name = s(item)?;
        if !valid_tool_name(&name) {
            return None;
        }
        out.push(name);
    }
    Some(out)
}

fn header_value(v: &Value) -> Option<String> {
    match v {
        Value::String(x) if !x.trim().is_empty() => Some(x.trim().to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `None` drops the entry (disabled, incomplete, or it would have stored a
/// secret). A GitHub entry that survives is clamped to the read-only list.
fn mcp_server_from_yaml(name: &str, sv: &Value) -> Option<McpServerCfg> {
    if sv["enabled"].as_bool() == Some(false) {
        return None;
    }
    let command = s(&sv["command"]);
    let url = s(&sv["url"]);
    if command.is_some() == url.is_some() {
        return None;
    }
    let args = sv["args"]
        .as_array()
        .map(|a| a.iter().filter_map(s).collect())
        .unwrap_or_default();
    let allow = if sv.get("tools").is_none() {
        ToolAllow::All
    } else {
        ToolAllow::Only(string_list(&sv["tools"])?)
    };
    let token_env = match s(&sv["token_env"]) {
        Some(n) if valid_env_name(&n) => Some(n),
        Some(_) => return None,
        None => None,
    };
    let headers = match sv.get("headers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(map)) => {
            let mut h = Vec::new();
            for (k, v) in map {
                let val = header_value(v)?;
                if !header_pair_ok(k, &val) {
                    return None;
                }
                h.push((k.clone(), val));
            }
            h
        }
        Some(_) => return None,
    };
    let mut cfg = McpServerCfg {
        name: name.to_string(),
        command: command.unwrap_or_default(),
        args,
        url,
        timeout_s: sv["timeout"].as_u64().unwrap_or(30),
        connect_timeout_s: sv["connect_timeout"].as_u64().unwrap_or(15),
        lazy: sv["lazy"].as_bool().unwrap_or(false),
        allow,
        token_env,
        headers,
    };
    clamp_github_readonly(&mut cfg);
    Some(cfg)
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
                if let Some(cfg) = mcp_server_from_yaml(name, sv) {
                    self.mcp_servers.push(cfg);
                }
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
        // HERMES_RS_MCP=0 skips every configured MCP server (shadow runs),
        // including the GitHub entry. Otherwise HERMES_RS_MCP_GITHUB=1 adds
        // the read-only GitHub server when config.yaml did not already
        // name one. The token stays in GITHUB_TOKEN.
        let mcp_off = get("HERMES_RS_MCP")
            .is_some_and(|x| matches!(x.as_str(), "0" | "false" | "no" | "off"));
        if mcp_off {
            self.mcp_servers.clear();
        } else if get("HERMES_RS_MCP_GITHUB")
            .is_some_and(|x| matches!(x.as_str(), "1" | "true" | "yes" | "on"))
            && !self
                .mcp_servers
                .iter()
                .any(|s| s.name == "github" || s.url.as_deref().is_some_and(github_mcp_host))
        {
            self.mcp_servers.push(github_readonly_server());
        }
        if let Some(x) = get("HERMES_RS_PERSONA") {
            self.persona_file = Some(PathBuf::from(x));
        }
        if let Some(x) = get("OPENROUTER_BASE_URL") {
            if let Some(p) = self.providers.get_mut("openrouter") {
                p.base_url = x;
            }
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
        assert_eq!(c.mcp_servers[0].allow, ToolAllow::All);
        assert!(c.mcp_servers[0].url.is_none());
        assert!(c.mcp_servers[0].token_env.is_none());
        assert!(c.free_only_background);
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
            timeout_s: 1,
            connect_timeout_s: 1,
            ..McpServerCfg::stdio("x", "true", vec![])
        });
        d.apply_env(&env).unwrap();
        assert!(d.mcp_servers.is_empty());
    }

    #[test]
    fn http_allowlist_and_github_clamp() {
        // The leak entry's token_env is a token pasted where a variable name
        // belongs. The pieces are split so this source file has no token.
        let leaked = concat!("gh", "p_abcdefghijklmnopqrstuvwxyz0123456789");
        let yaml = format!(
            "
mcp_servers:
  lab:
    url: http://127.0.0.1/mcp
    tools: [list_notes, get_note]
    timeout: 12
  github:
    url: https://api.githubcopilot.com/mcp/
    token_env: GITHUB_TOKEN
    tools: [get_file_contents, push_files, issue_write, merge_pull_request]
  leak:
    url: https://example.com/mcp
    token_env: {leaked}
  both:
    command: /bin/true
    url: http://127.0.0.1/mcp
  auth:
    url: https://example.com/mcp
    headers:
      Authorization: \"Bearer nope\"
  off:
    command: /bin/true
    enabled: false
"
        );
        let mut c = Config::defaults(Path::new("/h"));
        c.apply_yaml(&yaml::parse(&yaml).unwrap());
        let names: Vec<&str> = c.mcp_servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["lab", "github"], "{names:?}");
        let lab = c.mcp_servers.iter().find(|s| s.name == "lab").unwrap();
        assert_eq!(
            lab.allow,
            ToolAllow::Only(vec!["list_notes".into(), "get_note".into()])
        );
        assert_eq!(lab.timeout_s, 12);
        let gh = c.mcp_servers.iter().find(|s| s.name == "github").unwrap();
        assert!(gh.allow.permits("get_file_contents"));
        assert!(!gh.allow.permits("push_files"));
        assert!(!gh.allow.permits("issue_write"));
        assert!(!gh.allow.permits("merge_pull_request"));
        assert!(!gh.allow.permits("mark_all_notifications_read"));
        assert_eq!(gh.token_env.as_deref(), Some(GITHUB_TOKEN_ENV));
        assert!(gh
            .headers
            .iter()
            .any(|(k, v)| k == "X-MCP-Readonly" && v == "true"));
        assert!(gh.headers.iter().all(|(k, _)| k != "Authorization"));
        let dumped = format!("{gh:?}");
        assert!(!dumped.contains(concat!("gh", "p_")));
        assert!(!dumped.contains("Bearer"));
    }

    #[test]
    fn github_entry_comes_from_env_and_mcp_off_wins() {
        let preset = github_readonly_server();
        assert_eq!(preset.url.as_deref(), Some(GITHUB_MCP_URL));
        assert!(preset.url.as_deref().unwrap().starts_with("https://"));
        assert_eq!(preset.token_env.as_deref(), Some(GITHUB_TOKEN_ENV));
        assert!(preset.command.is_empty());
        assert!(preset.allow.permits("get_file_contents"));
        assert!(preset.allow.permits("issue_read"));
        assert!(preset.allow.permits("pull_request_read"));
        assert!(preset.allow.permits("search_code"));
        assert!(!preset.allow.permits("push_files"));
        assert!(!preset.allow.permits("create_or_update_file"));
        assert!(!preset.allow.permits("create_pull_request"));
        assert!(!preset.allow.permits("actions_run_trigger"));
        let tools = match &preset.allow {
            ToolAllow::Only(names) => names.clone(),
            ToolAllow::All => panic!("github allowlist must be explicit"),
        };
        let mut sorted = tools.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, tools);
        for w in [
            "push_files",
            "create_or_update_file",
            "delete_file",
            "issue_write",
            "add_issue_comment",
            "merge_pull_request",
            "create_pull_request",
            "fork_repository",
            "actions_run_trigger",
            "mark_all_notifications_read",
            "star_repository",
        ] {
            assert!(!tools.iter().any(|t| t == w), "{w}");
        }

        let on = |k: &str| match k {
            "HERMES_RS_MCP_GITHUB" => Some("1".into()),
            _ => None,
        };
        let mut c = Config::defaults(Path::new("/h"));
        c.apply_env(&on).unwrap();
        assert_eq!(c.mcp_servers.len(), 1);
        assert_eq!(c.mcp_servers[0].name, "github");
        c.apply_env(&on).unwrap();
        assert_eq!(c.mcp_servers.len(), 1, "the flag must not duplicate github");

        let off = |k: &str| match k {
            "HERMES_RS_MCP" => Some("off".into()),
            "HERMES_RS_MCP_GITHUB" => Some("1".into()),
            _ => None,
        };
        c.apply_env(&off).unwrap();
        assert!(c.mcp_servers.is_empty());
    }
}
