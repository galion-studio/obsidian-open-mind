// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//! tunnel-lint: read-only checks for a cloudflared ingress `config.yml`.
//!
//! It parses only the subset of YAML that cloudflared ingress files use
//! (`ingress:` list items with `hostname`, `path`, `service`, plus nested
//! blocks it skips), resolves each rule's origin to a local port, and asks a
//! [`Listeners`] source whether anything listens there. It never reads the
//! `credentials-file` the config points at and never writes the config: the
//! `stage` output is a new text the operator can diff and apply by hand.

use std::collections::{BTreeMap, HashSet};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// One ingress rule as written in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 1-based index in the ingress list.
    pub index: usize,
    pub hostname: Option<String>,
    pub path: Option<String>,
    pub service: String,
    /// 0-based line range `[first, last]` of the rule's own lines in the file
    /// (trailing blank and comment lines excluded).
    pub lines: (usize, usize),
}

/// Where a rule sends traffic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A TCP origin on this host (`localhost`, `127.x`, `::1`, `0.0.0.0`).
    Local { host: String, port: u16 },
    /// A TCP origin on another host; not probed.
    Remote { host: String, port: u16 },
    /// `http_status:NNN`, `hello_world`, `bastion`, `unix:` and so on.
    Builtin(String),
    /// Could not be understood.
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub msg: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line + 1, self.msg)
    }
}

fn indent_of(s: &str) -> usize {
    s.len() - s.trim_start_matches(' ').len()
}

/// Remove a trailing ` # comment` that is outside quotes.
fn strip_comment(s: &str) -> &str {
    let b = s.as_bytes();
    let mut q: Option<u8> = None;
    for i in 0..b.len() {
        match (q, b[i]) {
            (None, b'"') | (None, b'\'') => q = Some(b[i]),
            (Some(c), x) if c == x => q = None,
            (None, b'#') if i == 0 || b[i - 1] == b' ' || b[i - 1] == b'\t' => return &s[..i],
            _ => {}
        }
    }
    s
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

fn is_blank_or_comment(s: &str) -> bool {
    let t = s.trim();
    t.is_empty() || t.starts_with('#')
}

/// Parse the `ingress:` list of a cloudflared config.
pub fn parse(text: &str) -> Result<Vec<Rule>, ParseError> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| strip_comment(l).trim_end() == "ingress:" && indent_of(l) == 0)
        .ok_or(ParseError {
            line: 0,
            msg: "no top-level `ingress:` key".into(),
        })?;
    if let Some((i, _)) = lines.iter().enumerate().find(|(_, l)| l.contains('\t')) {
        return Err(ParseError {
            line: i,
            msg: "tab character (YAML forbids tabs for indentation)".into(),
        });
    }

    struct Cur {
        first: usize,
        last: usize,
        kv: BTreeMap<String, (String, usize)>,
    }
    let mut rules = Vec::new();
    let mut item_indent: Option<usize> = None;
    let mut key_indent: Option<usize> = None;
    let mut cur: Option<Cur> = None;

    let finish = |c: Cur, rules: &mut Vec<Rule>| -> Result<(), ParseError> {
        let get = |k: &str| c.kv.get(k).map(|(v, _)| v.clone());
        let service = get("service").ok_or(ParseError {
            line: c.first,
            msg: "ingress rule without `service`".into(),
        })?;
        rules.push(Rule {
            index: rules.len() + 1,
            hostname: get("hostname"),
            path: get("path"),
            service,
            lines: (c.first, c.last),
        });
        Ok(())
    };

    for (i, raw) in lines.iter().enumerate().skip(start + 1) {
        if is_blank_or_comment(raw) {
            continue;
        }
        let line = strip_comment(raw).trim_end();
        let ind = indent_of(line);
        let body = &line[ind..];
        // A new top-level key ends the ingress list.
        if ind == 0 && !body.starts_with('-') {
            break;
        }
        if let Some(rest) = body
            .strip_prefix("- ")
            .or(if body == "-" { Some("") } else { None })
        {
            match item_indent {
                None => item_indent = Some(ind),
                Some(ii) if ii == ind => {}
                Some(ii) if ind > ii => {
                    // A list nested inside a rule (e.g. originRequest.access lists).
                    if let Some(c) = cur.as_mut() {
                        c.last = i;
                    }
                    continue;
                }
                Some(_) => {
                    // YAML has no list item shallower than the first one under
                    // the same key; cloudflared refuses such a file. Silently
                    // stopping here hid every later rule (T5.2 fuzz find).
                    return Err(ParseError {
                        line: i,
                        msg: "list item less indented than the first ingress item".into(),
                    });
                }
            }
            if let Some(c) = cur.take() {
                finish(c, &mut rules)?;
            }
            key_indent =
                (!rest.trim().is_empty()).then(|| ind + body.len() - rest.trim_start().len());
            let mut c = Cur {
                first: i,
                last: i,
                kv: BTreeMap::new(),
            };
            if !rest.trim().is_empty() {
                put_kv(&mut c.kv, rest, i)?;
            }
            cur = Some(c);
            continue;
        }
        let Some(c) = cur.as_mut() else {
            return Err(ParseError {
                line: i,
                msg: "expected `- ` list item under ingress".into(),
            });
        };
        if let Some(ii) = item_indent {
            if ind <= ii {
                break;
            }
        }
        c.last = i;
        if key_indent.is_none() {
            key_indent = Some(ind);
        }
        if Some(ind) == key_indent {
            put_kv(&mut c.kv, body, i)?;
        }
        // Deeper lines belong to a nested block (originRequest) and are skipped.
    }
    if let Some(c) = cur.take() {
        finish(c, &mut rules)?;
    }
    if rules.is_empty() {
        return Err(ParseError {
            line: start,
            msg: "`ingress:` has no rules".into(),
        });
    }
    Ok(rules)
}

