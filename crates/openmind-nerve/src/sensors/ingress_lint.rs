// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.6 tunnel ingress lint: the cloudflared `ingress:` rules of a config
//! COPY, checked against this cycle's listening sockets. Reuses
//! `crates/tunnel-lint` (parser + rule checks); no new dependency.
//!
//! A rule whose origin is a local port where nothing listens makes
//! cloudflared answer 502 and log an error per request. The built-in `edge`
//! sensor only sees that once traffic arrives (refused dials in the journal);
//! this sensor sees it from the configuration, before the first request, and
//! names the rule.
//!
//! Safety:
//! - Off by default. `NERVE_INGRESS_LINT_CONFIG=<path>` turns it on.
//! - Reads that one file only, read-only, only when [`Input::live`] is true,
//!   re-read only when its size / mtime / inode change, at most 256 KiB.
//! - A COPY only: a path with a `cloudflared` / `.cloudflared` directory
//!   component (where cloudflared keeps its live config and its credentials),
//!   a `.json` / `.pem` file, or text that looks like tunnel credentials or a
//!   certificate is refused, its content dropped and never shown.
//! - Never follows `credentials-file:` / `origincert:` and never repeats
//!   their values (only `ingress:` rules are parsed).
//! - Never dials an origin: open/closed comes from [`Input::listeners`]
//!   (the nerve's own `/proc/net/tcp*` read), so replay is deterministic.
//! - Titles carry counts and ports only; hostnames and services stay in the
//!   evidence (loopback API, never in B33 events).
//!
//! Env: `NERVE_INGRESS_LINT_CONFIG` (on), `NERVE_INGRESS_LINT_COMPONENT`
//! (default `cloudflared`).
//!
//! Findings (source `ingress-lint`, kind bleeding, cause `edge` so an open
//! `edge` incident is not scored twice):
//! - `ingress-closed-<port>`: warn; error when the cloudflared journal window
//!   also shows refused dials to that port (requests are failing now).
//! - `ingress-invalid-origin`: error, a `service:` that is not an origin.
//! - `ingress-no-catch-all`: error, last rule has a hostname or path.
//! - `ingress-shadowed`: warn, duplicate or after-catch-all rules.
//! - `ingress-config`: warn, copy unreadable / too large / unparsable.
//! - `ingress-config-refused` (kind vulnerability): error, the path or text
//!   is not a plain config copy (see Safety).
//! - info `ingress-lint-no-listeners`: listeners not read this cycle, ports
//!   not judged.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use crate::probe::Listener;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use tunnel_lint::Origin;

/// Copies above this size are refused (the VPS config is ~3.3 KiB).
pub const MAX_CONFIG: u64 = 256 * 1024;

/// The sensor's B33 source / registry name.
pub const NAME: &str = "ingress-lint";

/// Why a path or text is not accepted as a config copy (`None`: accepted).
pub fn refuse_path(p: &Path) -> Option<&'static str> {
    for c in p.components() {
        if let Component::Normal(s) = c {
            let s = s.to_string_lossy();
            if s.eq_ignore_ascii_case("cloudflared") || s.eq_ignore_ascii_case(".cloudflared") {
                return Some("path is inside a cloudflared directory (live config / credentials); point the sensor at a copy");
            }
        }
    }
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(ext.as_str(), "json" | "pem" | "key" | "crt") {
        return Some("file type is a credentials/certificate file, not a config copy");
    }
    None
}

/// Text checks after reading: credentials JSON or PEM material is refused.
pub fn refuse_text(t: &str) -> Option<&'static str> {
    let head = t.trim_start();
    if head.starts_with('{')
        || t.contains("TunnelSecret")
        || t.contains("AccountTag")
        || t.contains("-----BEGIN")
    {
        return Some("content looks like tunnel credentials or a certificate, not a config copy");
    }
    None
}

/// `tunnel_lint::Listeners` over the nerve's listener list. A `::` listener
/// counts for IPv4 origins too (Linux dual-stack default, `bindv6only=0`).
struct Socks<'a>(&'a [Listener]);

