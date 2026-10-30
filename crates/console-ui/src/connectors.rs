// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/connectors` (owner t453u, menu placement t454u, 2026-10-30 CET): the
//! connectors settings screen. Each connector is an MCP server for Hermes
//! (hermes-rs). Layout and order follow the Grok app connectors screen
//! (Mate's reference screenshots, specs/grok-connectors-ref, steering
//! 2026-10-30 12:58 CET): back arrow, title, subtitle, search box, a
//! Connected group, then Featured, Productivity, Finance, Developer,
//! Microsoft 365, Sales, Social Media, Analytics, Advertising and Custom
//! Connector. Rows carry each brand's own logo, downloaded once and served
//! from the console (`logos.rs`, sources in `assets/logos/SOURCES.md`).
//! Only the Featured connectors and GitHub are wired (steering 13:05 CET);
//! every other row says "Coming soon".
//!
//! What a connect does (all behind the console owner gate, CSRF-checked):
//! * writes one entry `servers.d/<id>.yaml` in `--connectors-dir`, in the
//!   `mcp_servers:` shape of the generic Hermes MCP client
//!   (branch swarm/hermes-mcp-generic-20261030, `McpServerCfg::from_value`);
//! * writes the token, if any, to `secrets/<id>.env` (`NAME=value`, mode
//!   0600, dirs 0700). The entry only names the file and uses
//!   an Authorization header (Bearer scheme) built from `${NAME}`; the value is never shown, logged or
//!   sent back (the page says "saved");
//! * regenerates `mcp-servers.yaml` (all live entries, atomic rename).
//!
//! With `--connectors-hermes-file` (steering 13:11 CET: the generic client
//! is live and reads `/etc/hermes-rs/mcp-servers.yaml`) the screen works on
//! that file instead: tokens go to `mcp.d/<id>.env` beside it, entries the
//! owner wrote by hand (openmind, github) are never edited (connecting
//! github only writes its token file), and the console's own entries live
//! in one marked block at the end of `mcp_servers:` (old file copied to
//! `.bak-console-<unix>` first). A connector shows Connected only when its
//! token file is complete and older than the running Hermes gateway.
//!
//! New servers start read-only: `class: read`, `allow_writes: false`.
//! Disconnect renames the entry (and its secrets file) to `.off`; nothing
//! is deleted. A replaced entry or token is renamed to `.bak.<unix>` first.
//! OAuth-only connectors show "needs sign-in setup" and write nothing
//! (TODO: OAuth flow; the generic client does not start `auth: oauth` yet).
//! Hermes reads the file only at start (no reload path). This console does
//! restart services; anything saved after Hermes started says "Saved, not
//! live yet" until the owner restarts `hermes-rs`.

use crate::json::{self, Value};
use crate::layout;
use crate::{gate, html, Reply, Request, Ui};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The static catalog (sections + featured connectors).
pub const CATALOG_JSON: &str = include_str!("assets/connectors.json");

/// Routes.
pub const PATH: &str = "/connectors";

/// Name of the CSRF cookie (double submit, plus the same-origin check).
pub const CSRF_COOKIE: &str = "galion_cn_csrf";

/// Generated servers file name inside `--connectors-dir`.
pub const SERVERS_FILE: &str = "mcp-servers.yaml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// Owner pastes a key; sent as a Bearer Authorization header from `${token_env}`.
    Token,
    /// Needs an OAuth sign-in (not built: stub).
    OAuth,
    /// No auth.
    None,
    /// Configured in the Hermes config; read-only here.
    Builtin,
    /// Listed for the layout; no MCP endpoint checked yet.
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connector {
    pub id: String,
    pub name: String,
    pub section: String,
    pub initials: String,
    /// Logo name in [`crate::logos`] (defaults to the id; may be missing).
    pub logo: String,
    pub blurb: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub transport: String,
    pub auth: Auth,
    pub token_env: String,
    pub token_label: String,
    pub verified: bool,
    pub source: String,
    pub note: String,
    /// Added through "Add custom connector".
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub sections: Vec<Section>,
    pub connectors: Vec<Connector>,
}

fn st(v: &Value, k: &str) -> String {
    v.get(k).as_str().unwrap_or("").to_string()
}

/// Parse the catalog JSON. Every entry must name a known section, a valid
/// id, and an https URL or an allowlisted command.
pub fn parse_catalog(text: &str) -> Result<Catalog, String> {
    let v = json::parse(text.as_bytes()).map_err(|_| "catalog: not JSON".to_string())?;
    let sections: Vec<Section> = v
        .get("sections")
        .as_array()
        .iter()
        .map(|s| Section {
            id: st(s, "id"),
            title: st(s, "title"),
        })
        .collect();
    if sections.is_empty() {
        return Err("catalog: no sections".into());
    }
    let mut connectors = Vec::new();
    for c in v.get("connectors").as_array() {
        let id = st(c, "id");
        if !id_ok(&id) {
            return Err(format!("catalog: bad id {id:?}"));
        }
        let section = st(c, "section");
        if !sections.iter().any(|s| s.id == section) {
            return Err(format!("catalog: {id}: unknown section {section:?}"));
        }
        let auth = match st(c, "auth").as_str() {
            "token" => Auth::Token,
            "oauth" => Auth::OAuth,
            "none" => Auth::None,
            "builtin" => Auth::Builtin,
            "unverified" => Auth::Unverified,
            a => return Err(format!("catalog: {id}: unknown auth {a:?}")),
        };
        let url = c.get("url").as_str().map(str::to_string);
        let command = c.get("command").as_str().map(str::to_string);
        let verified = c.get("verified").as_bool() == Some(true);
        if (auth == Auth::Unverified) == verified {
            return Err(format!(
                "catalog: {id}: verified must be false exactly for unverified"
            ));
        }
        if verified && auth != Auth::Builtin && !st(c, "source").starts_with("https://") {
            return Err(format!(
                "catalog: {id}: verified entries need an https source"
            ));
        }
        match (&url, &command) {
            (None, None) if auth == Auth::Unverified => {}
            (_, _) if auth == Auth::Unverified => {
                return Err(format!(
                    "catalog: {id}: unverified entries carry no endpoint"
                ))
            }
            (Some(u), None) => check_url(u).map_err(|e| format!("catalog: {id}: {e}"))?,
            (None, Some(_)) if auth == Auth::Builtin => {}
            (None, Some(_)) => {
                return Err(format!(
                    "catalog: {id}: only builtin entries may name a command"
                ))
            }
            _ => {
                return Err(format!(
                    "catalog: {id}: needs exactly one of url or command"
                ))
            }
        }
        let token_env = st(c, "token_env");
        if auth == Auth::Token && !env_name_ok(&token_env) {
            return Err(format!("catalog: {id}: token connector needs token_env"));
        }
        let logo = match st(c, "logo") {
            l if l.is_empty() => id.clone(),
            l => l,
        };
        connectors.push(Connector {
            logo,
            name: st(c, "name"),
            section,
            initials: st(c, "initials"),
            blurb: st(c, "blurb"),
            url,
            command,
            args: Vec::new(),
            transport: st(c, "transport"),
            auth,
            token_env,
            token_label: st(c, "token_label"),
            verified,
            source: st(c, "source"),
            note: st(c, "note"),
            custom: false,
            id,
        });
    }
    let mut ids: Vec<&str> = connectors.iter().map(|c| c.id.as_str()).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    if ids.len() != n {
        return Err("catalog: duplicate id".into());
    }
    Ok(Catalog {
        sections,
        connectors,
    })
}

/// Connectors with a backend now (owner t456u/t457u): the Featured ones,
/// GitHub, built-ins and customs. Everything else reads "Coming soon".
pub fn wired(c: &Connector) -> bool {
    c.custom || c.auth == Auth::Builtin || c.section == "featured" || c.id == "github"
}

/// The built-in catalog (panics only if the shipped JSON is broken, which
/// the tests catch).
pub fn catalog() -> Catalog {
    parse_catalog(CATALOG_JSON).expect("assets/connectors.json")
}

