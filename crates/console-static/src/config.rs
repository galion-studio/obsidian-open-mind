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
                       (default /api/arena and /api/v1/bank)
  --survival-file FILE survival.json whose age GET /api/v1/survival reports
                       (mtime only, never the content), e.g.
                       /opt/galion/galion-light/state/survival.json; needs
                       --local-api
  --survival-writer BIN absolute writer run when the file is older than
                       --survival-refresh-s (no shell, no args, cleared env),
                       e.g. /opt/galion/rust/bin/galion-survival
  --survival-refresh-s N  refresh age in seconds (60..3600, default 900)
  --survival-mode M    on-request (GET /bank, /api/v1/bank, /api/v1/survival
                       start a due run; default) or interval (in-process
                       check every min(N, 60) s); no systemd timer either way
  --survival-env K=V   writer env (repeatable; GALION_LIGHT_ROOT,
                       GALION_HERMES_ROOT, GALION_SURVIVAL_DISK,
                       OPENROUTER_KEY_FILE only)
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
    /// `--survival-*`: timer-free survival.json refresh ([`crate::survival`]).
    pub survival: Option<crate::survival::SurvivalConfig>,
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
            ArgsError::Bad(m) => write!(f, "{m}\n\n{USAGE}"),
        }
    }
}

fn bad(m: impl Into<String>) -> ArgsError {
    ArgsError::Bad(m.into())
}

fn valid_prefix(p: &str) -> bool {
    p.len() > 1 && p.ends_with('/') && crate::path::sanitize(p).as_deref() == Ok(p)
}

/// Default `--data-path` list when `--data-upstream` is set.
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
        let mut survival_file = None;
        let mut survival_writer = None;
        let mut survival_refresh: Option<u64> = None;
        let mut survival_mode: Option<crate::survival::Mode> = None;
        let mut survival_env: Vec<(String, String)> = Vec::new();
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
                "--survival-file" => {
                    survival_file = Some(abs_path(val("--survival-file")?, "--survival-file")?)
                }
                "--survival-writer" => {
                    survival_writer =
                        Some(abs_path(val("--survival-writer")?, "--survival-writer")?)
                }
                "--survival-refresh-s" => {
                    let n: u64 = val("--survival-refresh-s")?
                        .parse()
                        .map_err(|_| bad("--survival-refresh-s must be a number"))?;
                    if !(crate::survival::REFRESH_MIN_S..=crate::survival::REFRESH_MAX_S)
                        .contains(&n)
                    {
                        return Err(bad("--survival-refresh-s must be 60..3600"));
                    }
                    survival_refresh = Some(n);
                }
                "--survival-mode" => {
                    let m = val("--survival-mode")?;
                    survival_mode = Some(crate::survival::Mode::parse(&m).ok_or_else(|| {
                        bad(format!(
                            "--survival-mode must be on-request or interval, not {m:?}"
                        ))
                    })?);
                }
                "--survival-env" => {
                    let e = crate::survival::parse_env(&val("--survival-env")?).map_err(bad)?;
                    survival_env.retain(|(k, _)| *k != e.0);
                    survival_env.push(e);
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
        if !data_paths.is_empty() && data_upstream.is_none() {
            return Err(bad("--data-path needs --data-upstream"));
        }
        if data_upstream.is_some_and(|u| {
            u.port() == bind.port() && (u.ip() == bind.ip() || bind.ip().is_unspecified())
        }) {
            return Err(bad("--data-upstream must not be --bind (would loop)"));
        }
        if data_upstream.is_some() && data_paths.is_empty() {
            data_paths = DATA_DEFAULT_PATHS.iter().map(|s| s.to_string()).collect();
        }
        let survival = match survival_file {
            None => {
                if survival_writer.is_some()
                    || survival_refresh.is_some()
                    || survival_mode.is_some()
                    || !survival_env.is_empty()
                {
                    return Err(bad(
                        "--survival-writer/-refresh-s/-mode/-env need --survival-file",
                    ));
                }
                None
            }
            Some(file) => {
                if !local_api {
                    return Err(bad(
                        "--survival-file needs --local-api or --dist (the API gate)",
                    ));
                }
                let mode = survival_mode.unwrap_or(crate::survival::Mode::OnRequest);
                if survival_writer.is_none() && mode == crate::survival::Mode::Interval {
                    return Err(bad("--survival-mode interval needs --survival-writer"));
                }
                if survival_writer.is_none() && !survival_env.is_empty() {
                    return Err(bad("--survival-env needs --survival-writer"));
                }
                Some(crate::survival::SurvivalConfig {
                    file,
                    writer: survival_writer,
                    refresh_s: survival_refresh.unwrap_or(crate::survival::REFRESH_DEFAULT_S),
                    mode,
                    env: survival_env,
                })
            }
        };
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
            survival,
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
    #[test]
    fn survival_flags() {
        let c = Config::from_args(args(
            "--dist /d --survival-file /s/survival.json --survival-writer /b/galion-survival --survival-refresh-s 120 --survival-mode interval --survival-env GALION_LIGHT_ROOT=/var/tmp/r --survival-env GALION_LIGHT_ROOT=/var/tmp/q",
        ))
        .unwrap();
        let s = c.survival.unwrap();
        assert_eq!(s.file, std::path::PathBuf::from("/s/survival.json"));
        assert_eq!(
            s.writer.as_deref(),
            Some(std::path::Path::new("/b/galion-survival"))
        );
        assert_eq!(s.refresh_s, 120);
        assert_eq!(s.mode, crate::survival::Mode::Interval);
        assert_eq!(
            s.env,
            vec![("GALION_LIGHT_ROOT".to_string(), "/var/tmp/q".to_string())]
        );
        // Defaults: on-request, 900 s, status-only without a writer.
        let d = Config::from_args(args("--dist /d --survival-file /s/survival.json"))
            .unwrap()
            .survival
            .unwrap();
        assert_eq!(
            (d.refresh_s, d.mode, d.writer),
            (900, crate::survival::Mode::OnRequest, None)
        );
        assert!(Config::from_args(args("--dist /d"))
            .unwrap()
            .survival
            .is_none());
        for bad_args in [
            "--root /r --survival-file /s/x.json",
            "--dist /d --survival-writer /b/w",
            "--dist /d --survival-file s.json",
            "--dist /d --survival-file /s/x.json --survival-writer w",
            "--dist /d --survival-file /s/../x.json",
            "--dist /d --survival-file /s/x.json --survival-refresh-s 59",
            "--dist /d --survival-file /s/x.json --survival-refresh-s 3601",
            "--dist /d --survival-file /s/x.json --survival-mode timer",
            "--dist /d --survival-file /s/x.json --survival-mode interval",
            "--dist /d --survival-file /s/x.json --survival-env GALION_LIGHT_ROOT=/x",
            "--dist /d --survival-file /s/x.json --survival-writer /w --survival-env OPENROUTER_API_KEY=k",
        ] {
            assert!(Config::from_args(args(bad_args)).is_err(), "{bad_args}");
        }
    }
}
