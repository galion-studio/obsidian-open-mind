// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `hades` sensor (cortex plan T2.13): agent / approval misbehaviour.
//!
//! Watches what the HADES approval gate and the hermes-tools sandbox
//! already write, read-only, and reports denied or anomalous plans:
//!
//! | source | what | env |
//! |---|---|---|
//! | hades-gate ledger JSONL (`ledger.rs` `note`/`consume`: `{ts, event, reason, ..}`) | refusals by class | `NERVE_HADES_LEDGER` |
//! | hades-gate disable file | HADES switched off by the admin | `NERVE_HADES_DISABLE_FILE` |
//! | hermes-tools audit JSONL (`{ts, tool, decision, reason \| result.sandbox}`) | sandbox escape attempts, layers not enforced | `NERVE_HADES_AUDIT` (comma list) |
//! | `input.journal`, units named `hades*` | `ledger corrupt`/`ledger unavailable`, `stopped by admin request` | - |
//!
//! Files are read only when [`Input::live`] (tailed from the last offset,
//! at most [`MAX_READ`] bytes per file per cycle, last [`MAX_READ`] bytes on
//! the first read); the journal part also runs in replay. Counts are kept in
//! a sliding window (`NERVE_HADES_WINDOW_S`, default 900 s) keyed on each
//! line's own Zagreb `ts`.
//!
//! Only classes and counts leave the sensor: never a plan, target, token
//! hash, path, argument or reason text. The gate's reasons are matched to a
//! fixed class list (hades-gate `Refusal::reason`, unchanged from ccaab42
//! through the hardened swarm/hades-redteam e6ceca7).
//!
//! Off unless one of `NERVE_HADES_LEDGER`, `NERVE_HADES_DISABLE_FILE`,
//! `NERVE_HADES_AUDIT` is set or `NERVE_HADES=1` (journal only).

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Bytes read per file per cycle (and the tail read on first open).
pub const MAX_READ: u64 = 256 * 1024;
/// Window entries kept at most (oldest dropped first).
pub const MAX_EVENTS: usize = 20_000;

