// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `/connectors` (owner t453u, menu placement t454u, 2026-10-30 CET): the
//! connectors settings screen. Each connector is an MCP server for Hermes
//! (hermes-rs). The layout follows the Grok connectors page as described in
//! the public docs (docs.x.ai/grok/connectors): a catalog of named
//! connectors you sign in to, plus "custom" = your own MCP server URL with
//! its auth. No Grok logo or branding: rows use plain initials.
//!
//! What a connect does (all behind the console owner gate, CSRF-checked):
//! * writes one entry `servers.d/<id>.yaml` in `--connectors-dir`, in the
//!   `mcp_servers:` shape of the generic Hermes MCP client
//!   (branch swarm/hermes-mcp-generic-20261030, `McpServerCfg::from_value`);
//! * writes the token, if any, to `secrets/<id>.env` (`NAME=value`, mode
//!   0600, dirs 0700). The entry only names the file and uses
//!   `Authorization: Bearer ${NAME}`; the value is never shown, logged or
//!   sent back (the page says "saved");
//! * regenerates `mcp-servers.yaml` (all live entries, atomic rename): the
//!   file the Hermes unit points `HERMES_RS_MCP_SERVERS_FILE` at.
//!
//! New servers start read-only: `class: read`, `allow_writes: false`.
//! Disconnect renames the entry (and its secrets file) to `.off`; nothing
//! is deleted. A replaced entry or token is renamed to `.bak.<unix>` first.
//! OAuth-only connectors show "needs sign-in setup" and write nothing
//! (TODO: OAuth flow; the generic client does not start `auth: oauth` yet).
//! Hermes reads the file only at start: "Apply" restarts `hermes-rs`, and is
//! offered only with `--connectors-apply` (set once the generic client is
//! deployed).

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

/// Directories a custom stdio command may live in (local allowlist). The
/// file name must also contain `mcp`.
pub const COMMAND_DIRS: &[&str] = &["/opt/galion/rust/bin/"];

