// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The agent tools the Python Hermes (hermes-agent 0.21.4, toolset
//! `hermes-cli`) gave the model, rebuilt in Rust behind the Hermes deny
//! list: `terminal`, `read_file`, `write_file`, `edit_file`, `list_files`,
//! `search_files`, `browser` and `vision_analyze`.
//!
//! Safety, in the order a call meets it:
//! 1. the deny list ([`hermes_rs::denylist::check_tool_call`]) runs before
//!    every call (the registry wraps each tool; there is no off switch), with
//!    the Hermes workspace as its only writable root;
//! 2. spend caps and a per-session call cap (200, as the prior
//!    `code_execution.max_tool_calls`);
//! 3. this module's own path confinement: writes only inside the workspace,
//!    no `..`, no symlink anywhere on the way, a fixed list of names that may
//!    never be written (SOUL.md, config.yaml, the self-modify lock...), reads
//!    only inside the workspace and the configured read-only roots;
//! 4. processes (`terminal`, `browser`) only through `openmind_sandbox::run`:
//!    cleared env, read-only root with only the workspace writable, secret
//!    directories covered by an empty tmpfs, private PID/mount namespaces,
//!    rlimits, a wall timeout and output caps. `terminal` has no network at
//!    all (empty network namespace, fail closed); `browser` keeps the network
//!    but a PAC script sends every loopback/private address to a dead proxy.
//!
//! Off unless `HERMES_RS_TOOLS_WORKSPACE` is set. docs/hermes-agent-tools.md
//! has the full table and the env knobs.

pub mod browser;
pub mod files;
pub mod html;
pub mod shell;
pub mod vision;

use hermes_rs::config::{Config, SpendCfg};
use hermes_rs::provider::Provider;
use hermes_rs::spend::SpendTracker;
use hermes_rs::tools::{Registry, ToolCtx};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Names `register` may add (browser / vision only when available).
pub const TOOL_NAMES: [&str; 8] = [
    "terminal",
    "read_file",
    "write_file",
    "edit_file",
    "list_files",
    "search_files",
    "browser",
    "vision_analyze",
];

/// The free vision model the prior stack used (`auxiliary.vision`).
pub const PRIOR_VISION_MODEL: &str = "nvidia/nemotron-3.5-super-vl:free";

/// File names no agent tool may ever write, even inside the workspace
/// (on top of the deny list's own list): the owner's self-modify lock.
pub const NEVER_WRITE_NAMES: &[&str] = &[
    "SOUL.md",
    "config.yaml",
    "hermes-self-modify.json",
    "guard.json",
    "AGENTS.md",
    "USER.md",
    "dangerous-patterns.txt",
    ".env",
    "auth.json",
];

/// Directories covered by an empty read-only tmpfs in every sandboxed run
/// (on top of the deny list's secret roots).
pub const EXTRA_HIDE: &[&str] = &[
    "/etc/galion",
    "/var/lib/galion-gate",
    "/opt/galion/gate",
    "/opt/galion/hermes",
    "/opt/galion/galion-light/secrets",
    "/root/galion-secrets",
    "/workspace/hermes",
    "/var/lib/hermes-rs",
    "/var/backups",
];

#[derive(Debug, Clone)]
pub struct AgentToolsConfig {
    /// The only writable directory (canonical).
    pub workspace: PathBuf,
    /// Read-only roots on top of the workspace (canonical, existing).
    pub read_roots: Vec<PathBuf>,
    pub shell_timeout_s: u64,
    pub shell_timeout_max_s: u64,
    /// Bytes of stdout/stderr kept per stream.
    pub output_cap: usize,
    pub max_read_bytes: u64,
    pub max_write_bytes: u64,
    /// Total workspace size writes may grow it to.
    pub workspace_quota_bytes: u64,
    pub max_calls_per_session: u64,
    /// None = browser off.
    pub browser_bin: Option<PathBuf>,
    pub browser_timeout_s: u64,
    pub browser_text_cap: usize,
    /// Empty = vision off.
    pub vision_models: Vec<String>,
    pub vision_max_image_bytes: u64,
    /// Directories hidden from sandboxed processes.
    pub hide: Vec<PathBuf>,
    /// Delegated cgroup v2 parent for per-run cgroups (memory.max, pids.max
    /// for the whole process tree). None = rlimits + wall timeout only.
    pub cgroup_parent: Option<PathBuf>,
    /// (uid, gid) the terminal and browser run as. Set when the gateway runs
    /// as root (default 65534:65534, `HERMES_RS_TOOLS_RUN_AS`), so a
    /// sandboxed process is an ordinary user outside its namespace. None =
    /// the gateway user.
    pub run_as: Option<(u32, u32)>,
}

