// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Argument parsing (hand-written; no dependencies) and global settings.

use crate::sandbox::StartOpts;
use std::path::PathBuf;

pub const USAGE: &str = "galion-admin 0.1.0 - Galion OS admin CLI (L1 skeleton; never uses su/root)

USAGE:
  galion-admin [--state DIR] [--box PATH] COMMAND

COMMANDS:
  sandbox start NAME --rootfs DIR [OPTIONS] -- CMD [ARGS...]
                             start a sandbox in the background via `galion-box run`
      --net none|lo          network (default none = default-deny)
      --ro-root              read-only root
      --bind-ro SRC:DST      read-only bind (repeatable)
      --timeout SECS         galion-box kills the sandbox after SECS
      --nproc N  --as-mb N  --nice N   limits passed to galion-box
      --settle-ms N          how long to watch for an early failure (default 500)
  sandbox stop NAME [--grace SECS]   SIGTERM, then SIGKILL after SECS (default 5)
  sandbox status [NAME]      running / exited (with exit code) / stale
  sandbox probe              run `galion-box probe` (read-only capability report)
  tunnel status [--config FILE] [--timeout-ms N]
                             TCP-connect each loopback tunnel end in FILE
                             (default STATE/tunnel.conf, lines `name = 127.0.0.1:PORT`)
  audit tail [-n N] [--json] [--follow] [--follow-secs N]
  audit verify               check the hash chain; prints the head hash to pin elsewhere
  image verify DIR --key PUBFILE [--json]
                             check a gold image: Ed25519 signature (hades-gate key format
                             `ed25519-pub <hex>`), then every listed file's size, mode and
                             SHA-256; unlisted files, symlinks and special files fail.
                             Read-only; docs/galion-os/gold-image-format.md
  version | help

STATE DIR: --state, else $GALION_ADMIN_HOME, else $HOME/.galion-admin (created 0700).
GALION-BOX: --box, else $GALION_BOX, else `galion-box` on PATH.
EXIT: 0 ok, 1 error, 2 usage, 3 refused (root),
      4 not running / tunnel down / audit broken / image verification failed.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Help,
    Version,
    SandboxStart(StartOpts),
    SandboxStop {
        name: String,
        grace_secs: u64,
    },
    SandboxStatus(Option<String>),
    SandboxProbe,
    TunnelStatus {
        config: Option<PathBuf>,
        timeout_ms: u64,
    },
    AuditTail {
        n: usize,
        json: bool,
        follow: bool,
        follow_secs: Option<u64>,
    },
    AuditVerify,
    ImageVerify {
        dir: PathBuf,
        key: PathBuf,
        json: bool,
    },
    Supervise {
        name: String,
        cmd: String,
        box_bin: PathBuf,
        box_args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Global {
    pub state: Option<PathBuf>,
    pub box_bin: Option<PathBuf>,
}

fn num<T: std::str::FromStr>(flag: &str, v: Option<&String>) -> Result<T, String> {
    let v = v.ok_or_else(|| format!("{flag} needs a value"))?;
    v.parse()
        .map_err(|_| format!("{flag}: invalid number {v:?}"))
}

pub fn parse(args: &[String]) -> Result<(Global, Cmd), String> {
    let mut g = Global {
        state: None,
        box_bin: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--state" => {
                g.state = Some(PathBuf::from(args.get(i + 1).ok_or("--state needs DIR")?));
                i += 2;
            }
            "--box" => {
                g.box_bin = Some(PathBuf::from(args.get(i + 1).ok_or("--box needs PATH")?));
                i += 2;
            }
            _ => break,
        }
    }
    let rest = &args[i..];
    let words: Vec<&str> = rest.iter().map(String::as_str).collect();
    let cmd = match words.as_slice() {
        [] | ["help"] | ["-h"] | ["--help"] => Cmd::Help,
        ["version"] | ["-V"] | ["--version"] => Cmd::Version,
        ["sandbox", "start", ..] => Cmd::SandboxStart(parse_start(&rest[2..])?),
        ["sandbox", "stop", name, tail @ ..] => {
            let mut grace = 5;
            let mut j = 0;
            while j < tail.len() {
                match tail[j] {
                    "--grace" => {
                        grace = num("--grace", rest.get(3 + j + 1))?;
                        j += 2;
                    }
                    o => return Err(format!("sandbox stop: unknown option {o:?}")),
                }
            }
            Cmd::SandboxStop {
                name: name.to_string(),
                grace_secs: grace,
            }
        }
        ["sandbox", "status"] => Cmd::SandboxStatus(None),
        ["sandbox", "status", name] => Cmd::SandboxStatus(Some(name.to_string())),
        ["sandbox", "probe"] => Cmd::SandboxProbe,
        ["tunnel", "status", tail @ ..] => {
            let (mut config, mut timeout_ms) = (None, 1500);
            let mut j = 0;
            while j < tail.len() {
                match tail[j] {
                    "--config" => {
                        config = Some(PathBuf::from(
                            rest.get(2 + j + 1).ok_or("--config needs FILE")?,
                        ));
                        j += 2;
                    }
                    "--timeout-ms" => {
                        timeout_ms = num("--timeout-ms", rest.get(2 + j + 1))?;
                        j += 2;
                    }
                    o => return Err(format!("tunnel status: unknown option {o:?}")),
                }
            }
            if timeout_ms == 0 || timeout_ms > 30_000 {
                return Err("--timeout-ms must be 1..30000".into());
            }
            Cmd::TunnelStatus { config, timeout_ms }
        }
        ["audit", "tail", tail @ ..] => {
            let (mut n, mut json, mut follow, mut follow_secs) = (10usize, false, false, None);
            let mut j = 0;
            while j < tail.len() {
                match tail[j] {
                    "-n" => {
                        n = num("-n", rest.get(2 + j + 1))?;
                        j += 2;
                    }
                    "--json" => {
                        json = true;
                        j += 1;
                    }
                    "--follow" | "-f" => {
                        follow = true;
                        j += 1;
                    }
                    "--follow-secs" => {
                        follow = true;
                        follow_secs = Some(num("--follow-secs", rest.get(2 + j + 1))?);
                        j += 2;
                    }
                    o => return Err(format!("audit tail: unknown option {o:?}")),
                }
            }
            Cmd::AuditTail {
                n,
                json,
                follow,
                follow_secs,
            }
        }
        ["audit", "verify"] => Cmd::AuditVerify,
        ["image", "verify", tail @ ..] => parse_image_verify(tail)?,
        ["__supervise", name, cmd, box_bin, ..] => Cmd::Supervise {
            name: name.to_string(),
            cmd: cmd.to_string(),
            box_bin: PathBuf::from(box_bin),
            box_args: rest[4..].to_vec(),
        },
        _ => {
            return Err(format!(
                "unknown command {:?} (try `galion-admin help`)",
                words.join(" ")
            ))
        }
    };
    Ok((g, cmd))
}

