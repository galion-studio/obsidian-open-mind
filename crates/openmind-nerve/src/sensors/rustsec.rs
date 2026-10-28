// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.4 `rustsec`: RustSec advisories against a `Cargo.lock`, fully offline.
//!
//! The advisory database is a committed snapshot
//! (`data/rustsec/advisories.tsv`, CC0-1.0 data from
//! <https://github.com/rustsec/advisory-db>, built by
//! `examples/rustsec_snapshot.rs`) embedded in the binary; nothing is
//! fetched. A package is affected when its version matches no `patched`
//! and no `unaffected` requirement (the RustSec rule). Only crates.io
//! packages are judged (git and path packages are not the advisory's crate).
//!
//! One `vulnerability` finding per affected package version:
//! `rustsec-<package>-<version>`, severity = worst advisory:
//! CVSS v3 >= 9.0 -> critical (B41 `crit`), other vulnerabilities -> error
//! (`high`), `unsound` -> warn, `unmaintained` / `notice` -> info (shown,
//! not scored). Titles and evidence carry the package, version and
//! advisory ids only.
//!
//! Host reads (the lock file, an optional DB override) happen only when
//! [`Input::live`]; the result is cached and refreshed when the lock's
//! mtime changes or every `NERVE_RUSTSEC_EVERY_S` (default 3600 s).
//!
//! Env (sensor off unless `NERVE_RUSTSEC_LOCK` is set):
//! - `NERVE_RUSTSEC_LOCK` path of the `Cargo.lock` of the deployed SHA.
//! - `NERVE_RUSTSEC_COMPONENT` component label (default `workspace`).
//! - `NERVE_RUSTSEC_DB` a newer snapshot file in the same format (optional).
//! - `NERVE_RUSTSEC_EVERY_S` refresh period in seconds (default 3600).

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::SystemTime;

/// The embedded offline snapshot (feature `rustsec-snapshot`, on by default;
/// a build without it needs `NERVE_RUSTSEC_DB`).
#[cfg(feature = "rustsec-snapshot")]
pub const SNAPSHOT: &str = include_str!("../../data/rustsec/advisories.tsv");

/// Advisory kind (`informational` in RustSec, `vuln` when absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Notice,
    Unmaintained,
    Unsound,
    Vuln,
}

/// One advisory row of the snapshot.
#[derive(Debug, Clone)]
pub struct Advisory {
    pub id: String,
    pub package: String,
    pub kind: Kind,
    pub cvss3: Option<f32>,
    pub patched: Vec<VersionReq>,
    pub unaffected: Vec<VersionReq>,
}

impl Advisory {
    pub fn sev(&self) -> Sev {
        match self.kind {
            Kind::Vuln if self.cvss3.is_some_and(|s| s >= 9.0) => Sev::Critical,
            Kind::Vuln => Sev::Error,
            Kind::Unsound => Sev::Warn,
            Kind::Unmaintained | Kind::Notice => Sev::Info,
        }
    }

    pub fn affects(&self, v: &Version) -> bool {
        !self
            .patched
            .iter()
            .chain(&self.unaffected)
            .any(|r| r.matches(v))
    }

    fn kind_str(&self) -> &'static str {
        match self.kind {
            Kind::Vuln => "vulnerability",
            Kind::Unsound => "unsound",
            Kind::Unmaintained => "unmaintained",
            Kind::Notice => "notice",
        }
    }
}

/// Parsed snapshot, indexed by package.
#[derive(Debug, Default)]
pub struct Db {
    pub commit: String,
    pub by_package: HashMap<String, Vec<Advisory>>,
    pub count: usize,
    /// Rows or requirements that did not parse (should be 0; tested).
    pub bad: usize,
}