impl AgentToolsConfig {
    /// Hand `p` (a file or dir the in-process tools made in the workspace)
    /// to the `run_as` user, without following symlinks. No-op otherwise.
    pub fn own(&self, p: &Path) {
        if let Some((u, g)) = self.run_as {
            let _ = std::os::unix::fs::lchown(p, Some(u), Some(g));
        }
    }

    /// `own` on `p` and every dir between it and the workspace.
    pub fn own_up(&self, p: &Path) {
        if self.run_as.is_none() {
            return;
        }
        let mut cur = Some(p);
        while let Some(c) = cur {
            if !c.starts_with(&self.workspace) || c == self.workspace {
                break;
            }
            self.own(c);
            cur = c.parent();
        }
    }
}

fn num(env: &dyn Fn(&str) -> Option<String>, k: &str, d: u64) -> Result<u64, String> {
    match env(k) {
        None => Ok(d),
        Some(v) if v.trim().is_empty() => Ok(d),
        Some(v) => v
            .trim()
            .parse::<u64>()
            .map_err(|_| format!("{k}={v:?} is not a whole number")),
    }
}

/// The newest Playwright headless shell under `root`/.cache/ms-playwright.
pub fn find_playwright_shell(cache: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for e in std::fs::read_dir(cache).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        let (kind, rev) = match n.rsplit_once('-') {
            Some(x) => x,
            None => continue,
        };
        let Ok(rev) = rev.parse::<u64>() else {
            continue;
        };
        let cands: &[&str] = match kind {
            "chromium_headless_shell" => &[
                "chrome-headless-shell-linux64/chrome-headless-shell",
                "chrome-linux/headless_shell",
            ],
            _ => continue,
        };
        for c in cands {
            let p = e.path().join(c);
            if p.is_file() && best.as_ref().is_none_or(|(r, _)| rev > *r) {
                best = Some((rev, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

impl AgentToolsConfig {
    /// `Ok(None)` when `HERMES_RS_TOOLS_WORKSPACE` is unset (tools off).
    pub fn from_env(
        env: &dyn Fn(&str) -> Option<String>,
        hermes: &Config,
    ) -> Result<Option<AgentToolsConfig>, String> {
        let Some(ws) = env("HERMES_RS_TOOLS_WORKSPACE").filter(|s| !s.trim().is_empty()) else {
            return Ok(None);
        };
        let ws = PathBuf::from(ws.trim());
        if !ws.is_absolute() {
            return Err("HERMES_RS_TOOLS_WORKSPACE must be an absolute path".into());
        }
        if !ws.is_dir() {
            std::fs::create_dir_all(&ws)
                .map_err(|e| format!("create workspace {}: {e}", ws.display()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&ws, std::fs::Permissions::from_mode(0o750));
            }
        }
        let ws = ws
            .canonicalize()
            .map_err(|e| format!("workspace {}: {e}", ws.display()))?;
        // the deny list drops such roots; refuse instead of running unwritable
        let pol = hermes_rs::denylist::Policy::workspace(&ws);
        if pol.write_roots.is_empty() {
            return Err(format!(
                "workspace {} is /, a top-level dir, a secret root or a vault; the deny list would make it unwritable",
                ws.display()
            ));
        }
        let mut read_roots = vec![];
        if let Some(r) = env("HERMES_RS_TOOLS_READ_ROOTS") {
            for p in r.split(':').map(str::trim).filter(|p| !p.is_empty()) {
                let pb = PathBuf::from(p);
                if !pb.is_absolute() {
                    return Err(format!("HERMES_RS_TOOLS_READ_ROOTS: {p} is not absolute"));
                }
                if let Ok(c) = pb.canonicalize() {
                    if c.components().count() < 3 {
                        return Err(format!(
                            "HERMES_RS_TOOLS_READ_ROOTS: {p} is too wide (/ or a top-level dir)"
                        ));
                    }
                    read_roots.push(c);
                }
            }
        }
        let shell_timeout_max_s =
            num(env, "HERMES_RS_TOOLS_SHELL_TIMEOUT_MAX_S", 300)?.clamp(1, 1800);
        let shell_timeout_s =
            num(env, "HERMES_RS_TOOLS_SHELL_TIMEOUT_S", 60)?.clamp(1, shell_timeout_max_s);
        let browser_bin = if env("HERMES_RS_TOOLS_BROWSER").as_deref() == Some("0") {
            None
        } else if let Some(b) = env("HERMES_RS_BROWSER_BIN").filter(|s| !s.trim().is_empty()) {
            let b = PathBuf::from(b.trim());
            if !b.is_file() {
                return Err(format!(
                    "HERMES_RS_BROWSER_BIN {} is not a file",
                    b.display()
                ));
            }
            Some(b)
        } else {
            let home = env("HOME").unwrap_or_else(|| "/root".into());
            find_playwright_shell(&Path::new(&home).join(".cache/ms-playwright"))
                .or_else(|| find_playwright_shell(Path::new("/root/.cache/ms-playwright")))
        };
        let vision_models = if env("HERMES_RS_TOOLS_VISION").as_deref() == Some("0") {
            vec![]
        } else if let Some(l) = &hermes.model_lock {
            // Model lock: vision uses the locked model only (no free fallback).
            vec![l.model.clone()]
        } else {
            let first = env("HERMES_RS_VISION_MODEL")
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| hermes.default_model.clone());
            let fb =
                env("HERMES_RS_VISION_FALLBACK").unwrap_or_else(|| PRIOR_VISION_MODEL.to_string());
            let mut v = vec![first.trim().to_string()];
            for m in fb.split(',').map(str::trim).filter(|m| !m.is_empty()) {
                if !v.iter().any(|x| x == m) {
                    v.push(m.to_string());
                }
            }
            v
        };
        let mut hide: Vec<PathBuf> = hermes_rs::denylist::rules::SECRET_ROOTS
            .iter()
            .chain(EXTRA_HIDE.iter())
            .map(PathBuf::from)
            .filter(|p| p.is_dir() && !ws.starts_with(p))
            .collect();
        hide.sort();
        hide.dedup();
        let cgroup_parent = match env("HERMES_RS_TOOLS_CGROUP").filter(|s| !s.trim().is_empty()) {
            None => None,
            Some(c) => {
                let c = PathBuf::from(c.trim());
                if !c.is_absolute() || !c.starts_with("/sys/fs/cgroup") || !c.is_dir() {
                    return Err(format!(
                        "HERMES_RS_TOOLS_CGROUP {} must be an existing dir under /sys/fs/cgroup",
                        c.display()
                    ));
                }
                Some(c)
            }
        };
        let euid = openmind_sandbox::effective_uid();
        let run_as = match env("HERMES_RS_TOOLS_RUN_AS").map(|s| s.trim().to_string()) {
            Some(s) if s == "gateway" => None,
            Some(s) if !s.is_empty() => {
                let (u, g) = s
                    .split_once(':')
                    .ok_or("HERMES_RS_TOOLS_RUN_AS must be uid:gid or gateway")?;
                let u: u32 = u.parse().map_err(|_| "HERMES_RS_TOOLS_RUN_AS: bad uid")?;
                let g: u32 = g.parse().map_err(|_| "HERMES_RS_TOOLS_RUN_AS: bad gid")?;
                if u == 0 || g == 0 {
                    return Err("HERMES_RS_TOOLS_RUN_AS may not be root".into());
                }
                if euid != 0 {
                    return Err("HERMES_RS_TOOLS_RUN_AS needs a root gateway".into());
                }
                Some((u, g))
            }
            _ if euid == 0 => Some((65534, 65534)),
            _ => None,
        };
        if let Some((u, g)) = run_as {
            let _ = std::os::unix::fs::chown(&ws, Some(u), Some(g));
        }
        Ok(Some(AgentToolsConfig {
            workspace: ws,
            read_roots,
            shell_timeout_s,
            shell_timeout_max_s,
            output_cap: num(env, "HERMES_RS_TOOLS_OUTPUT_CAP", 64 * 1024)?.clamp(1024, 1 << 20)
                as usize,
            max_read_bytes: num(env, "HERMES_RS_TOOLS_MAX_READ_BYTES", 256 * 1024)?
                .clamp(1024, 8 << 20),
            max_write_bytes: num(env, "HERMES_RS_TOOLS_MAX_WRITE_BYTES", 1 << 20)?
                .clamp(1024, 8 << 20),
            workspace_quota_bytes: num(env, "HERMES_RS_TOOLS_WORKSPACE_MB", 1024)?.max(1) << 20,
            max_calls_per_session: num(env, "HERMES_RS_TOOLS_MAX_CALLS", 200)?.max(1),
            browser_bin,
            browser_timeout_s: num(env, "HERMES_RS_BROWSER_TIMEOUT_S", 45)?.clamp(5, 120),
            browser_text_cap: num(env, "HERMES_RS_BROWSER_TEXT_CAP", 15_000)?.clamp(1000, 200_000)
                as usize,
            vision_models,
            vision_max_image_bytes: num(env, "HERMES_RS_VISION_MAX_BYTES", 5 << 20)?
                .clamp(1024, 20 << 20),
            hide,
            cgroup_parent,
            run_as,
        }))
    }

    /// A config for tests: workspace `ws`, no browser, no vision.
    pub fn for_tests(ws: &Path) -> AgentToolsConfig {
        let ws = ws.canonicalize().unwrap_or_else(|_| ws.to_path_buf());
        AgentToolsConfig {
            workspace: ws,
            read_roots: vec![],
            shell_timeout_s: 20,
            shell_timeout_max_s: 60,
            output_cap: 64 * 1024,
            max_read_bytes: 256 * 1024,
            max_write_bytes: 1 << 20,
            workspace_quota_bytes: 64 << 20,
            max_calls_per_session: 200,
            browser_bin: None,
            browser_timeout_s: 45,
            browser_text_cap: 15_000,
            vision_models: vec![],
            vision_max_image_bytes: 5 << 20,
            hide: vec![],
            cgroup_parent: None,
            run_as: None,
        }
    }
}

/// State shared by the agent tools.
pub struct Shared {
    pub cfg: AgentToolsConfig,
    pub spend: Option<Arc<SpendTracker>>,
    calls: Mutex<HashMap<String, u64>>,
}

impl Shared {
    pub fn new(cfg: AgentToolsConfig, spend: Option<Arc<SpendTracker>>) -> Self {
        Shared {
            cfg,
            spend,
            calls: Mutex::new(HashMap::new()),
        }
    }

    /// Spend cap, then the per-session call cap. `Err` = refused.
    pub fn gate(&self, ctx: &ToolCtx) -> Result<(), String> {
        if let Some(s) = &self.spend {
            s.check(ctx.spend_session).map_err(|b| b.message())?;
        }
        let mut c = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        let n = c.entry(ctx.spend_session.to_string()).or_insert(0);
        if *n >= self.cfg.max_calls_per_session {
            return Err(format!(
                "agent tool call cap reached for this session ({} calls)",
                self.cfg.max_calls_per_session
            ));
        }
        *n += 1;
        Ok(())
    }

    pub fn log(&self, tool: &str, ctx: &ToolCtx, what: &str) {
        eprintln!(
            "hermes-rs agent-tools: tool={tool} session={} depth={} {}",
            ctx.session_id,
            ctx.depth,
            hermes_rs::redact(&cut(what, 300))
        );
    }
}

/// At most `n` bytes of `s`, cut on a char boundary.
pub fn cut(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut i = n;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    format!("{}…[{} more bytes]", &s[..i], s.len() - i)
}

/// Startup checks; returns notes for the log.
pub fn preflight(cfg: &AgentToolsConfig, spend: &SpendCfg) -> Result<Vec<String>, String> {
    if spend.session_usd_cap <= 0.0 || spend.daily_usd_cap <= 0.0 {
        return Err(format!(
            "agent tools need finite spend caps (session ${:.2}, daily ${:.2})",
            spend.session_usd_cap, spend.daily_usd_cap
        ));
    }
    let f = openmind_sandbox::features();
    let mut notes = vec![format!(
        "workspace={} read_roots={:?} landlock_abi={} userns={} hidden_dirs={}",
        cfg.workspace.display(),
        cfg.read_roots,
        f.landlock_abi,
        f.userns,
        cfg.hide.len()
    )];
    if !f.userns && f.landlock_abi < 4 {
        return Err(
            "no network namespace and no Landlock TCP deny: terminal could not be kept offline"
                .into(),
        );
    }
    notes.push(match &cfg.browser_bin {
        Some(b) => format!("browser: {}", b.display()),
        None => "browser: off (no Playwright headless shell found)".into(),
    });
    notes.push(if cfg.vision_models.is_empty() {
        "vision: off".into()
    } else {
        format!("vision models: {:?}", cfg.vision_models)
    });
    Ok(notes)
}

/// Add the agent tools to `reg`, each inside the workspace deny-list policy.
pub fn register(
    reg: &mut Registry,
    cfg: AgentToolsConfig,
    spend: Option<Arc<SpendTracker>>,
    provider: Option<Arc<dyn Provider>>,
    hermes: Option<Arc<Config>>,
) -> Vec<String> {
    let ws = cfg.workspace.clone();
    let sh = Arc::new(Shared::new(cfg, spend));
    let mut names = vec![];
    let mut add = |reg: &mut Registry, t: Arc<dyn hermes_rs::tools::Tool>| {
        names.push(t.name());
        reg.add_in_workspace(t, &ws);
    };
    add(reg, Arc::new(shell::Terminal(sh.clone())));
    add(reg, Arc::new(files::ReadFile(sh.clone())));
    add(reg, Arc::new(files::WriteFile(sh.clone())));
    add(reg, Arc::new(files::EditFile(sh.clone())));
    add(reg, Arc::new(files::ListFiles(sh.clone())));
    add(reg, Arc::new(files::SearchFiles(sh.clone())));
    if sh.cfg.browser_bin.is_some() {
        add(reg, Arc::new(browser::Browser(sh.clone())));
    }
    if let (false, Some(p), Some(h)) = (sh.cfg.vision_models.is_empty(), provider, hermes) {
        add(
            reg,
            Arc::new(vision::Vision {
                sh: sh.clone(),
                provider: p,
                hermes: h,
            }),
        );
    }
    names
}
