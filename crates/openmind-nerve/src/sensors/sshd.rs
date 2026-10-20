// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.1 `sshd`: ssh authentication failures and credential floods, counted
//! from the sshd lines in [`Input::journal`] over a sliding 300 s window,
//! with the B41 thresholds (docs/cortex.md section 4) verbatim.
//!
//! | kind | id | B41 rule |
//! |---|---|---|
//! | vulnerability | `sshd-accepted-password` | crit `accepted_password >= 1` |
//! | vulnerability | `sshd-accepted-publickey-other` | high `>= 5`, warn `>= 1` |
//! | vulnerability | `sshd-invalid-user` | warn `invalid_user >= 30` |
//! | vulnerability | `sshd-max-auth-attempts` | warn `max_auth_attempts >= 1` |
//! | ddos | `sshd-credential-flood` (+ `ssh-flood` signal) | section 4.2 table |
//! | bleeding | `sshd-auth-log-growth` | high `bytes_added >= 1 MiB`, warn `>= 256 KiB` |
//!
//! B33 `source` is `auth.log` (the B41 logical source) for every finding.
//! Titles and evidence carry counts only: addresses are hashed in memory to
//! count distinct sources and never leave this file; account names are not
//! kept at all. `accepted_publickey_root` is counted but is never a
//! threshold (the desk logs in as root with a key). `preauth_other`
//! (other `[preauth]` lines) is counted as data only: the VPS runs with
//! password auth off, so it logs no `Failed password` lines at all and the
//! pressure shows up there (docs/real-tests/cortex-s2-a.md; owner decision).

//!
//! Host reads: only `bytes_added`, a `stat` (size only, the file is never
//! opened) of the auth log, and only when [`Input::live`]. Replays and tests
//! never touch the host.
//!
//! Env (sensor off unless `NERVE_SSHD=1`):
//! - `NERVE_SSHD_UNITS` journal units to read, comma list
//!   (default `ssh.service,sshd.service`; `sshd@*.service` always matches).
//! - `NERVE_SSHD_AUTH_LOG` file for `bytes_added` (default
//!   `/var/log/auth.log`; empty or `0` = off; a missing file is skipped).
//!
//! The live engine reads the journal of every component unit plus
//! `collect::JOURNAL_EXTRA_UNITS` (`ssh.service`, T2.0 merge). An
//! `sshd.service` or `sshd@*.service` host needs that unit in a component
//! spec (or added to the extra list) before live lines reach this sensor.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::{Sev, Signal};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// B41 window for the auth rules (seconds).
pub const WINDOW_S: u64 = 300;
/// Bound on remembered auth events (memory cap under an extreme flood:
/// 50 000 x 24 bytes = 1.2 MB). Counts saturate there, which is far above
/// every crit line.
const MAX_EVENTS: usize = 50_000;
const SOURCE: &str = "auth.log";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    /// `Failed password` with a hash of the `from` address (0 = none).
    Failed(u64),
    InvalidUser,
    AcceptedPassword,
    AcceptedKeyRoot,
    AcceptedKeyOther,
    MaxAuth,
    /// Any other `[preauth]` line (disconnects before auth). Data only.
    Preauth,
}

/// The B41 `data` counts for one window (section 3.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub failed_password: u64,
    pub invalid_user: u64,
    pub accepted_password: u64,
    pub accepted_publickey_root: u64,
    pub accepted_publickey_other: u64,
    pub max_auth_attempts: u64,
    pub distinct_fail_sources: u64,
    pub top_fail_share_pct: u64,
    pub bytes_added: u64,
    /// Not a B41 key: other `[preauth]` lines (with password auth off,
    /// credential pressure shows up here, not as `Failed password`).
    /// Counted for T3.1 calibration; no threshold (owner decision).
    pub preauth_other: u64,
}

impl Counts {
    /// `key=value` counts, B41 key order (evidence line; no addresses).
    pub fn line(&self) -> String {
        format!(
            "window_s={WINDOW_S} failed_password={} invalid_user={} accepted_password={} accepted_publickey_root={} accepted_publickey_other={} distinct_fail_sources={} top_fail_share_pct={} max_auth_attempts={} bytes_added={} preauth_other={}",
            self.failed_password,
            self.invalid_user,
            self.accepted_password,
            self.accepted_publickey_root,
            self.accepted_publickey_other,
            self.distinct_fail_sources,
            self.top_fail_share_pct,
            self.max_auth_attempts,
            self.bytes_added,
            self.preauth_other
        )
    }
}