/// What one ledger / audit / journal line means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Admin-safety rule refused the plan (sub-class names the hazard).
    Safety(&'static str),
    /// Forged, replayed, wrong-scope or malformed token.
    TokenAbuse(&'static str),
    /// Expired / future / over-long token (stale, not hostile by itself).
    TokenStale,
    /// `kill`, `rotate`, teardown, ... asked for: can never be approved.
    Unapprovable,
    /// Refused because the admin disabled HADES.
    Disabled,
    /// Ledger corrupt or unavailable: the gate fails closed.
    LedgerFail,
    /// Any other refusal.
    OtherRefusal,
    /// An approval consumed a token.
    Approved,
    /// hades-gate stopped by the admin (`POST /v1/stop`).
    Stopped,
    /// hermes-tools refused a path/sandbox escape (sub-class).
    Escape(&'static str),
    /// hermes-tools ran with a sandbox layer not enforced (layer name), or
    /// the sandbox refused to start (`start`). Layers off *by policy* are
    /// not counted.
    SandboxWeak(&'static str),
    /// Any other hermes-tools refusal (caps, allowlist).
    AgentRefusal,
}

/// Classify a hades-gate refusal reason (`Refusal::reason`).
pub fn gate_reason(r: &str) -> Class {
    if let Some(h) = r.strip_prefix("admin-safety: ") {
        return Class::Safety(if h.starts_with("destructive") {
            "destructive"
        } else if h.starts_with("secret") {
            "secret-exposure"
        } else if h.starts_with("admin account") {
            "admin-access"
        } else if h.starts_with("protected target") {
            "protected-target"
        } else if h.starts_with("plan cannot be reviewed") {
            "opaque-plan"
        } else {
            "other"
        });
    }
    if r.starts_with("forged or tampered") {
        Class::TokenAbuse("forged")
    } else if r.starts_with("replayed token") {
        Class::TokenAbuse("replayed")
    } else if r.starts_with("token is scoped to a different") {
        Class::TokenAbuse("wrong-scope")
    } else if r.starts_with("malformed token") {
        Class::TokenAbuse("malformed")
    } else if r == "expired token"
        || r.starts_with("token issued in the future")
        || r.starts_with("token lifetime")
    {
        Class::TokenStale
    } else if r.contains("can never be approved") {
        Class::Unapprovable
    } else if r.starts_with("HADES is disabled") {
        Class::Disabled
    } else if r.starts_with("ledger corrupt") || r.starts_with("ledger unavailable") {
        Class::LedgerFail
    } else {
        Class::OtherRefusal
    }
}

/// One hades-gate ledger line, read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerLine {
    /// A counted decision: line `ts` (unix, None when unparsable), class.
    Counted(Option<u64>, Class),
    /// Valid but not counted (`dry-run` notes, unknown events).
    Skip,
    /// Not JSON: a corrupt ledger (the gate fails closed on it).
    Corrupt,
}

/// Read one hades-gate ledger line.
pub fn ledger_line(line: &str) -> LedgerLine {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return LedgerLine::Corrupt;
    };
    let t = v["ts"].as_str().and_then(parse_ts);
    match v["event"].as_str() {
        Some("refused") => LedgerLine::Counted(t, gate_reason(v["reason"].as_str().unwrap_or(""))),
        Some("consumed") => LedgerLine::Counted(t, Class::Approved),
        _ => LedgerLine::Skip,
    }
}

/// One hermes-tools audit line.
pub fn audit_line(line: &str) -> Option<(Option<u64>, Class)> {
    let v: Value = serde_json::from_str(line).ok()?;
    let t = v["ts"].as_str().and_then(parse_ts);
    match v["decision"].as_str()? {
        "refused" => {
            let r = v["reason"].as_str().unwrap_or("");
            let c = if r.starts_with("path traversal") {
                Class::Escape("path-traversal")
            } else if r.contains("outside the allowlisted worktree") {
                Class::Escape("outside-worktree")
            } else if r.starts_with("symlink in path") {
                Class::Escape("symlink")
            } else if r.contains("denied path") || r.contains("is denied") {
                Class::Escape("denied-path")
            } else if r.starts_with("sandbox refused to start") || r.contains("no Landlock") {
                Class::SandboxWeak("start")
            } else {
                Class::AgentRefusal
            };
            Some((t, c))
        }
        "ran" => {
            // hermes-tools exec::summary: `not_enforced` = {layer: why}.
            let off = v["result"]["sandbox"]["not_enforced"].as_object()?;
            let layer = off
                .iter()
                .find(|(_, why)| !why.as_str().unwrap_or("").contains("policy"))
                .map(|(k, _)| layer_name(k))?;
            Some((t, Class::SandboxWeak(layer)))
        }
        _ => None,
    }
}

/// openmind-sandbox layer names (`LAYERS`); anything else is `other`.
pub fn layer_name(k: &str) -> &'static str {
    const LAYERS: &[&str] = &[
        "rlimits",
        "env",
        "workdir",
        "no_new_privs",
        "userns",
        "fds",
        "cgroup",
        "net",
        "pidns",
        "mountns",
        "ro_root",
        "hide_home",
        "proc",
        "landlock",
        "caps",
    ];
    LAYERS.iter().find(|l| **l == k).copied().unwrap_or("other")
}

/// `YYYY-MM-DDTHH:MM:SS(+|-)HH:MM` (or `Z`) to unix seconds.
pub fn parse_ts(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let off = match rest.as_bytes().first()? {
        b'Z' => 0,
        sign @ (b'+' | b'-') => {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            let o = oh * 3600 + om * 60;
            if *sign == b'+' {
                o
            } else {
                -o
            }
        }
        _ => return None,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm, public domain).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + se - off).ok()
}

/// A JSONL file read from where the last read stopped.
#[derive(Debug)]
struct Tail {
    path: PathBuf,
    pos: Option<u64>,
    partial: String,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Tail {
            path,
            pos: None,
            partial: String::new(),
        }
    }

    /// New complete lines (None: file absent / unreadable).
    fn read(&mut self) -> Option<Vec<String>> {
        let mut f = File::open(&self.path).ok()?;
        let len = f.metadata().ok()?.len();
        let first = self.pos.is_none();
        let mut pos = self.pos.unwrap_or(len.saturating_sub(MAX_READ));
        if pos > len {
            // Truncated or replaced: start over.
            pos = 0;
            self.partial.clear();
        }
        f.seek(SeekFrom::Start(pos)).ok()?;
        let mut buf = Vec::new();
        f.take(MAX_READ).read_to_end(&mut buf).ok()?;
        self.pos = Some(pos + buf.len() as u64);
        let mut text = std::mem::take(&mut self.partial);
        text.push_str(&String::from_utf8_lossy(&buf));
        if first && pos > 0 {
            // Started mid-file: drop the cut first line.
            text = text
                .split_once('\n')
                .map(|x| x.1.to_string())
                .unwrap_or_default();
        }
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        self.partial = lines.pop().unwrap_or_default();
        if self.partial.len() > MAX_READ as usize {
            self.partial.clear();
        }
        Some(lines.into_iter().filter(|l| !l.trim().is_empty()).collect())
    }
}

