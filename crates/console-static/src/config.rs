// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Command-line configuration. No config file, no environment lookups: the
//! systemd unit spells every value out, so what runs is what the unit says.

use std::net::SocketAddr;
use std::path::PathBuf;

pub const USAGE: &str = "galion-console-static: static front for the Galion console (B61)

usage: galion-console-static --root DIR [options]
       galion-console-static --ui rust --upstream ADDR [--root DIR] [options]
       galion-console-static --dist DIR [--ui rust] [options]
       galion-console-serve …   (same binary; alias for console-dist cutover prep)

  --bind ADDR          listen address (default 127.0.0.1:8090; loopback only
                       unless --allow-public-bind)
  --root DIR           pre-built console (dist/client), indexed once at start;
                       optional with --ui rust (then only Rust pages are served)
  --dist DIR           console-dist root (live layout): uses DIR/static when
                       present else DIR; sets --shell none and --local-api;
                       with --ui rust and no --upstream, upstream defaults to
                       --bind (in-process local-api for health/session gate)
  --shell NAME         HTML shell under root (default _shell.html); use `none`
                       when the tree has only assets (console-dist/static)
  --routes FILE        page/redirect list (see routes.rs); default: only `/`
                       (with --dist and no file: redirect / -> /console)
  --upstream ADDR      loopback IP:port that answers the API (e.g. 127.0.0.1:8081)
  --proxy-prefix P     path prefix sent upstream; repeatable
                       (default /api/ and /_serverFn/ when --upstream is set)
  --local-api          answer /api/v1/health, the board catalog (services,
                       domains, repos, catalog, ledger, docs, protocol,
                       markers, boot), admin pairing and the session gate
                       in-process (before any proxy)
  --admin-store FILE   admin-device JSON store (hashes only) for
                       /api/v1/admin and remote console hosts, e.g.
                       /root/galion-secrets/admin-device.json; without it
                       remote console hosts get 401 on /api/* (fail closed)
  --admin-mirror FILE  second copy written after each store write
  --ui rust            render the ported pages in Rust (crates/console-ui):
                       server-side HTML, Rust-generated CSS, no JavaScript;
                       needs --upstream (or --dist/--local-api self-upstream)
  --pulse ADDR         loopback desk pulse for /services (GET /api/v1/awake),
                       e.g. 127.0.0.1:8091; default none
  --desk ADDR          loopback galion-desk for /cache, /context (GET /desk),
                       e.g. 127.0.0.1:8646; default none
  --connectors-dir DIR  where /connectors keeps MCP server entries (servers.d/),
                       0600 token files (secrets/) and the generated
                       mcp-servers.yaml for HERMES_RS_MCP_SERVERS_FILE, e.g.
                       /var/lib/galion-console-connectors; default none (read-only)
  --connectors-command-dir DIR  custom connectors may run local stdio MCP
                       servers only from directly inside DIR (file name
                       containing mcp); default none (https URLs only)
  --connectors-hermes-file FILE  the live Hermes MCP servers file
                       (HERMES_RS_MCP_SERVERS_FILE), e.g.
                       /etc/hermes-rs/mcp-servers.yaml: tokens go to mcp.d/
                       beside it, console entries to a marked block in it
  --connectors-logo-dir DIR  brand logos for /connectors (<name>.png, read
                       once at start); default none (monograms)
  --connectors-oauth-dir DIR  account sign-in (OAuth 2.0 + PKCE) for the
                       featured Google, Canva, Box and Wix connectors: app
                       credentials in DIR/<provider>.env (0600, root); tokens
                       stay under --connectors-dir/oauth; default none (off)
  --memory-root DIR    host folder of Hermes memory for /memory and
                       /md/persona, e.g. /var/lib/memory-tencentdb; default none
  --model-file FILE    file holding the model name shown on /memory,
                       e.g. /opt/galion/galion-light/one/model; default none
  --user-md FILE       USER.md shown on /md/user,
                       e.g. /opt/galion/hermes/memories/USER.md; default none
  --arena-root DIR     arena files (board.jsonl, champions/current.json,
                       spend.json, STOP) for an in-process read-only
                       GET /api/arena (port of `arena.py status`; non-GET
                       -> 405), e.g. /workspace/arena; needs --local-api
  --data-upstream ADDR loopback IP:port that answers the data routes the
                       local API does not (GET/HEAD only, same API gate),
                       e.g. 127.0.0.1:18080; needs --local-api; works with
                       --dist self-upstream
  --data-path P        exact path for --data-upstream; repeatable
                       (default /api/arena and /api/v1/bank; with
                       --owner-routes the default is /api/arena only, so the
                       retired /api/v1/bank answers 410 unless passed here)
  --owner-routes       (needs --local-api) forward the Node-era rows owned by
                       another Rust service: /api/v1/analytics, /api/v1/awake
                       -> --pulse; /api/hermes/health -> --hermes /health
                       (exact path, GET/HEAD, no Cookie/Authorization sent);
                       answer retired Node-era rows (instances, keys, mesh, ...)
                       with 410 Gone JSON instead of 404 (slot 395); needs
                       --arena-root (or --data-path /api/arena) so /api/arena
                       is answered too (slot 413)
  --hermes ADDR        loopback hermes-rs for --owner-routes / --hermes-api,
                       e.g. 127.0.0.1:8642
  --hermes-api         (needs --local-api and --hermes) the Hermes research
                       adapter: /api/hermes/{health,guard,spend,history,models,
                       modes,skills,providers}, POST /api/hermes/ask, runs/ID,
                       runs/ID/events (SSE) and runs/ID/stop, behind the same
                       pairing gate; the gateway key is added server-side
  --hermes-key-file F  file with an API_SERVER_KEY=... line (the gateway's own
                       env file); env HERMES_API_KEY / API_SERVER_KEY win
  --hermes-providers L comma list of gateway providers the picker may send:
                       laptop, runpod, openrouter, river (default openrouter);
                       unlisted ones show as not set up and are never sent
  --hermes-laptop ADDR loopback address probed (TCP) for the laptop model,
                       e.g. 127.0.0.1:11434; closed = offline
  --hermes-prefs F     JSON file for the model picker's saved provider, model
                       and starred models (written atomically; the parent
                       directory must be writable); default: not stored
  --hermes-openrouter-models URL / --hermes-river-models URL
                       model list sources (default the providers' public
                       https APIs; http://127.0.0.1:PORT allowed for tests);
                       the River key (RIVER_API_KEY) comes from the env or
                       --hermes-key-file, sent to curl on stdin, never logged
  --files DIR          (needs --local-api) the file explorer /files: browse,
                       preview (images, video/audio with play, PDF, markdown,
                       code in a CodeMirror 6 editor, archives listed, hex for
                       the rest) and download everything under DIR only (path
                       traversal and symlinks out of DIR refused), behind the
                       same pairing gate; read-only unless --files-write
  --files-write        allow edits, uploads, new file/folder, rename, move,
                       copy, extract and delete (delete = move to DIR/.trash,
                       never a real delete; every edit keeps a .bak there)
  --files-max-upload N largest upload in bytes (default 2147483648)
  --files-extract-max N
                       largest total an archive may expand to (default 8589934592)
  --files-ffmpeg PATH  host ffmpeg (ffprobe next to it) for video posters and
                       media facts, e.g. /usr/bin/ffmpeg; default none
  --files-cache DIR    poster cache (outside --files DIR); default none (no cache)
  --phone              (needs --local-api) the phone home page /phone: Hermes
                       up/down + build id, OpenRouter spend vs the limit,
                       watchdog alerts waiting, cortex/nerve/console health,
                       then the galion-one approval cards (same signing flow);
                       read-only GET /api/v1/phone/status + /api/approvals*,
                       /api/owner-key* behind the same pairing gate
  --nerve ADDR / --cortex ADDR
                       (with --phone) loopback openmind-nerve / gaia-cortex,
                       default 127.0.0.1:8670 / 127.0.0.1:8674; --hermes is
                       read for /health (default 127.0.0.1:8642)
  --status-dir DIR     (with --phone) OpenMind no-secrets snapshots (spend.json,
                       backup.json), default /var/lib/openmind-mcp-stack/status
  --unit-dir DIR       (with --phone, repeatable) systemd unit dirs read for the
                       Hermes build id; --hermes-unit NAME (default hermes-rs.service)
  --approval-queue DIR / --approval-state DIR / --approval-out DIR /
  --owner-pub F / --sandbox-pub F / --stack-pub F / --webauthn-rp-id ID /
  --webauthn-origin URL (repeatable)
                       (with --phone) as galion-one ONE_APPROVAL_* / ONE_WEBAUTHN_*;
                       origins default https://console.galion.app,
                       https://galion.app, https://one.galion.app
  --stt ADDR           (needs --local-api) loopback galion-stt for live dictation:
                       GET /api/stt/health and the WebSocket /api/stt/stream,
                       behind the same pairing gate, e.g. 127.0.0.1:8650
  --stt-max-sessions N concurrent dictation sessions (default 2, 1..=16)
  --console-host HOST  extra host treated as a console host (repeatable),
                       e.g. the machine public IP (the TSX had
                       the two server IPs hard-coded; pass them here)
  --max-conns N        concurrent connections (default 256)
  --max-body BYTES     largest proxied request body (default 26214400)
  --allow-public-bind  allow a non-loopback --bind (tunnel-only hosts never need it)
  --quiet              no access log on stderr
  --help               this text

       galion-console-static gen-routes ROUTE_TREE_GEN_TS [--redirect FROM TO]...
                       print a routes file from a TanStack routeTree.gen.ts
";

/// Default routes when `--dist` is used without `--routes`.
pub const DIST_DEFAULT_ROUTES: &str = "\
# console-dist / Console V2 default (console-rust-serve)
page /
page /soon
page /status
page /agents
page /analytics
page /arena
page /bank
page /cache
page /chill
page /cloudflare
page /console
page /context
page /data
page /docs
page /domains
page /gateway
page /guardian
page /hermes
page /instances
page /jupyter
page /keys
page /markers
page /memory
page /mesh
page /node
page /pack
page /repos
page /security
page /services
page /session
page /md/$name
redirect / /console
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind: SocketAddr,
    pub root: Option<PathBuf>,
    pub shell: String,
    pub routes: Option<PathBuf>,
    /// When set, use this routes text instead of reading `routes` (dist default).
    pub routes_text: Option<String>,
    pub upstream: Option<SocketAddr>,
    pub proxy_prefixes: Vec<String>,
    pub max_conns: usize,
    pub max_body: u64,
    pub access_log: bool,
    pub rust_ui: bool,
    pub pulse: Option<SocketAddr>,
    pub extra_hosts: Vec<String>,
    pub desk: Option<SocketAddr>,
    pub files: console_ui::port4::HostFiles,
    /// Answer health/session gate in-process.
    pub local_api: bool,
    /// True when `--dist` was passed (for logging / docs).
    pub dist_mode: bool,
    /// `--admin-store`: admin-device hash store.
    pub admin_store: Option<PathBuf>,
    /// `--admin-mirror`: best-effort second copy of the store.
    pub admin_mirror: Option<PathBuf>,
    /// `--arena-root`: arena files for the in-process `GET /api/arena`.
    pub arena_root: Option<PathBuf>,
    /// `--data-upstream`: loopback reader for [`Config::data_paths`] (GET/HEAD only).
    pub data_upstream: Option<SocketAddr>,
    /// Exact API paths sent to `--data-upstream` when local-api does not answer
    /// them (default `/api/arena`, `/api/v1/bank`).
    pub data_paths: Vec<String>,
    /// `--hermes`: loopback hermes-rs for the `--owner-routes` health row.
    pub hermes: Option<SocketAddr>,
    /// `--owner-routes`: forward [`crate::owners::OWNER_ROUTES`] to their
    /// owner and answer [`crate::owners::RETIRED`] with 410 (slot 395).
    pub owner_routes: bool,
    /// `--hermes-api`: the Hermes research adapter ([`crate::hermes`]).
    pub hermes_api: bool,
    /// `--hermes-key-file`: env-style file holding `API_SERVER_KEY=` (read
    /// at start; the value never leaves the server).
    pub hermes_key_file: Option<PathBuf>,
    /// `--hermes-providers`: providers the picker may send.
    pub hermes_providers: Vec<String>,
    /// `--hermes-laptop`: TCP probe for the laptop model.
    pub hermes_laptop: Option<SocketAddr>,
    /// `--hermes-prefs`: model picker choices file.
    pub hermes_prefs: Option<PathBuf>,
    /// `--hermes-openrouter-models` / `--hermes-river-models`.
    pub hermes_sources: crate::hermes_models::Sources,
    /// `--stt`: loopback galion-stt ([`crate::stt`]).
    pub stt: Option<SocketAddr>,
    /// `--stt-max-sessions`: concurrent `/api/stt/stream` tunnels.
    pub stt_max_sessions: usize,
    /// `--files`: the file explorer ([`crate::files`]).
    pub files_explorer: Option<crate::files::FilesCfg>,
    /// `--phone`: the phone home page ([`crate::phone`]).
    pub phone: Option<crate::phone::PhoneCfg>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ArgsError {
    Help,
    Bad(String),
}

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArgsError::Help => f.write_str(USAGE),
            // One line: under systemd a bad drop-in restarts in a loop, and
            // ~150 lines of usage per try buried the one line that matters.
            ArgsError::Bad(m) => write!(f, "{m} (run with --help for every option)"),
        }
    }
}

fn bad(m: impl Into<String>) -> ArgsError {
    ArgsError::Bad(m.into())
}

fn valid_prefix(p: &str) -> bool {
    p.len() > 1 && p.ends_with('/') && crate::path::sanitize(p).as_deref() == Ok(p)
}

/// The arena row: answered by `--arena-root` or a `--data-path`.
pub const ARENA_PATH: &str = "/api/arena";

/// Default `--data-path` list when `--data-upstream` is set (minus the
/// [`crate::owners::RETIRED`] rows when `--owner-routes` is set, F-395-3).
pub const DATA_DEFAULT_PATHS: &[&str] = &["/api/arena", "/api/v1/bank"];

/// An exact data path: sanitized, under `/api/`, no trailing slash or query.
fn valid_data_path(p: &str) -> bool {
    p.starts_with("/api/")
        && p.len() > "/api/".len()
        && !p.ends_with('/')
        && !p.contains('?')
        && crate::path::sanitize(p).as_deref() == Ok(p)
}

fn resolve_dist_root(dir: PathBuf) -> PathBuf {
    let nested = dir.join("static");
    if nested.is_dir() {
        nested
    } else {
        dir
    }
}

impl Config {
    /// Parse arguments (without the program name).
    pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Result<Config, ArgsError> {
        let mut bind: SocketAddr = "127.0.0.1:8090".parse().expect("default bind");
        let mut root = None;
        let mut shell = "_shell.html".to_string();
        let mut routes = None;
        let mut upstream = None;
        let mut prefixes: Vec<String> = Vec::new();
        let mut max_conns = 256usize;
        let mut max_body = 25 * 1024 * 1024u64;
        let mut access_log = true;
        let mut public = false;
        let mut rust_ui = false;
        let mut pulse = None;
        let mut desk = None;
        let mut extra_hosts = Vec::new();
        let mut files = console_ui::port4::HostFiles::default();
        let mut local_api = false;
        let mut dist_mode = false;
        let mut admin_store = None;
        let mut admin_mirror = None;
        let mut arena_root = None;
        let mut data_upstream: Option<SocketAddr> = None;
        let mut data_paths: Vec<String> = Vec::new();
        let mut hermes: Option<SocketAddr> = None;
        let mut owner_routes = false;
        let mut hermes_api = false;
        let mut hermes_key_file: Option<PathBuf> = None;
        let mut hermes_providers: Vec<String> = Vec::new();
        let mut hermes_laptop: Option<SocketAddr> = None;
        let mut hermes_prefs: Option<PathBuf> = None;
        let mut hermes_sources = crate::hermes_models::Sources::default();
        let mut sources_set = false;
        let mut stt: Option<SocketAddr> = None;
        let mut stt_max_sessions: Option<usize> = None;
        let mut fx: Option<crate::files::FilesCfg> = None;
        let mut fx_flags: Vec<String> = Vec::new();
        let mut fx_write = false;
        let mut fx_upload: Option<u64> = None;
        let mut fx_extract: Option<u64> = None;
        let mut fx_ffmpeg: Option<PathBuf> = None;
        let mut fx_cache: Option<PathBuf> = None;
        let mut phone = false;
        let mut pc = crate::phone::PhoneCfg::defaults();
        let mut phone_flags: Vec<String> = Vec::new();
        let mut unit_dirs: Vec<PathBuf> = Vec::new();
        let mut origins: Vec<String> = Vec::new();
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            let mut val = |name: &str| {
                it.next()
                    .ok_or_else(|| bad(format!("{name} needs a value")))
            };
            match a.as_str() {
                "--help" | "-h" => return Err(ArgsError::Help),
                "--bind" => {
                    bind = val("--bind")?
                        .parse()
                        .map_err(|_| bad("--bind must be IP:port"))?
                }
                "--root" => root = Some(PathBuf::from(val("--root")?)),
                "--dist" => {
                    let d = PathBuf::from(val("--dist")?);
                    root = Some(resolve_dist_root(d));
                    shell = "none".into();
                    local_api = true;
                    dist_mode = true;
                }
                "--shell" => {
                    let s = val("--shell")?;
                    if s == "none" {
                        shell = "none".into();
                    } else if s.contains('/') || s.starts_with('.') || !s.ends_with(".html") {
                        return Err(bad("--shell must be a plain .html file name or `none`"));
                    } else {
                        shell = s;
                    }
                }
                "--routes" => routes = Some(PathBuf::from(val("--routes")?)),
                "--upstream" => {
                    let u: SocketAddr = val("--upstream")?
                        .parse()
                        .map_err(|_| bad("--upstream must be IP:port"))?;
                    if !u.ip().is_loopback() {
                        return Err(bad("--upstream must be a loopback address"));
                    }
                    upstream = Some(u);
                }
                "--proxy-prefix" => {
                    let p = val("--proxy-prefix")?;
                    if !valid_prefix(&p) {
                        return Err(bad(format!(
                            "bad --proxy-prefix {p:?} (must look like /api/)"
                        )));
                    }
                    prefixes.push(p);
                }
                "--local-api" => local_api = true,
                "--max-conns" => {
                    max_conns = val("--max-conns")?
                        .parse()
                        .map_err(|_| bad("--max-conns must be a number"))?;
                    if max_conns == 0 {
                        return Err(bad("--max-conns must be > 0"));
                    }
                }
                "--max-body" => {
                    max_body = val("--max-body")?
                        .parse()
                        .map_err(|_| bad("--max-body must be a number"))?
                }
                "--ui" => match val("--ui")?.as_str() {
                    "rust" => rust_ui = true,
                    "spa" => rust_ui = false,
                    other => return Err(bad(format!("--ui must be rust or spa, not {other:?}"))),
                },
                "--pulse" => {
                    let p: SocketAddr = val("--pulse")?
                        .parse()
                        .map_err(|_| bad("--pulse must be IP:port"))?;
                    if !p.ip().is_loopback() {
                        return Err(bad("--pulse must be a loopback address"));
                    }
                    pulse = Some(p);
                }
                "--desk" => {
                    let p: SocketAddr = val("--desk")?
                        .parse()
                        .map_err(|_| bad("--desk must be IP:port"))?;
                    if !p.ip().is_loopback() {
                        return Err(bad("--desk must be a loopback address"));
                    }
                    desk = Some(p);
                }
                "--console-host" => {
                    let h = val("--console-host")?.to_ascii_lowercase();
                    if h.is_empty()
                        || !h.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b':'
                        })
                    {
                        return Err(bad("--console-host must be a plain host name or IP"));
                    }
                    extra_hosts.push(h);
                }
                "--connectors-dir" => {
                    files.connectors_dir =
                        Some(abs_path(val("--connectors-dir")?, "--connectors-dir")?)
                }
                "--connectors-hermes-file" => {
                    files.connectors_hermes_file = Some(abs_path(
                        val("--connectors-hermes-file")?,
                        "--connectors-hermes-file",
                    )?)
                }
                "--connectors-logo-dir" => {
                    files.connectors_logo_dir = Some(abs_path(
                        val("--connectors-logo-dir")?,
                        "--connectors-logo-dir",
                    )?)
                }
                "--connectors-oauth-dir" => {
                    files.connectors_oauth_dir = Some(abs_path(
                        val("--connectors-oauth-dir")?,
                        "--connectors-oauth-dir",
                    )?)
                }
                "--connectors-command-dir" => {
                    files.connectors_command_dir = Some(abs_path(
                        val("--connectors-command-dir")?,
                        "--connectors-command-dir",
                    )?)
                }
                "--memory-root" => {
                    files.memory_root = Some(abs_path(val("--memory-root")?, "--memory-root")?)
                }
                "--model-file" => {
                    files.model_file = Some(abs_path(val("--model-file")?, "--model-file")?)
                }
                "--admin-store" => {
                    admin_store = Some(abs_path(val("--admin-store")?, "--admin-store")?)
                }
                "--admin-mirror" => {
                    admin_mirror = Some(abs_path(val("--admin-mirror")?, "--admin-mirror")?)
                }
                "--arena-root" => {
                    arena_root = Some(abs_path(val("--arena-root")?, "--arena-root")?)
                }
                "--data-upstream" => {
                    let u: SocketAddr = val("--data-upstream")?
                        .parse()
                        .map_err(|_| bad("--data-upstream must be IP:port"))?;
                    if !u.ip().is_loopback() {
                        return Err(bad("--data-upstream must be a loopback address"));
                    }
                    data_upstream = Some(u);
                }
                "--data-path" => {
                    let p = val("--data-path")?;
                    if !valid_data_path(&p) {
                        return Err(bad(format!(
                            "bad --data-path {p:?} (exact path under /api/, e.g. /api/v1/bank)"
                        )));
                    }
                    data_paths.push(p);
                }
                "--hermes" => {
                    let p: SocketAddr = val("--hermes")?
                        .parse()
                        .map_err(|_| bad("--hermes must be IP:port"))?;
                    if !p.ip().is_loopback() {
                        return Err(bad("--hermes must be a loopback address"));
                    }
                    hermes = Some(p);
                }
                "--owner-routes" => owner_routes = true,
                "--hermes-api" => hermes_api = true,
                "--hermes-key-file" => {
                    hermes_key_file =
                        Some(abs_path(val("--hermes-key-file")?, "--hermes-key-file")?)
                }
                "--hermes-providers" => {
                    for p in val("--hermes-providers")?
                        .split(',')
                        .map(str::trim)
                        .filter(|p| !p.is_empty())
                    {
                        if !crate::hermes::PROVIDERS.iter().any(|x| x.id == p) {
                            return Err(bad(format!("unknown --hermes-providers entry {p:?} (laptop, runpod, openrouter, river)")));
                        }
                        if !hermes_providers.iter().any(|x| x == p) {
                            hermes_providers.push(p.to_string());
                        }
                    }
                }
                "--hermes-laptop" => {
                    let p: SocketAddr = val("--hermes-laptop")?
                        .parse()
                        .map_err(|_| bad("--hermes-laptop must be IP:port"))?;
                    if !p.ip().is_loopback() {
                        return Err(bad("--hermes-laptop must be a loopback address"));
                    }
                    hermes_laptop = Some(p);
                }
                "--hermes-prefs" => {
                    hermes_prefs = Some(abs_path(val("--hermes-prefs")?, "--hermes-prefs")?);
                }
                "--hermes-openrouter-models" | "--hermes-river-models" => {
                    let flag = a.clone();
                    let u = val(&flag)?;
                    if !crate::hermes_models::url_ok(&u) {
                        return Err(bad(format!(
                            "{flag} must be an https:// URL (or http://127.0.0.1:PORT/...)"
                        )));
                    }
                    if flag == "--hermes-openrouter-models" {
                        hermes_sources.openrouter = u;
                    } else {
                        hermes_sources.river = u;
                    }
                    sources_set = true;
                }
                "--stt" => {
                    let p: SocketAddr = val("--stt")?
                        .parse()
                        .map_err(|_| bad("--stt must be IP:port"))?;
                    if !p.ip().is_loopback() {
                        return Err(bad("--stt must be a loopback address"));
                    }
                    stt = Some(p);
                }
                "--stt-max-sessions" => {
                    let n: usize = val("--stt-max-sessions")?
                        .parse()
                        .map_err(|_| bad("--stt-max-sessions must be a number"))?;
                    if !(1..=16).contains(&n) {
                        return Err(bad("--stt-max-sessions must be 1..=16"));
                    }
                    stt_max_sessions = Some(n);
                }
                "--files" => {
                    let d = abs_path(val("--files")?, "--files")?;
                    let d = std::fs::canonicalize(&d)
                        .ok()
                        .filter(|c| c.is_dir())
                        .ok_or_else(|| bad("--files must be an existing folder"))?;
                    if d.parent().is_none() {
                        return Err(bad("--files may not be / (pick one folder to share)"));
                    }
                    fx = Some(crate::files::FilesCfg::new(d));
                }
                "--files-write" => {
                    fx_write = true;
                    fx_flags.push(a.clone());
                }
                "--files-max-upload" | "--files-extract-max" => {
                    let flag = a.clone();
                    let n: u64 = val(&flag)?
                        .parse()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| bad(format!("{flag} must be a byte count > 0")))?;
                    if flag == "--files-max-upload" {
                        fx_upload = Some(n);
                    } else {
                        fx_extract = Some(n);
                    }
                    fx_flags.push(flag);
                }
                "--files-ffmpeg" => {
                    let p = abs_path(val("--files-ffmpeg")?, "--files-ffmpeg")?;
                    if !p.is_file() {
                        return Err(bad("--files-ffmpeg must be an existing file"));
                    }
                    fx_ffmpeg = Some(p);
                    fx_flags.push(a.clone());
                }
                "--files-cache" => {
                    fx_cache = Some(abs_path(val("--files-cache")?, "--files-cache")?);
                    fx_flags.push(a.clone());
                }
                "--phone" => phone = true,
                "--nerve" | "--cortex" => {
                    let flag = a.clone();
                    let p: SocketAddr = val(&flag)?
                        .parse()
                        .map_err(|_| bad(format!("{flag} must be IP:port")))?;
                    if !p.ip().is_loopback() {
                        return Err(bad(format!("{flag} must be a loopback address")));
                    }
                    if flag == "--nerve" {
                        pc.nerve = p;
                    } else {
                        pc.cortex = p;
                    }
                    phone_flags.push(flag);
                }
                "--status-dir" | "--unit-dir" | "--approval-queue" | "--approval-state"
                | "--approval-out" | "--owner-pub" | "--sandbox-pub" | "--stack-pub" => {
                    let flag = a.clone();
                    let p = abs_path(val(&flag)?, &flag)?;
                    match flag.as_str() {
                        "--status-dir" => pc.status_dir = p,
                        "--unit-dir" => unit_dirs.push(p),
                        "--approval-queue" => pc.queue = p,
                        "--approval-state" => pc.state = p,
                        "--approval-out" => pc.out = Some(p),
                        "--owner-pub" => pc.owner_pub = p,
                        "--sandbox-pub" => pc.sandbox_pub = p,
                        _ => pc.stack_pub = p,
                    }
                    phone_flags.push(flag);
                }
                "--hermes-unit" => {
                    let u = val("--hermes-unit")?;
                    if !u.ends_with(".service")
                        || !u
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
                    {
                        return Err(bad("--hermes-unit must be a plain NAME.service"));
                    }
                    pc.hermes_unit = u;
                    phone_flags.push(a.clone());
                }
                "--webauthn-rp-id" => {
                    let r = val("--webauthn-rp-id")?.to_ascii_lowercase();
                    if r.is_empty()
                        || !r
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
                    {
                        return Err(bad("--webauthn-rp-id must be a plain domain"));
                    }
                    pc.rp_id = r;
                    phone_flags.push(a.clone());
                }
                "--webauthn-origin" => {
                    let o = val("--webauthn-origin")?;
                    let ok = o
                        .strip_prefix("https://")
                        .or_else(|| o.strip_prefix("http://127.0.0.1:"))
                        .or_else(|| o.strip_prefix("http://localhost:"))
                        .is_some_and(|rest| {
                            !rest.is_empty()
                                && rest
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
                        });
                    if !ok {
                        return Err(bad(
                            "--webauthn-origin must be https://HOST (or http://127.0.0.1:PORT)",
                        ));
                    }
                    origins.push(o);
                    phone_flags.push(a.clone());
                }
                "--user-md" => files.user_md = Some(abs_path(val("--user-md")?, "--user-md")?),
                "--allow-public-bind" => public = true,
                "--quiet" => access_log = false,
                other => return Err(bad(format!("unknown argument {other:?}"))),
            }
        }
        if root.is_none() && !rust_ui {
            return Err(bad("--root is required (or use --ui rust / --dist)"));
        }
        // Dist + rust UI with no external upstream: loop API back to self so
        // the in-process --local-api answers /api/v1/health for page renders.
        if rust_ui && upstream.is_none() {
            if local_api {
                upstream = Some(bind);
            } else {
                return Err(bad(
                    "--ui rust needs --upstream (or --local-api / --dist for self-upstream)",
                ));
            }
        }
        if !public && !bind.ip().is_loopback() {
            return Err(bad(
                "--bind is not loopback; pass --allow-public-bind if that is intended",
            ));
        }
        let self_upstream = upstream == Some(bind);
        if upstream.is_some() && prefixes.is_empty() && !self_upstream {
            prefixes = vec!["/api/".to_string(), "/_serverFn/".to_string()];
        }
        // Self-upstream must not proxy (would loop); local-api owns health/session.
        if self_upstream {
            prefixes.clear();
        }
        if upstream.is_none() && !prefixes.is_empty() {
            return Err(bad("--proxy-prefix needs --upstream"));
        }
        if admin_mirror.is_some() && admin_store.is_none() {
            return Err(bad("--admin-mirror needs --admin-store"));
        }
        if admin_store.is_some() && admin_store == admin_mirror {
            return Err(bad("--admin-mirror must differ from --admin-store"));
        }
        if (arena_root.is_some() || data_upstream.is_some()) && !local_api {
            return Err(bad(
                "--arena-root / --data-upstream need --local-api or --dist (the API gate)",
            ));
        }
        if owner_routes && !local_api {
            return Err(bad(
                "--owner-routes needs --local-api or --dist (the API gate)",
            ));
        }
        if hermes.is_some() && !owner_routes && !hermes_api && !phone {
            return Err(bad(
                "--hermes needs --owner-routes, --hermes-api or --phone",
            ));
        }
        if phone && !local_api {
            return Err(bad(
                "--phone needs --local-api (or --dist): the pairing gate",
            ));
        }
        if !phone {
            if let Some(f) = phone_flags.first() {
                return Err(bad(format!("{f} needs --phone")));
            }
        }
        let phone = phone.then(|| {
            if let Some(h) = hermes {
                pc.hermes = h;
            }
            if !unit_dirs.is_empty() {
                pc.unit_dirs = unit_dirs;
            }
            if !origins.is_empty() {
                pc.origins = origins;
            }
            pc
        });
        if hermes_api && (!local_api || hermes.is_none()) {
            return Err(bad(
                "--hermes-api needs --local-api (or --dist) and --hermes",
            ));
        }
        if (hermes_key_file.is_some()
            || !hermes_providers.is_empty()
            || hermes_laptop.is_some()
            || hermes_prefs.is_some()
            || sources_set)
            && !hermes_api
        {
            return Err(bad(
                "--hermes-key-file / --hermes-providers / --hermes-laptop / --hermes-prefs / --hermes-*-models need --hermes-api",
            ));
        }
        if fx.is_some() && !local_api {
            return Err(bad(
                "--files needs --local-api (or --dist): the pairing gate",
            ));
        }
        if fx.is_none() {
            if let Some(f) = fx_flags.first() {
                return Err(bad(format!("{f} needs --files")));
            }
        }
        if let Some(f) = fx.as_mut() {
            f.write = fx_write;
            if let Some(n) = fx_upload {
                f.max_upload = n;
            }
            if let Some(n) = fx_extract {
                f.extract_max = n;
            }
            f.ffmpeg = fx_ffmpeg;
            if let Some(c) = fx_cache {
                if c.starts_with(&f.root) {
                    return Err(bad("--files-cache must be outside the --files folder"));
                }
                f.cache = Some(c);
            }
        }
        if stt.is_some() && !local_api {
            return Err(bad("--stt needs --local-api (or --dist): the pairing gate"));
        }
        if stt_max_sessions.is_some() && stt.is_none() {
            return Err(bad("--stt-max-sessions needs --stt"));
        }
        let stt_max_sessions = stt_max_sessions.unwrap_or(2);
        if hermes_api && hermes_providers.is_empty() {
            hermes_providers.push("openrouter".to_string());
        }
        if !data_paths.is_empty() && data_upstream.is_none() {
            return Err(bad("--data-path needs --data-upstream"));
        }
        if data_upstream.is_some_and(|u| {
            u.port() == bind.port() && (u.ip() == bind.ip() || bind.ip().is_unspecified())
        }) {
            return Err(bad("--data-upstream must not be --bind (would loop)"));
        }
        if data_upstream.is_some() && data_paths.is_empty() {
            // F-395-3 (slot 413): with --owner-routes the default list drops
            // the retired rows (D-384-2: /api/v1/bank answers 410), so the
            // data proxy never wins over the 410 by default. An explicit
            // `--data-path /api/v1/bank` is the owner's opt-in and is kept.
            data_paths = DATA_DEFAULT_PATHS
                .iter()
                .filter(|p| !(owner_routes && crate::owners::is_retired(p)))
                .map(|s| s.to_string())
                .collect();
        }
        // F-395-2 (slot 413): --owner-routes is the full Node-era route set;
        // without an arena reader `/api/arena` would be a bare 404 on a
        // console that otherwise answers every row. Refuse at startup.
        if owner_routes && arena_root.is_none() && !data_paths.iter().any(|p| p == ARENA_PATH) {
            return Err(bad(
                "--owner-routes needs --arena-root DIR (or --data-upstream with --data-path /api/arena): /api/arena would 404",
            ));
        }
        let routes_text = if dist_mode && routes.is_none() {
            Some(DIST_DEFAULT_ROUTES.to_string())
        } else {
            None
        };
        Ok(Config {
            bind,
            root,
            shell,
            routes,
            routes_text,
            upstream,
            proxy_prefixes: prefixes,
            max_conns,
            max_body,
            access_log,
            rust_ui,
            pulse,
            extra_hosts,
            desk,
            files,
            local_api,
            dist_mode,
            admin_store,
            admin_mirror,
            arena_root,
            data_upstream,
            data_paths,
            hermes,
            owner_routes,
            hermes_api,
            hermes_key_file,
            hermes_providers,
            hermes_laptop,
            hermes_prefs,
            hermes_sources,
            stt,
            stt_max_sessions,
            files_explorer: fx,
            phone,
        })
    }
}

/// Host-file flags take absolute paths only (no `..`), so the unit file says
/// exactly which file the console reads.
fn abs_path(v: String, flag: &str) -> Result<PathBuf, ArgsError> {
    let p = PathBuf::from(&v);
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(bad(format!("{flag} must be an absolute path without ..")));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn bad_flag_error_is_one_line() {
        let e = Config::from_args(args("--status-dir rel/path --phone --local-api")).unwrap_err();
        let text = e.to_string();
        assert!(!text.contains('\n'), "{text}");
        assert!(text.contains("--status-dir"), "{text}");
        assert!(
            text.ends_with("(run with --help for every option)"),
            "{text}"
        );
        assert!(!text.contains("usage:"), "{text}");
        let e = Config::from_args(args("--no-such-flag")).unwrap_err();
        assert!(!e.to_string().contains('\n'), "{e}");
        assert_eq!(
            Config::from_args(args("--help")).unwrap_err().to_string(),
            USAGE
        );
    }

    #[test]
    fn defaults() {
        let c = Config::from_args(args("--root /srv/c")).unwrap();
        assert_eq!(c.bind.to_string(), "127.0.0.1:8090");
        assert_eq!(c.shell, "_shell.html");
        assert_eq!(c.root.as_deref(), Some(std::path::Path::new("/srv/c")));
        assert!(!c.rust_ui);
        assert!(c.upstream.is_none());
        assert!(c.proxy_prefixes.is_empty());
        assert!(!c.local_api);
    }

    #[test]
    fn upstream_defaults_prefixes() {
        let c = Config::from_args(args("--root r --upstream 127.0.0.1:8081")).unwrap();
        assert_eq!(c.proxy_prefixes, vec!["/api/", "/_serverFn/"]);
        let c = Config::from_args(args(
            "--root r --upstream [::1]:8081 --proxy-prefix /api/v1/",
        ))
        .unwrap();
        assert_eq!(c.proxy_prefixes, vec!["/api/v1/"]);
    }

    #[test]
    fn refusals() {
        assert!(Config::from_args(args("")).is_err());
        assert!(Config::from_args(args("--root r --upstream 10.0.0.1:80")).is_err());
        assert!(Config::from_args(args("--root r --upstream localhost:80")).is_err());
        assert!(Config::from_args(args("--root r --bind 0.0.0.0:80")).is_err());
        assert!(Config::from_args(args("--root r --bind 0.0.0.0:80 --allow-public-bind")).is_ok());
        assert!(Config::from_args(args("--root r --proxy-prefix /api/")).is_err());
        assert!(Config::from_args(args(
            "--root r --upstream 127.0.0.1:1 --proxy-prefix /.git/"
        ))
        .is_err());
        assert!(
            Config::from_args(args("--root r --upstream 127.0.0.1:1 --proxy-prefix /")).is_err()
        );
        assert!(
            Config::from_args(args("--root r --upstream 127.0.0.1:1 --proxy-prefix /api")).is_err()
        );
        assert!(Config::from_args(args("--root r --shell ../x.html")).is_err());
        assert!(Config::from_args(args("--root r --max-conns 0")).is_err());
        assert!(Config::from_args(args("--root r --bogus")).is_err());
        assert_eq!(Config::from_args(args("--help")), Err(ArgsError::Help));
    }

    #[test]
    fn rust_ui_flags() {
        let c = Config::from_args(args(
            "--ui rust --upstream 127.0.0.1:8088 --pulse 127.0.0.1:8091 --console-host 203.0.113.7",
        ))
        .unwrap();
        assert!(c.rust_ui);
        assert!(c.root.is_none());
        assert_eq!(c.pulse.unwrap().port(), 8091);
        assert_eq!(c.extra_hosts, vec!["203.0.113.7"]);
        assert!(Config::from_args(args("--ui rust")).is_err());
        assert!(Config::from_args(args("--ui vue --root r")).is_err());
        assert!(
            Config::from_args(args("--ui rust --upstream 127.0.0.1:1 --pulse 10.0.0.1:1")).is_err()
        );
        let d = Config::from_args(args(
            "--ui rust --upstream 127.0.0.1:1 --desk 127.0.0.1:8646",
        ))
        .unwrap();
        assert_eq!(d.desk.unwrap().port(), 8646);
        assert!(c.desk.is_none());
        assert!(
            Config::from_args(args("--ui rust --upstream 127.0.0.1:1 --desk 10.0.0.1:1")).is_err()
        );
        assert!(Config::from_args(args("--ui rust --upstream 127.0.0.1:1 --desk x")).is_err());
        assert!(
            Config::from_args(args("--ui rust --upstream 127.0.0.1:1 --console-host a/b")).is_err()
        );
    }

    #[test]
    fn host_file_flags() {
        let c = Config::from_args(args(
            "--ui rust --upstream 127.0.0.1:1 --memory-root /var/lib/memory-tencentdb --model-file /opt/m --user-md /opt/U.md",
        ))
        .unwrap();
        assert_eq!(
            c.files.memory_root.as_deref(),
            Some(std::path::Path::new("/var/lib/memory-tencentdb"))
        );
        assert_eq!(
            c.files.model_file.as_deref(),
            Some(std::path::Path::new("/opt/m"))
        );
        assert_eq!(
            c.files.user_md.as_deref(),
            Some(std::path::Path::new("/opt/U.md"))
        );
        let d = Config::from_args(args("--ui rust --upstream 127.0.0.1:1")).unwrap();
        assert_eq!(d.files, console_ui::port4::HostFiles::default());
        for bad in [
            "--memory-root rel/dir",
            "--memory-root /a/../etc",
            "--user-md ./U.md",
            "--model-file",
        ] {
            assert!(
                Config::from_args(args(&format!("--ui rust --upstream 127.0.0.1:1 {bad}")))
                    .is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn admin_store_flags() {
        let c = Config::from_args(args(
            "--dist /d --admin-store /s/admin-device.json --admin-mirror /m/admin-device.json",
        ))
        .unwrap();
        assert_eq!(
            c.admin_store.as_deref(),
            Some(std::path::Path::new("/s/admin-device.json"))
        );
        assert!(c.admin_mirror.is_some());
        assert!(Config::from_args(args("--dist /d --admin-mirror /m/a.json")).is_err());
        assert!(Config::from_args(args("--dist /d --admin-store rel.json")).is_err());
        assert!(Config::from_args(args(
            "--dist /d --admin-store /a.json --admin-mirror /a.json"
        ))
        .is_err());
        assert!(Config::from_args(args("--dist /d"))
            .unwrap()
            .admin_store
            .is_none());
    }

    #[test]
    fn dist_and_local_api() {
        let c = Config::from_args(args(
            "--dist /tmp/console-dist --bind 127.0.0.1:21610 --ui rust",
        ))
        .unwrap();
        assert!(c.dist_mode);
        assert!(c.local_api);
        assert_eq!(c.shell, "none");
        assert!(c.rust_ui);
        assert_eq!(c.upstream, Some(c.bind));
        assert!(c.proxy_prefixes.is_empty());
        assert!(c.routes_text.is_some());
        assert!(c.root.as_ref().unwrap().ends_with("static") || c.root.is_some());
        let d = Config::from_args(args("--root /srv/c --shell none --local-api")).unwrap();
        assert_eq!(d.shell, "none");
        assert!(d.local_api);
    }

    #[test]
    fn arena_root_and_data_upstream() {
        let c = Config::from_args(args(
            "--dist /d --ui rust --bind 127.0.0.1:22060 --arena-root /workspace/arena --data-upstream 127.0.0.1:18080",
        ))
        .unwrap();
        assert_eq!(
            c.arena_root.as_deref(),
            Some(std::path::Path::new("/workspace/arena"))
        );
        assert_eq!(c.data_upstream.unwrap().to_string(), "127.0.0.1:18080");
        assert_eq!(c.data_paths, vec!["/api/arena", "/api/v1/bank"]);
        // Self-upstream stays without prefix proxying.
        assert_eq!(c.upstream, Some(c.bind));
        assert!(c.proxy_prefixes.is_empty());
        let c = Config::from_args(args(
            "--dist /d --data-upstream 127.0.0.1:18080 --data-path /api/v1/bank",
        ))
        .unwrap();
        assert_eq!(c.data_paths, vec!["/api/v1/bank"]);
        for bad_args in [
            "--root /r --arena-root /workspace/arena",
            "--root /r --data-upstream 127.0.0.1:18080",
            "--dist /d --arena-root rel/arena",
            "--dist /d --arena-root /a/../b",
            "--dist /d --data-upstream 10.0.0.1:18080",
            "--dist /d --data-upstream nope",
            "--dist /d --data-path /api/v1/bank",
            "--dist /d --bind 127.0.0.1:22061 --data-upstream 127.0.0.1:22061",
            "--dist /d --bind 0.0.0.0:22061 --allow-public-bind --data-upstream 127.0.0.1:22061",
            "--dist /d --data-upstream 127.0.0.1:1 --data-path /api/",
            "--dist /d --data-upstream 127.0.0.1:1 --data-path /api/x/",
            "--dist /d --data-upstream 127.0.0.1:1 --data-path /other",
            "--dist /d --data-upstream 127.0.0.1:1 --data-path /api/../x",
            "--dist /d --data-upstream 127.0.0.1:1 --data-path /api/x?y=1",
        ] {
            assert!(Config::from_args(args(bad_args)).is_err(), "{bad_args}");
        }
        let c = Config::from_args(args("--dist /d")).unwrap();
        assert!(c.arena_root.is_none() && c.data_upstream.is_none() && c.data_paths.is_empty());
    }
}