/// B41 section 4.2 (ddos). Returns the severity and the rule that fired.
pub fn ddos_rule(c: &Counts) -> Option<(Sev, &'static str)> {
    let (f, d) = (c.failed_password, c.distinct_fail_sources);
    if f >= 300 {
        Some((Sev::Critical, "failed_password"))
    } else if f >= 100 && d >= 40 {
        Some((Sev::Critical, "failed_password+distinct_fail_sources"))
    } else if f >= 100 {
        Some((Sev::Error, "failed_password"))
    } else if d >= 25 {
        Some((Sev::Error, "distinct_fail_sources"))
    } else if c.top_fail_share_pct >= 80 && f >= 50 {
        Some((Sev::Error, "top_fail_share_pct"))
    } else if f >= 30 {
        Some((Sev::Warn, "failed_password"))
    } else if d >= 10 {
        Some((Sev::Warn, "distinct_fail_sources"))
    } else {
        None
    }
}

/// B41 section 4.3, auth.log row.
pub fn bleed_rule(c: &Counts) -> Option<Sev> {
    if c.bytes_added >= 1_048_576 {
        Some(Sev::Error)
    } else if c.bytes_added >= 262_144 {
        Some(Sev::Warn)
    } else {
        None
    }
}

/// Classify one sshd message: (event, multiplier). `message repeated N
/// times: [ ... ]` (rsyslog) counts N.
fn classify(msg: &str) -> Option<(Ev, u64)> {
    let (msg, n) = match msg
        .strip_prefix("message repeated ")
        .and_then(|r| r.split_once(" times: [ "))
    {
        Some((n, rest)) => (
            rest.trim_end_matches(']').trim_end(),
            n.parse().unwrap_or(1),
        ),
        None => (msg, 1),
    };
    let ev = if msg.contains("Failed password") {
        Ev::Failed(source_hash(msg))
    } else if msg.contains("Accepted password") {
        Ev::AcceptedPassword
    } else if msg.contains("Accepted publickey for root ") {
        Ev::AcceptedKeyRoot
    } else if msg.contains("Accepted publickey") {
        Ev::AcceptedKeyOther
    } else if msg.contains("maximum authentication attempts") {
        Ev::MaxAuth
    } else if msg.contains("Invalid user") {
        Ev::InvalidUser
    } else if msg.contains("[preauth]") {
        Ev::Preauth
    } else {
        return None;
    };
    Some((ev, n.max(1)))
}

/// Hash of the token after the last ` from ` (the client address). The
/// address itself is dropped here.
fn source_hash(msg: &str) -> u64 {
    let Some((_, rest)) = msg.rsplit_once(" from ") else {
        return 0;
    };
    let addr = rest.split_whitespace().next().unwrap_or("");
    if addr.is_empty() {
        return 0;
    }
    let mut h = DefaultHasher::new();
    addr.hash(&mut h);
    h.finish() | 1
}

/// Sensor `sshd` (T2.1).
pub struct SshAuth {
    units: Vec<String>,
    auth_log: Option<PathBuf>,
    events: VecDeque<(u64, Ev)>,
    /// (t, size) samples of the auth log, oldest first.
    sizes: VecDeque<(u64, u64)>,
    /// Last window's counts (for tests and the real-test record).
    pub last: Counts,
}

impl SshAuth {
    pub fn new(units: Vec<String>, auth_log: Option<PathBuf>) -> Self {
        SshAuth {
            units,
            auth_log,
            events: VecDeque::new(),
            sizes: VecDeque::new(),
            last: Counts::default(),
        }
    }