fn parse_image_verify(tail: &[&str]) -> Result<Cmd, String> {
    let (mut dir, mut key, mut json) = (None, None, false);
    let mut it = tail.iter();
    while let Some(x) = it.next() {
        match *x {
            "--key" => key = Some(PathBuf::from(it.next().ok_or("--key needs PUBFILE")?)),
            "--json" => json = true,
            o if o.starts_with("--") => return Err(format!("image verify: unknown option {o:?}")),
            d if dir.is_none() => dir = Some(PathBuf::from(d)),
            d => return Err(format!("image verify: unexpected argument {d:?}")),
        }
    }
    Ok(Cmd::ImageVerify {
        dir: dir.ok_or("image verify needs DIR")?,
        key: key.ok_or("image verify needs --key PUBFILE (no default key: pin it explicitly)")?,
        json,
    })
}

fn parse_start(a: &[String]) -> Result<StartOpts, String> {
    let mut o = StartOpts {
        settle_ms: 500,
        ..Default::default()
    };
    let mut it = a.iter();
    o.name = it.next().ok_or("sandbox start needs NAME")?.clone();
    if o.name.starts_with("--") {
        return Err("sandbox start needs NAME before options".into());
    }
    let mut rootfs = None;
    while let Some(x) = it.next() {
        match x.as_str() {
            "--" => {
                o.argv = it.by_ref().cloned().collect();
                break;
            }
            "--rootfs" => rootfs = Some(PathBuf::from(it.next().ok_or("--rootfs needs DIR")?)),
            "--net" => {
                o.net_lo = match it.next().map(String::as_str) {
                    Some("none") => false,
                    Some("lo") => true,
                    v => return Err(format!("--net must be none or lo, got {v:?}")),
                }
            }
            "--ro-root" => o.ro_root = true,
            "--bind-ro" => o
                .binds_ro
                .push(it.next().ok_or("--bind-ro needs SRC:DST")?.clone()),
            "--timeout" => {
                let v = it.next().ok_or("--timeout needs SECS")?;
                match v.parse::<f64>() {
                    Ok(t) if t > 0.0 && t.is_finite() => o.timeout_secs = Some(v.clone()),
                    _ => return Err(format!("--timeout: invalid seconds {v:?}")),
                }
            }
            "--nproc" => o.nproc = Some(num("--nproc", it.next())?),
            "--as-mb" => o.as_mb = Some(num("--as-mb", it.next())?),
            "--nice" => o.nice = Some(num("--nice", it.next())?),
            "--settle-ms" => o.settle_ms = num("--settle-ms", it.next())?,
            other => return Err(format!("sandbox start: unknown option {other:?}")),
        }
    }
    o.rootfs = rootfs.ok_or("sandbox start needs --rootfs DIR")?;
    o.validate()?;
    Ok(o)
}