impl tunnel_lint::Listeners for Socks<'_> {
    fn check(&self, host: &str, port: u16) -> Option<String> {
        let h = host.trim_start_matches('[').trim_end_matches(']');
        let v4 = h == "localhost" || h == "0.0.0.0" || h.starts_with("127.");
        let v6 = h == "localhost" || h == "::1" || h == "::";
        self.0
            .iter()
            .find(|l| {
                l.port == port
                    && (l.addr == "*"
                        || l.addr == "::"
                        || (v4
                            && (l.addr == "0.0.0.0"
                                || l.addr == h
                                || (h == "localhost" && l.addr == "127.0.0.1")))
                        || (v6 && l.addr == "::1"))
            })
            .map(|l| format!("{}:{}", l.addr, l.port))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Meta {
    len: u64,
    mtime: i64,
    ino: u64,
}

fn meta_of(p: &Path) -> std::io::Result<Meta> {
    let m = std::fs::metadata(p)?;
    if !m.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Meta {
            len: m.len(),
            mtime: m.mtime() * 1_000_000_000 + m.mtime_nsec(),
            ino: m.ino(),
        })
    }
    #[cfg(not(unix))]
    {
        let t = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .unwrap_or_default();
        Ok(Meta {
            len: m.len(),
            mtime: t.as_nanos() as i64,
            ino: 0,
        })
    }
}

/// What the sensor currently holds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Text {
    /// Not read yet (or nothing to read: replay without text).
    None,
    Ok(String),
    /// Unreadable / too large / unparsable: warn, message has no content.
    Bad(String),
    /// Not a plain config copy: error, content dropped.
    Refused(&'static str),
}

/// Sensor `ingress-lint`.
pub struct IngressLint {
    path: Option<PathBuf>,
    component: String,
    meta: Option<Meta>,
    text: Text,
    /// File reads done (tests: re-read only on change).
    pub reads: u32,
}

impl IngressLint {
    /// `NERVE_INGRESS_LINT_CONFIG=<path>` on; unset or empty: off.
    pub fn from_env(env: Env) -> Option<Self> {
        let p = env("NERVE_INGRESS_LINT_CONFIG").filter(|v| !v.trim().is_empty())?;
        let mut s = IngressLint::with_path(PathBuf::from(p.trim()));
        if let Some(c) = env("NERVE_INGRESS_LINT_COMPONENT").filter(|v| !v.trim().is_empty()) {
            s.component = c.trim().to_string();
        }
        Some(s)
    }

    /// Live sensor reading `path` (when [`Input::live`]).
    pub fn with_path(path: PathBuf) -> Self {
        let text = match refuse_path(&path) {
            Some(why) => Text::Refused(why),
            None => Text::None,
        };
        IngressLint {
            path: Some(path),
            component: "cloudflared".into(),
            meta: None,
            text,
            reads: 0,
        }
    }

    /// Replay/test sensor over an already captured (redacted) config text.
    /// Never reads the host.
    pub fn from_text(text: &str) -> Self {
        IngressLint {
            path: None,
            component: "cloudflared".into(),
            meta: None,
            text: match refuse_text(text) {
                Some(why) => Text::Refused(why),
                None => Text::Ok(text.to_string()),
            },
            reads: 0,
        }
    }

    fn refresh(&mut self) {
        let Some(p) = self.path.clone() else { return };
        if refuse_path(&p).is_some() {
            return; // path refused at start: never opened
        }
        // A symlink into a cloudflared directory is refused like the path.
        if let Ok(real) = std::fs::canonicalize(&p) {
            if let Some(why) = refuse_path(&real) {
                self.text = Text::Refused(why);
                return;
            }
        }
        let meta = match meta_of(&p) {
            Ok(m) => m,
            Err(e) => {
                self.meta = None;
                self.text = Text::Bad(format!("config copy not readable: {}", e.kind()));
                return;
            }
        };
        if self.meta == Some(meta) && !matches!(self.text, Text::None) {
            return;
        }
        self.meta = Some(meta);
        if meta.len > MAX_CONFIG {
            self.text = Text::Bad(format!(
                "config copy is {} B, above the {} B limit",
                meta.len, MAX_CONFIG
            ));
            return;
        }
        self.reads += 1;
        self.text = match std::fs::read(&p) {
            Ok(b) => match String::from_utf8(b) {
                Ok(t) => match refuse_text(&t) {
                    Some(why) => Text::Refused(why),
                    None => Text::Ok(t),
                },
                Err(_) => Text::Bad("config copy is not UTF-8".into()),
            },
            Err(e) => Text::Bad(format!("config copy not readable: {}", e.kind())),
        };
    }

