// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! fail2ban log format: writer (shadow mode) and reader (comparison).
//!
//! fail2ban 1.0.2 writes `%(asctime)s %(name)-24s[%(process)d]: %(levelname)-7s %(message)s`:
//! ```text
//! 2026-10-21 02:21:52,185 fail2ban.filter         [1268562]: INFO    [sshd] Found 203.0.113.5 - 2026-10-21 02:21:52
//! 2026-10-21 02:21:52,883 fail2ban.actions        [1268562]: NOTICE  [sshd] Ban 203.0.113.5
//! 2026-10-19 00:03:02,450 fail2ban.observer       [1268562]: NOTICE  [sshd] Increase Ban 203.0.113.5 (2 # 2h -> 2026-10-19 02:03:01)
//! 2026-10-21 02:24:41,150 fail2ban.actions        [1268562]: NOTICE  [sshd] Unban 203.0.113.5
//! ```
//! (shapes probed read-only on the VPS 2026-10-21 04:40 +02:00). The shadow
//! log uses the same layout with the logger names `galion-guard.*`, so the
//! two files can be read side by side, diffed and parsed by one reader.
//! The stamps carry no offset (fail2ban's own format), so the zone is a
//! parameter on both sides: the VPS fail2ban writes the system zone (UTC),
//! the shadow log defaults to Europe/Zagreb.

use std::io::Write;
use std::net::IpAddr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use crate::engine::Event;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Zone {
    Utc,
    Zagreb,
}

impl Zone {
    pub fn parse(s: &str) -> Result<Zone, String> {
        match s.to_ascii_lowercase().as_str() {
            "utc" => Ok(Zone::Utc),
            "zagreb" | "europe/zagreb" => Ok(Zone::Zagreb),
            _ => Err(format!("zone must be zagreb or utc, not {s:?}")),
        }
    }
}

fn utc(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).unwrap_or_default()
}

/// `YYYY-MM-DD HH:MM:SS` in `zone` (fail2ban `MyTime.time2str`).
pub fn fmt_time(secs: i64, zone: Zone) -> String {
    const F: &str = "%Y-%m-%d %H:%M:%S";
    match zone {
        Zone::Utc => utc(secs).format(F).to_string(),
        Zone::Zagreb => utc(secs)
            .with_timezone(&Tz::Europe__Zagreb)
            .format(F)
            .to_string(),
    }
}

/// Inverse of [`fmt_time`]; also accepts a `,mmm` fraction (ignored). In the
/// one ambiguous autumn hour a Zagreb stamp is read as the earlier (CEST)
/// instant; a stamp in the spring gap is rejected.
pub fn parse_time(s: &str, zone: Zone) -> Option<i64> {
    let s = s.split(',').next()?;
    let n = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()?;
    match zone {
        Zone::Utc => Some(n.and_utc().timestamp()),
        Zone::Zagreb => Tz::Europe__Zagreb
            .from_local_datetime(&n)
            .earliest()
            .map(|d| d.timestamp()),
    }
}

/// One record of a fail2ban-format log (jail lines only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Record {
    Found {
        at: i64,
        ip: IpAddr,
        logged: i64,
    },
    Ban {
        at: i64,
        ip: IpAddr,
        restored: bool,
    },
    Increase {
        at: i64,
        ip: IpAddr,
        count: u32,
        secs: u64,
    },
    Unban {
        at: i64,
        ip: IpAddr,
    },
}

impl Record {
    pub fn at(&self) -> i64 {
        match self {
            Record::Found { at, .. }
            | Record::Ban { at, .. }
            | Record::Increase { at, .. }
            | Record::Unban { at, .. } => *at,
        }
    }
    pub fn ip(&self) -> IpAddr {
        match self {
            Record::Found { ip, .. }
            | Record::Ban { ip, .. }
            | Record::Increase { ip, .. }
            | Record::Unban { ip, .. } => *ip,
        }
    }
}

fn ip_of(s: &str) -> Option<IpAddr> {
    s.parse::<IpAddr>().ok().map(crate::ipnet::canonical)
}

