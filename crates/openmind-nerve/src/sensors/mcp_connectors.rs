// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `mcp-connectors` (t460u, 2026-10-30 CET): the nerve watches the Hermes
//! MCP connectors, one read-only check per server in the hermes-rs servers
//! file (`/etc/hermes-rs/mcp-servers.yaml`).
//!
//! No model calls and no MCP calls: hermes-rs a318ea93 has no unauthenticated
//! status endpoint (`/health/detailed` needs the gateway key, a secret), so
//! the check reads only
//! - the servers file (names, `command`/`url`, `env_file`, `lazy`, `enabled`),
//! - the token file's *metadata* (exists, size, mtime, mode); the nerve never
//!   opens it, so a token value can never be read or logged,
//! - `systemctl show <hermes unit>` (MainPID, ActiveState, start time),
//! - the hermes-rs journal lines `mcp: <name>: N tools (...)` and
//!   `mcp: <name>: NOT CONNECTED: <error>` (startup discovery report),
//! - for non-lazy stdio servers, whether a child of the Hermes main PID runs
//!   the configured command (`/proc/<pid>/task/*/children`, then each child's
//!   `exe` link and argv[0] only; never `environ`).
//!
//! States: `configured` (in the file, not live yet), `waiting` (token file
//! missing: not an alert), `starting` (Hermes started moments ago, no report
//! yet), `healthy` (reported N tools for the running Hermes), `failing`
//! (reported an error, or its process exited, or Hermes is down).
//!
//! Alerts: a server that was `healthy` (in the journal look-back window or
//! since the nerve started) and is now `failing` while Hermes runs gives a
//! warn finding plus a warn [`Signal`] (cause `mcp-connector:<name>`), so the
//! normal incident path scores it and alerts once per incident id (the same
//! dedupe as every other nerve alert). A server that never worked and fails
//! is an info finding (setup, no alert). `waiting` never alerts.
//!
//! The full table is served at `GET /api/v1/connectors` (refreshed at most
//! every `NERVE_MCP_TTL_S`), in `/state` as `connectors` and, summarised, as
//! `system.connectors` (so `stack_health` and the console see it unchanged).
//!
//! Env (sensor OFF unless `NERVE_MCP_SERVERS_FILE` is an absolute path):
//! `NERVE_MCP_UNIT` (`hermes-rs.service`), `NERVE_MCP_TTL_S` (10),
//! `NERVE_MCP_START_GRACE_S` (120), `NERVE_MCP_LOOKBACK_H` (24),
//! `NERVE_MCP_TIMEOUT_MS` (5000), and for tests `NERVE_MCP_SYSTEMCTL`,
//! `NERVE_MCP_JOURNALCTL` (absolute paths) and `NERVE_MCP_PROC` (`/proc`).

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Sensor name, B33 source and `NERVE_SENSORS_OFF` key.
pub const NAME: &str = "mcp-connectors";
/// Incident cause prefix (`mcp-connector:<server>`).
pub const CAUSE_PREFIX: &str = "mcp-connector";
/// Largest child stdout kept (journal look-back included).
pub const MAX_OUT: u64 = 2 * 1024 * 1024;
const CHILD_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const MAX_ERR: usize = 240;

/// One server entry of the servers file (no secret-bearing keys are kept).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServerCfg {
    pub name: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub env_file: Option<String>,
    pub lazy: bool,
    pub enabled: bool,
}

impl ServerCfg {
    pub fn transport(&self) -> &'static str {
        if self.url.is_some() && self.command.is_none() {
            "http"
        } else {
            "stdio"
        }
    }
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn strip_comment(s: &str) -> &str {
    // `#` starts a comment at line start or after whitespace, outside quotes.
    let (mut q, b) = (None::<u8>, s.as_bytes());
    for (i, &c) in b.iter().enumerate() {
        match (q, c) {
            (None, b'"' | b'\'') => q = Some(c),
            (Some(o), _) if o == c => q = None,
            (None, b'#') if i == 0 || b[i - 1].is_ascii_whitespace() => return &s[..i],
            _ => {}
        }
    }
    s
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(t);
    t.to_string()
}

/// Parse the `mcp_servers:` map of a hermes-rs servers file (block style,
/// the format hermes-rs and the console write). Only the keys the check
/// needs are kept; `headers`/`env` values are never looked at.
pub fn parse_servers(text: &str) -> (Vec<ServerCfg>, Vec<String>) {
    let (mut out, mut warn) = (Vec::<ServerCfg>::new(), Vec::new());
    let mut inside = false;
    let (mut srv_indent, mut key_indent) = (None::<usize>, None::<usize>);
    let mut found = false;
    for raw in text.lines() {
        let line = strip_comment(raw).trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            inside = line == "mcp_servers:";
            found |= inside;
            srv_indent = None;
            continue;
        }
        if !inside {
            continue;
        }
        let body = line.trim_start();
        let si = *srv_indent.get_or_insert(indent);
        if indent < si {
            continue;
        }
        if indent == si {
            key_indent = None;
            match body.strip_suffix(':').map(unquote) {
                Some(n) if valid_name(&n) => out.push(ServerCfg {
                    name: n,
                    enabled: true,
                    ..Default::default()
                }),
                _ => warn.push("servers file: an entry name is not a plain name; skipped".into()),
            }
            continue;
        }
        let ki = *key_indent.get_or_insert(indent);
        if indent != ki {
            continue;
        }
        let Some(cur) = out.last_mut() else { continue };
        let Some((k, v)) = body.split_once(':') else {
            continue;
        };
        let v = unquote(v);
        let some = |v: String| (!v.is_empty()).then_some(v);
        match k.trim() {
            "command" => cur.command = some(v),
            "url" => cur.url = some(v),
            "env_file" | "secrets_file" => cur.env_file = some(v),
            "lazy" => cur.lazy = v == "true",
            "enabled" => cur.enabled = v != "false",
            _ => {}
        }
    }
    if !found {
        warn.push("servers file has no top-level mcp_servers: map".into());
    }
    out.retain(|s| s.enabled);
    (out, warn)
}