    /// `NERVE_SSHD=1` turns the sensor on (see module docs).
    pub fn from_env(env: Env) -> Option<Self> {
        if env("NERVE_SSHD").as_deref() != Some("1") {
            return None;
        }
        let units = env("NERVE_SSHD_UNITS")
            .unwrap_or_else(|| "ssh.service,sshd.service".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let auth_log = match env("NERVE_SSHD_AUTH_LOG") {
            None => Some(PathBuf::from("/var/log/auth.log")),
            Some(p) if p.is_empty() || p == "0" => None,
            Some(p) => Some(PathBuf::from(p)),
        };
        Some(Self::new(units, auth_log))
    }

    fn wanted(units: &[String], unit: &str) -> bool {
        units.iter().any(|u| u == unit) || (unit.starts_with("sshd@") && unit.ends_with(".service"))
    }

    /// Add this cycle's lines, expire the window, count.
    fn window(&mut self, i: &Input<'_>) -> Counts {
        let units = std::mem::take(&mut self.units);
        for l in i.journal.iter().filter(|l| Self::wanted(&units, &l.unit)) {
            if let Some((ev, n)) = classify(&l.msg) {
                let t = if l.t == 0 { i.now } else { l.t };
                for _ in 0..n.min(MAX_EVENTS as u64) {
                    if self.events.len() >= MAX_EVENTS {
                        self.events.pop_front();
                    }
                    self.events.push_back((t, ev));
                }
            }
        }
        self.units = units;
        let from = i.now.saturating_sub(WINDOW_S);
        self.events.retain(|(t, _)| *t > from && *t <= i.now);
        let mut c = Counts::default();
        let mut by_src: HashMap<u64, u64> = HashMap::new();
        for (_, ev) in &self.events {
            match ev {
                Ev::Failed(h) => {
                    c.failed_password += 1;
                    if *h != 0 {
                        *by_src.entry(*h).or_default() += 1;
                    }
                }
                Ev::InvalidUser => c.invalid_user += 1,
                Ev::AcceptedPassword => c.accepted_password += 1,
                Ev::AcceptedKeyRoot => c.accepted_publickey_root += 1,
                Ev::AcceptedKeyOther => c.accepted_publickey_other += 1,
                Ev::MaxAuth => c.max_auth_attempts += 1,
                Ev::Preauth => c.preauth_other += 1,
            }
        }
        c.distinct_fail_sources = by_src.len() as u64;
        let top = by_src.values().copied().max().unwrap_or(0);
        c.top_fail_share_pct = (top * 100).checked_div(c.failed_password).unwrap_or(0);
        if i.live {
            c.bytes_added = self.bytes_added(i.now);
        }
        c
    }

    /// Growth of the auth log over the window (stat only). Rotation (size
    /// went down) restarts the baseline.
    fn bytes_added(&mut self, now: u64) -> u64 {
        let Some(p) = &self.auth_log else { return 0 };
        let Ok(size) = std::fs::metadata(p).map(|m| m.len()) else {
            self.sizes.clear();
            return 0;
        };
        if self.sizes.back().is_some_and(|(_, s)| *s > size) {
            self.sizes.clear();
        }
        self.sizes.push_back((now, size));
        // Keep one sample at or before the window start as the baseline.
        let from = now.saturating_sub(WINDOW_S);
        while self.sizes.len() > 1 && self.sizes[1].0 <= from {
            self.sizes.pop_front();
        }
        size.saturating_sub(self.sizes.front().map(|x| x.1).unwrap_or(size))
    }
}

fn vuln(id: &str, sev: Sev, title: String, c: &Counts, fix: &str) -> Finding {
    Finding::new(FindingKind::Vulnerability, id, sev, "host", title)
        .with_evidence(vec![c.line()])
        .with_fix(fix)
        .with_source(SOURCE)
}

impl Sensor for SshAuth {
    fn name(&self) -> &'static str {
        "sshd"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        let c = self.window(i);
        self.last = c;
        if c.accepted_password >= 1 {
            out.finding(vuln(
                "sshd-accepted-password",
                Sev::Critical,
                format!("{} ssh password login(s) accepted in {WINDOW_S} s (password auth should be off)", c.accepted_password),
                &c,
                "check `sshd -T` for passwordauthentication no and who logged in (owner); the nerve never edits sshd",
            ));
        }
        if c.accepted_publickey_other >= 1 {
            let sev = if c.accepted_publickey_other >= 5 {
                Sev::Error
            } else {
                Sev::Warn
            };
            out.finding(vuln(
                "sshd-accepted-publickey-other",
                sev,
                format!("{} non-root ssh key login(s) in {WINDOW_S} s", c.accepted_publickey_other),
                &c,
                "confirm the logins were expected (names are not stored; read the journal on the host)",
            ));
        }
        if c.invalid_user >= 30 {
            out.finding(vuln(
                "sshd-invalid-user",
                Sev::Warn,
                format!("{} ssh invalid-user attempts in {WINDOW_S} s (account enumeration)", c.invalid_user),
                &c,
                "keep password auth off; fail2ban sshd jail and edge rate limits are HADES plans (owner approval)",
            ));
        }
        if c.max_auth_attempts >= 1 {
            out.finding(vuln(
                "sshd-max-auth-attempts",
                Sev::Warn,
                format!(
                    "{} ssh client(s) passed the auth-tries cap in {WINDOW_S} s",
                    c.max_auth_attempts
                ),
                &c,
                "check maxauthtries in `sshd -T`; ban decisions stay with fail2ban / the owner",
            ));
        }
        if let Some((sev, rule)) = ddos_rule(&c) {
            let title = format!(
                "ssh credential flood: {} failed password(s) from {} source(s) in {WINDOW_S} s (rule {rule})",
                c.failed_password, c.distinct_fail_sources
            );
            out.finding(
                Finding::new(FindingKind::Ddos, "sshd-credential-flood", sev, "host", title.clone())
                    .with_evidence(vec![c.line(), format!("top source share {} percent", c.top_fail_share_pct)])
                    .with_fix("password auth stays off; a ban or edge rate limit is a HADES plan (owner approval), never a nerve action")
                    .with_cause("ssh-flood")
                    .with_source(SOURCE),
            );
            out.signal(Signal::new("ssh-flood", "host", sev, title));
        }
        if let Some(sev) = bleed_rule(&c) {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "sshd-auth-log-growth",
                    sev,
                    "host",
                    format!(
                        "auth log grew {} KiB in {WINDOW_S} s (disk bleed)",
                        c.bytes_added / 1024
                    ),
                )
                .with_evidence(vec![c.line()])
                .with_fix(
                    "check logrotate for the auth log; the flood behind it is the ddos finding",
                )
                .with_cause("ssh-flood")
                .with_source(SOURCE),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::probe::JLine;
    use crate::sensors::Sensors;