/// `[a-z0-9-]`, 1..=32, starts with a letter.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `[A-Z][A-Z0-9_]{0,63}`.
pub fn env_name_ok(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.as_bytes()[0].is_ascii_uppercase()
        && n.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// Characters allowed in any value written into the YAML file: printable
/// ASCII without quotes, backslash, `#`, `$`, braces or whitespace, so a
/// double-quoted YAML scalar needs no escaping at all.
fn yaml_safe(s: &str) -> bool {
    s.bytes().all(|b| {
        b.is_ascii_graphic()
            && !matches!(b, b'"' | b'\'' | b'\\' | b'#' | b'$' | b'{' | b'}' | b'`')
    })
}

/// An MCP server URL: `https://host[...]`, no credentials, at most 512 chars.
pub fn check_url(u: &str) -> Result<(), String> {
    if u.len() > 512 || !yaml_safe(u) {
        return Err("URL has characters that are not allowed".into());
    }
    let Some(rest) = u.strip_prefix("https://") else {
        return Err("URL must start with https://".into());
    };
    let host = rest.split(['/', '?']).next().unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err("URL needs a host (and no user:password@)".into());
    }
    Ok(())
}

/// A local stdio command: absolute, directly inside `dir` (the console flag
/// `--connectors-command-dir`; `None` = no local commands), file name with
/// `mcp` in it, no `..`.
pub fn check_command(cmd: &str, dir: Option<&str>) -> Result<(), String> {
    if cmd.len() > 256 || !yaml_safe(cmd) || cmd.contains("..") {
        return Err("command has characters that are not allowed".into());
    }
    let Some(dir) = dir.filter(|d| d.starts_with('/') && d.ends_with('/')) else {
        return Err("local commands are not enabled on this console".into());
    };
    if !cmd.starts_with(dir) {
        return Err(format!("command must be in {dir} (local allowlist)"));
    }
    let file = &cmd[dir.len()..];
    if file.is_empty()
        || file.contains('/')
        || !file.contains("mcp")
        || !file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("command file name must be plain and contain \"mcp\"".into());
    }
    Ok(())
}

/// Arguments for a stdio command: up to 16, each 1..=200 plain characters.
pub fn check_args(args: &[String]) -> Result<(), String> {
    if args.len() > 16 {
        return Err("at most 16 arguments".into());
    }
    for a in args {
        if a.is_empty()
            || a.len() > 200
            || !a.bytes().all(|b| {
                b.is_ascii_alphanumeric()
                    || matches!(
                        b,
                        b'.' | b'_' | b'-' | b'/' | b'=' | b':' | b'@' | b',' | b'+'
                    )
            })
        {
            return Err("arguments may use letters, digits and . _ - / = : @ , + only".into());
        }
    }
    Ok(())
}

/// A pasted token: 8..=4096 visible ASCII characters, no spaces or quotes.
pub fn check_token(t: &str) -> Result<(), String> {
    if t.len() < 8 || t.len() > 4096 || !t.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("the token must be 8 to 4096 visible characters, no spaces".into());
    }
    Ok(())
}

/// A custom connector's id from its display name (`My Server` -> `my-server`).
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(32).collect();
    out.trim_matches('-').to_string()
}

/// The `mcp_servers:` entry for `c` (no secret values; `secrets` is the
/// path of the env file, when there is a token).
pub fn entry_yaml(c: &Connector, secrets: Option<&Path>) -> String {
    let q = |s: &str| format!("\"{s}\"");
    let mut y = format!("  {}:\n", c.id);
    if let Some(u) = &c.url {
        y.push_str(&format!("    url: {}\n", q(u)));
        let t = if c.transport == "sse" { "sse" } else { "http" };
        y.push_str(&format!("    transport: {}\n", q(t)));
    } else if let Some(cmd) = &c.command {
        y.push_str(&format!("    command: {}\n", q(cmd)));
        let args: Vec<String> = c.args.iter().map(|a| q(a)).collect();
        y.push_str(&format!("    args: [{}]\n", args.join(", ")));
        y.push_str("    transport: \"stdio\"\n");
    }
    if let Some(p) = secrets {
        y.push_str(&format!("    env_file: {}\n", q(&p.display().to_string())));
        y.push_str(&format!("    require_env: [{}]\n", q(&c.token_env)));
        if c.url.is_some() {
            y.push_str(&format!(
                "    headers: {{Authorization: \"Bearer ${{{}}}\"}}\n",
                c.token_env
            ));
        } else {
            y.push_str(&format!("    pass_env: [{}]\n", q(&c.token_env)));
        }
    }
    y.push_str("    class: \"read\"\n    allow_writes: false\n    lazy: true\n");
    y
}

/// What the screen shows for one connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Configured, token present, and loaded by the running Hermes.
    Connected,
    /// Saved, but not loaded by Hermes yet (needs a restart).
    Saved,
    /// No MCP endpoint checked yet.
    Unavailable,
    /// An `.off` entry exists (disconnected earlier).
    Off,
    Available,
    NeedsSignIn,
    Builtin,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(unix)]
fn create_private(path: &Path, dir: bool) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if dir {
        if !path.is_dir() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)?;
            // Only folders we create; an existing one keeps its mode.
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn create_private(path: &Path, dir: bool) -> std::io::Result<()> {
    if dir {
        fs::create_dir_all(path)
    } else {
        Ok(())
    }
}

/// Write `data` to `path` as a new 0600 file (via a temp name + rename).
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    write_mode(path, data, 0o600)
}

/// Atomic write (temp name in the same folder, then rename) with `mode`.
fn write_mode(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{t}.{n}"));
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(mode);
    }
    let mut f = o.open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)
}

/// Rename `path` out of the way (`<path><suffix>`, then `.<unix>` if that
/// name is taken). Nothing is deleted.
fn rename_aside(path: &Path, suffix: &str) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut to = PathBuf::from(format!("{}{suffix}", path.display()));
    if to.exists() {
        let stamp = now_unix();
        let mut n = 0;
        loop {
            to = PathBuf::from(format!("{}{suffix}.{stamp}.{n}", path.display()));
            if !to.exists() {
                break;
            }
            n += 1;
        }
    }
    fs::rename(path, to)
}

/// The connectors directory on the console host.
#[derive(Debug, Clone)]
pub struct Store {
    pub dir: PathBuf,
    /// Local command allowlist dir (ends with `/`); `None` = URLs only.
    pub cmd_dir: Option<String>,
    /// `--connectors-hermes-file`: the live Hermes MCP servers file
    /// (`HERMES_RS_MCP_SERVERS_FILE`). When set, tokens go to `mcp.d/` next
    /// to it and the console keeps its own entries in a marked block there.
    pub hermes_file: Option<PathBuf>,
    /// Unix start time of the running Hermes gateway (from `/proc`).
    pub hermes_started: Option<u64>,
}

