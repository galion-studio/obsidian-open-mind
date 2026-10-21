// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Per-service flight manifest: a plain `key = value` file (`#` comments).
//! It holds the expected live state a change starts from (preflight) or
//! must end in (postflight). Every value that reaches a remote command is
//! validated here against a strict character set, so nothing a manifest
//! says can turn into shell syntax on the VPS.

use std::fmt;

/// How a response body is judged. Bodies are never printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    Any,
    /// Trimmed body equals this.
    Exact(String),
    Contains(String),
    /// Top-level (or dotted) JSON key whose scalar renders as this text.
    Json(String, String),
}

impl fmt::Display for Match {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Match::Any => write!(f, "any"),
            Match::Exact(s) => write!(f, "exact:{s}"),
            Match::Contains(s) => write!(f, "contains:{s}"),
            Match::Json(k, v) => write!(f, "json:{k}={v}"),
        }
    }
}

/// One HTTP GET on loopback of the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Get {
    /// `None` = the manifest's main `port`.
    pub port: Option<u16>,
    pub path: String,
    pub status: Vec<u16>,
    pub body: Match,
}

/// One HTTPS/HTTP GET from the box to a public URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Public {
    pub url: String,
    pub status: Vec<u16>,
    pub body: Match,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub service: String,
    /// systemd unit; empty = no unit (box instance, use `--pid`).
    pub unit: String,
    pub unit_file_state: String,
    pub port: u16,
    pub exe: String,
    pub sha256: String,
    pub mode: String,
    pub owner: String,
    /// Expected drop-in file names (exact set). `None` = not checked.
    pub dropins: Option<Vec<String>>,
    pub restarts_max: u32,
    pub gets: Vec<Get>,
    /// Component name in the nerve's /state; empty = not checked.
    pub nerve: String,
    pub nerve_ok: Vec<String>,
    pub nerve_port: u16,
    /// Extra units that must be active/running (e.g. tuner-quick).
    pub also_units: Vec<String>,
    /// (path, sha256) files that must exist with that hash (rollback
    /// binaries, deploy tool, config).
    pub files: Vec<(String, String)>,
    /// Directories/files that must exist (stat only).
    pub exists: Vec<String>,
    /// Timer that must not fire within N seconds (preflight only).
    pub quiet_timer: Option<(String, u64)>,
    /// Path whose filesystem must have at least N MiB free (preflight only).
    pub free: Option<(String, u64)>,
    pub public: Vec<Public>,
}

pub fn safe_path(p: &str) -> Result<(), String> {
    let ok = p.starts_with('/')
        && !p.contains("..")
        && p.len() < 256
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+@".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("unsafe path {p:?}"))
    }
}

pub fn safe_url_path(p: &str) -> Result<(), String> {
    let ok = p.starts_with('/')
        && p.len() < 200
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-~=&?".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("unsafe url path {p:?}"))
    }
}

pub fn safe_unit(u: &str) -> Result<(), String> {
    let ok = !u.is_empty()
        && u.len() < 100
        && (u.ends_with(".service") || u.ends_with(".timer"))
        && u.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_@".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("unsafe unit {u:?}"))
    }
}

pub fn safe_sha(s: &str) -> Result<(), String> {
    if s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(format!(
            "sha256 must be 64 lower-case hex chars, got {} chars",
            s.len()
        ))
    }
}

fn safe_public(u: &str) -> Result<(), String> {
    let ok = (u.starts_with("https://") || u.starts_with("http://"))
        && u.len() < 200
        && u.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":/._-~".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(format!("unsafe public url {u:?}"))
    }
}

fn parse_status(s: &str) -> Result<Vec<u16>, String> {
    s.split(',')
        .map(|x| x.parse::<u16>().map_err(|_| format!("bad status {x:?}")))
        .collect()
}

