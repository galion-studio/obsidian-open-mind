// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Command-line configuration. No config file, no environment lookups: the
//! systemd unit spells every value out, so what runs is what the unit says.

use std::net::SocketAddr;
use std::path::PathBuf;

pub const USAGE: &str = "galion-console-static: static front for the Galion console (B61)

usage: galion-console-static --root DIR [options]

  --bind ADDR          listen address (default 127.0.0.1:8090; loopback only
                       unless --allow-public-bind)
  --root DIR           built console (vite dist/client), indexed once at start
  --shell NAME         HTML shell under root (default _shell.html)
  --routes FILE        page/redirect list (see routes.rs); default: only `/`
  --upstream ADDR      loopback IP:port that answers the API (e.g. 127.0.0.1:8081)
  --proxy-prefix P     path prefix sent upstream; repeatable
                       (default /api/ and /_serverFn/ when --upstream is set)
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
    pub root: PathBuf,
    pub shell: String,
    pub routes: Option<PathBuf>,
    pub upstream: Option<SocketAddr>,
    pub proxy_prefixes: Vec<String>,
    pub max_conns: usize,
    pub max_body: u64,
    pub access_log: bool,
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
                "--allow-public-bind" => public = true,
                "--quiet" => access_log = false,
                other => return Err(bad(format!("unknown argument {other:?}"))),
            }
        }
        let root = root.ok_or_else(|| bad("--root is required"))?;
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
        })
    }
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
}