/// Thresholds (per window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub window_s: u64,
    /// Refusals of any class in the window that make a burst (agent loop).
    pub burst: usize,
    /// Admin-safety refusals that raise warn -> error.
    pub safety_error: usize,
    /// Escape attempts that raise warn -> error.
    pub escape_error: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            window_s: 900,
            burst: 20,
            safety_error: 5,
            escape_error: 5,
        }
    }
}

/// The sensor.
#[derive(Debug)]
pub struct Hades {
    ledger: Option<Tail>,
    audits: Vec<Tail>,
    disable_file: Option<PathBuf>,
    pub limits: Limits,
    /// `(unix ts, class)` inside the window.
    seen: Vec<(u64, Class)>,
    /// Unparsable ledger lines seen in the window (corrupt ledger).
    corrupt: Vec<u64>,
    /// Lines dropped because the window was full.
    pub dropped: u64,
}

impl Hades {
    /// A sensor with explicit sources (tests, `from_env`).
    pub fn new(
        ledger: Option<PathBuf>,
        audits: Vec<PathBuf>,
        disable_file: Option<PathBuf>,
    ) -> Self {
        Hades {
            ledger: ledger.map(Tail::new),
            audits: audits.into_iter().map(Tail::new).collect(),
            disable_file,
            limits: Limits::default(),
            seen: vec![],
            corrupt: vec![],
            dropped: 0,
        }
    }

    /// `NERVE_HADES_LEDGER`, `NERVE_HADES_AUDIT` (comma list),
    /// `NERVE_HADES_DISABLE_FILE`, `NERVE_HADES_WINDOW_S`,
    /// `NERVE_HADES_BURST`; on when a path is set or `NERVE_HADES=1`.
    pub fn from_env(env: Env) -> Option<Self> {
        let path = |k: &str| {
            env(k)
                .filter(|v| !v.trim().is_empty())
                .map(|v| PathBuf::from(v.trim()))
        };
        let ledger = path("NERVE_HADES_LEDGER");
        let audits: Vec<PathBuf> = env("NERVE_HADES_AUDIT")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        let disable = path("NERVE_HADES_DISABLE_FILE");
        let on = env("NERVE_HADES").as_deref() == Some("1");
        if ledger.is_none() && audits.is_empty() && disable.is_none() && !on {
            return None;
        }
        let mut s = Hades::new(ledger, audits, disable);
        let num = |k: &str| env(k).and_then(|v| v.trim().parse::<u64>().ok());
        if let Some(w) = num("NERVE_HADES_WINDOW_S").filter(|w| *w > 0) {
            s.limits.window_s = w;
        }
        if let Some(b) = num("NERVE_HADES_BURST").filter(|b| *b > 0) {
            s.limits.burst = b as usize;
        }
        Some(s)
    }

    fn push(&mut self, t: u64, c: Class) {
        if self.seen.len() >= MAX_EVENTS {
            self.seen.remove(0);
            self.dropped += 1;
        }
        self.seen.push((t, c));
    }

    /// Feed one cycle's raw lines (exposed for fixtures and replay tests).
    pub fn ingest(&mut self, now: u64, ledger: &[String], audit: &[String]) {
        for l in ledger {
            match ledger_line(l) {
                LedgerLine::Counted(t, c) => self.push(t.unwrap_or(now), c),
                LedgerLine::Skip => {}
                LedgerLine::Corrupt => self.corrupt.push(now),
            }
        }
        for l in audit {
            if let Some((t, c)) = audit_line(l) {
                self.push(t.unwrap_or(now), c);
            }
        }
    }