/// What Hermes said about one server at start-up.
#[derive(Debug, Clone, PartialEq)]
pub enum ReportKind {
    Tools { n: u64, gated: u64 },
    NotConnected(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub t: u64,
    pub pid: u32,
    pub name: String,
    pub kind: ReportKind,
}

/// One `journalctl -o short-unix` line: `<unix.frac> <host> <ident>[<pid>]: <msg>`.
pub fn parse_journal_line(line: &str) -> Option<Report> {
    let mut it = line.splitn(3, ' ');
    let ts = it.next()?;
    let _host = it.next()?;
    let rest = it.next()?;
    let t = ts.split('.').next()?.parse::<u64>().ok()?;
    let (ident, msg) = rest.split_once(": ")?;
    let pid = ident.rsplit_once('[')?.1.strip_suffix(']')?.parse().ok()?;
    let at = msg.find("mcp: ")?;
    let (name, what) = msg[at + 5..].split_once(": ")?;
    if !valid_name(name) || name == "WARNING" {
        return None;
    }
    let kind = if let Some(e) = what.strip_prefix("NOT CONNECTED: ") {
        ReportKind::NotConnected(sanitize(e))
    } else {
        let (n, tail) = what.split_once(" tools")?;
        let n = n.trim().parse().ok()?;
        let gated = tail
            .trim_start()
            .strip_prefix('(')
            .and_then(|x| x.split_whitespace().next())
            .and_then(|x| x.parse().ok())
            .unwrap_or(0);
        ReportKind::Tools { n, gated }
    };
    Some(Report {
        t,
        pid,
        name: name.to_string(),
        kind,
    })
}

/// Error text safe to show: control characters dropped, token-like words
/// and non-loopback IPv4 addresses replaced, capped at 240 characters.
pub fn sanitize(s: &str) -> String {
    let words: Vec<String> = s
        .split(' ')
        .map(|w| {
            let core = w.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')'));
            let tokenish = [
                "ghp_",
                "gho_",
                "ghu_",
                "ghs_",
                "ghr_",
                "github_pat_",
                "sk-",
                "xox",
            ]
            .iter()
            .any(|p| core.starts_with(p))
                || (core.len() >= 32
                    && !core.contains('/')
                    && core
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '=')));
            if tokenish {
                "[redacted]".to_string()
            } else if is_public_ipv4ish(core) {
                "[addr]".to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    let mut j = words.join(" ");
    for marker in ["Bearer ", "bearer ", "token=", "TOKEN="] {
        while let Some(i) = j.find(marker) {
            let end = j[i + marker.len()..]
                .find(char::is_whitespace)
                .map(|e| i + marker.len() + e)
                .unwrap_or(j.len());
            j.replace_range(i..end, "[redacted]");
        }
    }
    let clean: String = j.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() > MAX_ERR {
        let mut c: String = clean.chars().take(MAX_ERR).collect();
        c.push('…');
        c
    } else {
        clean
    }
}

fn is_public_ipv4ish(w: &str) -> bool {
    let host = w.split(['/', ':']).find(|p| p.contains('.')).unwrap_or(w);
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
        && !host.starts_with("127.")
        && host != "0.0.0.0"
}

/// The Hermes unit as `systemctl show` reports it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hermes {
    pub active: String,
    pub pid: u32,
    pub started: u64,
}

pub fn parse_show(out: &str) -> Hermes {
    let mut h = Hermes::default();
    for l in out.lines() {
        if let Some((k, v)) = l.split_once('=') {
            match k {
                "ActiveState" => h.active = v.trim().to_string(),
                "MainPID" => h.pid = v.trim().parse().unwrap_or(0),
                "ExecMainStartTimestamp" => {
                    h.started = v.trim().trim_start_matches('@').parse().unwrap_or(0)
                }
                _ => {}
            }
        }
    }
    h
}

/// Token file metadata (never its content).
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    NotNeeded,
    Missing,
    Present {
        mtime: u64,
        empty: bool,
        private: bool,
    },
}

impl Token {
    fn label(&self) -> &'static str {
        match self {
            Token::NotNeeded => "not-needed",
            Token::Missing => "missing",
            Token::Present { .. } => "present",
        }
    }
}

pub fn token_meta(env_file: Option<&str>) -> Token {
    use std::os::unix::fs::MetadataExt;
    let Some(p) = env_file else {
        return Token::NotNeeded;
    };
    match std::fs::metadata(p) {
        Ok(m) => Token::Present {
            mtime: m.mtime().max(0) as u64,
            empty: m.len() == 0,
            private: m.mode() & 0o077 == 0,
        },
        Err(_) => Token::Missing,
    }
}