    fn fix_closed() -> &'static str {
        "start the service behind that port, or remove/comment the rule in the live cloudflared config (`tunnel-lint stage` writes a staged copy to diff; a cloudflared config or DNS change needs owner approval, the nerve never touches either)"
    }
}

impl Sensor for IngressLint {
    fn name(&self) -> &'static str {
        NAME
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if i.live {
            self.refresh();
        }
        let comp = self.component.clone();
        let text = match &self.text {
            Text::None => return,
            Text::Refused(why) => {
                out.finding(
                    Finding::new(
                        FindingKind::Vulnerability,
                        "ingress-config-refused",
                        Sev::Error,
                        comp,
                        "ingress lint refused its input: not a plain cloudflared config copy",
                    )
                    .with_evidence(vec![(*why).to_string()])
                    .with_fix("set NERVE_INGRESS_LINT_CONFIG to a redacted copy of config.yml outside any cloudflared directory (never the credentials file)"),
                );
                return;
            }
            Text::Bad(msg) => {
                out.finding(
                    Finding::new(
                        FindingKind::Bleeding,
                        "ingress-config",
                        Sev::Warn,
                        comp,
                        "ingress lint cannot use its config copy",
                    )
                    .with_evidence(vec![msg.clone()])
                    .with_fix("refresh the config copy NERVE_INGRESS_LINT_CONFIG points at"),
                );
                return;
            }
            Text::Ok(t) => t,
        };
        let rules = match tunnel_lint::parse(text) {
            Ok(r) => r,
            Err(e) => {
                out.finding(
                    Finding::new(
                        FindingKind::Bleeding,
                        "ingress-config",
                        Sev::Warn,
                        comp,
                        "ingress lint cannot parse its config copy",
                    )
                    .with_evidence(vec![format!("parse error at line {}", e.line + 1)])
                    .with_fix("refresh the config copy; check it with `tunnel-lint <copy>` and `cloudflared tunnel ingress validate`"),
                );
                return;
            }
        };
        let none: [Listener; 0] = [];
        let socks = Socks(i.listeners.unwrap_or(&none));
        let rep = tunnel_lint::lint(&rules, &socks);
        let label = |r: &tunnel_lint::Rule| {
            format!(
                "rule #{} {}{} -> {}",
                r.index,
                r.hostname.as_deref().unwrap_or("*"),
                r.path
                    .as_deref()
                    .map(|p| format!(" path {p}"))
                    .unwrap_or_default(),
                r.service
            )
        };