fn parse_match(s: &str) -> Result<Match, String> {
    if s == "any" {
        return Ok(Match::Any);
    }
    if let Some(v) = s.strip_prefix("exact:") {
        return Ok(Match::Exact(v.to_string()));
    }
    if let Some(v) = s.strip_prefix("contains:") {
        return Ok(Match::Contains(v.to_string()));
    }
    if let Some(v) = s.strip_prefix("json:") {
        let (k, val) = v.split_once('=').ok_or("json:KEY=VALUE")?;
        return Ok(Match::Json(k.to_string(), val.to_string()));
    }
    Err(format!(
        "bad body match {s:?} (any|exact:|contains:|json:K=V)"
    ))
}

/// `get = [:PORT] /path STATUS[,STATUS] MATCH`
fn parse_get(v: &str) -> Result<Get, String> {
    let mut it = v.split_whitespace().peekable();
    let mut port = None;
    if let Some(p) = it.peek().and_then(|t| t.strip_prefix(':')) {
        port = Some(p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?);
        it.next();
    }
    let path = it.next().ok_or("get needs a path")?.to_string();
    safe_url_path(&path)?;
    let status = parse_status(it.next().ok_or("get needs a status")?)?;
    let rest: Vec<&str> = it.collect();
    let body = if rest.is_empty() {
        Match::Any
    } else {
        parse_match(&rest.join(" "))?
    };
    Ok(Get {
        port,
        path,
        status,
        body,
    })
}

fn parse_public(v: &str) -> Result<Public, String> {
    let mut it = v.split_whitespace();
    let url = it.next().ok_or("public needs a url")?.to_string();
    safe_public(&url)?;
    let status = parse_status(it.next().ok_or("public needs a status")?)?;
    let rest: Vec<&str> = it.collect();
    let body = if rest.is_empty() {
        Match::Any
    } else {
        parse_match(&rest.join(" "))?
    };
    Ok(Public { url, status, body })
}