/// Everything one assessment needs (gathered by [`Tracker::refresh`] or a test).
pub struct Facts<'a> {
    pub now: u64,
    pub hermes: &'a Hermes,
    pub cfg_mtime: u64,
    pub start_grace_s: u64,
    pub start_grace_known: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    Healthy,
    Configured,
    Waiting,
    Starting,
    Failing,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Healthy => "healthy",
            State::Configured => "configured",
            State::Waiting => "waiting",
            State::Starting => "starting",
            State::Failing => "failing",
        }
    }
}

/// One server's verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub name: String,
    pub state: State,
    pub tools: Option<u64>,
    pub gated: Option<u64>,
    pub last_error: Option<String>,
    pub note: Option<String>,
    pub token: Token,
    pub lazy: bool,
    pub transport: &'static str,
    pub ever_healthy: Option<u64>,
    pub since: u64,
    pub hermes_down: bool,
}

impl Status {
    /// Failing after having been healthy while Hermes runs: the alert case.
    pub fn alerting(&self) -> bool {
        self.state == State::Failing && self.ever_healthy.is_some() && !self.hermes_down
    }
}

fn waiting_error(e: &str) -> bool {
    e.contains("waiting for secret") || (e.contains("secrets file") && e.contains("No such file"))
}

/// Judge one server (pure).
pub fn assess(
    s: &ServerCfg,
    f: &Facts,
    last: Option<&Report>,
    token: Token,
    child_alive: Option<bool>,
    ever_healthy: Option<u64>,
) -> Status {
    let mut st = Status {
        name: s.name.clone(),
        state: State::Configured,
        tools: None,
        gated: None,
        last_error: None,
        note: None,
        token: token.clone(),
        lazy: s.lazy,
        transport: s.transport(),
        ever_healthy,
        since: f.now,
        hermes_down: false,
    };
    let h = f.hermes;
    let token_after_start =
        matches!(token, Token::Present { mtime, .. } if h.started > 0 && mtime > h.started);
    if h.active != "active" {
        if h.active == "activating" || h.active == "reloading" {
            st.state = State::Starting;
            st.note = Some(format!("hermes-rs is {}", h.active));
        } else {
            st.state = State::Failing;
            st.hermes_down = true;
            st.last_error = Some(format!(
                "hermes-rs is {}",
                if h.active.is_empty() {
                    "unknown"
                } else {
                    h.active.as_str()
                }
            ));
        }
        return st;
    }
    if token == Token::Missing {
        st.state = State::Waiting;
        st.note = Some(
            "token file not present yet (connect it in the console, then restart Hermes)".into(),
        );
        return st;
    }
    let current = last.filter(|r| r.pid == h.pid && h.pid != 0);
    match current.map(|r| &r.kind) {
        Some(ReportKind::Tools { n, gated }) => {
            st.tools = Some(*n);
            st.gated = Some(*gated);
            st.state = State::Healthy;
            if s.transport() == "stdio" && !s.lazy && child_alive == Some(false) {
                st.state = State::Failing;
                st.last_error = Some(
                    "server process is no longer running (Hermes restarts it on the next tool call)"
                        .into(),
                );
            } else if token_after_start {
                st.note = Some("token file changed after Hermes started; Hermes keeps the token it read at start until its next restart".into());
            }
        }
        Some(ReportKind::NotConnected(e)) if waiting_error(e) => {
            if token_after_start {
                st.state = State::Configured;
                st.note = Some(
                    "token saved after Hermes started; live after the next Hermes restart".into(),
                );
            } else {
                st.state = State::Waiting;
                st.note = Some("Hermes reported the token missing".into());
                st.last_error = Some(e.clone());
            }
        }
        Some(ReportKind::NotConnected(e)) => {
            st.state = State::Failing;
            st.last_error = Some(e.clone());
            if token_after_start {
                st.note = Some("a new token was saved after this error; it is used after the next Hermes restart".into());
            }
        }
        None => {
            if f.now.saturating_sub(h.started) < f.start_grace_s || !f.start_grace_known {
                st.state = State::Starting;
                st.note = Some("Hermes started moments ago; waiting for its MCP report".into());
            } else if f.cfg_mtime > h.started {
                st.state = State::Configured;
                st.note = Some(
                    "added or changed after Hermes started; live after the next Hermes restart"
                        .into(),
                );
            } else {
                st.state = State::Configured;
                st.note =
                    Some("no MCP start-up report from the running Hermes for this server".into());
            }
            if let Some(Report {
                kind: ReportKind::NotConnected(e),
                ..
            }) = last
            {
                st.last_error = Some(e.clone());
            }
        }
    }
    if st.state != State::Healthy && st.last_error.is_none() {
        if let Some(c) = s.command.as_deref().filter(|c| !Path::new(c).exists()) {
            st.last_error = Some(format!("command {c} not found"));
        }
    }
    st
}