/// Result of a write action (the page shows a fixed message for each).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Saved(String),
    Disconnected(String),
    Applied,
    Refused(String),
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store {
            dir: dir.into(),
            cmd_dir: None,
            hermes_file: None,
            hermes_started: None,
        }
    }
    /// Allow local stdio commands directly inside `d`.
    pub fn with_command_dir(mut self, d: Option<String>) -> Store {
        self.cmd_dir = d.map(|d| if d.ends_with('/') { d } else { format!("{d}/") });
        self
    }
    fn servers_d(&self) -> PathBuf {
        self.dir.join("servers.d")
    }
    /// Point at the live Hermes servers file (and read Hermes' start time).
    pub fn with_hermes_file(mut self, f: Option<PathBuf>) -> Store {
        self.hermes_started = f.as_ref().and_then(|_| hermes_started());
        self.hermes_file = f;
        self
    }
    fn secrets_d(&self) -> PathBuf {
        match self.hermes_file.as_deref().and_then(Path::parent) {
            Some(p) => p.join("mcp.d"),
            None => self.dir.join("secrets"),
        }
    }
    /// Entries written by hand in the live Hermes file (outside our block).
    pub fn hand_entries(&self) -> Vec<HandEntry> {
        self.hermes_file
            .as_deref()
            .and_then(|f| fs::read_to_string(f).ok())
            .map(|t| hand_entries(&t))
            .unwrap_or_default()
    }
    fn hand(&self, id: &str) -> Option<HandEntry> {
        self.hand_entries().into_iter().find(|h| h.name == id)
    }
    /// Configured by hand in the live Hermes file (shown in the panel).
    pub fn hand_configured(&self, id: &str) -> bool {
        self.hand(id).is_some()
    }
    /// Every name in `need` has a `NAME=value` line in `p` (values never read out).
    fn env_complete(p: &Path, need: &[String]) -> bool {
        let Ok(t) = fs::read_to_string(p) else {
            return need.is_empty();
        };
        need.iter().all(|n| {
            t.lines().any(|l| {
                l.strip_prefix(n.as_str())
                    .is_some_and(|r| r.len() > 1 && r.starts_with('='))
            })
        })
    }
    /// Written after Hermes started (or Hermes not found): not loaded yet.
    fn pending(&self, files: &[PathBuf]) -> bool {
        let Some(start) = self.hermes_started else {
            return true;
        };
        files.iter().any(|f| {
            fs::metadata(f)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .is_some_and(|d| d.as_secs() >= start)
        })
    }
    pub fn entry_path(&self, id: &str) -> PathBuf {
        self.servers_d().join(format!("{id}.yaml"))
    }
    pub fn secret_path(&self, id: &str) -> PathBuf {
        self.secrets_d().join(format!("{id}.env"))
    }
    pub fn servers_file(&self) -> PathBuf {
        self.dir.join(SERVERS_FILE)
    }

    pub fn state(&self, c: &Connector) -> State {
        match c.auth {
            Auth::Builtin => return State::Builtin,
            Auth::OAuth => return State::NeedsSignIn,
            Auth::Unverified => return State::Unavailable,
            _ => {}
        }
        if self.hermes_file.is_some() {
            let secret = self.secret_path(&c.id);
            if let Some(h) = self.hand(&c.id) {
                let env = h.env_file.map(PathBuf::from).unwrap_or(secret);
                if !Self::env_complete(&env, &h.require_env) {
                    return State::Available;
                }
                return if self.pending(&[env]) {
                    State::Saved
                } else {
                    State::Connected
                };
            }
            if self.entry_path(&c.id).is_file() {
                let need: Vec<String> = if c.auth == Auth::Token {
                    vec![c.token_env.clone()]
                } else {
                    Vec::new()
                };
                if !Self::env_complete(&secret, &need) {
                    return State::Available;
                }
                let f = self.hermes_file.clone().unwrap_or_default();
                return if self.pending(&[secret, f]) {
                    State::Saved
                } else {
                    State::Connected
                };
            }
        } else if self.entry_path(&c.id).is_file() {
            return State::Saved;
        }
        if Path::new(&format!("{}.off", self.entry_path(&c.id).display())).exists() {
            State::Off
        } else {
            State::Available
        }
    }

    pub fn has_secret(&self, id: &str) -> bool {
        self.secret_path(id).is_file()
    }

    /// Custom connectors saved earlier (live entries with a `# custom` tag).
    pub fn customs(&self) -> Vec<Connector> {
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(self.servers_d()) else {
            return out;
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".yaml"))
            .collect();
        names.sort();
        for n in names {
            let Ok(text) = fs::read_to_string(self.servers_d().join(&n)) else {
                continue;
            };
            if let Some(c) = parse_custom_tag(&text) {
                out.push(c);
            }
        }
        out
    }

    /// Unix mtime of the generated servers file.
    pub fn saved_at(&self) -> Option<u64> {
        fs::metadata(self.servers_file())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
    }

    fn ensure_dirs(&self) -> std::io::Result<()> {
        create_private(&self.dir, true)?;
        create_private(&self.servers_d(), true)?;
        create_private(&self.secrets_d(), true)
    }

    /// Connect a catalog or custom connector. `token` is required for
    /// token auth, refused for others.
    pub fn connect(&self, c: &Connector, token: Option<&str>) -> Result<(), String> {
        if !wired(c) {
            return Err("coming soon: this connector is not wired up yet".into());
        }
        match c.auth {
            Auth::OAuth => return Err("needs sign-in setup (OAuth is not built yet)".into()),
            Auth::Builtin => return Err("built in; managed in the Hermes config".into()),
            Auth::Unverified => {
                return Err("no MCP endpoint is verified for this connector yet".into())
            }
            _ => {}
        }
        if !id_ok(&c.id) {
            return Err("bad connector id".into());
        }
        match (&c.url, &c.command) {
            (Some(u), None) => check_url(u)?,
            (None, Some(cmd)) => {
                check_command(cmd, self.cmd_dir.as_deref())?;
                check_args(&c.args)?;
            }
            _ => return Err("needs a URL or a command".into()),
        }
        let token = match (c.auth, token.map(str::trim).filter(|t| !t.is_empty())) {
            (Auth::Token, Some(t)) => {
                check_token(t)?;
                if !env_name_ok(&c.token_env) {
                    return Err("bad token variable name".into());
                }
                Some(t)
            }
            (Auth::Token, None) => return Err("paste the token first".into()),
            _ => None,
        };
        let io = |e: std::io::Error| format!("could not write the connector files ({})", e.kind());
        self.ensure_dirs().map_err(io)?;
        if let Some(h) = self.hand(&c.id) {
            // Configured by hand in the live file: only its token file is ours
            // to write, and only at the conventional path.
            let Some(t) = token else {
                return Err("configured in the Hermes file; paste its token".into());
            };
            let p = self.secret_path(&c.id);
            if h.env_file.as_deref() != Some(&*p.display().to_string())
                || !h.require_env.contains(&c.token_env)
            {
                return Err("the Hermes file names a different token file or variable".into());
            }
            rename_aside(&p, ".bak").map_err(io)?;
            let body = format!(
                "# {} token for Hermes MCP. Written by the console /connectors screen.\n{}={t}\n",
                c.id, c.token_env
            );
            return write_private(&p, body.as_bytes()).map_err(io);
        }
        let secrets = match token {
            Some(t) => {
                let p = self.secret_path(&c.id);
                rename_aside(&p, ".bak").map_err(io)?;
                let body = format!(
                    "# {} token for Hermes MCP. Written by the console /connectors screen.\n{}={t}\n",
                    c.id, c.token_env
                );
                write_private(&p, body.as_bytes()).map_err(io)?;
                Some(p)
            }
            None => None,
        };
        let mut text = String::new();
        if c.custom {
            text.push_str(&custom_tag(c));
        }
        text.push_str(&entry_yaml(c, secrets.as_deref()));
        let p = self.entry_path(&c.id);
        rename_aside(&p, ".bak").map_err(io)?;
        write_private(&p, text.as_bytes()).map_err(io)?;
        self.regenerate().map_err(io)
    }

    /// Rename the entry (and its secrets file) to `.off`; regenerate.
    pub fn disconnect(&self, id: &str) -> Result<(), String> {
        if !id_ok(id) {
            return Err("bad connector id".into());
        }
        let io = |e: std::io::Error| format!("could not rename the connector files ({})", e.kind());
        if self.hand(id).is_some() {
            // Hand-written entry: take its token away (renamed .off).
            let s = self.secret_path(id);
            if !s.is_file() {
                return Err("not connected".into());
            }
            return rename_aside(&s, ".off").map_err(io);
        }
        let p = self.entry_path(id);
        if !p.is_file() {
            return Err("not connected".into());
        }
        rename_aside(&p, ".off").map_err(io)?;
        rename_aside(&self.secret_path(id), ".off").map_err(io)?;
        self.regenerate().map_err(io)
    }

    /// Rebuild `mcp-servers.yaml` from every live `servers.d/*.yaml`.
    pub fn regenerate(&self) -> std::io::Result<()> {
        self.ensure_dirs()?;
        let mut names: Vec<String> = fs::read_dir(self.servers_d())?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".yaml") && id_ok(n.trim_end_matches(".yaml")))
            .collect();
        names.sort();
        let mut out = String::from(
            "# Hermes MCP servers added from the console /connectors screen.\n\
             # Generated; edit through the screen. Secrets live in secrets/*.env (0600).\n\
             mcp_servers:\n",
        );
        let mut n = 0;
        for name in names {
            let mut s = String::new();
            fs::File::open(self.servers_d().join(&name))?.read_to_string(&mut s)?;
            for line in s.lines().filter(|l| !l.trim_start().starts_with('#')) {
                out.push_str(line);
                out.push('\n');
            }
            n += 1;
        }
        if let Some(f) = &self.hermes_file {
            let block: String = out
                .lines()
                .filter(|l| l.starts_with("  "))
                .map(|l| format!("{l}\n"))
                .collect();
            splice_hermes_file(f, &block)?;
        }
        if n == 0 {
            // An empty map keeps the file valid for the loader.
            out = out.replace("mcp_servers:\n", "mcp_servers: {}\n");
        }
        write_private(&self.servers_file(), out.as_bytes())
    }
}