/// Parse one line. `None` for other jails, start-up chatter, `Ignore`,
/// `already banned` and anything malformed.
pub fn parse_line(line: &str, zone: Zone, jail: &str) -> Option<Record> {
    // "<date> <time>,<ms> <logger> [<pid>]: <LEVEL> [<jail>] <message>"
    let line = line.trim_end_matches(['\r', '\n']);
    if line.len() < 24 || !line.is_char_boundary(23) {
        return None;
    }
    let at = parse_time(&line[..23], zone)?;
    let rest = &line[23..];
    let close = rest.find("]: ")?;
    let after = rest[close + 3..].trim_start();
    // level
    let (_, after) = after.split_once(' ')?;
    let after = after.trim_start();
    let tag = format!("[{jail}] ");
    let msg = after.strip_prefix(tag.as_str())?;
    if let Some(m) = msg.strip_prefix("Found ") {
        let (ip, logged) = m.split_once(" - ")?;
        let logged = parse_time(logged.trim(), zone)?;
        return Some(Record::Found {
            at,
            ip: ip_of(ip)?,
            logged,
        });
    }
    if let Some(m) = msg.strip_prefix("Restore Ban ") {
        return Some(Record::Ban {
            at,
            ip: ip_of(m.trim())?,
            restored: true,
        });
    }
    if let Some(m) = msg.strip_prefix("Increase Ban ") {
        // "<ip> (<n> # <dur> -> <end>)"
        let (ip, tail) = m.split_once(" (")?;
        let (n, tail) = tail.split_once(" # ")?;
        let (dur, _) = tail.split_once(" -> ")?;
        return Some(Record::Increase {
            at,
            ip: ip_of(ip)?,
            count: n.trim().parse().ok()?,
            secs: crate::duration::parse(dur).ok()?,
        });
    }
    if let Some(m) = msg.strip_prefix("Ban ") {
        return Some(Record::Ban {
            at,
            ip: ip_of(m.trim())?,
            restored: false,
        });
    }
    if let Some(m) = msg.strip_prefix("Unban ") {
        return Some(Record::Unban {
            at,
            ip: ip_of(m.trim())?,
        });
    }
    None
}

pub fn parse_log(text: &str, zone: Zone, jail: &str) -> Vec<Record> {
    text.lines()
        .filter_map(|l| parse_line(l, zone, jail))
        .collect()
}

/// `<asctime> <logger:24>[<pid>]: <LEVEL:7> <message>`.
pub fn format_line(at: i64, zone: Zone, logger: &str, pid: u32, level: &str, msg: &str) -> String {
    format!(
        "{},000 {:<24}[{pid}]: {:<7} {msg}",
        fmt_time(at, zone),
        format!("galion-guard.{logger}"),
        level
    )
}

/// The fail2ban-format lines for one engine event (empty for events
/// fail2ban has no line for). Protected addresses are never written.
pub fn event_lines(ev: &Event, zone: Zone, pid: u32, jail: &str) -> Vec<String> {
    let l = |at, logger, level, msg: String| format_line(at, zone, logger, pid, level, &msg);
    match ev {
        Event::Found { at, ip, logged } => vec![l(
            *at,
            "filter",
            "INFO",
            format!("[{jail}] Found {ip} - {}", fmt_time(*logged, zone)),
        )],
        Event::Ban {
            ip,
            at,
            until,
            count,
            failures,
            manual,
        } => {
            let mut v = vec![l(*at, "actions", "NOTICE", format!("[{jail}] Ban {ip}"))];
            if *count >= 2 {
                v.push(l(
                    *at,
                    "observer",
                    "NOTICE",
                    format!(
                        "[{jail}] Increase Ban {ip} ({count} # {} -> {})",
                        crate::duration::format((until - at).max(0) as u64),
                        fmt_time(*until, zone)
                    ),
                ));
            }
            v.push(l(
                *at,
                "shadow",
                "INFO",
                format!(
                    "[{jail}] would ban {ip} until {} ({failures} failures{}; no firewall write)",
                    zagreb_time::from_unix(*until),
                    if *manual { ", manual" } else { "" }
                ),
            ));
            v
        }
        Event::Restored { at, ip, until } => vec![
            l(
                *at,
                "actions",
                "NOTICE",
                format!("[{jail}] Restore Ban {ip}"),
            ),
            l(
                *at,
                "shadow",
                "INFO",
                format!(
                    "[{jail}] restored from state, until {}",
                    zagreb_time::from_unix(*until)
                ),
            ),
        ],
        Event::Unban { ip, at, reason } => vec![
            l(*at, "actions", "NOTICE", format!("[{jail}] Unban {ip}")),
            l(
                *at,
                "shadow",
                "INFO",
                format!("[{jail}] unban reason: {reason}"),
            ),
        ],
        Event::Protected { at, reason } => vec![l(
            *at,
            "shadow",
            "INFO",
            format!("[{jail}] Ignore <protected address withheld> by {reason:?}"),
        )],
        Event::Unresolved { at, rule } => vec![l(
            *at,
            "shadow",
            "WARNING",
            format!("[{jail}] non-IP host ignored (rule {rule}, no DNS lookups)"),
        )],
        Event::BackendError { at, error } => vec![l(
            *at,
            "shadow",
            "ERROR",
            format!("[{jail}] backend error: {error}"),
        )],
    }
}