/// Run `argv` (no shell) with an empty environment plus a fixed PATH,
/// stdin closed, stderr discarded, stdout capped; killed after `timeout`.
pub fn run_capped(argv: &[OsString], timeout: Duration) -> Result<(Option<i32>, String), String> {
    let (bin, args) = argv.split_first().ok_or("empty argv")?;
    let mut child = Command::new(bin)
        .args(args)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .env("SYSTEMD_COLORS", "0")
        .env("SYSTEMD_PAGER", "")
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn failed ({:?})", e.kind()))?;
    let mut out = child.stdout.take().ok_or("no stdout pipe")?;
    let reader = std::thread::Builder::new()
        .name("nerve-mcp-out".into())
        .spawn(move || {
            let mut kept = Vec::new();
            let _ = (&mut out).take(MAX_OUT).read_to_end(&mut kept);
            let _ = std::io::copy(&mut out, &mut std::io::sink());
            kept
        })
        .map_err(|_| "reader thread failed".to_string())?;
    let t0 = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Ok(st),
            Ok(None) if t0.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("timeout after {} ms (killed)", timeout.as_millis()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("wait failed ({:?})", e.kind()));
            }
        }
    };
    let kept = reader.join().unwrap_or_default();
    let st = status?;
    Ok((st.code(), String::from_utf8_lossy(&kept).into_owned()))
}

/// Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub servers_file: PathBuf,
    pub unit: String,
    pub systemctl: PathBuf,
    pub journalctl: PathBuf,
    pub proc_dir: PathBuf,
    pub ttl_s: u64,
    pub start_grace_s: u64,
    pub lookback_h: u64,
    pub timeout: Duration,
}

fn valid_unit(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'@'))
}

impl Settings {
    pub fn from_env(env: Env) -> Option<Result<Self, String>> {
        let file = env("NERVE_MCP_SERVERS_FILE").filter(|s| !s.trim().is_empty())?;
        let num = |k: &str, d: u64| env(k).and_then(|v| v.trim().parse().ok()).unwrap_or(d);
        let text = |k: &str, d: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| d.to_string())
        };
        let s = Settings {
            servers_file: PathBuf::from(file.trim()),
            unit: text("NERVE_MCP_UNIT", "hermes-rs.service"),
            systemctl: PathBuf::from(text("NERVE_MCP_SYSTEMCTL", "/usr/bin/systemctl")),
            journalctl: PathBuf::from(text("NERVE_MCP_JOURNALCTL", "/usr/bin/journalctl")),
            proc_dir: PathBuf::from(text("NERVE_MCP_PROC", "/proc")),
            ttl_s: num("NERVE_MCP_TTL_S", 10).clamp(1, 3600),
            start_grace_s: num("NERVE_MCP_START_GRACE_S", 120).clamp(5, 3600),
            lookback_h: num("NERVE_MCP_LOOKBACK_H", 24).clamp(1, 24 * 14),
            timeout: Duration::from_millis(num("NERVE_MCP_TIMEOUT_MS", 5000).clamp(100, 60_000)),
        };
        for (k, p) in [
            ("NERVE_MCP_SERVERS_FILE", &s.servers_file),
            ("NERVE_MCP_SYSTEMCTL", &s.systemctl),
            ("NERVE_MCP_JOURNALCTL", &s.journalctl),
            ("NERVE_MCP_PROC", &s.proc_dir),
        ] {
            if !p.is_absolute() {
                return Some(Err(format!("{k} must be an absolute path")));
            }
        }
        if !valid_unit(&s.unit) {
            return Some(Err("NERVE_MCP_UNIT must be a plain unit name".into()));
        }
        Some(Ok(s))
    }
}

/// Per-server memory across refreshes.
#[derive(Debug, Clone, Default)]
struct Seen {
    last: Option<Report>,
    ever_healthy: Option<u64>,
    state: Option<(State, u64)>,
}

/// The connector watcher: journal cursor, per-server history, last table.
pub struct Tracker {
    pub settings: Settings,
    seen: BTreeMap<String, Seen>,
    journal_upto: u64,
    pub snapshot: Option<(u64, Vec<Status>, Value)>,
    pub refreshes: u64,
}

fn mtime_of(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The child processes of `pid` (all threads), each as (exe link, argv[0]).
pub fn children(proc_dir: &Path, pid: u32) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(tasks) = std::fs::read_dir(proc_dir.join(pid.to_string()).join("task")) else {
        return out;
    };
    for t in tasks.flatten() {
        let Ok(list) = std::fs::read_to_string(t.path().join("children")) else {
            continue;
        };
        for c in list
            .split_whitespace()
            .filter_map(|c| c.parse::<u32>().ok())
        {
            let base = proc_dir.join(c.to_string());
            let exe = std::fs::read_link(base.join("exe"))
                .map(|p| {
                    p.to_string_lossy()
                        .trim_end_matches(" (deleted)")
                        .to_string()
                })
                .unwrap_or_default();
            let argv0 = std::fs::read(base.join("cmdline"))
                .ok()
                .and_then(|b| {
                    b.split(|&x| x == 0)
                        .next()
                        .map(|a| String::from_utf8_lossy(a).into_owned())
                })
                .unwrap_or_default();
            out.push((exe, argv0));
        }
    }
    out
}

impl Tracker {
    pub fn new(settings: Settings) -> Self {
        Tracker {
            settings,
            seen: BTreeMap::new(),
            journal_upto: 0,
            snapshot: None,
            refreshes: 0,
        }
    }