impl Db {
    pub fn parse(text: &str) -> Db {
        let mut db = Db::default();
        for line in text.lines() {
            if let Some(c) = line.strip_prefix("# commit: ") {
                db.commit = c.split_whitespace().next().unwrap_or("").to_string();
            }
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 6 {
                db.bad += 1;
                continue;
            }
            let kind = match f[2] {
                "vuln" => Kind::Vuln,
                "unsound" => Kind::Unsound,
                "unmaintained" => Kind::Unmaintained,
                "notice" => Kind::Notice,
                _ => {
                    db.bad += 1;
                    continue;
                }
            };
            let mut reqs = |s: &str| -> Vec<VersionReq> {
                s.split(';')
                    .filter(|r| *r != "-" && !r.is_empty())
                    .filter_map(|r| match VersionReq::parse(r) {
                        Ok(v) => Some(v),
                        Err(_) => {
                            db.bad += 1;
                            None
                        }
                    })
                    .collect()
            };
            let patched = reqs(f[4]);
            let unaffected = reqs(f[5]);
            let a = Advisory {
                id: f[0].to_string(),
                package: f[1].to_string(),
                kind,
                cvss3: f[3].parse().ok(),
                patched,
                unaffected,
            };
            db.count += 1;
            db.by_package.entry(a.package.clone()).or_default().push(a);
        }
        db
    }
}

/// crates.io packages of a `Cargo.lock` (v1-v4): (name, version). Git and
/// path packages are skipped.
pub fn lock_packages(text: &str) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut cur: Option<(String, String, bool)> = None;
    let mut flush = |c: &mut Option<(String, String, bool)>| {
        if let Some((n, v, reg)) = c.take() {
            if reg && !n.is_empty() && !v.is_empty() {
                out.push((n, v));
            }
        }
    };
    let val = |l: &str, k: &str| -> Option<String> {
        let r = l.strip_prefix(k)?.trim_start().strip_prefix('=')?.trim();
        Some(r.trim_matches('"').to_string())
    };
    for l in text.lines().map(str::trim) {
        if l.starts_with('[') {
            flush(&mut cur);
            if l == "[[package]]" {
                cur = Some(Default::default());
            }
            continue;
        }
        let Some(c) = cur.as_mut() else { continue };
        if let Some(v) = val(l, "name") {
            c.0 = v;
        } else if let Some(v) = val(l, "version") {
            c.1 = v;
        } else if let Some(v) = val(l, "source") {
            c.2 = v.starts_with("registry+") || v.starts_with("sparse+");
        }
    }
    flush(&mut cur);
    out
}

/// One affected package version and its advisories.
#[derive(Debug, Clone)]
pub struct Hit<'a> {
    pub package: String,
    pub version: String,
    pub advisories: Vec<&'a Advisory>,
}

/// Every affected package of `lock` (sorted by package, version).
pub fn audit<'a>(db: &'a Db, lock: &[(String, String)]) -> Vec<Hit<'a>> {
    let mut hits: BTreeMap<(String, String), Vec<&Advisory>> = BTreeMap::new();
    for (n, v) in lock {
        let (Some(advs), Ok(ver)) = (db.by_package.get(n), Version::parse(v)) else {
            continue;
        };
        for a in advs.iter().filter(|a| a.affects(&ver)) {
            hits.entry((n.clone(), v.clone())).or_default().push(a);
        }
    }
    hits.into_iter()
        .map(|((package, version), mut advisories)| {
            advisories.sort_by(|a, b| a.id.cmp(&b.id));
            Hit {
                package,
                version,
                advisories,
            }
        })
        .collect()
}