/// Start of the console's block in the live Hermes servers file.
pub const BLOCK_BEGIN: &str =
    "# >>> console /connectors entries (managed by the console; edit through the screen) >>>";
/// End of that block.
pub const BLOCK_END: &str = "# <<< console /connectors entries <<<";

/// A server written by hand in the live Hermes file (not by this screen).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HandEntry {
    pub name: String,
    pub env_file: Option<String>,
    pub require_env: Vec<String>,
}

fn yaml_scalar(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

/// Servers under `mcp_servers:` outside the console block, with their
/// `env_file` and `require_env` (two-space names, four-space keys).
pub fn hand_entries(text: &str) -> Vec<HandEntry> {
    let mut out: Vec<HandEntry> = Vec::new();
    let (mut in_map, mut in_block) = (false, false);
    for line in text.lines() {
        if line == BLOCK_BEGIN {
            in_block = true;
            continue;
        }
        if line == BLOCK_END {
            in_block = false;
            continue;
        }
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            in_map = line.trim_end() == "mcp_servers:";
            continue;
        }
        if !in_map || in_block {
            continue;
        }
        if let Some(name) = line.strip_prefix("  ").and_then(|r| r.strip_suffix(':')) {
            if id_ok(name) {
                out.push(HandEntry {
                    name: name.to_string(),
                    ..HandEntry::default()
                });
            }
            continue;
        }
        let Some(e) = out.last_mut() else { continue };
        let kv = line.trim_start();
        if let Some(v) = kv.strip_prefix("env_file:") {
            e.env_file = Some(yaml_scalar(v));
        } else if let Some(v) = kv.strip_prefix("require_env:") {
            let v = v.trim().trim_start_matches('[').trim_end_matches(']');
            e.require_env = v
                .split(',')
                .map(yaml_scalar)
                .filter(|s| env_name_ok(s))
                .collect();
        }
    }
    out
}

/// Put `entries` (two-space YAML under `mcp_servers:`) into the console
/// block of the live Hermes file, keeping everything else byte for byte.
/// `mcp_servers:` must be the last top-level key. A copy of the old file is
/// kept as `<file>.bak-console-<unix>` first; the file keeps its mode.
pub fn splice_hermes_file(f: &Path, entries: &str) -> std::io::Result<()> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());
    let old = fs::read_to_string(f)?;
    let mut keep = String::new();
    let (mut in_block, mut last_top) = (false, String::new());
    for line in old.lines() {
        if line == BLOCK_BEGIN {
            in_block = true;
            continue;
        }
        if line == BLOCK_END {
            in_block = false;
            continue;
        }
        if in_block {
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('#') && !line.trim().is_empty() {
            last_top = line.trim_end().to_string();
        }
        keep.push_str(line);
        keep.push('\n');
    }
    if in_block {
        return Err(bad("console block is not closed"));
    }
    if last_top != "mcp_servers:" {
        return Err(bad("mcp_servers: is not the last top-level key"));
    }
    let hand: Vec<String> = hand_entries(&keep).into_iter().map(|h| h.name).collect();
    for l in entries.lines() {
        if let Some(n) = l.strip_prefix("  ").and_then(|r| r.strip_suffix(':')) {
            if hand.iter().any(|h| h == n) {
                return Err(bad("name already configured by hand"));
            }
        }
    }
    let mut new = keep;
    if !entries.is_empty() {
        new.push_str(&format!("{BLOCK_BEGIN}\n{entries}{BLOCK_END}\n"));
    }
    if new == old {
        return Ok(());
    }
    let mode = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::metadata(f)?.permissions().mode() & 0o777
        }
        #[cfg(not(unix))]
        {
            0o644
        }
    };
    let mut bak = PathBuf::from(format!("{}.bak-console-{}", f.display(), now_unix()));
    let mut n = 0;
    while bak.exists() {
        n += 1;
        bak = PathBuf::from(format!("{}.bak-console-{}.{n}", f.display(), now_unix()));
    }
    fs::copy(f, &bak)?;
    write_mode(f, new.as_bytes(), mode)
}

/// Start (unix seconds) of the running Hermes gateway: the oldest process
/// whose executable name starts with `hermes-gateway`, from `/proc`.
pub fn hermes_started() -> Option<u64> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let btime: u64 = stat
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    let mut best: Option<u64> = None;
    for e in fs::read_dir("/proc").ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(exe) = fs::read_link(e.path().join("exe")) else {
            continue;
        };
        let base = exe
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if !base.starts_with("hermes-gateway") {
            continue;
        }
        let Ok(st) = fs::read_to_string(e.path().join("stat")) else {
            continue;
        };
        // Field 22 (starttime, clock ticks after boot); fields after the ")".
        let after = st.rsplit_once(')').map(|x| x.1).unwrap_or("");
        let ticks: u64 = after
            .split_whitespace()
            .nth(19)
            .and_then(|t| t.parse().ok())
            .unwrap_or(0);
        let t = btime + ticks / 100;
        best = Some(best.map_or(t, |b| b.min(t)));
    }
    best
}

/// First line of a custom entry: `# custom name=<name> section=custom auth=<a> env=<E>`.
fn custom_tag(c: &Connector) -> String {
    let auth = match c.auth {
        Auth::Token => "token",
        _ => "none",
    };
    let name: String = c
        .name
        .chars()
        .map(|ch| if ch.is_whitespace() { '_' } else { ch })
        .collect();
    format!("# custom name={name} auth={auth} env={}\n", c.token_env)
}