fn put_kv(
    kv: &mut BTreeMap<String, (String, usize)>,
    s: &str,
    line: usize,
) -> Result<(), ParseError> {
    let Some((k, v)) = s.split_once(':') else {
        return Err(ParseError {
            line,
            msg: format!("expected `key: value`, got `{}`", s.trim()),
        });
    };
    let k = k.trim().to_string();
    let v = unquote(v);
    if matches!(k.as_str(), "hostname" | "path" | "service") && v.is_empty() {
        return Err(ParseError {
            line,
            msg: format!("`{k}` is empty"),
        });
    }
    kv.insert(k, (v, line));
    Ok(())
}

fn is_local_host(h: &str) -> bool {
    let h = h.trim_start_matches('[').trim_end_matches(']');
    h == "localhost" || h == "::1" || h == "0.0.0.0" || h == "::" || h.starts_with("127.")
}

/// Resolve a `service:` value to an origin.
pub fn origin(service: &str) -> Origin {
    let s = service.trim();
    let builtin = [
        "http_status:",
        "hello_world",
        "bastion",
        "unix:",
        "unix+tls:",
    ];
    if builtin.iter().any(|b| s.starts_with(b)) {
        return Origin::Builtin(s.to_string());
    }
    let Some((scheme, rest)) = s.split_once("://") else {
        return Origin::Invalid(s.to_string());
    };
    let default_port = match scheme {
        "http" | "ws" => 80,
        "https" | "wss" => 443,
        "ssh" => 22,
        "rdp" => 3389,
        "smb" => 445,
        "tcp" => 0,
        _ => return Origin::Invalid(s.to_string()),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = if let Some(r) = authority.strip_prefix('[') {
        match r.split_once(']') {
            Some((h, p)) => (h.to_string(), p.strip_prefix(':').map(str::to_string)),
            None => return Origin::Invalid(s.to_string()),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.to_string())),
            None => (authority.to_string(), None),
        }
    };
    let port = match port {
        Some(p) => match p.parse::<u16>() {
            Ok(n) if n > 0 => n,
            _ => return Origin::Invalid(s.to_string()),
        },
        None if default_port > 0 => default_port,
        None => return Origin::Invalid(s.to_string()),
    };
    if host.is_empty() {
        return Origin::Invalid(s.to_string());
    }
    if is_local_host(&host) {
        Origin::Local { host, port }
    } else {
        Origin::Remote { host, port }
    }
}