        // Closed local origins, one finding per port.
        if i.listeners.is_none() {
            let local = rep
                .rules
                .iter()
                .filter(|r| matches!(r.origin, Origin::Local { .. }))
                .count();
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "ingress-lint-no-listeners",
                    Sev::Info,
                    comp.clone(),
                    format!("ingress lint: {local} local origin(s) not judged, listeners not read this cycle"),
                )
                .with_cause("edge"),
            );
        } else {
            let mut by_port: BTreeMap<u16, Vec<&tunnel_lint::RuleReport>> = BTreeMap::new();
            for r in rep.closed() {
                if let Origin::Local { port, .. } = r.origin {
                    by_port.entry(port).or_default().push(r);
                }
            }
            for (port, rs) in by_port {
                let mut ev: Vec<String> = rs.iter().map(|r| label(&r.rule)).collect();
                ev.push(format!(
                    "no listener on :{port} in this cycle's socket list ({} sockets)",
                    socks.0.len()
                ));
                let dead = i.logs.dead_origins.get(&port);
                let sev = match dead {
                    Some(d) => {
                        ev.push(format!(
                            "cloudflared journal window: {} refused dial(s), {} failed request(s) on :{port}",
                            d.refusals, d.failed_requests
                        ));
                        Sev::Error
                    }
                    None => {
                        ev.push("no refused dials in the journal window yet (route idle or journal not collected)".into());
                        Sev::Warn
                    }
                };
                if rs
                    .iter()
                    .any(|r| r.rule.hostname.is_none() && r.rule.path.is_none())
                {
                    ev.push(
                        "this is the catch-all rule: every unmatched hostname gets a 502".into(),
                    );
                }
                out.finding(
                    Finding::new(
                        FindingKind::Bleeding,
                        format!("ingress-closed-{port}"),
                        sev,
                        comp.clone(),
                        format!(
                            "tunnel ingress: {} route(s) to local port :{port} where nothing listens (cloudflared answers 502)",
                            rs.len()
                        ),
                    )
                    .with_evidence(ev)
                    .with_fix(Self::fix_closed())
                    .with_cause("edge"),
                );
            }
        }

        // Rule-shape checks (independent of listeners).
        let invalid: Vec<String> = rep
            .rules
            .iter()
            .filter(|r| matches!(r.origin, Origin::Invalid(_)))
            .map(|r| label(&r.rule))
            .collect();
        if !invalid.is_empty() {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "ingress-invalid-origin",
                    Sev::Error,
                    comp.clone(),
                    format!("tunnel ingress: {} rule(s) with a service that is not a valid origin", invalid.len()),
                )
                .with_evidence(invalid)
                .with_fix("correct the service URL (scheme://host:port) in the live config; owner approval")
                .with_cause("edge"),
            );
        }
        let last_ok = rules
            .last()
            .is_some_and(|l| l.hostname.is_none() && l.path.is_none());
        if !last_ok {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "ingress-no-catch-all",
                    Sev::Error,
                    comp.clone(),
                    "tunnel ingress: last rule is not a catch-all (cloudflared refuses to load it)",
                )
                .with_evidence(rules.last().map(|r| vec![label(r)]).unwrap_or_default())
                .with_fix("end the ingress list with `- service: http_status:404`; owner approval")
                .with_cause("edge"),
            );
        }
        let shadowed: Vec<String> = rep
            .findings
            .iter()
            .filter(|f| f.severity == tunnel_lint::Severity::Warn)
            .filter_map(|f| f.rule)
            .filter_map(|n| rules.iter().find(|r| r.index == n))
            .map(label)
            .collect();
        if !shadowed.is_empty() {
            out.finding(
                Finding::new(
                    FindingKind::Bleeding,
                    "ingress-shadowed",
                    Sev::Warn,
                    comp.clone(),
                    format!("tunnel ingress: {} rule(s) can never match (duplicate or after the catch-all)", shadowed.len()),
                )
                .with_evidence(shadowed)
                .with_fix("delete or reorder the shadowed rules in the live config; owner approval")
                .with_cause("edge"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::{DeadOrigin, LogInsights};
    use crate::sensors::Sensors;

    const CFG: &str = "tunnel: 00000000-0000-0000-0000-000000000000
credentials-file: /nonexistent/REDACTED.json
ingress:
- hostname: a.example
  service: http://127.0.0.1:20291
- hostname: b.example
  path: ^/x
  service: http://localhost:20292
- hostname: c.example
  service: http://127.0.0.1:20292
- hostname: d.example
  service: https://remote.example:443
- hostname: e.example
  service: ftp://127.0.0.1:21
- hostname: a.example
  service: http://127.0.0.1:20291
- service: http_status:404
";

    fn l(addr: &str, port: u16) -> Listener {
        Listener {
            addr: addr.into(),
            port,
            loopback: addr.starts_with("127.") || addr == "::1",
        }
    }

    fn run(s: IngressLint, ls: Option<&[Listener]>, logs: &LogInsights) -> crate::cortex::Cortex {
        let mut reg = Sensors::empty();
        reg.add(s);
        let mut i = Input::new(1_792_520_000, &[], logs);
        i.listeners = ls;
        reg.run(&i)
    }

    fn ids(cx: &crate::cortex::Cortex) -> Vec<String> {
        cx.findings().map(|f| f.id.clone()).collect()
    }

    #[test]
    fn closed_port_findings_per_port_and_shape_checks() {
        let ls = [l("127.0.0.1", 20291), l("0.0.0.0", 22)];
        let logs = LogInsights::default();
        let cx = run(IngressLint::from_text(CFG), Some(&ls), &logs);
        let got = ids(&cx);
        assert!(got.contains(&"ingress-closed-20292".to_string()), "{got:?}");
        assert!(!got.iter().any(|g| g == "ingress-closed-20291"), "{got:?}");
        assert!(got.contains(&"ingress-invalid-origin".to_string()));
        assert!(got.contains(&"ingress-shadowed".to_string()));
        assert!(!got.contains(&"ingress-no-catch-all".to_string()));
        let f = cx
            .bleeding
            .iter()
            .find(|f| f.id == "ingress-closed-20292")
            .unwrap();
        assert_eq!(f.sev, Sev::Warn);
        assert_eq!(f.source, NAME);
        assert_eq!(f.cause.as_deref(), Some("edge"));
        assert!(f.title.contains("2 route(s)"), "{}", f.title);
        assert!(!f.title.contains("example"), "no hostnames in titles");
        assert_eq!(
            f.evidence
                .iter()
                .filter(|e| e.starts_with("rule #"))
                .count(),
            2
        );
        // Events: ids/titles only, no hostname, no credentials path.
        let ev = serde_json::to_string(&cx.nerve_events(1_792_520_000)).unwrap();
        assert!(!ev.contains("example") && !ev.contains("REDACTED"), "{ev}");
    }

    #[test]
    fn dead_origin_in_journal_raises_to_error() {
        let ls = [l("127.0.0.1", 20291)];
        let mut logs = LogInsights::default();
        logs.dead_origins.insert(
            20292,
            DeadOrigin {
                port: 20292,
                service: "http://127.0.0.1:20292".into(),
                hosts: Default::default(),
                failed_requests: 7,
                refusals: 9,
                first: 1,
                last: 2,
            },
        );
        let cx = run(IngressLint::from_text(CFG), Some(&ls), &logs);
        let f = cx
            .bleeding
            .iter()
            .find(|f| f.id == "ingress-closed-20292")
            .unwrap();
        assert_eq!(f.sev, Sev::Error);
        assert!(f.evidence.iter().any(|e| e.contains("7 failed request")));
    }

    #[test]
    fn dual_stack_and_v6_listeners_count_as_open() {
        let ls = [l("::", 20291), l("::1", 20292)];
        let logs = LogInsights::default();
        let cx = run(IngressLint::from_text(CFG), Some(&ls), &logs);
        let got = ids(&cx);
        // ::1 satisfies localhost (b.example) but not 127.0.0.1 (c.example).
        let f = cx
            .bleeding
            .iter()
            .find(|f| f.id == "ingress-closed-20292")
            .expect("c.example still closed");
        assert!(f.title.contains("1 route(s)"), "{got:?}");
        assert!(!got.iter().any(|g| g == "ingress-closed-20291"));
    }

    #[test]
    fn no_listeners_is_info_only() {
        let logs = LogInsights::default();
        let cx = run(IngressLint::from_text(CFG), None, &logs);
        let got = ids(&cx);
        assert!(got.contains(&"ingress-lint-no-listeners".to_string()));
        assert!(!got.iter().any(|g| g.starts_with("ingress-closed-")));
    }

    #[test]
    fn missing_catch_all_and_parse_error() {
        let logs = LogInsights::default();
        let ls: [Listener; 0] = [];
        let cx = run(
            IngressLint::from_text("ingress:\n- hostname: x\n  service: http_status:404\n"),
            Some(&ls),
            &logs,
        );
        assert!(ids(&cx).contains(&"ingress-no-catch-all".to_string()));
        let cx = run(IngressLint::from_text("tunnel: x\n"), Some(&ls), &logs);
        assert_eq!(ids(&cx), ["ingress-config"]);
        assert!(cx.bleeding[0].evidence[0].starts_with("parse error"));
    }

    #[test]
    fn credentials_and_live_dirs_refused() {
        for p in [
            "/etc/cloudflared/config.yml",
            "/root/.cloudflared/config.yml",
            "/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml",
            "/var/lib/nerve/tunnel.json",
            "/var/lib/nerve/cert.pem",
        ] {
            assert!(refuse_path(Path::new(p)).is_some(), "{p}");
        }
        assert!(refuse_path(Path::new("/var/lib/galion-nerve/ingress-copy.yml")).is_none());
        let logs = LogInsights::default();
        let creds = r#"{"AccountTag":"x","TunnelSecret":"SECRETVALUE","TunnelID":"y"}"#;
        let cx = run(IngressLint::from_text(creds), Some(&[]), &logs);
        assert_eq!(ids(&cx), ["ingress-config-refused"]);
        let all = serde_json::to_string(&cx.to_json()).unwrap();
        assert!(!all.contains("SECRETVALUE"), "content never shown");
        // A refused path is never opened, even live.
        let mut s = IngressLint::with_path("/etc/cloudflared/config.yml".into());
        let mut i = Input::new(1, &[], &logs);
        i.live = true;
        let mut reg = Sensors::empty();
        s.refresh();
        assert_eq!(s.reads, 0);
        reg.add(s);
        let cx = reg.run(&i);
        assert_eq!(ids(&cx), ["ingress-config-refused"]);
    }

    #[test]
    fn live_reads_only_when_live_and_only_on_change() {
        let dir = std::env::temp_dir().join(format!("nerve-ingress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("ingress-copy.yml");
        std::fs::write(&p, CFG).unwrap();
        let logs = LogInsights::default();
        let ls = [l("127.0.0.1", 20291), l("127.0.0.1", 20292)];
        let mut s = IngressLint::with_path(p.clone());
        // Not live: nothing read, nothing reported.
        let mut cx = crate::cortex::Cortex::default();
        let mut i = Input::new(1, &[], &logs);
        i.listeners = Some(&ls);
        s.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!((s.reads, cx.findings().count()), (0, 0));
        i.live = true;
        for _ in 0..3 {
            s.sense(&i, &mut Sink::new(&mut cx, NAME));
        }
        assert_eq!(s.reads, 1, "re-read only on change");
        std::fs::write(&p, format!("{CFG}# changed size\n")).unwrap();
        s.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!(s.reads, 2);
        std::fs::remove_file(&p).unwrap();
        let mut cx = crate::cortex::Cortex::default();
        s.sense(&i, &mut Sink::new(&mut cx, NAME));
        assert_eq!(cx.bleeding[0].id, "ingress-config");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn from_env_off_by_default() {
        assert!(IngressLint::from_env(&|_| None).is_none());
        assert!(
            IngressLint::from_env(&|k| (k == "NERVE_INGRESS_LINT_CONFIG").then(String::new))
                .is_none()
        );
        let s = IngressLint::from_env(&|k| match k {
            "NERVE_INGRESS_LINT_CONFIG" => Some("/tmp/x.yml".into()),
            "NERVE_INGRESS_LINT_COMPONENT" => Some("tunnel".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(s.component, "tunnel");
        let env = |k: &str| (k == "NERVE_INGRESS_LINT_CONFIG").then(|| "/tmp/x.yml".to_string());
        // main-staging: edge-rate (cortex-merge-all) registers after ingress-lint.
        assert!(Sensors::from_env(&env).names().contains(&NAME));
    }
}