fn parse_custom_tag(text: &str) -> Option<Connector> {
    let first = text.lines().next()?.strip_prefix("# custom ")?;
    let mut name = String::new();
    let mut auth = Auth::None;
    let mut env = String::new();
    for kv in first.split(' ') {
        match kv.split_once('=') {
            Some(("name", v)) => name = v.replace('_', " "),
            Some(("auth", "token")) => auth = Auth::Token,
            Some(("env", v)) => env = v.to_string(),
            _ => {}
        }
    }
    let id = text.lines().find_map(|l| {
        let t = l.strip_prefix("  ")?;
        if t.starts_with(' ') {
            return None;
        }
        t.strip_suffix(':').map(str::to_string)
    })?;
    let val = |k: &str| {
        text.lines().find_map(|l| {
            l.trim()
                .strip_prefix(k)
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    };
    let url = val("url:");
    let command = val("command:");
    let initials: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    Some(Connector {
        logo: String::new(),
        blurb: match (&url, &command) {
            (Some(u), _) => u.clone(),
            (_, Some(c)) => c.clone(),
            _ => String::new(),
        },
        id,
        name,
        section: "custom".into(),
        initials,
        url,
        command,
        args: Vec::new(),
        transport: String::new(),
        auth,
        token_env: env,
        token_label: "Token".into(),
        verified: false,
        source: String::new(),
        note: String::new(),
        custom: true,
    })
}

/// Build a custom connector from the add form. `target` is an https URL or
/// an allowlisted command (arguments after spaces).
pub fn custom_from_form(
    name: &str,
    target: &str,
    auth: &str,
    env: &str,
    catalog: &Catalog,
    cmd_dir: Option<&str>,
) -> Result<Connector, String> {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 {
        return Err("give the connector a name (up to 48 characters)".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '.')
    {
        return Err("the name may use letters, digits, spaces, - and . only".into());
    }
    let id = slug(name);
    if !id_ok(&id) {
        return Err("the name must start with a letter".into());
    }
    if catalog.connectors.iter().any(|c| c.id == id) {
        return Err("that name belongs to a featured connector; connect it from its row".into());
    }
    let target = target.trim();
    let (url, command, args, transport) = if target.starts_with("https://") {
        check_url(target)?;
        (Some(target.to_string()), None, Vec::new(), "http")
    } else if target.starts_with('/') {
        let mut parts = target.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_string();
        check_command(&cmd, cmd_dir)?;
        let args: Vec<String> = parts.map(str::to_string).collect();
        check_args(&args)?;
        (None, Some(cmd), args, "stdio")
    } else {
        return Err("enter an https:// URL or a local command path".into());
    };
    let auth = match auth {
        "token" => Auth::Token,
        "none" | "" => Auth::None,
        _ => return Err("unknown auth choice".into()),
    };
    let token_env = if auth == Auth::Token {
        let e = env.trim();
        let e = if e.is_empty() {
            format!("{}_TOKEN", id.to_ascii_uppercase().replace('-', "_"))
        } else {
            e.to_string()
        };
        if !env_name_ok(&e) {
            return Err("the token variable must look like MY_TOKEN".into());
        }
        e
    } else {
        String::new()
    };
    Ok(Connector {
        logo: String::new(),
        id,
        name: name.to_string(),
        section: "custom".into(),
        initials: name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase(),
        blurb: url.clone().or(command.clone()).unwrap_or_default(),
        url,
        command,
        args,
        transport: transport.into(),
        auth,
        token_env,
        token_label: "Token".into(),
        verified: false,
        source: String::new(),
        note: String::new(),
        custom: true,
    })
}

/// 32 hex characters from the OS (`/dev/urandom`); `None` if unreadable.
pub fn new_csrf() -> Option<String> {
    let mut b = [0u8; 16];
    fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .ok()?;
    Some(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// A token we issued: exactly 32 lowercase hex characters.
pub fn csrf_ok(t: &str) -> bool {
    t.len() == 32
        && t.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Constant-time equality for two CSRF tokens.
pub fn ct_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ---- the page handler (owner gate already passed in `Ui::handle`) ----

/// Fixed messages after a successful post (PRG redirect `?done=..&id=..`).
fn done_text(done: &str, name: &str) -> Option<String> {
    Some(match done {
        "saved" => format!("{name}: saved. Read-only (writes off)."),
        "added" => format!("{name}: added. Read-only (writes off)."),
        "disconnected" => format!("{name}: disconnected (entry renamed to .off, nothing deleted)."),
        _ => return None,
    })
}

impl Ui {
    fn cn_csrf_cookie(req: &Request<'_>, token: &str) -> String {
        let mut c = format!("{CSRF_COOKIE}={token}; HttpOnly; Path={PATH}; SameSite=Strict");
        if Self::cookie_secure(req) {
            c.push_str("; Secure");
        }
        c
    }

    pub(crate) fn connectors_room(&self, req: &Request<'_>) -> Reply {
        let cat = catalog();
        let cmd_dir = self.cfg.files.connectors_command_dir.as_ref().map(|p| {
            let mut s = p.display().to_string();
            if !s.ends_with('/') {
                s.push('/');
            }
            s
        });
        let store = self.cfg.files.connectors_dir.as_ref().map(|d| {
            Store::new(d)
                .with_command_dir(cmd_dir)
                .with_hermes_file(self.cfg.files.connectors_hermes_file.clone())
        });
        let have = gate::cookie(req.cookie, CSRF_COOKIE).filter(|t| csrf_ok(t));
        let customs = store.as_ref().map(Store::customs).unwrap_or_default();
        let name_of = |id: &str| -> String {
            cat.connectors
                .iter()
                .chain(customs.iter())
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        };
        let mut status = 200;
        let mut notice = None;
        let mut q = String::new();
        if req.method == "POST" {
            let form = html::form_pairs(&String::from_utf8_lossy(req.body));
            let sent = html::form_get(&form, "csrf").unwrap_or("");
            let csrf_good = have.as_deref().is_some_and(|c| ct_eq(c, sent));
            if !Self::same_origin(req) || !csrf_good {
                return Reply::text(403, "Forbidden: CSRF check failed\n");
            }
            let Some(st) = store.as_ref() else {
                return Reply::text(409, "Connector storage is not configured\n");
            };
            let id = html::form_get(&form, "id").unwrap_or("");
            let token = html::form_get(&form, "token");
            let r: Result<(&str, String), String> = match html::form_get(&form, "op") {
                Some("connect") => match cat
                    .connectors
                    .iter()
                    .chain(customs.iter())
                    .find(|c| c.id == id)
                {
                    Some(c) => st.connect(c, token).map(|_| ("saved", c.id.clone())),
                    None => Err("unknown connector".into()),
                },
                Some("disconnect") => st.disconnect(id).map(|_| ("disconnected", id.to_string())),
                Some("add") => custom_from_form(
                    html::form_get(&form, "name").unwrap_or(""),
                    html::form_get(&form, "target").unwrap_or(""),
                    html::form_get(&form, "auth").unwrap_or("none"),
                    html::form_get(&form, "env").unwrap_or(""),
                    &cat,
                    st.cmd_dir.as_deref(),
                )
                .and_then(|c| {
                    if st.entry_path(&c.id).exists() {
                        return Err("a connector with that name already exists".into());
                    }
                    if c.auth == Auth::None && token.is_some_and(|t| !t.trim().is_empty()) {
                        return Err("pick \"Bearer token\" to save a token".into());
                    }
                    st.connect(&c, token).map(|_| ("added", c.id.clone()))
                }),
                _ => Err("unknown action".into()),
            };
            match r {
                Ok((done, id)) => {
                    return Reply::redirect(
                        303,
                        &format!("{PATH}?done={done}&id={}", html::url_enc(&id)),
                    )
                }
                Err(e) => {
                    status = 400;
                    notice = Some((false, e));
                }
            }
        } else {
            let qp = html::form_pairs(req.query);
            q = html::form_get(&qp, "q")
                .unwrap_or("")
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect();
            if let Some(d) = html::form_get(&qp, "done") {
                let id = html::form_get(&qp, "id").filter(|i| id_ok(i)).unwrap_or("");
                notice = done_text(d, &name_of(id)).map(|m| (true, m));
            }
        }
        let token = have.clone().or_else(new_csrf).unwrap_or_default();
        let saved = store
            .as_ref()
            .and_then(Store::saved_at)
            .map(|t| crate::pages::p3::zagreb_hms(t as f64 * 1000.0));
        let view = crate::pages::connectors::View {
            catalog: &cat,
            customs: &customs,
            store: store.as_ref(),
            csrf: &token,
            notice,
            saved,
            q: &q,
        };
        let body = crate::pages::connectors::body(&view);
        let mut f = self.frame(req, "Galion Console · Connectors");
        f.extra_css_href = Some(&self.cn_css_href);
        // The reference screen is dark whatever the console theme is.
        f.light = Some(false);
        let mut r = Reply::html(status, layout::document(&f, " class=\"cn-body\"", &body));
        if have.is_none() && csrf_ok(&token) {
            r.headers
                .push(("Set-Cookie".into(), Self::cn_csrf_cookie(req, &token)));
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_catalog_parses_with_sections() {
        let c = catalog();
        let ids: Vec<&str> = c.connectors.iter().map(|x| x.id.as_str()).collect();
        // Mate's reference order (steering 2026-10-30 12:58 CET).
        let order = |sec: &str| -> Vec<&str> {
            c.connectors
                .iter()
                .filter(|x| x.section == sec)
                .map(|x| x.name.as_str())
                .collect()
        };
        assert_eq!(
            order("featured"),
            [
                "Gmail",
                "Google Calendar",
                "Google Drive",
                "Canva",
                "Stripe",
                "Notion",
                "Box",
                "Wix"
            ]
        );
        assert_eq!(order("productivity").len(), 11);
        assert_eq!(order("productivity")[0], "Asana");
        assert_eq!(order("finance").len(), 11);
        assert_eq!(order("finance")[10], "etoro");
        assert_eq!(
            order("developer"),
            [
                "GitHub",
                "Datadog",
                "Excalidraw",
                "Mixpanel",
                "Netlify",
                "Vercel"
            ]
        );
        assert_eq!(
            order("microsoft"),
            ["Outlook", "Outlook Calendar", "Microsoft Teams"]
        );
        assert_eq!(order("sales"), ["HubSpot", "Meltwater"]);
        assert_eq!(order("social"), ["Buffer"]);
        assert_eq!(order("analytics"), ["Google Cloud BigQuery"]);
        assert_eq!(order("advertising"), ["X Ads"]);
        let titles: Vec<&str> = c.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            &titles[..9],
            [
                "Featured",
                "Productivity",
                "Finance",
                "Developer",
                "Microsoft 365",
                "Sales",
                "Social Media",
                "Analytics",
                "Advertising"
            ]
        );
        assert!(ids.contains(&"openmind"));
        for x in &c.connectors {
            if x.verified {
                assert!(x.source.starts_with("https://") || x.auth == Auth::Builtin);
                assert!(x.url.is_some() || x.command.is_some());
            } else {
                assert_eq!(x.auth, Auth::Unverified, "{}", x.id);
                assert!(x.url.is_none() && x.command.is_none());
            }
            assert!(!x.initials.is_empty() && x.initials.len() <= 3);
        }
        let oauth: Vec<&str> = c
            .connectors
            .iter()
            .filter(|x| x.auth == Auth::OAuth)
            .map(|x| x.id.as_str())
            .collect();
        for g in ["gmail", "google-calendar", "google-drive"] {
            assert!(oauth.contains(&g), "{g} is OAuth-only");
        }
    }

    #[test]
    fn catalog_rejects_bad_entries() {
        let base =
            |c: &str| format!(r#"{{"sections":[{{"id":"p","title":"P"}}],"connectors":[{c}]}}"#);
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"none","url":"http://x"}"#
        ))
        .is_err());
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"q","auth":"none","url":"https://x"}"#
        ))
        .is_err());
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"token","url":"https://x"}"#
        ))
        .is_err());
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"none","command":"/usr/lib/x-mcp","verified":true,"source":"https://s"}"#
        ))
        .is_err());
        assert!(
            parse_catalog(&base(
                r#"{"id":"x","section":"p","auth":"none","url":"https://x/mcp"}"#
            ))
            .is_err(),
            "verified entries need a source"
        );
        assert!(
            parse_catalog(&base(
                r#"{"id":"x","section":"p","auth":"unverified","url":"https://x/mcp"}"#
            ))
            .is_err(),
            "unverified entries carry no endpoint"
        );
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"unverified","verified":true}"#
        ))
        .is_err());
        assert!(parse_catalog(&base(r#"{"id":"x","section":"p","auth":"unverified"}"#)).is_ok());
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"none","url":"https://x/mcp","verified":true,"source":"https://s"}"#
        ))
        .is_ok());
    }

    #[test]
    fn validation() {
        assert!(check_url("https://mcp.stripe.com").is_ok());
        for bad in [
            "http://x",
            "file:///etc/passwd",
            "https://",
            "https://u:p@x",
            "https://x/\"y",
            "https://x/${A}",
            "https://x #c",
            "javascript:alert(1)",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
        let dir = Some("/srv/mcp-bin/");
        assert!(check_command("/srv/mcp-bin/openmind-mcp-stack", dir).is_ok());
        assert!(check_command("/srv/mcp-bin/openmind-mcp-stack", None).is_err());
        for bad in [
            "/usr/bin/env",
            "/srv/mcp-bin/console-serve",
            "/srv/mcp-bin/../../../usr/bin/mcp",
            "/srv/mcp-bin/sub/mcp",
            "npx",
        ] {
            assert!(check_command(bad, dir).is_err(), "{bad}");
        }
        assert!(check_args(&["--vault".into(), "/a/b".into()]).is_ok());
        assert!(check_args(&["$(id)".into()]).is_err());
        assert!(check_token("short").is_err());
        assert!(check_token("has space in it").is_err());
        assert!(check_token("abc_defghijklmnop1").is_ok());
        assert_eq!(slug("  My Server 2 "), "my-server-2");
        assert!(id_ok("my-server") && !id_ok("1x") && !id_ok("A"));
        assert!(csrf_ok(&new_csrf().unwrap()));
        assert!(ct_eq("ab", "ab") && !ct_eq("ab", "ac") && !ct_eq("a", "ab"));
    }

    #[test]
    fn entry_yaml_has_no_secret_and_writes_off() {
        let c = catalog();
        let gh = c.connectors.iter().find(|x| x.id == "github").unwrap();
        let y = entry_yaml(gh, Some(Path::new("/d/secrets/github.env")));
        assert!(y.starts_with("  github:\n"));
        assert!(y.contains("url: \"https://api.githubcopilot.com/mcp/\""));
        assert!(y.contains("headers: {Authorization: \"Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}\"}"));
        assert!(y.contains("allow_writes: false") && y.contains("class: \"read\""));
        assert!(y.contains("env_file: \"/d/secrets/github.env\""));
    }
}