    fn count(&self, f: impl Fn(&Class) -> bool) -> (usize, BTreeMap<&'static str, usize>) {
        let mut by = BTreeMap::new();
        let mut n = 0;
        for (_, c) in &self.seen {
            if f(c) {
                n += 1;
                let k: &'static str = match *c {
                    Class::Safety(s)
                    | Class::TokenAbuse(s)
                    | Class::Escape(s)
                    | Class::SandboxWeak(s) => s,
                    _ => "total",
                };
                *by.entry(k).or_insert(0) += 1;
            }
        }
        (n, by)
    }
}

fn evidence(by: &BTreeMap<&'static str, usize>, window_s: u64) -> Vec<String> {
    let mut v: Vec<String> = by.iter().map(|(k, n)| format!("{k}: {n}")).collect();
    v.push(format!("window: {window_s} s"));
    v
}

impl Sensor for Hades {
    fn name(&self) -> &'static str {
        "hades"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        let now = i.now;
        let w = self.limits.window_s;
        // 1. Host files, live only.
        let mut disabled = false;
        if i.live {
            let ledger = self
                .ledger
                .as_mut()
                .and_then(Tail::read)
                .unwrap_or_default();
            let mut audit = vec![];
            for t in &mut self.audits {
                audit.extend(t.read().unwrap_or_default());
            }
            self.ingest(now, &ledger, &audit);
            disabled = self.disable_file.as_ref().is_some_and(|p| p.exists());
        }
        // 2. Journal (also in replay): the gate's own stderr lines.
        for l in i.journal.iter().filter(|l| l.unit.starts_with("hades")) {
            let t = if l.t == 0 { now } else { l.t };
            if l.msg.contains("ledger corrupt") || l.msg.contains("ledger unavailable") {
                self.push(t, Class::LedgerFail);
            } else if l.msg.contains("stopped by admin request") {
                self.push(t, Class::Stopped);
            }
        }
        // 3. Window.
        let from = now.saturating_sub(w);
        self.seen.retain(|(t, _)| *t >= from && *t <= now + 60);
        self.corrupt.retain(|t| *t >= from);