    /// Feed journal lines (any order); keeps the newest report per server
    /// and the newest healthy time.
    pub fn feed(&mut self, lines: &str) {
        for r in lines.lines().filter_map(parse_journal_line) {
            self.journal_upto = self.journal_upto.max(r.t);
            let e = self.seen.entry(r.name.clone()).or_default();
            if matches!(r.kind, ReportKind::Tools { .. }) {
                e.ever_healthy = Some(e.ever_healthy.map_or(r.t, |t| t.max(r.t)));
            }
            if e.last.as_ref().is_none_or(|l| r.t >= l.t) {
                e.last = Some(r);
            }
        }
    }

    /// Fresh table unless the last one is younger than the TTL.
    pub fn get(&mut self, now: u64) -> (Vec<Status>, Value) {
        match &self.snapshot {
            Some((t, s, v)) if now.saturating_sub(*t) < self.settings.ttl_s => {
                (s.clone(), v.clone())
            }
            _ => self.refresh(now),
        }
    }

    /// Read the host (read-only) and judge every server.
    pub fn refresh(&mut self, now: u64) -> (Vec<Status>, Value) {
        self.refreshes += 1;
        let st = self.settings.clone();
        let mut errors: Vec<String> = Vec::new();
        let (servers, warn) = match std::fs::read_to_string(&st.servers_file) {
            Ok(t) => parse_servers(&t),
            Err(e) => (
                vec![],
                vec![format!("servers file unreadable ({:?})", e.kind())],
            ),
        };
        errors.extend(warn);
        let show = run_capped(
            &[
                st.systemctl.clone().into(),
                "show".into(),
                st.unit.clone().into(),
                "-p".into(),
                "MainPID".into(),
                "-p".into(),
                "ActiveState".into(),
                "-p".into(),
                "ExecMainStartTimestamp".into(),
                "--timestamp=unix".into(),
            ],
            st.timeout,
        );
        let hermes = match show {
            Ok((Some(0), out)) => parse_show(&out),
            Ok((c, _)) => {
                errors.push(format!("systemctl show exit {c:?}"));
                Hermes::default()
            }
            Err(e) => {
                errors.push(format!("systemctl show: {e}"));
                Hermes::default()
            }
        };
        let floor = now.saturating_sub(st.lookback_h * 3600);
        let since = if self.journal_upto == 0 {
            floor
        } else {
            self.journal_upto.max(floor)
        };
        match run_capped(
            &[
                st.journalctl.clone().into(),
                "-u".into(),
                st.unit.clone().into(),
                "-o".into(),
                "short-unix".into(),
                "--no-pager".into(),
                "-q".into(),
                format!("--since=@{since}").into(),
            ],
            st.timeout,
        ) {
            Ok((Some(0), out)) => self.feed(&out),
            Ok((c, _)) => errors.push(format!("journalctl exit {c:?}")),
            Err(e) => errors.push(format!("journalctl: {e}")),
        }
        let kids = if hermes.pid > 0 {
            children(&st.proc_dir, hermes.pid)
        } else {
            vec![]
        };
        let facts = Facts {
            now,
            hermes: &hermes,
            cfg_mtime: mtime_of(&st.servers_file),
            start_grace_s: st.start_grace_s,
            start_grace_known: hermes.started > 0,
        };
        let mut out = Vec::new();
        for s in &servers {
            let seen = self.seen.entry(s.name.clone()).or_default();
            let alive = s
                .command
                .as_ref()
                .map(|c| kids.iter().any(|(exe, a0)| exe == c || a0 == c));
            let mut v = assess(
                s,
                &facts,
                seen.last.as_ref(),
                token_meta(s.env_file.as_deref()),
                alive,
                seen.ever_healthy,
            );
            if v.state == State::Healthy {
                seen.ever_healthy = Some(now);
                v.ever_healthy = Some(now);
            }
            v.since = match seen.state {
                Some((prev, t)) if prev == v.state => t,
                _ => now,
            };
            seen.state = Some((v.state, v.since));
            out.push(v);
        }
        let json = table_json(now, &st, &hermes, &out, &errors);
        self.snapshot = Some((now, out.clone(), json.clone()));
        (out, json)
    }
}

fn ts(t: u64) -> Value {
    if t == 0 {
        Value::Null
    } else {
        crate::util::ts(t).into()
    }
}

/// The `/api/v1/connectors` document.
pub fn table_json(
    now: u64,
    st: &Settings,
    h: &Hermes,
    list: &[Status],
    errors: &[String],
) -> Value {
    let mut counts = BTreeMap::new();
    for s in ["healthy", "configured", "waiting", "starting", "failing"] {
        counts.insert(s, 0u64);
    }
    for s in list {
        *counts.entry(s.state.as_str()).or_default() += 1;
    }
    let worst = list.iter().map(|s| s.state).max().map(State::as_str);
    let tools: u64 = list
        .iter()
        .filter(|s| s.state == State::Healthy)
        .filter_map(|s| s.tools)
        .sum();
    let servers: Vec<Value> = list
        .iter()
        .map(|s| {
            json!({"name": s.name, "state": s.state.as_str(), "tools": s.tools, "write_tools_gated": s.gated,
                   "last_error": s.last_error, "note": s.note, "token": s.token.label(),
                   "token_private": match s.token { Token::Present { private, .. } => Value::from(private), _ => Value::Null },
                   "token_empty": match s.token { Token::Present { empty, .. } => Value::from(empty), _ => Value::Null },
                   "transport": s.transport, "lazy": s.lazy, "ever_healthy": ts(s.ever_healthy.unwrap_or(0)),
                   "since": ts(s.since), "alerting": s.alerting()})
        })
        .collect();
    json!({
        "enabled": true,
        "checked": ts(now),
        "source": "servers file + token-file presence (metadata only) + hermes-rs journal + /proc children; no model or MCP calls",
        "servers_file": st.servers_file.to_string_lossy(),
        "hermes": {"unit": st.unit, "active": h.active, "pid": h.pid, "started": ts(h.started)},
        "summary": {"servers": list.len(), "counts": counts, "tools": tools, "worst": worst,
                    "alerting": list.iter().filter(|s| s.alerting()).count()},
        "servers": servers,
        "errors": errors,
    })
}