/// Root is refused outright: L1 has no su/root path (owner decision D-A1).
pub fn refuse_root(euid: u32) -> Result<(), String> {
    if euid == 0 {
        Err("galion-admin refuses to run as root (L1 has no su/root path; see D-A1)".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn globals_and_basic() {
        let (g, c) = parse(&v("--state /s --box /b/galion-box sandbox status")).unwrap();
        assert_eq!(g.state, Some("/s".into()));
        assert_eq!(g.box_bin, Some("/b/galion-box".into()));
        assert_eq!(c, Cmd::SandboxStatus(None));
        assert_eq!(parse(&[]).unwrap().1, Cmd::Help);
        assert_eq!(parse(&v("version")).unwrap().1, Cmd::Version);
        assert!(parse(&v("sandbox frobnicate")).is_err());
        assert!(parse(&v("--state")).is_err());
    }

    #[test]
    fn start_parsing() {
        let (_, c) = parse(&v(
            "sandbox start w1 --rootfs /r --net lo --ro-root --timeout 2.5 --nproc 8 --settle-ms 100 -- /bin/x --flag y",
        ))
        .unwrap();
        match c {
            Cmd::SandboxStart(o) => {
                assert_eq!(o.name, "w1");
                assert!(o.net_lo && o.ro_root);
                assert_eq!(o.timeout_secs.as_deref(), Some("2.5"));
                assert_eq!(o.nproc, Some(8));
                assert_eq!(o.settle_ms, 100);
                assert_eq!(o.argv, v("/bin/x --flag y"));
            }
            c => panic!("{c:?}"),
        }
        assert!(
            parse(&v("sandbox start w1 -- /bin/x")).is_err(),
            "no rootfs"
        );
        assert!(parse(&v("sandbox start w1 --rootfs / -- /bin/x")).is_err());
        assert!(
            parse(&v("sandbox start w1 --rootfs /r")).is_err(),
            "no payload"
        );
        assert!(parse(&v("sandbox start w1 --rootfs /r --timeout -1 -- x")).is_err());
        assert!(parse(&v("sandbox start w1 --rootfs /r --timeout nan -- x")).is_err());
        assert!(parse(&v("sandbox start w1 --rootfs /r --net wan -- x")).is_err());
        assert!(parse(&v("sandbox start BAD --rootfs /r -- x")).is_err());
        assert!(parse(&v("sandbox start --rootfs /r -- x")).is_err());
    }

    #[test]
    fn stop_tunnel_audit() {
        assert_eq!(
            parse(&v("sandbox stop w --grace 2")).unwrap().1,
            Cmd::SandboxStop {
                name: "w".into(),
                grace_secs: 2
            }
        );
        assert_eq!(
            parse(&v("tunnel status --config /c --timeout-ms 300"))
                .unwrap()
                .1,
            Cmd::TunnelStatus {
                config: Some("/c".into()),
                timeout_ms: 300
            }
        );
        assert!(parse(&v("tunnel status --timeout-ms 0")).is_err());
        assert_eq!(
            parse(&v("audit tail -n 3 --json --follow-secs 2"))
                .unwrap()
                .1,
            Cmd::AuditTail {
                n: 3,
                json: true,
                follow: true,
                follow_secs: Some(2)
            }
        );
        assert!(parse(&v("audit tail -n x")).is_err());
    }

    #[test]
    fn image_verify_parsing() {
        assert_eq!(
            parse(&v("image verify /img --key /k.pub --json"))
                .unwrap()
                .1,
            Cmd::ImageVerify {
                dir: "/img".into(),
                key: "/k.pub".into(),
                json: true
            }
        );
        assert_eq!(
            parse(&v("image verify --key /k.pub /img")).unwrap().1,
            Cmd::ImageVerify {
                dir: "/img".into(),
                key: "/k.pub".into(),
                json: false
            }
        );
        assert!(parse(&v("image verify /img")).is_err(), "no key");
        assert!(parse(&v("image verify --key /k")).is_err(), "no dir");
        assert!(parse(&v("image verify /a /b --key /k")).is_err());
        assert!(parse(&v("image verify /a --key")).is_err());
        assert!(parse(&v("image verify /a --key /k --force")).is_err());
    }

    #[test]
    fn root_refused() {
        assert!(refuse_root(0).is_err());
        assert!(refuse_root(1000).is_ok());
    }
}