/// Findings for an audit result (`component` = lock label).
pub fn findings(hits: &[Hit<'_>], component: &str) -> Vec<Finding> {
    hits.iter()
        .map(|h| {
            let sev = h
                .advisories
                .iter()
                .map(|a| a.sev())
                .max()
                .unwrap_or(Sev::Info);
            let ids: Vec<&str> = h.advisories.iter().map(|a| a.id.as_str()).collect();
            let fixed: Vec<String> = h
                .advisories
                .iter()
                .flat_map(|a| a.patched.iter().map(|r| r.to_string()))
                .collect();
            let ev = h
                .advisories
                .iter()
                .map(|a| {
                    let p: Vec<String> = a.patched.iter().map(|r| r.to_string()).collect();
                    format!(
                        "{} {}{} patched: {}",
                        a.id,
                        a.kind_str(),
                        a.cvss3
                            .map(|s| format!(" cvss3 {s:.1}"))
                            .unwrap_or_default(),
                        if p.is_empty() {
                            "none".into()
                        } else {
                            p.join(" | ")
                        }
                    )
                })
                .collect();
            Finding::new(
                FindingKind::Vulnerability,
                format!("rustsec-{}-{}", h.package, h.version),
                sev,
                component,
                format!(
                    "{} {} in Cargo.lock: {} RustSec advisor{} ({})",
                    h.package,
                    h.version,
                    ids.len(),
                    if ids.len() == 1 { "y" } else { "ies" },
                    ids.join(", ")
                ),
            )
            .with_evidence(ev)
            .with_fix(if fixed.is_empty() {
                format!(
                    "no patched release of {}: replace the crate or accept the risk (owner)",
                    h.package
                )
            } else {
                format!(
                    "cargo update -p {}@{} to a patched version ({})",
                    h.package,
                    h.version,
                    fixed.join(" | ")
                )
            })
            .with_cause(format!("rustsec:{component}"))
        })
        .collect()
}

/// Sensor `rustsec` (T2.4).
pub struct RustSec {
    lock: PathBuf,
    component: String,
    db_path: Option<PathBuf>,
    every_s: u64,
    checked_at: Option<u64>,
    lock_mtime: Option<SystemTime>,
    cached: Vec<Finding>,
    /// Packages judged and advisories loaded at the last refresh.
    pub last: (usize, usize),
}

impl RustSec {
    pub fn new(lock: PathBuf, component: &str, db_path: Option<PathBuf>, every_s: u64) -> Self {
        RustSec {
            lock,
            component: component.to_string(),
            db_path,
            every_s,
            checked_at: None,
            lock_mtime: None,
            cached: vec![],
            last: (0, 0),
        }
    }

    pub fn from_env(env: Env) -> Option<Self> {
        let lock = env("NERVE_RUSTSEC_LOCK").filter(|s| !s.is_empty())?;
        Some(Self::new(
            PathBuf::from(lock),
            &env("NERVE_RUSTSEC_COMPONENT").unwrap_or_else(|| "workspace".into()),
            env("NERVE_RUSTSEC_DB")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            env("NERVE_RUSTSEC_EVERY_S")
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
        ))
    }

    fn refresh(&mut self, now: u64) {
        let comp = self.component.clone();
        let unreadable = |what: String| {
            vec![Finding::new(
                FindingKind::Vulnerability,
                "rustsec-unreadable",
                Sev::Info,
                comp.clone(),
                format!("rustsec check skipped: {what}"),
            )]
        };
        self.checked_at = Some(now);
        self.lock_mtime = std::fs::metadata(&self.lock)
            .and_then(|m| m.modified())
            .ok();
        let lock = match std::fs::read_to_string(&self.lock) {
            Ok(t) => t,
            Err(e) => {
                self.cached = unreadable(format!("Cargo.lock not readable ({})", e.kind()));
                return;
            }
        };
        let db_text;
        let db = match &self.db_path {
            #[cfg(feature = "rustsec-snapshot")]
            None => Db::parse(SNAPSHOT),
            #[cfg(not(feature = "rustsec-snapshot"))]
            None => {
                self.cached = unreadable(
                    "no advisory snapshot embedded in this build; set NERVE_RUSTSEC_DB".into(),
                );
                return;
            }
            Some(p) => match std::fs::read_to_string(p) {
                Ok(t) => {
                    db_text = t;
                    Db::parse(&db_text)
                }
                Err(e) => {
                    self.cached =
                        unreadable(format!("advisory snapshot not readable ({})", e.kind()));
                    return;
                }
            },
        };
        let pkgs = lock_packages(&lock);
        self.last = (pkgs.len(), db.count);
        self.cached = findings(&audit(&db, &pkgs), &self.component);
    }
}

impl Sensor for RustSec {
    fn name(&self) -> &'static str {
        "rustsec"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return;
        }
        let mtime = std::fs::metadata(&self.lock)
            .and_then(|m| m.modified())
            .ok();
        let due = match self.checked_at {
            None => true,
            Some(t) => mtime != self.lock_mtime || i.now.saturating_sub(t) >= self.every_s,
        };
        if due {
            self.refresh(i.now);
        }
        for f in &self.cached {
            out.finding(f.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;

    const DB: &str = "# commit: abc123 2026-10-20T21:24:19+02:00\n\
RUSTSEC-2021-0003\tsmallvec\tvuln\t9.8\t>= 0.6.14, < 1.0.0;>= 1.6.1\t< 0.6.3\tCVE-2021-25900\n\
RUSTSEC-2020-0071\ttime\tvuln\t6.2\t>= 0.2.23\t= 0.2.0;= 0.2.1;= 0.2.2;= 0.2.3;= 0.2.4;= 0.2.5;= 0.2.6\t-\n\
RUSTSEC-2021-0139\tansi_term\tunmaintained\t-\t-\t-\t-\n\
RUSTSEC-2021-0145\tatty\tunsound\t-\t-\t-\t-\n\
RUSTSEC-2099-0001\tserde\tvuln\t-\t>= 1.0.0\t-\t-\n";

    const LOCK: &str = r#"version = 4

[[package]]
name = "smallvec"
version = "1.6.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "time"
version = "0.1.45"
source = "registry+https://github.com/rust-lang/crates.io-index"
dependencies = [
 "libc",
]

[[package]]
name = "time"
version = "0.2.3"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "ansi_term"
version = "0.12.1"
source = "sparse+https://index.crates.io/"

[[package]]
name = "atty"
version = "0.2.14"
source = "git+https://example.test/atty#abc"

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "smallvec"
version = "0.6.2"

[metadata]
"#;

    #[test]
    fn lock_parser_keeps_registry_packages_only() {
        let p = lock_packages(LOCK);
        let names: Vec<String> = p.iter().map(|(n, v)| format!("{n}@{v}")).collect();
        assert_eq!(
            names,
            [
                "smallvec@1.6.0",
                "time@0.1.45",
                "time@0.2.3",
                "ansi_term@0.12.1",
                "serde@1.0.228"
            ]
        );
    }

    #[test]
    fn audit_rules_patched_unaffected_and_severity() {
        let db = Db::parse(DB);
        assert_eq!((db.count, db.bad, db.commit.as_str()), (5, 0, "abc123"));
        let hits = audit(&db, &lock_packages(LOCK));
        let got: Vec<String> = hits
            .iter()
            .map(|h| {
                format!(
                    "{}@{} {}",
                    h.package,
                    h.version,
                    h.advisories
                        .iter()
                        .map(|a| a.id.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                "ansi_term@0.12.1 RUSTSEC-2021-0139",
                "smallvec@1.6.0 RUSTSEC-2021-0003",
                "time@0.1.45 RUSTSEC-2020-0071"
            ]
        );
        let f = findings(&hits, "workspace");
        let by: Vec<(&str, Sev)> = f.iter().map(|x| (x.id.as_str(), x.sev)).collect();
        assert_eq!(
            by,
            [
                ("rustsec-ansi_term-0.12.1", Sev::Info),
                ("rustsec-smallvec-1.6.0", Sev::Critical),
                ("rustsec-time-0.1.45", Sev::Error),
            ]
        );
        assert_eq!(
            f[1].title,
            "smallvec 1.6.0 in Cargo.lock: 1 RustSec advisory (RUSTSEC-2021-0003)"
        );
        assert!(f[1].fix.contains(">=1.6.1"), "{}", f[1].fix);
        assert!(f[0].fix.starts_with("no patched release"));
        assert_eq!(
            f[1].evidence,
            ["RUSTSEC-2021-0003 vulnerability cvss3 9.8 patched: >=0.6.14, <1.0.0 | >=1.6.1"]
        );
    }

    #[cfg(feature = "rustsec-snapshot")]
    #[test]
    fn embedded_snapshot_parses_cleanly() {
        let db = Db::parse(SNAPSHOT);
        assert!(db.count > 1000, "{}", db.count);
        assert_eq!(db.bad, 0, "every row and version requirement parses");
        assert_eq!(db.commit.len(), 40);
        let sv = &db.by_package["smallvec"];
        let a = sv.iter().find(|a| a.id == "RUSTSEC-2021-0003").unwrap();
        assert_eq!(a.sev(), Sev::Critical);
        assert!(a.affects(&Version::parse("1.6.0").unwrap()));
        assert!(!a.affects(&Version::parse("1.6.1").unwrap()));
    }

    #[test]
    fn sensor_reads_only_when_live_and_refreshes_on_lock_change() {
        let dir = std::env::temp_dir().join(format!("nerve-rustsec-unit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (lock, dbp) = (dir.join("Cargo.lock"), dir.join("db.tsv"));
        std::fs::write(&lock, LOCK).unwrap();
        std::fs::write(&dbp, DB).unwrap();
        let env = |k: &str| match k {
            "NERVE_RUSTSEC_LOCK" => Some(lock.display().to_string()),
            "NERVE_RUSTSEC_DB" => Some(dbp.display().to_string()),
            "NERVE_RUSTSEC_COMPONENT" => Some("galion-stack".into()),
            _ => None,
        };
        assert!(RustSec::from_env(&|_| None).is_none());
        let mut s = Sensors::empty();
        s.add(RustSec::from_env(&env).unwrap());
        let logs = LogInsights::default();
        let mut i = Input::new(1_792_520_000, &[], &logs);
        assert!(
            s.run(&i).vulnerabilities.is_empty(),
            "not live: no host read, nothing"
        );
        i.live = true;
        let cx = s.run(&i);
        assert_eq!(cx.vulnerabilities.len(), 3);
        assert_eq!(cx.vulnerabilities[0].id, "rustsec-smallvec-1.6.0");
        assert_eq!(cx.vulnerabilities[0].component, "galion-stack");
        assert_eq!(cx.vulnerabilities[0].source, "rustsec");
        let ev = cx.nerve_events(i.now);
        assert_eq!((ev.len(), ev[0]["severity"].as_str()), (1, Some("crit")));
        assert_eq!(ev[0]["data"]["findings"], 3);
        // Cached between refreshes; a fixed lock (new mtime) clears it.
        i.now += 5;
        assert_eq!(s.run(&i).vulnerabilities.len(), 3);
        let fixed = LOCK
            .replace("1.6.0", "1.6.1")
            .replace("0.1.45", "0.3.36")
            .replace("ansi_term", "nu-ansi-term");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&lock, fixed).unwrap();
        let f = std::fs::File::options().write(true).open(&lock).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
        i.now += 5;
        assert!(s.run(&i).vulnerabilities.is_empty());
        // An unreadable lock is an info finding, not a crash.
        std::fs::remove_file(&lock).unwrap();
        i.now += 5;
        let cx = s.run(&i);
        assert_eq!(cx.vulnerabilities[0].id, "rustsec-unreadable");
        assert_eq!(cx.vulnerabilities[0].sev, Sev::Info);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Slot 385: a build without `rustsec-snapshot` and without
    /// `NERVE_RUSTSEC_DB` says why it skipped (info finding), it never
    /// pretends the lock is clean.
    #[cfg(not(feature = "rustsec-snapshot"))]
    #[test]
    fn no_embedded_snapshot_needs_a_db_file() {
        let dir = std::env::temp_dir().join(format!("nerve-rustsec-nosnap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("Cargo.lock");
        std::fs::write(&lock, LOCK).unwrap();
        let mut s = Sensors::empty();
        s.add(RustSec::new(lock, "w", None, 3600));
        let logs = LogInsights::default();
        let mut i = Input::new(1_792_520_000, &[], &logs);
        i.live = true;
        let cx = s.run(&i);
        assert_eq!(cx.vulnerabilities.len(), 1);
        assert_eq!(cx.vulnerabilities[0].id, "rustsec-unreadable");
        assert_eq!(cx.vulnerabilities[0].sev, Sev::Info);
        assert!(cx.vulnerabilities[0].title.contains("NERVE_RUSTSEC_DB"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