        let gate = "hades-gate";
        let (n, by) = self.count(|c| matches!(c, Class::Safety(_)));
        if n > 0 {
            let sev = if n >= self.limits.safety_error {
                Sev::Error
            } else {
                Sev::Warn
            };
            out.finding(
                Finding::new(FindingKind::Vulnerability, "hades-safety-refusals", sev, gate,
                    format!("{n} plan(s) refused by the HADES admin-safety rule in {}", crate::cortex::dur(w)))
                .with_evidence(evidence(&by, w))
                .with_fix("find which agent or Gaia system proposed them (ledger on the gate host); the gate held, nothing ran")
                .with_cause("hades"),
            );
            if sev >= Sev::Error {
                out.signal(Signal::new(
                    "hades",
                    gate,
                    sev,
                    format!("{n} admin-safety refusals"),
                ));
            }
        }
        let (n, by) = self.count(|c| matches!(c, Class::TokenAbuse(_)));
        if n > 0 {
            let sev = if n >= 3 { Sev::Critical } else { Sev::Error };
            out.finding(
                Finding::new(FindingKind::Vulnerability, "hades-token-abuse", sev, gate,
                    format!("{n} forged, replayed or mis-scoped approval token(s) in {}", crate::cortex::dur(w)))
                .with_evidence(evidence(&by, w))
                .with_fix("treat as an attack on the approval path: check who can reach the gate; rotate the admin key only by the admin")
                .with_cause("hades"),
            );
            out.signal(Signal::new(
                "hades",
                gate,
                sev,
                format!("{n} bad approval tokens"),
            ));
        }
        let (n, by) = self.count(|c| *c == Class::Unapprovable);
        if n > 0 {
            out.finding(
                Finding::new(
                    FindingKind::Vulnerability,
                    "hades-unapprovable-actions",
                    Sev::Warn,
                    gate,
                    format!(
                        "{n} request(s) for actions HADES never approves (kill/rotate/teardown)"
                    ),
                )
                .with_evidence(evidence(&by, w))
                .with_cause("hades"),
            );
        }
        let (n, by) = self.count(|c| *c == Class::LedgerFail);
        let corrupt = self.corrupt.len();
        if n + corrupt > 0 {
            let mut ev = evidence(&by, w);
            ev.insert(0, format!("unparsable ledger lines: {corrupt}"));
            out.finding(
                Finding::new(FindingKind::Bleeding, "hades-ledger-failing", Sev::Error, gate,
                    "HADES ledger corrupt or unavailable: the gate fails closed, no approval can pass")
                .with_evidence(ev)
                .with_fix("admin: inspect the ledger file on the gate host (read-only first); never delete it")
                .with_cause("hades"),
            );
            out.signal(Signal::new(
                "hades",
                gate,
                Sev::Error,
                "ledger failing closed",
            ));
        }
        let (n, _) = self.count(|c| {
            matches!(
                c,
                Class::Safety(_)
                    | Class::TokenAbuse(_)
                    | Class::TokenStale
                    | Class::Unapprovable
                    | Class::OtherRefusal
            )
        });
        if n >= self.limits.burst {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "hades-refusal-burst",
                    Sev::Warn,
                    gate,
                    format!(
                        "{n} HADES refusals in {}: an agent may be retrying in a loop",
                        crate::cortex::dur(w)
                    ),
                )
                .with_evidence(vec![format!("refusals: {n}"), format!("window: {w} s")])
                .with_cause("hades"),
            );
        }
        let (stopped, _) = self.count(|c| *c == Class::Stopped);
        if disabled || stopped > 0 {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "hades-off",
                    Sev::Info,
                    gate,
                    if disabled {
                        "HADES disabled by the admin: no plan can be approved"
                    } else {
                        "HADES stopped by the admin"
                    },
                )
                .with_evidence(vec![
                    format!("disabled: {disabled}"),
                    format!("stopped: {stopped}"),
                ]),
            );
        }
        // hermes-tools sandbox.
        let agent = "hermes-rs";
        let (n, by) = self.count(|c| matches!(c, Class::Escape(_)));
        if n > 0 {
            let sev = if n >= self.limits.escape_error {
                Sev::Error
            } else {
                Sev::Warn
            };
            out.finding(
                Finding::new(
                    FindingKind::Vulnerability,
                    "sandbox-escape-attempts",
                    sev,
                    agent,
                    format!(
                        "{n} coding-tool call(s) tried to leave the sandbox worktree (refused)"
                    ),
                )
                .with_evidence(evidence(&by, w))
                .with_fix("review the hermes session in the audit log; the guard refused every one")
                .with_cause("sandbox"),
            );
            if sev >= Sev::Error {
                out.signal(Signal::new(
                    "sandbox",
                    agent,
                    sev,
                    format!("{n} escape attempts"),
                ));
            }
        }
        let (n, by) = self.count(|c| matches!(c, Class::SandboxWeak(_)));
        if n > 0 {
            out.finding(
                Finding::new(FindingKind::Vulnerability, "sandbox-not-enforced", Sev::Warn, agent,
                    format!("{n} coding-tool run(s) with a sandbox layer not enforced or refused to start"))
                .with_evidence(evidence(&by, w))
                .with_fix("check kernel Landlock/userns support on the agent host")
                .with_cause("sandbox"),
            );
        }
        let (n, _) = self.count(|c| matches!(c, Class::Escape(_) | Class::AgentRefusal));
        if n >= self.limits.burst {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "agent-refusal-burst",
                    Sev::Warn,
                    agent,
                    format!(
                        "{n} refused coding-tool calls in {}: an agent may be looping",
                        crate::cortex::dur(w)
                    ),
                )
                .with_evidence(vec![format!("refusals: {n}"), format!("window: {w} s")])
                .with_cause("sandbox"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cortex::Cortex;
    use crate::logscan::LogInsights;
    use crate::probe::JLine;
    use crate::sensors::Sensors;
    use std::path::Path;

    /// 2026-10-20 23:25:00+02:00, just after the fixture lines.
    const NOW: u64 = 1_792_531_500;

    fn fx(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/hades")
            .join(name)
    }

    fn run(s: Hades, live: bool, now: u64, journal: &[JLine]) -> (Cortex, Sensors) {
        let mut reg = Sensors::empty();
        reg.add(s);
        let logs = LogInsights::default();
        let mut i = Input::new(now, &[], &logs);
        i.live = live;
        i.journal = journal;
        let cx = reg.run(&i);
        (cx, reg)
    }

    fn ids(cx: &Cortex) -> Vec<(String, &'static str)> {
        cx.findings()
            .map(|f| (f.id.clone(), f.sev.as_str()))
            .collect()
    }

    fn fixture_sensor() -> Hades {
        Hades::new(
            Some(fx("ledger.jsonl")),
            vec![fx("audit.jsonl")],
            Some(fx("no-such-disable-file")),
        )
    }

    #[test]
    fn every_gate_reason_has_a_class() {
        // The exact strings of hades-gate Refusal::reason (ccaab42 = e6ceca7).
        for (r, c) in [
            (
                "admin-safety: destructive command refused (rm -r)",
                Class::Safety("destructive"),
            ),
            (
                "admin-safety: secret exposure refused (x)",
                Class::Safety("secret-exposure"),
            ),
            (
                "admin-safety: admin account/access change refused (x)",
                Class::Safety("admin-access"),
            ),
            (
                "admin-safety: protected target refused (x)",
                Class::Safety("protected-target"),
            ),
            (
                "admin-safety: plan cannot be reviewed (x)",
                Class::Safety("opaque-plan"),
            ),
            (
                "forged or tampered token: signature does not verify",
                Class::TokenAbuse("forged"),
            ),
            (
                "replayed token: nonce already consumed",
                Class::TokenAbuse("replayed"),
            ),
            (
                "token is scoped to a different target",
                Class::TokenAbuse("wrong-scope"),
            ),
            (
                "malformed token: target name",
                Class::TokenAbuse("malformed"),
            ),
            ("expired token", Class::TokenStale),
            ("token issued in the future", Class::TokenStale),
            ("token lifetime exceeds the allowed TTL", Class::TokenStale),
            ("action \"kill\" can never be approved", Class::Unapprovable),
            (
                "HADES is disabled by the admin (disable file present)",
                Class::Disabled,
            ),
            (
                "ledger corrupt at line 3: failing closed",
                Class::LedgerFail,
            ),
            (
                "ledger unavailable (permission denied): failing closed",
                Class::LedgerFail,
            ),
            ("something new", Class::OtherRefusal),
        ] {
            assert_eq!(gate_reason(r), c, "{r}");
        }
    }

    #[test]
    fn zagreb_ts_parse_and_dst_fold() {
        assert_eq!(parse_ts("2026-10-20T23:25:00+02:00"), Some(NOW));
        assert_eq!(parse_ts("2026-10-25T02:59:59+02:00"), Some(1_792_889_999));
        assert_eq!(parse_ts("2026-10-25T02:00:00+01:00"), Some(1_792_890_000));
        assert_eq!(parse_ts("2026-10-20T21:25:00.123Z"), Some(NOW));
        assert_eq!(parse_ts("1970-01-01T00:00:00+00:00"), Some(0));
        for bad in [
            "",
            "2026-10-20 23:25:00+02:00",
            "2026-13-20T23:25:00+02:00",
            "2026-10-20T23:25:00",
        ] {
            assert_eq!(parse_ts(bad), None, "{bad}");
        }
    }

    #[test]
    fn not_live_never_reads_the_host() {
        let (cx, _) = run(fixture_sensor(), false, NOW, &[]);
        assert!(cx.findings().next().is_none(), "{:?}", ids(&cx));
    }

    #[test]
    fn fixture_live_findings_counts_only() {
        let (cx, reg) = run(fixture_sensor(), true, NOW, &[]);
        assert_eq!(
            ids(&cx),
            [
                ("hades-token-abuse".to_string(), "error"),
                ("hades-safety-refusals".to_string(), "warn"),
                ("hades-unapprovable-actions".to_string(), "warn"),
                ("sandbox-escape-attempts".to_string(), "warn"),
                ("sandbox-not-enforced".to_string(), "warn"),
            ]
        );
        let safety = cx
            .vulnerabilities
            .iter()
            .find(|f| f.id == "hades-safety-refusals")
            .unwrap();
        assert_eq!(
            safety.evidence,
            [
                "destructive: 1",
                "opaque-plan: 1",
                "secret-exposure: 1",
                "window: 900 s"
            ]
        );
        assert_eq!(safety.source, "hades");
        let esc = cx
            .vulnerabilities
            .iter()
            .find(|f| f.id == "sandbox-escape-attempts")
            .unwrap();
        assert_eq!(
            esc.evidence,
            [
                "outside-worktree: 1",
                "path-traversal: 1",
                "symlink: 1",
                "window: 900 s"
            ]
        );
        let weak = cx
            .vulnerabilities
            .iter()
            .find(|f| f.id == "sandbox-not-enforced")
            .unwrap();
        assert_eq!(
            weak.evidence[0], "net: 1",
            "policy-off layers are not counted"
        );
        // Token abuse is scored; warn-level findings are not.
        assert_eq!(
            cx.signals
                .iter()
                .map(|s| s.cause.as_str())
                .collect::<Vec<_>>(),
            ["hades"]
        );
        assert_eq!((reg.last[0].findings, reg.last[0].signals), (5, 1));
        // Nothing from the lines themselves leaks: plans, targets, paths,
        // token hashes, sessions, reason details.
        let all = format!("{}{:?}", cx.to_json(), cx.nerve_events(NOW));
        for leak in [
            "rm -r",
            "/etc/shadow",
            "galion-desk",
            "tuner",
            "a1b2c3d4",
            "0badc0de",
            "passwd",
            "id_ed25519",
            "sess_7f3a",
            "link/x",
            "UDP",
            "plan verb",
            "00112233",
        ] {
            assert!(!all.contains(leak), "{leak} leaked");
        }
    }

    #[test]
    fn contract_b33_nerve_event() {
        let (cx, _) = run(fixture_sensor(), true, NOW, &[]);
        let evs = cx.nerve_events(NOW);
        assert_eq!(evs.len(), 1, "one (source, kind) pair: hades/vulnerability");
        for e in &evs {
            let ne: openmind_schema::NerveEvent = serde_json::from_value(e.clone()).expect("B33");
            assert_eq!(
                (ne.source.as_str(), ne.kind.as_str(), ne.severity.as_str()),
                ("hades", "vulnerability", "high")
            );
            assert_eq!(ne.ts, "2026-10-20T23:25:00+02:00");
            assert_eq!(ne.data["findings"], 5);
            assert_eq!(ne.data["top"]["id"], "hades-token-abuse");
            // And through the T2.10 feed projection.
            let fe = crate::feed::project(e, 3);
            let ne: openmind_schema::NerveEvent = serde_json::from_value(fe).expect("B33 feed");
            assert_eq!(ne.data["ids"].as_array().unwrap().len(), 5);
            assert!(ne.data["top"].get("title").is_none());
        }
    }

    #[test]
    fn window_expires_and_findings_clear() {
        let mut reg = Sensors::empty();
        reg.add(fixture_sensor());
        let logs = LogInsights::default();
        let mut i = Input::new(NOW, &[], &logs);
        i.live = true;
        assert_eq!(reg.run(&i).findings().count(), 5);
        // Same file, nothing new: still in the window.
        i.now = NOW + 300;
        assert_eq!(reg.run(&i).findings().count(), 5);
        // 16 minutes after the last line: all gone.
        i.now = NOW + 16 * 60;
        let cx = reg.run(&i);
        assert!(cx.findings().next().is_none(), "{:?}", ids(&cx));
    }

    #[test]
    fn tail_is_incremental_and_survives_truncation() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("hades-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ledger.jsonl");
        let refused = |r: &str| {
            format!("{{\"ts\":\"2026-10-20T23:24:00+02:00\",\"event\":\"refused\",\"action\":\"restart\",\"target\":\"x\",\"reason\":\"{r}\",\"token_sha256_16\":\"\"}}\n")
        };
        std::fs::write(&p, refused("expired token")).unwrap();
        let mut t = Tail::new(p.clone());
        assert_eq!(t.read().unwrap().len(), 1);
        assert_eq!(t.read().unwrap().len(), 0, "nothing new");
        // A half-written line waits for its newline.
        let line = refused("forged or tampered token: signature does not verify");
        let (a, b) = line.split_at(40);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(a.as_bytes())
            .unwrap();
        assert_eq!(t.read().unwrap().len(), 0);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b.as_bytes())
            .unwrap();
        let got = t.read().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            ledger_line(&got[0]),
            LedgerLine::Counted(Some(NOW - 60), Class::TokenAbuse("forged"))
        );
        // Rotated to a shorter file: read from the start again.
        std::fs::write(&p, refused("expired token")).unwrap();
        assert_eq!(t.read().unwrap().len(), 1);
        // A missing file is not an error.
        let mut gone = Tail::new(dir.join("absent"));
        assert!(gone.read().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_ledger_and_journal_lines() {
        let dir = std::env::temp_dir().join(format!("hades-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ledger.jsonl");
        std::fs::write(
            &p,
            "{\"ts\":\"2026-10-20T23:24:00+02:00\",\"event\":\"consumed\"}\nnot json at all\n",
        )
        .unwrap();
        let disable = dir.join("disabled");
        std::fs::write(&disable, "").unwrap();
        let s = Hades::new(Some(p), vec![], Some(disable));
        let (cx, _) = run(s, true, NOW, &[]);
        assert_eq!(
            ids(&cx),
            [
                ("hades-ledger-failing".to_string(), "error"),
                ("hades-off".to_string(), "info")
            ]
        );
        assert_eq!(cx.bleeding[0].evidence[0], "unparsable ledger lines: 1");
        let _ = std::fs::remove_dir_all(&dir);
        // Journal only (replay-safe, not live): the gate's stderr.
        let j = |unit: &str, msg: &str| JLine {
            t: NOW - 10,
            unit: unit.into(),
            prio: 3,
            msg: msg.into(),
        };
        let lines = [
            j(
                "hades-gate.service",
                "hades-gate: ledger corrupt at line 9: failing closed",
            ),
            j("hades-gate.service", "hades-gate: stopped by admin request"),
            j("tuner.service", "ledger corrupt"),
        ];
        let (cx, _) = run(Hades::new(None, vec![], None), false, NOW, &lines);
        assert_eq!(
            ids(&cx),
            [
                ("hades-ledger-failing".to_string(), "error"),
                ("hades-off".to_string(), "info")
            ]
        );
        assert_eq!(cx.bleeding[0].evidence[0], "unparsable ledger lines: 0");
    }

    #[test]
    fn bursts_escalate() {
        let mut s = Hades::new(None, vec![], None);
        let gate = |r: &str| {
            format!(
                "{{\"ts\":\"2026-10-20T23:24:00+02:00\",\"event\":\"refused\",\"reason\":\"{r}\"}}"
            )
        };
        let audit = "{\"decision\":\"refused\",\"reason\":\"path traversal (..) refused: x\",\"ts\":\"2026-10-20T23:24:00+02:00\"}".to_string();
        let mut ledger = vec![gate("admin-safety: destructive command refused (dd)"); 5];
        ledger.extend(vec![gate("expired token"); 15]);
        ledger.extend(vec![gate("replayed token: nonce already consumed"); 3]);
        s.ingest(NOW, &ledger, &vec![audit; 20]);
        let (cx, _) = run(s, false, NOW, &[]);
        assert_eq!(
            ids(&cx),
            [
                ("hades-token-abuse".to_string(), "critical"),
                ("hades-safety-refusals".to_string(), "error"),
                ("sandbox-escape-attempts".to_string(), "error"),
                ("agent-refusal-burst".to_string(), "warn"),
                ("hades-refusal-burst".to_string(), "warn"),
            ]
        );
        let causes: Vec<&str> = cx.signals.iter().map(|s| s.cause.as_str()).collect();
        assert_eq!(causes, ["hades", "hades", "sandbox"]);
    }

    #[test]
    fn from_env_off_by_default_and_registered() {
        assert!(Hades::from_env(&|_| None).is_none());
        let env = |k: &str| match k {
            "NERVE_HADES_LEDGER" => Some("/var/lib/hades/ledger.jsonl".to_string()),
            "NERVE_HADES_AUDIT" => Some(" /a.jsonl, ,/b.jsonl".to_string()),
            "NERVE_HADES_WINDOW_S" => Some("60".to_string()),
            "NERVE_HADES_BURST" => Some("0".to_string()),
            _ => None,
        };
        let s = Hades::from_env(&env).unwrap();
        assert_eq!(
            (s.audits.len(), s.limits.window_s, s.limits.burst),
            (2, 60, 20)
        );
        assert!(Hades::from_env(&|k| (k == "NERVE_HADES").then(|| "1".into())).is_some());
        let names = Sensors::from_env(&env).names();
        assert_eq!(names.last(), Some(&"hades"));
        let off = |k: &str| {
            if k == "NERVE_SENSORS_OFF" {
                Some("hades".into())
            } else {
                env(k)
            }
        };
        assert!(!Sensors::from_env(&off).names().contains(&"hades"));
    }
}