    fn jl(t: u64, unit: &str, msg: &str) -> JLine {
        JLine {
            t,
            unit: unit.into(),
            prio: 6,
            msg: msg.into(),
        }
    }

    fn fail(t: u64, ip: u32) -> JLine {
        jl(
            t,
            "ssh.service",
            &format!(
                "Failed password for invalid user admin from 198.51.100.{ip} port 4{ip:04} ssh2"
            ),
        )
    }

    fn sensor() -> SshAuth {
        SshAuth::from_env(&|k| match k {
            "NERVE_SSHD" => Some("1".into()),
            "NERVE_SSHD_AUTH_LOG" => Some("0".into()),
            _ => None,
        })
        .unwrap()
    }

    fn run(s: &mut Sensors, now: u64, lines: &[JLine]) -> crate::cortex::Cortex {
        let logs = LogInsights::default();
        let mut i = Input::new(now, &[], &logs);
        i.journal = lines;
        s.run(&i)
    }

    #[test]
    fn off_unless_enabled() {
        assert!(SshAuth::from_env(&|_| None).is_none());
        assert!(SshAuth::from_env(&|k| (k == "NERVE_SSHD").then(|| "0".into())).is_none());
        let s = SshAuth::from_env(&|k| (k == "NERVE_SSHD").then(|| "1".into())).unwrap();
        assert_eq!(s.units, ["ssh.service", "sshd.service"]);
        assert_eq!(
            s.auth_log.as_deref(),
            Some(std::path::Path::new("/var/log/auth.log"))
        );
    }

