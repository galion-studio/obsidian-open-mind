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

  --bind ADDR          listen address (default 127.0.0.1:8090; loopback only
                       unless --allow-public-bind)
  --root DIR           pre-built console (dist/client), indexed once at start;
                       optional with --ui rust (then only Rust pages are served)
  --shell NAME         HTML shell under root (default _shell.html)
  --routes FILE        page/redirect list (see routes.rs); default: only `/`
  --upstream ADDR      loopback IP:port that answers the API (e.g. 127.0.0.1:8081)
  --proxy-prefix P     path prefix sent upstream; repeatable
                       (default /api/ and /_serverFn/ when --upstream is set)
  --ui rust            render the ported pages in Rust (crates/console-ui):
                       server-side HTML, Rust-generated CSS, no JavaScript;
                       needs --upstream; with --root, unported pages fall back
                       to the pre-built SPA
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind: SocketAddr,
    pub root: Option<PathBuf>,
    pub shell: String,
    pub routes: Option<PathBuf>,
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
                "--shell" => {
                    let s = val("--shell")?;
                    if s.contains('/') || s.starts_with('.') || !s.ends_with(".html") {
                        return Err(bad("--shell must be a plain .html file name"));
                    }
                    shell = s;
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
                "--user-md" => files.user_md = Some(abs_path(val("--user-md")?, "--user-md")?),
                "--allow-public-bind" => public = true,
                "--quiet" => access_log = false,
                other => return Err(bad(format!("unknown argument {other:?}"))),
            }
        }
        if root.is_none() && !rust_ui {
            return Err(bad("--root is required (or use --ui rust)"));
        }
        if rust_ui && upstream.is_none() {
            return Err(bad(
                "--ui rust needs --upstream (the pages read the console API)",
            ));
        }
        if !public && !bind.ip().is_loopback() {
            return Err(bad(
                "--bind is not loopback; pass --allow-public-bind if that is intended",
            ));
        }
        if upstream.is_some() && prefixes.is_empty() {
            prefixes = vec!["/api/".to_string(), "/_serverFn/".to_string()];
        }
        if upstream.is_none() && !prefixes.is_empty() {
            return Err(bad("--proxy-prefix needs --upstream"));
        }
        Ok(Config {
            bind,
            root,
            shell,
            routes,
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
}