/// The unit restarted by "Apply".
pub const HERMES_UNIT: &str = "hermes-rs.service";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// Owner pastes a key; sent as `Authorization: Bearer ${token_env}`.
    Token,
    /// Needs an OAuth sign-in (not built: stub).
    OAuth,
    /// No auth.
    None,
    /// Configured in the Hermes config; read-only here.
    Builtin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connector {
    pub id: String,
    pub name: String,
    pub section: String,
    pub initials: String,
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
            a => return Err(format!("catalog: {id}: unknown auth {a:?}")),
        };
        let url = c.get("url").as_str().map(str::to_string);
        let command = c.get("command").as_str().map(str::to_string);
        match (&url, &command) {
            (Some(u), None) => check_url(u).map_err(|e| format!("catalog: {id}: {e}"))?,
            (None, Some(_)) if auth == Auth::Builtin => {}
            (None, Some(cmd)) => check_command(cmd).map_err(|e| format!("catalog: {id}: {e}"))?,
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
        connectors.push(Connector {
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
            verified: c.get("verified").as_bool() == Some(true),
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

/// A local stdio command: absolute, inside [`COMMAND_DIRS`], file name with
/// `mcp` in it, no `..`.
pub fn check_command(cmd: &str) -> Result<(), String> {
    if cmd.len() > 256 || !yaml_safe(cmd) || cmd.contains("..") {
        return Err("command has characters that are not allowed".into());
    }
    let Some(dir) = COMMAND_DIRS.iter().find(|d| cmd.starts_with(**d)) else {
        return Err(format!(
            "command must be in {} (local allowlist)",
            COMMAND_DIRS.join(", ")
        ));
    };
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
    Connected,
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
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
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
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut o = fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
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
        Store { dir: dir.into() }
    }
    fn servers_d(&self) -> PathBuf {
        self.dir.join("servers.d")
    }
    fn secrets_d(&self) -> PathBuf {
        self.dir.join("secrets")
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
            _ => {}
        }
        if self.entry_path(&c.id).is_file() {
            State::Connected
        } else if Path::new(&format!("{}.off", self.entry_path(&c.id).display())).exists() {
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
        match c.auth {
            Auth::OAuth => return Err("needs sign-in setup (OAuth is not built yet)".into()),
            Auth::Builtin => return Err("built in; managed in the Hermes config".into()),
            _ => {}
        }
        if !id_ok(&c.id) {
            return Err("bad connector id".into());
        }
        match (&c.url, &c.command) {
            (Some(u), None) => check_url(u)?,
            (None, Some(cmd)) => {
                check_command(cmd)?;
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
        let p = self.entry_path(id);
        if !p.is_file() {
            return Err("not connected".into());
        }
        let io = |e: std::io::Error| format!("could not rename the connector files ({})", e.kind());
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
        if n == 0 {
            // An empty map keeps the file valid for the loader.
            out = out.replace("mcp_servers:\n", "mcp_servers: {}\n");
        }
        write_private(&self.servers_file(), out.as_bytes())
    }
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
        check_command(&cmd)?;
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

/// Restart Hermes so it reloads the servers file (fixed argv, no shell).
pub fn apply_restart() -> Result<(), String> {
    match std::process::Command::new("systemctl")
        .args(["--no-block", "restart", HERMES_UNIT])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => Err("systemctl refused the restart".into()),
        Err(_) => Err("systemctl not available".into()),
    }
}

// ---- the page handler (owner gate already passed in `Ui::handle`) ----

/// Fixed messages after a successful post (PRG redirect `?done=..&id=..`).
fn done_text(done: &str, name: &str) -> Option<String> {
    Some(match done {
        "saved" => format!("{name}: saved. Read-only (writes off)."),
        "added" => format!("{name}: added. Read-only (writes off)."),
        "disconnected" => format!("{name}: disconnected (entry renamed to .off, nothing deleted)."),
        "applied" => "Hermes is restarting to load the connectors.".into(),
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
        let store = self.cfg.files.connectors_dir.as_ref().map(Store::new);
        let apply = self.cfg.files.connectors_apply;
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
                Some("apply") if apply => apply_restart().map(|_| ("applied", String::new())),
                Some("apply") => {
                    Err("Apply is off until the Hermes MCP client update is deployed".into())
                }
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
            let q = html::form_pairs(req.query);
            if let Some(d) = html::form_get(&q, "done") {
                let id = html::form_get(&q, "id").filter(|i| id_ok(i)).unwrap_or("");
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
            apply,
            notice,
            saved,
        };
        let body = crate::pages::connectors::body(&view);
        let fwd = self.forward(req);
        let origin = self.origin(&fwd);
        let mut f = self.frame(req, "Galion Console · Connectors");
        f.extra_css_href = Some(&self.cn_css_href);
        let mut r = Reply::html(status, layout::console(&f, origin, &body));
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
        for want in [
            "github",
            "gmail",
            "google-calendar",
            "google-drive",
            "notion",
            "slack",
            "linear",
            "atlassian",
            "stripe",
            "paypal",
            "openmind",
        ] {
            assert!(ids.contains(&want), "{want}");
        }
        let titles: Vec<&str> = c.sections.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Productivity") && titles.contains(&"Finance"));
        for x in &c.connectors {
            assert!(x.verified, "{} must be verified or dropped", x.id);
            assert!(x.source.starts_with("https://") || x.auth == Auth::Builtin);
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
            r#"{"id":"x","section":"p","auth":"none","command":"/bin/sh"}"#
        ))
        .is_err());
        assert!(parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"none","url":"https://x/mcp"}"#
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
        assert!(check_command("/opt/galion/rust/bin/openmind-mcp-stack-498244ec").is_ok());
        for bad in [
            "/bin/sh",
            "/opt/galion/rust/bin/galion-console-serve",
            "/opt/galion/rust/bin/../../../bin/mcp",
            "/opt/galion/rust/bin/sub/mcp",
            "npx",
        ] {
            assert!(check_command(bad).is_err(), "{bad}");
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
        let d = std::env::temp_dir().join(format!(
            "cn-test-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }

    /// A stand-in admin API on loopback that says "not the owner".
    fn not_owner_api() -> std::net::SocketAddr {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let b = "{\"you\":false}";
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
                    b.len()
                );
            }
        });
        a
    }

    fn ui(dir: Option<&Path>) -> Ui {
        ui_with(dir, "127.0.0.1:9".parse().unwrap())
    }

    fn ui_with(dir: Option<&Path>, upstream: std::net::SocketAddr) -> Ui {
        Ui::new(UiConfig {
            upstream,
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
            "Productivity",
            "Developer tools",
            "Finance",
            "Obsidian OpenMind",
            "Add custom connector",
            "GitHub",
            "Gmail",
            "Needs sign-in setup",
            "/_ui/connectors.",
        ] {
            assert!(b.contains(s), "missing {s}");
        }
        assert!(!b.contains("<script"), "no script on /connectors");
        // The three-dash console menu lists the room.
        assert!(b.contains("href=\"/connectors\""));
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
        let ui = ui_with(Some(&t.0), not_owner_api());
        // Public console host, admin API says "not the owner".
        let r = ui.handle(&get("console.galion.app", false, ""));
        assert_eq!(r.status, 401);
        assert!(!body(&r).contains("Add custom connector"));
        let mut p = post(None, b"op=apply");
        p.host = "console.galion.app";
        p.peer_loopback = false;
        assert_eq!(ui.handle(&p).status, 401);
        assert!(!t.0.join(SERVERS_FILE).exists());
        // Admin API down: still refused (fail closed).
        let down = ui_with(Some(&t.0), "127.0.0.1:9".parse().unwrap());
        let r = down.handle(&get("console.galion.app", false, ""));
        assert!(r.status >= 400 && !body(&r).contains("Add custom connector"));
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
        assert!(pb.contains("token saved"));
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
            "csrf={tok}&op=add&name=Echo&target=%2Fbin%2Fsh+-c+id&auth=none"
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
        // OAuth-only and Apply (client not deployed) are refused, nothing faked.
        assert_eq!(send(format!("csrf={tok}&op=connect&id=gmail")).status, 400);
        assert!(!t.0.join("servers.d/gmail.yaml").exists());
        assert_eq!(send(format!("csrf={tok}&op=apply")).status, 400);
    }

    #[test]
    fn without_a_dir_the_screen_is_read_only() {
        let ui = ui(None);
        let r = ui.handle(&get("127.0.0.1:18080", true, ""));
        assert_eq!(r.status, 200);
        let b = body(&r);
        assert!(b.contains("read-only") && !b.contains("Add custom connector"));
    }
}