/// Something that knows which local ports accept connections.
pub trait Listeners {
    /// `Some(owner)` when open (owner may be empty), `None` when closed.
    fn check(&self, host: &str, port: u16) -> Option<String>;
}

/// Live check: TCP connect to the loopback address(es) the host names.
/// `localhost` tries 127.0.0.1 and ::1, like cloudflared's dialer does.
pub struct Probe {
    pub timeout: Duration,
}

impl Listeners for Probe {
    fn check(&self, host: &str, port: u16) -> Option<String> {
        let h = host.trim_start_matches('[').trim_end_matches(']');
        let addrs: Vec<String> = match h {
            "localhost" => vec![format!("127.0.0.1:{port}"), format!("[::1]:{port}")],
            "0.0.0.0" => vec![format!("127.0.0.1:{port}")],
            "::" | "::1" => vec![format!("[::1]:{port}")],
            v4 => vec![format!("{v4}:{port}")],
        };
        for a in addrs {
            if let Ok(sa) = a.parse::<SocketAddr>() {
                if TcpStream::connect_timeout(&sa, self.timeout).is_ok() {
                    return Some(String::new());
                }
            }
        }
        None
    }
}

/// Offline check from captured `ss -ltn` / `ss -ltnp` output.
#[derive(Debug, Default)]
pub struct SsSnapshot {
    /// (address, port, owner)
    pub entries: Vec<(String, u16, String)>,
}

impl SsSnapshot {
    pub fn parse(text: &str) -> SsSnapshot {
        let mut entries = Vec::new();
        for line in text.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.first() != Some(&"LISTEN") || cols.len() < 4 {
                continue;
            }
            let local = cols[3];
            let Some((addr, port)) = local.rsplit_once(':') else {
                continue;
            };
            let Ok(port) = port.parse::<u16>() else {
                continue;
            };
            let addr = addr.split('%').next().unwrap_or(addr);
            let addr = addr
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            let owner = line
                .split("((\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
                .map(|name| {
                    let pid = line
                        .split("pid=")
                        .nth(1)
                        .and_then(|r| r.split(',').next())
                        .unwrap_or("");
                    if pid.is_empty() {
                        name.to_string()
                    } else {
                        format!("{name}[{pid}]")
                    }
                })
                .unwrap_or_default();
            entries.push((addr, port, owner));
        }
        SsSnapshot { entries }
    }
}