/// The page through `Ui::handle`: owner gate, CSRF, writes, no secret echo.
#[cfg(test)]
mod page_tests {
    use super::*;
    use crate::port4::HostFiles;
    use crate::UiConfig;
    use std::os::unix::fs::PermissionsExt;

    const TOKEN: &str = "test-only-not-a-real-token-0123456789";

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            // Test scratch only (under the system temp dir).
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tmp(tag: &str) -> Tmp {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "cn-test-{tag}-{}-{}",
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }

    fn ui(dir: Option<&Path>) -> Ui {
        Ui::new(UiConfig {
            upstream: "127.0.0.1:9".parse().unwrap(),
            pulse: None,
            extra_hosts: vec![],
            desk: None,
            files: HostFiles {
                connectors_dir: dir.map(Path::to_path_buf),
                ..HostFiles::default()
            },
        })
    }

    fn get<'a>(host: &'a str, loopback: bool, query: &'a str) -> Request<'a> {
        Request {
            method: "GET",
            path: PATH,
            query,
            host,
            cookie: None,
            origin: None,
            sec_fetch_site: None,
            forwarded_proto: None,
            peer_loopback: loopback,
            client_ip: if loopback {
                "127.0.0.1".into()
            } else {
                "203.0.113.9".into()
            },
            content_type: None,
            body: b"",
        }
    }

    fn post<'a>(cookie: Option<&'a str>, body: &'a [u8]) -> Request<'a> {
        Request {
            method: "POST",
            cookie,
            sec_fetch_site: Some("same-origin"),
            content_type: Some("application/x-www-form-urlencoded"),
            body,
            ..get("127.0.0.1:18080", true, "")
        }
    }

    fn csrf_of(r: &Reply) -> String {
        let c = r.header("Set-Cookie").expect("csrf cookie");
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict"));
        c.split(';')
            .next()
            .unwrap()
            .split_once('=')
            .unwrap()
            .1
            .to_string()
    }