fn words(v: &str) -> Vec<String> {
    v.split_whitespace().map(str::to_string).collect()
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let mut m = Manifest {
            nerve_port: 8670,
            unit_file_state: "enabled".into(),
            nerve_ok: vec!["healthy".into()],
            ..Manifest::default()
        };
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            m.set_line(line, false)
                .map_err(|e| format!("line {}: {e}", i + 1))?;
        }
        m.validate()?;
        Ok(m)
    }

    /// Apply one `key = value` (or `key=value` from `--set`). With
    /// `replace`, the first `--set` of a list key clears the list.
    pub fn set_line(&mut self, line: &str, replace: bool) -> Result<(), String> {
        let (k, v) = line.split_once('=').ok_or("expected key = value")?;
        let k = k.trim();
        let v = v.trim();
        match k {
            "service" => self.service = v.into(),
            "unit" => self.unit = v.into(),
            "unit_file_state" => self.unit_file_state = v.into(),
            "port" => self.port = v.parse().map_err(|_| format!("bad port {v:?}"))?,
            "exe" => self.exe = v.into(),
            "sha256" => self.sha256 = v.into(),
            "mode" => self.mode = v.into(),
            "owner" => self.owner = v.into(),
            "dropins" => {
                self.dropins = if v == "-" { None } else { Some(words(v)) };
            }
            "restarts_max" => {
                self.restarts_max = v.parse().map_err(|_| format!("bad number {v:?}"))?
            }
            "nerve" => self.nerve = v.into(),
            "nerve_ok" => self.nerve_ok = v.split(',').map(|s| s.trim().to_string()).collect(),
            "nerve_port" => self.nerve_port = v.parse().map_err(|_| format!("bad port {v:?}"))?,
            "get" => {
                if replace {
                    self.gets.clear();
                }
                if !v.is_empty() {
                    self.gets.push(parse_get(v)?);
                }
            }
            "also_unit" => {
                if replace {
                    self.also_units.clear();
                }
                if !v.is_empty() {
                    self.also_units.push(v.into());
                }
            }
            "file" => {
                if replace {
                    self.files.clear();
                }
                if !v.is_empty() {
                    let w = words(v);
                    if w.len() != 2 {
                        return Err("file = PATH SHA256".into());
                    }
                    self.files.push((w[0].clone(), w[1].clone()));
                }
            }
            "exists" => {
                if replace {
                    self.exists.clear();
                }
                if !v.is_empty() {
                    self.exists.push(v.into());
                }
            }
            "quiet_timer" => {
                self.quiet_timer = if v.is_empty() {
                    None
                } else {
                    let w = words(v);
                    if w.len() != 2 {
                        return Err("quiet_timer = UNIT SECONDS".into());
                    }
                    Some((w[0].clone(), w[1].parse().map_err(|_| "bad seconds")?))
                }
            }
            "free" => {
                self.free = if v.is_empty() {
                    None
                } else {
                    let w = words(v);
                    if w.len() != 2 {
                        return Err("free = PATH MIB".into());
                    }
                    Some((w[0].clone(), w[1].parse().map_err(|_| "bad MiB")?))
                }
            }
            "public" => {
                if replace {
                    self.public.clear();
                }
                if !v.is_empty() {
                    self.public.push(parse_public(v)?);
                }
            }
            _ => return Err(format!("unknown key {k:?}")),
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.service.is_empty()
            || !self
                .service
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("service must be [A-Za-z0-9-]+".into());
        }
        if !self.unit.is_empty() {
            safe_unit(&self.unit)?;
        }
        for u in &self.also_units {
            safe_unit(u)?;
        }
        if let Some((t, _)) = &self.quiet_timer {
            safe_unit(t)?;
        }
        if self.port == 0 {
            return Err("port must be set".into());
        }
        if !self.exe.is_empty() {
            safe_path(&self.exe)?;
        }
        if !self.sha256.is_empty() {
            safe_sha(&self.sha256)?;
        }
        if !self.mode.is_empty() && !self.mode.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
            return Err(format!("bad mode {:?}", self.mode));
        }
        if !self.owner.is_empty()
            && !self
                .owner
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":-_".contains(&b))
        {
            return Err(format!("bad owner {:?}", self.owner));
        }
        for d in self.dropins.iter().flatten() {
            if !d.ends_with(".conf") || d.contains('/') {
                return Err(format!("bad drop-in name {d:?}"));
            }
        }
        for (p, s) in &self.files {
            safe_path(p)?;
            safe_sha(s)?;
        }
        for p in &self.exists {
            safe_path(p)?;
        }
        if let Some((p, _)) = &self.free {
            safe_path(p)?;
        }
        for g in &self.gets {
            safe_url_path(&g.path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_forms() {
        let g = parse_get(":20242 /ready 200 any").unwrap();
        assert_eq!(g.port, Some(20242));
        let g = parse_get("/health 200,204 json:status=ok").unwrap();
        assert_eq!(g.status, vec![200, 204]);
        assert_eq!(g.body, Match::Json("status".into(), "ok".into()));
        let g = parse_get("/healthz 200 exact:ok").unwrap();
        assert_eq!(g.body, Match::Exact("ok".into()));
        assert!(parse_get("/x;rm 200").is_err());
        assert!(parse_get("/x' 200").is_err());
    }

    #[test]
    fn rejects_shell_syntax_everywhere() {
        for bad in [
            "/opt/x;id",
            "/opt/$(id)",
            "/opt/`id`",
            "/opt/x y",
            "/opt/../etc/shadow",
            "relative",
            "/opt/x'",
            "/opt/x|y",
            "/opt/x>y",
        ] {
            assert!(safe_path(bad).is_err(), "{bad}");
        }
        assert!(safe_unit("x.service;id").is_err());
        assert!(safe_unit("x").is_err());
        assert!(safe_sha("ABC").is_err());
        assert!(safe_public("https://x/$(id)").is_err());
    }

    #[test]
    fn set_replaces_lists() {
        let mut m = Manifest::parse("service = t\nport = 1\nget = /a 200\nget = /b 200\n").unwrap();
        assert_eq!(m.gets.len(), 2);
        m.set_line("get=/c 404", true).unwrap();
        assert_eq!(m.gets.len(), 1);
        assert!(m.set_line("nope=1", true).is_err());
    }
}