    #[test]
    fn classify_b41_line_tests() {
        let c = |m: &str| classify(m).map(|x| (std::mem::discriminant(&x.0), x.1));
        let d = |e: Ev| Some((std::mem::discriminant(&e), 1));
        assert_eq!(
            c("Failed password for root from 203.0.113.9 port 22 ssh2"),
            d(Ev::Failed(0))
        );
        assert_eq!(
            c("Failed password for invalid user x from 203.0.113.9 port 22 ssh2"),
            d(Ev::Failed(0))
        );
        assert_eq!(
            c("Invalid user oracle from 203.0.113.9 port 4242"),
            d(Ev::InvalidUser)
        );
        assert_eq!(
            c("Accepted password for bob from 203.0.113.9 port 1 ssh2"),
            d(Ev::AcceptedPassword)
        );
        assert_eq!(
            c("Accepted publickey for root from 203.0.113.9 port 1 ssh2: ED25519 SHA256:x"),
            d(Ev::AcceptedKeyRoot)
        );
        assert_eq!(
            c("Accepted publickey for rooted from 203.0.113.9 port 1 ssh2"),
            d(Ev::AcceptedKeyOther)
        );
        assert_eq!(c("error: maximum authentication attempts exceeded for root from 203.0.113.9 port 1 ssh2 [preauth]"), d(Ev::MaxAuth));
        assert_eq!(
            c("Connection closed by 203.0.113.9 port 1 [preauth]"),
            d(Ev::Preauth)
        );
        assert_eq!(
            c("Received disconnect from 203.0.113.9 port 1:11: Bye"),
            None
        );
        let r = classify(
            "message repeated 4 times: [ Failed password for root from 203.0.113.9 port 22 ssh2]",
        )
        .unwrap();
        assert_eq!(r.1, 4);
        assert_eq!(
            source_hash("Failed password for root from 203.0.113.9 port 22 ssh2"),
            source_hash("Failed password for invalid user from from 203.0.113.9 port 9 ssh2")
        );
        assert_ne!(
            source_hash("x from 203.0.113.9 port 1"),
            source_hash("x from 203.0.113.8 port 1")
        );
    }

    #[test]
    fn ddos_thresholds_verbatim() {
        let c = |f, d, top| Counts {
            failed_password: f,
            distinct_fail_sources: d,
            top_fail_share_pct: top,
            ..Default::default()
        };
        assert_eq!(ddos_rule(&c(29, 9, 100)), None);
        assert_eq!(ddos_rule(&c(30, 1, 100)).unwrap().0, Sev::Warn);
        assert_eq!(ddos_rule(&c(1, 10, 10)).unwrap().0, Sev::Warn);
        assert_eq!(
            ddos_rule(&c(50, 1, 80)),
            Some((Sev::Error, "top_fail_share_pct"))
        );
        assert_eq!(ddos_rule(&c(49, 1, 80)).unwrap().0, Sev::Warn);
        assert_eq!(
            ddos_rule(&c(5, 25, 4)),
            Some((Sev::Error, "distinct_fail_sources"))
        );
        assert_eq!(
            ddos_rule(&c(100, 39, 3)),
            Some((Sev::Error, "failed_password"))
        );
        assert_eq!(ddos_rule(&c(100, 40, 3)).unwrap().0, Sev::Critical);
        assert_eq!(
            ddos_rule(&c(300, 1, 100)),
            Some((Sev::Critical, "failed_password"))
        );
        let b = |n| Counts {
            bytes_added: n,
            ..Default::default()
        };
        assert_eq!(bleed_rule(&b(262_143)), None);
        assert_eq!(bleed_rule(&b(262_144)), Some(Sev::Warn));
        assert_eq!(bleed_rule(&b(1_048_576)), Some(Sev::Error));
        assert_eq!(Sev::Error.b41(), "high");
    }

    #[test]
    fn flood_window_expires_and_counts_only() {
        let now = 1_792_520_000;
        let mut s = Sensors::empty();
        s.add(sensor());
        // 120 failures from 45 sources, one line from an unrelated unit.
        let mut lines: Vec<JLine> = (0..120)
            .map(|k| fail(now - 100 + k / 2, (k % 45) as u32))
            .collect();
        lines.push(jl(
            now,
            "cron.service",
            "Failed password for root from 203.0.113.1 port 1 ssh2",
        ));
        let cx = run(&mut s, now, &lines);
        let f = &cx.ddos_findings[0];
        assert_eq!(
            (f.id.as_str(), f.sev, f.source.as_str()),
            ("sshd-credential-flood", Sev::Critical, "auth.log")
        );
        assert!(
            f.title.contains("120 failed password(s) from 45 source(s)"),
            "{}",
            f.title
        );
        assert_eq!(cx.signals.len(), 1);
        assert_eq!(cx.signals[0].cause, "ssh-flood");
        let ev = cx.nerve_events(now);
        assert_eq!(ev.len(), 1);
        assert_eq!(
            (
                ev[0]["source"].as_str(),
                ev[0]["kind"].as_str(),
                ev[0]["severity"].as_str()
            ),
            (Some("auth.log"), Some("ddos"), Some("crit"))
        );
        let all =
            serde_json::to_string(&cx.to_json()).unwrap() + &serde_json::to_string(&ev).unwrap();
        assert!(
            !all.contains("198.51.100") && !all.contains("admin"),
            "no addresses or names: {all}"
        );
        // Same lines are not re-counted; nothing new for 301 s -> clear.
        let cx = run(&mut s, now + 30, &[]);
        assert_eq!(cx.ddos_findings.len(), 1);
        let cx = run(&mut s, now + 301, &[]);
        assert!(cx.ddos_findings.is_empty() && cx.signals.is_empty());
    }