/// Findings and signals for one table (pure).
pub fn report(list: &[Status]) -> (Vec<Finding>, Vec<Signal>) {
    let (mut fs, mut ss) = (Vec::new(), Vec::new());
    for s in list {
        if s.state != State::Failing || s.hermes_down {
            continue;
        }
        let comp = format!("hermes-mcp:{}", s.name);
        let cause = format!("{CAUSE_PREFIX}:{}", s.name);
        let ev = vec![
            format!("state={}", s.state.as_str()),
            format!("last_error={}", s.last_error.as_deref().unwrap_or("none")),
            format!(
                "ever_healthy={}",
                s.ever_healthy
                    .map(crate::util::ts)
                    .unwrap_or_else(|| "never".into())
            ),
        ];
        if s.alerting() {
            let title = format!("MCP connector '{}' is failing after it was healthy", s.name);
            fs.push(
                Finding::new(FindingKind::Bleeding, format!("mcp-connector-failing-{}", s.name), Sev::Warn, comp.clone(), title.clone())
                    .with_evidence(ev)
                    .with_fix("see GET /api/v1/connectors for the last error; fix the server or its token, then restart hermes-rs (owner); the nerve only reports")
                    .with_cause(cause.clone()),
            );
            ss.push(Signal::new(&cause, &comp, Sev::Warn, title));
        } else {
            fs.push(
                Finding::new(
                    FindingKind::Bleeding,
                    format!("mcp-connector-setup-{}", s.name),
                    Sev::Info,
                    comp,
                    format!("MCP connector '{}' has not worked yet", s.name),
                )
                .with_evidence(ev)
                .with_cause(cause),
            );
        }
    }
    (fs, ss)
}

static LIVE: OnceLock<Arc<Mutex<Tracker>>> = OnceLock::new();