/// Append-only shadow log (mode 0600). Opened per write, so logrotate's
/// plain rename works without a signal.
#[derive(Debug, Clone)]
pub struct ShadowLog {
    pub path: PathBuf,
    pub zone: Zone,
    pub pid: u32,
    pub jail: String,
    pub write_errors: u64,
}

impl ShadowLog {
    pub fn new(path: PathBuf, zone: Zone) -> Self {
        ShadowLog {
            path,
            zone,
            pid: std::process::id(),
            jail: "sshd".into(),
            write_errors: 0,
        }
    }

    pub fn write_lines(&mut self, lines: &[String]) {
        if lines.is_empty() {
            return;
        }
        let r = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
            .and_then(|mut f| {
                let mut buf = String::new();
                for l in lines {
                    buf.push_str(l);
                    buf.push('\n');
                }
                f.write_all(buf.as_bytes())
            });
        if r.is_err() {
            self.write_errors += 1;
        }
    }

    pub fn write_event(&mut self, ev: &Event) {
        let lines = event_lines(ev, self.zone, self.pid, &self.jail);
        self.write_lines(&lines);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VPS: &str = "\
2026-10-18 18:10:04,207 fail2ban.filtersystemd  [1268562]: INFO    [sshd] Added journal match for: '_SYSTEMD_UNIT=sshd.service + _COMM=sshd'
2026-10-18 18:10:04,440 fail2ban.actions        [1268562]: NOTICE  [sshd] 203.0.113.9 already banned
2026-10-21 02:21:49,719 fail2ban.filter         [1268562]: INFO    [sshd] Found 203.0.113.5 - 2026-10-21 02:21:49
2026-10-21 02:21:52,883 fail2ban.actions        [1268562]: NOTICE  [sshd] Ban 203.0.113.5
2026-10-19 00:03:02,450 fail2ban.observer       [1268562]: INFO    [sshd] IP 203.0.113.5 is bad: 1 # last 2026-10-18 22:54:58 - incr 1h to 2h
2026-10-19 00:03:02,450 fail2ban.observer       [1268562]: NOTICE  [sshd] Increase Ban 203.0.113.5 (2 # 2h -> 2026-10-19 02:03:01)
2026-10-21 02:24:41,150 fail2ban.actions        [1268562]: NOTICE  [sshd] Unban 203.0.113.5
2026-10-21 02:24:41,150 fail2ban.actions        [1268562]: NOTICE  [sshd] Restore Ban 2001:db8::7
2026-10-21 02:24:41,150 fail2ban.actions        [1268562]: NOTICE  [recidive] Ban 198.51.100.1
";

    #[test]
    fn parses_vps_shapes() {
        let r = parse_log(VPS, Zone::Utc, "sshd");
        assert_eq!(r.len(), 5, "{r:?}");
        let t = parse_time("2026-10-21 02:21:49", Zone::Utc).unwrap();
        assert_eq!(
            r[0],
            Record::Found {
                at: t,
                ip: "203.0.113.5".parse().unwrap(),
                logged: t
            }
        );
        assert!(matches!(
            r[1],
            Record::Ban {
                restored: false,
                ..
            }
        ));
        assert!(matches!(
            r[2],
            Record::Increase {
                count: 2,
                secs: 7200,
                ..
            }
        ));
        assert!(matches!(r[3], Record::Unban { .. }));
        assert!(matches!(r[4], Record::Ban { restored: true, .. }));
    }

    #[test]
    fn zones_roundtrip() {
        // 2026-10-21 02:21:49 UTC = 04:21:49 CEST
        let t = parse_time("2026-10-21 02:21:49", Zone::Utc).unwrap();
        assert_eq!(fmt_time(t, Zone::Zagreb), "2026-10-21 04:21:49");
        assert_eq!(parse_time("2026-10-21 04:21:49,123", Zone::Zagreb), Some(t));
        // Winter: +01:00.
        let w = parse_time("2026-12-01 10:00:00", Zone::Utc).unwrap();
        assert_eq!(fmt_time(w, Zone::Zagreb), "2026-12-01 11:00:00");
        // Spring gap does not exist; autumn overlap takes CEST.
        assert_eq!(parse_time("2026-03-29 02:30:00", Zone::Zagreb), None);
        let a = parse_time("2026-10-25 02:30:00", Zone::Zagreb).unwrap();
        assert_eq!(fmt_time(a, Zone::Utc), "2026-10-25 00:30:00");
        assert!(Zone::parse("Europe/Zagreb").is_ok() && Zone::parse("cet").is_err());
    }

    #[test]
    fn writer_output_reads_back() {
        let ip: IpAddr = "203.0.113.5".parse().unwrap();
        let t = 1_792_519_200;
        let evs = [
            Event::Found {
                at: t,
                ip,
                logged: t - 1,
            },
            Event::Ban {
                ip,
                at: t,
                until: t + 7200,
                count: 2,
                failures: 3,
                manual: false,
            },
            Event::Protected {
                at: t,
                reason: crate::jail::Protected::Allowlist,
            },
            Event::Unban {
                ip,
                at: t + 7200,
                reason: "expired".into(),
            },
        ];
        let mut text = String::new();
        for e in &evs {
            for l in event_lines(e, Zone::Zagreb, 42, "sshd") {
                text += &l;
                text.push('\n');
            }
        }
        // Same column layout as fail2ban.
        let first = text.lines().next().unwrap();
        assert_eq!(
            first,
            "2026-10-20 20:00:00,000 galion-guard.filter     [42]: INFO    [sshd] Found 203.0.113.5 - 2026-10-20 19:59:59"
        );
        assert!(text
            .contains("NOTICE  [sshd] Increase Ban 203.0.113.5 (2 # 2h -> 2026-10-20 22:00:00)"));
        assert!(text.contains("<protected address withheld>"));
        let r = parse_log(&text, Zone::Zagreb, "sshd");
        assert_eq!(
            r,
            vec![
                Record::Found {
                    at: t,
                    ip,
                    logged: t - 1
                },
                Record::Ban {
                    at: t,
                    ip,
                    restored: false
                },
                Record::Increase {
                    at: t,
                    ip,
                    count: 2,
                    secs: 7200
                },
                Record::Unban { at: t + 7200, ip },
            ]
        );
    }

    #[test]
    fn junk_is_ignored() {
        for l in [
            "",
            "x",
            "2026-10-21 02:21:49,719",
            "2026-10-21 02:21:49,719 fail2ban.filter [1]: INFO    [sshd] Found nothere - 2026-10-21 02:21:49",
            "2026-13-21 02:21:49,719 fail2ban.filter [1]: INFO    [sshd] Ban 203.0.113.5",
            "2026-10-21 02:21:49,719 fail2ban.filter [1]: INFO    [sshd] Increase Ban 203.0.113.5 (x # 2h -> y)",
            "ééééééééééééééééééééééééééé",
        ] {
            assert_eq!(parse_line(l, Zone::Utc, "sshd"), None, "{l}");
        }
    }
}