    #[test]
    fn vulnerability_rules_and_root_key_is_not_a_signal() {
        let now = 1_792_520_000;
        let mut s = Sensors::empty();
        s.add(sensor());
        let mut lines: Vec<JLine> = (0..500)
            .map(|_| {
                jl(
                    now - 10,
                    "ssh.service",
                    "Accepted publickey for root from 192.0.2.7 port 5 ssh2: ED25519 SHA256:k",
                )
            })
            .collect();
        let cx = run(&mut s, now, &lines);
        assert!(
            cx.findings().next().is_none(),
            "500 root key logins are the desk working"
        );
        lines = (0..30)
            .map(|k| {
                jl(
                    now + 1,
                    "sshd.service",
                    &format!("Invalid user u{k} from 192.0.2.{k} port 7"),
                )
            })
            .collect();
        lines.push(jl(
            now + 1,
            "sshd@3-10.0.0.1:22-192.0.2.1:5.service",
            "Accepted publickey for deploy from 192.0.2.1 port 5 ssh2",
        ));
        lines.push(jl(
            now + 1,
            "ssh.service",
            "Accepted password for bob from 192.0.2.1 port 5 ssh2",
        ));
        lines.push(jl(now + 1, "ssh.service", "error: maximum authentication attempts exceeded for root from 192.0.2.1 port 1 ssh2 [preauth]"));
        let cx = run(&mut s, now + 1, &lines);
        let got: Vec<(&str, Sev)> = cx
            .vulnerabilities
            .iter()
            .map(|f| (f.id.as_str(), f.sev))
            .collect();
        assert_eq!(
            got,
            [
                ("sshd-accepted-password", Sev::Critical),
                ("sshd-accepted-publickey-other", Sev::Warn),
                ("sshd-invalid-user", Sev::Warn),
                ("sshd-max-auth-attempts", Sev::Warn),
            ]
        );
        assert!(
            cx.ddos_findings.is_empty(),
            "invalid users alone raise vulnerability, not ddos"
        );
        let ev = cx.nerve_events(now + 1);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0]["severity"], "crit");
        assert_eq!(ev[0]["data"]["top"]["id"], "sshd-accepted-password");
        let s0 = &cx.vulnerabilities[1];
        assert!(
            s0.evidence[0].contains("accepted_publickey_root=500"),
            "{:?}",
            s0.evidence
        );
        assert!(!serde_json::to_string(&cx.to_json())
            .unwrap()
            .contains("deploy"));
    }

    #[test]
    fn auth_log_growth_only_when_live() {
        let dir = std::env::temp_dir().join(format!("nerve-sshd-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("auth.log");
        std::fs::write(&p, vec![b'x'; 1000]).unwrap();
        let mut s = Sensors::empty();
        s.add(SshAuth::new(vec!["ssh.service".into()], Some(p.clone())));
        let logs = LogInsights::default();
        let now = 1_792_520_000;
        let mut i = Input::new(now, &[], &logs);
        assert!(s.run(&i).bleeding.is_empty());
        i.live = true;
        assert!(
            s.run(&i).bleeding.is_empty(),
            "first live sample is the baseline"
        );
        std::fs::write(&p, vec![b'x'; 1000 + 300_000]).unwrap();
        i.now = now + 60;
        let cx = s.run(&i);
        assert_eq!(cx.bleeding[0].id, "sshd-auth-log-growth");
        assert_eq!(cx.bleeding[0].sev, Sev::Warn);
        std::fs::write(&p, vec![b'x'; 1000 + 1_200_000]).unwrap();
        i.now = now + 120;
        assert_eq!(s.run(&i).bleeding[0].sev, Sev::Error);
        // Rotation: the file shrinks -> new baseline, no finding.
        std::fs::write(&p, b"y").unwrap();
        i.now = now + 180;
        assert!(s.run(&i).bleeding.is_empty());
        // Replay (not live) never stats the file.
        i.live = false;
        std::fs::write(&p, vec![b'x'; 5_000_000]).unwrap();
        i.now = now + 200;
        assert!(s.run(&i).bleeding.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