impl Listeners for SsSnapshot {
    fn check(&self, host: &str, port: u16) -> Option<String> {
        let h = host.trim_start_matches('[').trim_end_matches(']');
        let want_v4 = h == "localhost" || h == "0.0.0.0" || h.starts_with("127.");
        let want_v6 = h == "localhost" || h == "::1" || h == "::";
        self.entries
            .iter()
            .find(|(a, p, _)| {
                *p == port
                    && (a == "*"
                        || (want_v4
                            && (a == "0.0.0.0"
                                || a == h
                                || (h == "localhost" && a == "127.0.0.1")))
                        || (want_v6 && (a == "::" || a == "::1")))
            })
            .map(|(_, _, o)| o.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Open(String),
    Closed,
    NotChecked(String),
}

#[derive(Debug, Clone)]
pub struct RuleReport {
    pub rule: Rule,
    pub origin: Origin,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub rule: Option<usize>,
    pub msg: String,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub rules: Vec<RuleReport>,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn closed(&self) -> impl Iterator<Item = &RuleReport> {
        self.rules.iter().filter(|r| r.status == Status::Closed)
    }
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }
    /// Distinct closed ports, sorted.
    pub fn closed_ports(&self) -> Vec<u16> {
        let mut v: Vec<u16> = self
            .closed()
            .filter_map(|r| match r.origin {
                Origin::Local { port, .. } => Some(port),
                _ => None,
            })
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Run every check against a parsed rule list.
pub fn lint(rules: &[Rule], listeners: &dyn Listeners) -> Report {
    let mut findings = Vec::new();
    let mut out = Vec::new();
    let mut seen: HashSet<(Option<String>, Option<String>)> = HashSet::new();
    let mut catch_all: Option<usize> = None;

    for r in rules {
        let o = origin(&r.service);
        let status = match &o {
            Origin::Local { host, port } => match listeners.check(host, *port) {
                Some(owner) => Status::Open(owner),
                None => Status::Closed,
            },
            Origin::Remote { .. } => Status::NotChecked("remote origin".into()),
            Origin::Builtin(_) => Status::NotChecked("built-in".into()),
            Origin::Invalid(_) => Status::NotChecked("invalid".into()),
        };
        let host = r.hostname.as_deref().unwrap_or("*");
        match (&o, &status) {
            (Origin::Local { port, .. }, Status::Closed) => findings.push(Finding {
                severity: Severity::Error,
                rule: Some(r.index),
                msg: format!("{host} -> port {port}: nothing listens (cloudflared answers 502)"),
            }),
            (Origin::Invalid(s), _) => findings.push(Finding {
                severity: Severity::Error,
                rule: Some(r.index),
                msg: format!("{host}: service `{s}` is not a valid origin"),
            }),
            _ => {}
        }
        if let Some(c) = catch_all {
            findings.push(Finding {
                severity: Severity::Warn,
                rule: Some(r.index),
                msg: format!("{host}: unreachable, catch-all rule {c} comes first"),
            });
        }
        if r.hostname.is_none() && r.path.is_none() && catch_all.is_none() {
            catch_all = Some(r.index);
        }
        if !seen.insert((r.hostname.clone(), r.path.clone())) {
            findings.push(Finding {
                severity: Severity::Warn,
                rule: Some(r.index),
                msg: format!("{host}: duplicate hostname/path, shadowed by an earlier rule"),
            });
        }
        out.push(RuleReport {
            rule: r.clone(),
            origin: o,
            status,
        });
    }
    match rules.last() {
        Some(l) if l.hostname.is_none() && l.path.is_none() => {}
        _ => findings.push(Finding {
            severity: Severity::Error,
            rule: rules.last().map(|r| r.index),
            msg: "last ingress rule must be a catch-all (no hostname, no path)".into(),
        }),
    }
    Report {
        rules: out,
        findings,
    }
}

/// Produce a staged copy of `text` with every closed-port rule commented out,
/// so those hostnames fall through to the catch-all instead of a 502 and an
/// error log line per request. The input is not modified.
pub fn stage(text: &str, report: &Report) -> String {
    // line -> (port, first line of its rule)
    let mut dead: BTreeMap<usize, (u16, bool)> = BTreeMap::new();
    for r in report.closed() {
        // Never comment out the catch-all: cloudflared refuses a config without one.
        if r.rule.hostname.is_none() && r.rule.path.is_none() {
            continue;
        }
        if let Origin::Local { port, .. } = r.origin {
            for l in r.rule.lines.0..=r.rule.lines.1 {
                dead.insert(l, (port, l == r.rule.lines.0));
            }
        }
    }
    let mut out = String::with_capacity(text.len() + dead.len() * 32);
    for (i, line) in text.lines().enumerate() {
        if let Some((p, first)) = dead.get(&i) {
            if *first {
                out.push_str(&format!("# tunnel-lint: closed port {p}\n"));
            }
            out.push_str("# ");
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Whether the file mode lets any user rewrite the tunnel routing.
pub fn world_writable(mode: u32) -> bool {
    mode & 0o002 != 0
}