/// The live table for `GET /api/v1/connectors` (None: the sensor is off or
/// has not had its first live cycle yet).
pub fn live_json(now: u64) -> Option<Value> {
    let t = LIVE.get()?;
    let mut g = t.lock().unwrap_or_else(|e| e.into_inner());
    Some(g.get(now).1)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Sensor `mcp-connectors`.
pub struct McpConnectors {
    pub tracker: Option<Arc<Mutex<Tracker>>>,
    pub bad: Option<String>,
    /// Publish this tracker for the HTTP API on the first live cycle.
    pub publish: bool,
}

impl McpConnectors {
    pub fn new(settings: Settings) -> Self {
        McpConnectors {
            tracker: Some(Arc::new(Mutex::new(Tracker::new(settings)))),
            bad: None,
            publish: true,
        }
    }

    /// Off (None) unless `NERVE_MCP_SERVERS_FILE` is set and non-empty.
    pub fn from_env(env: Env) -> Option<Self> {
        Some(match Settings::from_env(env)? {
            Ok(s) => Self::new(s),
            Err(why) => McpConnectors {
                tracker: None,
                bad: Some(why),
                publish: false,
            },
        })
    }
}

impl Sensor for McpConnectors {
    fn name(&self) -> &'static str {
        NAME
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return;
        }
        let Some(t) = &self.tracker else {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "mcp-connectors-note",
                    Sev::Info,
                    "hermes-rs",
                    format!(
                        "mcp-connectors sensor not run: {}",
                        self.bad.as_deref().unwrap_or("bad settings")
                    ),
                )
                .with_cause(CAUSE_PREFIX),
            );
            return;
        };
        if self.publish {
            self.publish = false;
            let _ = LIVE.set(t.clone());
        }
        let now = if i.now > 0 { i.now } else { now_secs() };
        let (list, json) = t.lock().unwrap_or_else(|e| e.into_inner()).get(now);
        let (fs, ss) = report(&list);
        for f in fs {
            out.finding(f);
        }
        for s in ss {
            out.signal(s);
        }
        out.connectors(json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"# Hermes MCP servers file
# Add a server (3 lines):
#   weather:
#     command: /opt/galion/rust/bin/weather-mcp
mcp_servers:
  openmind:
    # comment
    command: /opt/galion/rust/bin/openmind-mcp-stack-498244ec
    args: ["--vault", "/x", "--memory-url", "http://127.0.0.1:8420"]
    env_file: /etc/hermes-rs/mcp.d/openmind.env
    class: read
    lazy: false
    env:
      lazy: true
  github:
    command: /opt/galion/rust/bin/github-mcp-rs-a318ea93
    env_file: "/etc/hermes-rs/mcp.d/github.env"
    headers: {Authorization: "Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}"}
    lazy: true # comment
  remote:
    url: https://mcp.example.com/mcp
  off:
    command: /bin/false
    enabled: false
"#;

    #[test]
    fn parses_the_live_servers_file_shape() {
        let (s, w) = parse_servers(FILE);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            s.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["openmind", "github", "remote"]
        );
        assert_eq!(
            s[0].command.as_deref(),
            Some("/opt/galion/rust/bin/openmind-mcp-stack-498244ec")
        );
        assert!(!s[0].lazy, "nested env: lazy must not leak");
        assert_eq!(
            s[1].env_file.as_deref(),
            Some("/etc/hermes-rs/mcp.d/github.env")
        );
        assert!(s[1].lazy);
        assert_eq!((s[2].transport(), s[0].transport()), ("http", "stdio"));
        assert!(!parse_servers("x: 1\n").1.is_empty());
    }

    #[test]
    fn parses_hermes_report_lines() {
        let ok = "1793362218.123456 ip50 hermes-gateway[3348699]: 2026-10-30T13:10:18+01:00 hermes-gateway mcp: openmind: 18 tools (0 write tools gated off)";
        let r = parse_journal_line(ok).unwrap();
        assert_eq!(
            (r.t, r.pid, r.name.as_str()),
            (1793362218, 3348699, "openmind")
        );
        assert_eq!(r.kind, ReportKind::Tools { n: 18, gated: 0 });
        let nc = "1793362218.2 ip50 hermes-gateway[3348699]: 2026-10-30T13:10:18+01:00 hermes-gateway mcp: github: NOT CONNECTED: secrets file /etc/hermes-rs/mcp.d/github.env: No such file or directory (os error 2)";
        assert!(
            matches!(parse_journal_line(nc).unwrap().kind, ReportKind::NotConnected(e) if waiting_error(&e))
        );
        for no in [
            "1793362218.2 h hermes-gateway[1]: x hermes-gateway mcp: 18 tools",
            "1793362218.2 h hermes-gateway[1]: x hermes-gateway mcp: WARNING: NOT LOADED: y",
            "garbage",
        ] {
            assert_eq!(parse_journal_line(no), None, "{no}");
        }
    }

    #[test]
    fn sanitize_drops_tokens_and_addresses() {
        let e = sanitize("HTTP 401 for Bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789 at 203.0.113.9:443 token=xyz github_pat_11ABC and http://127.0.0.1:8420 ok");
        for leak in ["ghp_", "203.0.113.9", "xyz", "github_pat_"] {
            assert!(!e.contains(leak), "{leak}: {e}");
        }
        assert!(e.contains("127.0.0.1:8420"), "{e}");
        assert!(sanitize(&"a ".repeat(400)).chars().count() <= MAX_ERR + 1);
    }

    fn cfg(name: &str, lazy: bool, env_file: Option<&str>) -> ServerCfg {
        ServerCfg {
            name: name.into(),
            command: Some(format!("/bin/{name}")),
            url: None,
            env_file: env_file.map(String::from),
            lazy,
            enabled: true,
        }
    }

    fn rep(pid: u32, t: u64, kind: ReportKind) -> Report {
        Report {
            t,
            pid,
            name: "x".into(),
            kind,
        }
    }

    #[test]
    fn states_cover_every_case() {
        let h = Hermes {
            active: "active".into(),
            pid: 10,
            started: 1000,
        };
        let f = Facts {
            now: 2000,
            hermes: &h,
            cfg_mtime: 500,
            start_grace_s: 120,
            start_grace_known: true,
        };
        let present = Token::Present {
            mtime: 900,
            empty: false,
            private: true,
        };
        let tools = rep(10, 1001, ReportKind::Tools { n: 18, gated: 0 });
        let s = cfg("om", false, Some("/x"));
        // healthy (child alive)
        let v = assess(&s, &f, Some(&tools), present.clone(), Some(true), None);
        assert_eq!((v.state, v.tools), (State::Healthy, Some(18)));
        // healthy then the process vanished: failing + alerting
        let v = assess(
            &s,
            &f,
            Some(&tools),
            present.clone(),
            Some(false),
            Some(1001),
        );
        assert_eq!(v.state, State::Failing);
        assert!(v.alerting());
        // lazy servers have no child: still healthy
        let v = assess(
            &cfg("gh", true, None),
            &f,
            Some(&tools),
            Token::NotNeeded,
            Some(false),
            None,
        );
        assert_eq!(v.state, State::Healthy);
        // token missing: waiting, never alerting even if it was healthy
        let v = assess(&s, &f, Some(&tools), Token::Missing, Some(true), Some(1001));
        assert_eq!(v.state, State::Waiting);
        assert!(!v.alerting());
        // Hermes said waiting; token saved after start -> configured
        let wait = rep(
            10,
            1001,
            ReportKind::NotConnected(
                "secrets file /x: No such file or directory (os error 2)".into(),
            ),
        );
        let late = Token::Present {
            mtime: 1500,
            empty: false,
            private: true,
        };
        assert_eq!(
            assess(&s, &f, Some(&wait), late, None, None).state,
            State::Configured
        );
        assert_eq!(
            assess(&s, &f, Some(&wait), present.clone(), None, None).state,
            State::Waiting
        );
        // a real error: failing; alerting only if it was healthy before
        let bad = rep(
            10,
            1001,
            ReportKind::NotConnected("initialize: timeout".into()),
        );
        let v = assess(&s, &f, Some(&bad), present.clone(), None, None);
        assert_eq!((v.state, v.alerting()), (State::Failing, false));
        assert_eq!(v.last_error.as_deref(), Some("initialize: timeout"));
        assert!(assess(&s, &f, Some(&bad), present.clone(), None, Some(900)).alerting());
        // report from an older Hermes PID: starting within grace, configured after
        let old = rep(9, 900, ReportKind::Tools { n: 18, gated: 0 });
        let f2 = Facts { now: 1050, ..f };
        assert_eq!(
            assess(&s, &f2, Some(&old), present.clone(), None, None).state,
            State::Starting
        );
        assert_eq!(
            assess(&s, &f, Some(&old), present.clone(), None, None).state,
            State::Configured
        );
        // Hermes down: failing but not alerting (the component check covers it)
        let down = Hermes {
            active: "failed".into(),
            pid: 0,
            started: 1000,
        };
        let f3 = Facts { hermes: &down, ..f };
        let v = assess(&s, &f3, Some(&tools), present.clone(), None, Some(1001));
        assert_eq!(v.state, State::Failing);
        assert!(!v.alerting());
        let act = Hermes {
            active: "activating".into(),
            ..down
        };
        assert_eq!(
            assess(&s, &Facts { hermes: &act, ..f }, None, present, None, None).state,
            State::Starting
        );
    }

    #[test]
    fn only_healthy_then_failing_signals() {
        let mk = |state, ever: Option<u64>, down| Status {
            name: "om".into(),
            state,
            tools: None,
            gated: None,
            last_error: Some("boom".into()),
            note: None,
            token: Token::NotNeeded,
            lazy: false,
            transport: "stdio",
            ever_healthy: ever,
            since: 1,
            hermes_down: down,
        };
        let (fs, ss) = report(&[mk(State::Failing, Some(5), false)]);
        assert_eq!((fs.len(), ss.len(), fs[0].sev), (1, 1, Sev::Warn));
        assert_eq!(ss[0].cause, "mcp-connector:om");
        assert!(
            !ss[0].msg.contains("boom"),
            "no raw error in the alert title"
        );
        let (fs, ss) = report(&[mk(State::Failing, None, false)]);
        assert_eq!((fs.len(), ss.len(), fs[0].sev), (1, 0, Sev::Info));
        for st in [
            State::Waiting,
            State::Configured,
            State::Starting,
            State::Healthy,
        ] {
            assert_eq!(report(&[mk(st, Some(5), false)]), (vec![], vec![]));
        }
        assert_eq!(
            report(&[mk(State::Failing, Some(5), true)]),
            (vec![], vec![])
        );
    }

    #[test]
    fn off_by_default_and_validated() {
        assert!(McpConnectors::from_env(&|_| None).is_none());
        let on = |k: &str| {
            (k == "NERVE_MCP_SERVERS_FILE").then(|| "/etc/hermes-rs/mcp-servers.yaml".to_string())
        };
        let m = McpConnectors::from_env(&on).unwrap();
        assert!(m.bad.is_none());
        assert!(super::super::Sensors::from_env(&on).names().contains(&NAME));
        let rel = |k: &str| (k == "NERVE_MCP_SERVERS_FILE").then(|| "rel.yaml".to_string());
        assert!(McpConnectors::from_env(&rel).unwrap().bad.is_some());
        let unit = |k: &str| match k {
            "NERVE_MCP_SERVERS_FILE" => Some("/x".to_string()),
            "NERVE_MCP_UNIT" => Some("--all".to_string()),
            _ => None,
        };
        assert!(McpConnectors::from_env(&unit).unwrap().bad.is_some());
    }

    #[test]
    fn tracker_keeps_history_and_never_opens_token_files() {
        let d = std::env::temp_dir().join(format!("nerve-mcp-unit-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        // A token file the nerve may stat but must never read: mode 000.
        let tok = d.join("om.env");
        std::fs::write(&tok, "SECRET=do-not-read\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(matches!(
            token_meta(tok.to_str()),
            Token::Present {
                empty: false,
                private: true,
                ..
            }
        ));
        assert_eq!(token_meta(Some("/nonexistent/x.env")), Token::Missing);
        let mut t = Tracker::new(Settings {
            servers_file: d.join("none.yaml"),
            unit: "hermes-rs.service".into(),
            systemctl: "/nonexistent/systemctl".into(),
            journalctl: "/nonexistent/journalctl".into(),
            proc_dir: "/proc".into(),
            ttl_s: 10,
            start_grace_s: 120,
            lookback_h: 24,
            timeout: Duration::from_millis(500),
        });
        t.feed("100.1 h hermes-gateway[7]: x hermes-gateway mcp: om: 3 tools (0 write tools gated off)\n200.1 h hermes-gateway[8]: x hermes-gateway mcp: om: NOT CONNECTED: boom\n");
        assert_eq!(t.seen["om"].ever_healthy, Some(100));
        assert_eq!(t.seen["om"].last.as_ref().unwrap().pid, 8);
        let (list, v) = t.refresh(1000);
        assert!(list.is_empty());
        assert!(v["errors"].as_array().unwrap().len() >= 2, "{v}");
        assert!(!v.to_string().contains("do-not-read"));
        let _ = t.get(1005);
        assert_eq!(t.refreshes, 1, "TTL cache");
        std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o600)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