    fn body(r: &Reply) -> String {
        String::from_utf8(r.body.clone()).unwrap()
    }

    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }

    #[test]
    fn renders_sections_for_the_owner_and_is_in_the_menu() {
        let t = tmp("render");
        let ui = ui(Some(&t.0));
        let r = ui.handle(&get("127.0.0.1:18080", true, ""));
        assert_eq!(r.status, 200);
        let b = body(&r);
        for s in [
            "Connectors let Hermes use external tools and data sources.",
            "placeholder=\"Search connectors...\"",
            "href=\"/console\" aria-label=\"Back",
            "Featured",
            "Productivity",
            "Finance",
            "Developer",
            "Microsoft 365",
            "Social Media",
            "Advertising",
            "Custom Connector",
            "Add your own custom connector",
            "GitHub",
            "Gmail",
            "Needs sign-in setup",
            "/_ui/connectors.",
            "data-theme=\"dark\"",
        ] {
            assert!(b.contains(s), "missing {s}");
        }
        // Section order follows the reference screenshots.
        let at = |s: &str| b.find(&format!("<h2 class=\"cn-lab\">{s}</h2>")).unwrap();
        assert!(at("Connected") < at("Featured") && at("Featured") < at("Productivity"));
        assert!(at("Finance") < at("Developer") && at("Advertising") < at("Custom Connector"));
        // Only really wired connectors are under Connected: OpenMind (built
        // in), not GitHub (nothing saved, client not live).
        let connected = &b[at("Connected")..at("Featured")];
        assert!(connected.contains("OpenMind") && !connected.contains("GitHub"));
        assert!(b[at("Developer")..].contains("GitHub"));
        // Real logos, served locally (never hotlinked).
        assert!(b.matches("<img ").count() >= 40);
        for src in b.split("src=\"").skip(1) {
            assert!(src.starts_with(crate::logos::PREFIX), "local logo only");
        }
        let src = b.split("src=\"").nth(1).unwrap();
        let src = &src[..src.find('"').unwrap()];
        let img = ui.handle(&Request {
            path: src,
            ..get("127.0.0.1:18080", true, "")
        });
        assert_eq!(img.status, 200);
        // Scope (t456u/t457u): Featured and GitHub connect; the rest say
        // "Coming soon" instead of Connect.
        let row_of = |id: &str| {
            let s = b.find(&format!("id=\"c-{id}\"")).unwrap();
            &b[s..s + b[s..].find("</summary>").unwrap()]
        };
        for id in ["gmail", "stripe", "wix", "github"] {
            assert!(
                row_of(id).contains(">Connect<") && !row_of(id).contains("Coming soon"),
                "{id}"
            );
        }
        for id in ["linear", "asana", "coinbase", "vercel", "hubspot", "x-ads"] {
            assert!(
                row_of(id).contains("Coming soon") && !row_of(id).contains(">Connect<"),
                "{id}"
            );
        }
        // Search filters on the server.
        let s = body(&ui.handle(&get("127.0.0.1:18080", true, "q=goo")));
        assert!(
            s.contains("Google Drive")
                && !s.contains(">Stripe<")
                && !s.contains("Custom Connector")
        );
        let s = body(&ui.handle(&get("127.0.0.1:18080", true, "q=zzzz")));
        assert!(s.contains("No connectors match."));
        assert!(!b.contains("<script"), "no script on /connectors");
        // The three-dash console menu lists the room.
        let m = ui.handle(&Request {
            path: "/console",
            ..get("127.0.0.1:18080", true, "")
        });
        assert!(body(&m).contains("href=\"/connectors\""));
        // Its stylesheet is served.
        let href = b.split("/_ui/connectors.").nth(1).unwrap();
        let href = format!("/_ui/connectors.{}", &href[..href.find('"').unwrap()]);
        let css = ui.handle(&Request {
            path: &href,
            ..get("127.0.0.1:18080", true, "")
        });
        assert_eq!(css.status, 200);
    }

    #[test]
    fn refused_without_owner_auth() {
        let t = tmp("gate");
        // Public console host, no owner cookie, admin API unreachable: fail
        // closed. (The "not the owner" answer -> 401 is covered end to end in
        // console-static tests/ui.rs with its fake admin API.)
        let ui = ui(Some(&t.0));
        let r = ui.handle(&get("console.galion.app", false, ""));
        assert!(r.status >= 400, "{}", r.status);
        assert!(!body(&r).contains("Add your own custom connector"));
        let b = format!("op=connect&id=github&token={TOKEN}");
        let mut p = post(None, b.as_bytes());
        p.host = "console.galion.app";
        p.peer_loopback = false;
        assert!(ui.handle(&p).status >= 400);
        assert!(!t.0.join(SERVERS_FILE).exists());
    }

    #[test]
    fn csrf_and_same_origin_are_required() {
        let t = tmp("csrf");
        let ui = ui(Some(&t.0));
        let tok = csrf_of(&ui.handle(&get("127.0.0.1:18080", true, "")));
        let cookie = format!("{CSRF_COOKIE}={tok}");
        let b = format!("csrf=wrong{}&op=connect&id=github&token={TOKEN}", &tok[5..]);
        assert_eq!(ui.handle(&post(Some(&cookie), b.as_bytes())).status, 403);
        let b = format!("op=connect&id=github&token={TOKEN}");
        assert_eq!(ui.handle(&post(Some(&cookie), b.as_bytes())).status, 403);
        let b = format!("csrf={tok}&op=connect&id=github&token={TOKEN}");
        let mut cross = post(Some(&cookie), b.as_bytes());
        cross.sec_fetch_site = Some("cross-site");
        assert_eq!(ui.handle(&cross).status, 403);
        assert!(!t.0.join("servers.d/github.yaml").exists());
    }

    #[test]
    fn connect_writes_config_and_0600_secret_and_never_echoes_it() {
        let t = tmp("write");
        let ui = ui(Some(&t.0));
        let tok = csrf_of(&ui.handle(&get("127.0.0.1:18080", true, "")));
        let cookie = format!("{CSRF_COOKIE}={tok}");
        let b = format!("csrf={tok}&op=connect&id=github&token={TOKEN}");
        let r = ui.handle(&post(Some(&cookie), b.as_bytes()));
        assert_eq!(r.status, 303);
        let loc = r.header("Location").unwrap().to_string();
        assert!(!loc.contains(TOKEN) && !body(&r).contains(TOKEN));
        // The follow-up page: "saved", connected, no token.
        let (path, q) = loc.split_once('?').unwrap();
        assert_eq!(path, PATH);
        let mut g = get("127.0.0.1:18080", true, q);
        g.cookie = Some(&cookie);
        let page = ui.handle(&g);
        let pb = body(&page);
        assert!(pb.contains("GitHub: saved"));
        assert!(pb.contains("Saved, not live yet"));
        // Saved is not Connected until Hermes loads the file.
        let c0 = pb.find("<h2 class=\"cn-lab\">Connected</h2>").unwrap();
        let sec = pb.find("<h2 class=\"cn-lab\">Featured</h2>").unwrap();
        assert!(!pb[c0..sec].contains("GitHub"));
        assert!(!pb.contains(TOKEN));
        for (_, v) in &page.headers {
            assert!(!v.contains(TOKEN));
        }
        // Files: entry, 0600 secret, servers file.
        let secret = t.0.join("secrets/github.env");
        let mode = fs::metadata(&secret).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(fs::read_to_string(&secret).unwrap().contains(TOKEN));
        let servers = fs::read_to_string(t.0.join(SERVERS_FILE)).unwrap();
        assert_eq!(
            fs::metadata(t.0.join(SERVERS_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(servers.contains("\nmcp_servers:\n  github:\n"));
        assert!(servers.contains("url: \"https://api.githubcopilot.com/mcp/\""));
        assert!(servers.contains("Authorization: \"Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}\""));
        assert!(servers.contains("allow_writes: false"));
        // Only the secrets file holds the token.
        let mut all = Vec::new();
        walk(&t.0, &mut all);
        for p in all {
            if p != secret {
                let s = fs::read(&p).unwrap();
                assert!(
                    !String::from_utf8_lossy(&s).contains(TOKEN),
                    "token leaked into {}",
                    p.display()
                );
            }
        }
        assert_eq!(
            fs::metadata(t.0.join("secrets"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn disconnect_renames_off_and_custom_add_validates() {
        let t = tmp("off");
        let ui = ui(Some(&t.0));
        let tok = csrf_of(&ui.handle(&get("127.0.0.1:18080", true, "")));
        let cookie = format!("{CSRF_COOKIE}={tok}");
        let send = |b: String| ui.handle(&post(Some(&cookie), b.as_bytes()));
        // Custom: http refused, unknown command dir refused.
        let r = send(format!(
            "csrf={tok}&op=add&name=Echo&target=http%3A%2F%2Fexample.com%2Fmcp&auth=none"
        ));
        assert_eq!(r.status, 400);
        let r = send(format!(
            "csrf={tok}&op=add&name=Echo&target=%2Fusr%2Fbin%2Fenv+id&auth=none"
        ));
        assert_eq!(r.status, 400);
        // Custom https, no auth.
        let r = send(format!(
            "csrf={tok}&op=add&name=Echo+Test&target=https%3A%2F%2Fexample.com%2Fmcp&auth=none"
        ));
        assert_eq!(r.status, 303, "{}", body(&r));
        let s = fs::read_to_string(t.0.join(SERVERS_FILE)).unwrap();
        assert!(s.contains("  echo-test:\n") && s.contains("allow_writes: false"));
        let page = ui.handle(&Request {
            cookie: Some(&cookie),
            ..get("127.0.0.1:18080", true, "")
        });
        assert!(body(&page).contains("Echo Test"));
        // Same name again: refused.
        let r = send(format!(
            "csrf={tok}&op=add&name=Echo+Test&target=https%3A%2F%2Fexample.com%2Fmcp&auth=none"
        ));
        assert_eq!(r.status, 400);
        // Disconnect: renamed .off, file regenerated empty.
        let r = send(format!("csrf={tok}&op=disconnect&id=echo-test"));
        assert_eq!(r.status, 303);
        assert!(t.0.join("servers.d/echo-test.yaml.off").exists());
        assert!(!t.0.join("servers.d/echo-test.yaml").exists());
        let s = fs::read_to_string(t.0.join(SERVERS_FILE)).unwrap();
        assert!(s.trim_end().ends_with("\nmcp_servers: {}"));
        // OAuth-only, unverified and unknown actions are refused, nothing faked.
        assert_eq!(
            send(format!("csrf={tok}&op=connect&id=coinbase")).status,
            400
        );
        let b = format!("csrf={tok}&op=connect&id=linear&token={TOKEN}");
        assert_eq!(send(b).status, 400, "not featured: coming soon");
        assert!(!t.0.join("servers.d/linear.yaml").exists());
        assert_eq!(send(format!("csrf={tok}&op=connect&id=gmail")).status, 400);
        assert!(!t.0.join("servers.d/gmail.yaml").exists());
        assert_eq!(send(format!("csrf={tok}&op=restart")).status, 400);
    }

    #[test]
    fn without_a_dir_the_screen_is_read_only() {
        let ui = ui(None);
        let r = ui.handle(&get("127.0.0.1:18080", true, ""));
        assert_eq!(r.status, 200);
        let b = body(&r);
        assert!(b.contains("read-only") && !b.contains("name=\"target\""));
    }

    const HERMES_YAML: &str = r#"# Hermes MCP servers (hand-written header)
log_level: info
mcp_servers:
  openmind:
    command: openmind-mcp-stack
    class: read
  github:
    command: github-mcp-local
    env_file: @/mcp.d/github.env
    require_env: ["GITHUB_PERSONAL_ACCESS_TOKEN"]
    class: read
    allow_writes: false
    lazy: true
"#;

    /// A fake /etc/hermes-rs: servers file (mode 0644) plus an mcp.d folder.
    fn hermes_dir(t: &Tmp) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let h = t.0.join("hermes");
        fs::create_dir_all(h.join("mcp.d")).unwrap();
        fs::set_permissions(h.join("mcp.d"), fs::Permissions::from_mode(0o711)).unwrap();
        let f = h.join("mcp-servers.yaml");
        fs::write(&f, HERMES_YAML.replace('@', &h.display().to_string())).unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
        f
    }

    fn by_id(id: &str) -> Connector {
        catalog()
            .connectors
            .into_iter()
            .find(|c| c.id == id)
            .unwrap()
    }

    #[test]
    fn hand_entries_parse_names_env_file_and_vars() {
        let h = hand_entries(&HERMES_YAML.replace('@', "/h"));
        let names: Vec<&str> = h.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["openmind", "github"]);
        assert_eq!(h[1].env_file.as_deref(), Some("/h/mcp.d/github.env"));
        assert_eq!(h[1].require_env, ["GITHUB_PERSONAL_ACCESS_TOKEN"]);
        assert!(h[0].require_env.is_empty());
    }

    #[test]
    fn live_file_github_token_only_touches_its_env_file() {
        use std::os::unix::fs::PermissionsExt;
        let t = tmp("hermes-gh");
        let f = hermes_dir(&t);
        let before = fs::read_to_string(&f).unwrap();
        let mut s = Store::new(t.0.join("console")).with_hermes_file(Some(f.clone()));
        s.hermes_started = None;
        let gh = by_id("github");
        assert!(s.hand_configured("github"));
        assert_eq!(s.state(&gh), State::Available, "no token yet: Connect");
        let secret = "test-token-not-real-123";
        s.connect(&gh, Some(secret)).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), before, "YAML untouched");
        let env = f.parent().unwrap().join("mcp.d/github.env");
        let m = fs::metadata(&env).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o600);
        assert!(fs::read_to_string(&env)
            .unwrap()
            .contains("GITHUB_PERSONAL_ACCESS_TOKEN=test-token"));
        let d = fs::metadata(f.parent().unwrap().join("mcp.d")).unwrap();
        assert_eq!(
            d.permissions().mode() & 0o777,
            0o711,
            "existing dir mode kept"
        );
        assert_eq!(s.state(&gh), State::Saved, "Hermes not seen: not live yet");
        s.hermes_started = Some(u64::MAX);
        assert_eq!(s.state(&gh), State::Connected, "token older than Hermes");
        s.hermes_started = Some(0);
        assert_eq!(s.state(&gh), State::Saved, "token newer than Hermes");
        // Replace keeps the old token as .bak; disconnect renames to .off.
        s.connect(&gh, Some(secret)).unwrap();
        assert!(env.with_extension("env.bak").exists());
        s.disconnect("github").unwrap();
        assert!(!env.exists() && env.with_extension("env.off").exists());
        assert_eq!(s.state(&gh), State::Available);
        assert_eq!(fs::read_to_string(&f).unwrap(), before);
    }

    #[test]
    fn live_file_new_entry_goes_to_marked_block_and_back_out() {
        use std::os::unix::fs::PermissionsExt;
        let t = tmp("hermes-block");
        let f = hermes_dir(&t);
        let before = fs::read_to_string(&f).unwrap();
        let s = Store::new(t.0.join("console")).with_hermes_file(Some(f.clone()));
        let stripe = by_id("stripe");
        assert_eq!(s.state(&stripe), State::Available);
        s.connect(&stripe, Some("test-stripe-not-real-456"))
            .unwrap();
        let after = fs::read_to_string(&f).unwrap();
        assert!(
            after.starts_with(&before),
            "hand-written part kept byte for byte"
        );
        assert!(after.contains(BLOCK_BEGIN) && after.contains(BLOCK_END));
        assert!(after.contains("  stripe:\n") && after.contains("allow_writes: false"));
        assert!(
            !after.contains("test-stripe-not-real"),
            "no token in the YAML"
        );
        let names: Vec<String> = hand_entries(&after).into_iter().map(|h| h.name).collect();
        assert_eq!(
            names,
            ["openmind", "github"],
            "block entries are not hand-written"
        );
        assert_eq!(
            fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let baks = fs::read_dir(f.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".bak-console-"))
            .count();
        assert_eq!(baks, 1, "old file copied aside first");
        assert!(f.parent().unwrap().join("mcp.d/stripe.env").exists());
        assert_eq!(s.state(&stripe), State::Saved);
        // Notion joins the same block; the block is rebuilt, not repeated.
        s.connect(&by_id("notion"), Some("test-notion-not-real-789"))
            .unwrap();
        let two = fs::read_to_string(&f).unwrap();
        assert_eq!(two.matches(BLOCK_BEGIN).count(), 1);
        assert!(two.contains("  notion:\n") && two.contains("  stripe:\n"));
        s.disconnect("stripe").unwrap();
        s.disconnect("notion").unwrap();
        assert_eq!(
            fs::read_to_string(&f).unwrap(),
            before,
            "back to the hand-written file"
        );
        assert!(f.parent().unwrap().join("mcp.d/stripe.env.off").exists());
        // Hermes-file names cannot be shadowed by a custom entry.
        assert!(splice_hermes_file(&f, "  github:\n    url: https://x.example/mcp\n").is_err());
    }

    #[test]
    fn splice_refuses_when_mcp_servers_is_not_last() {
        let t = tmp("hermes-order");
        let f = t.0.join("m.yaml");
        fs::write(&f, "mcp_servers:\n  a:\n    command: x\nother: 1\n").unwrap();
        let before = fs::read_to_string(&f).unwrap();
        assert!(splice_hermes_file(&f, "  b:\n    command: y\n").is_err());
        assert_eq!(fs::read_to_string(&f).unwrap(), before);
    }
}
