// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Release rehearsal checks beyond `--version` (Integration S5-T3).
//!
//! [`rehearse_full`] runs [`crate::rehearse`] and then adds:
//! * **stamps** (binding when expected): a `--version` line that carries
//!   `(commit X, built Y)` must name the expected commit/build time; all
//!   stamped binaries of one release must agree with each other.
//! * **parity** (binding): every target ships the same binary set and each
//!   binary prints the same `--version` line on every target.
//! * **--help** (non-binding, recorded): every binary is started once more
//!   with `--help`; a binary that does not exit in time (it ignored the flag
//!   and started serving) is reported as a gap, never as a failure.
//! * **headroom** (non-binding): binaries above [`HEADROOM_PCT`] of the size
//!   budget.

use crate::{rehearse, run_bounded, runner_for, Run, Verified};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

/// Binaries above this share of the budget are listed as a headroom warning.
pub const HEADROOM_PCT: f64 = 90.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub commit: String,
    pub built: String,
}

/// `tuner 0.1.0 (commit abc, built 2026-10-21T21:30:41+02:00)` -> stamp.
/// Lines without the suffix (or with a different shape) give `None`.
pub fn parse_stamp(line: &str, name: &str, version: &str) -> Option<Stamp> {
    let rest = line.strip_prefix(&format!("{name} {version} ("))?;
    let inner = rest.strip_suffix(')')?;
    let inner = inner.strip_prefix("commit ")?;
    let (commit, built) = inner.split_once(", built ")?;
    if commit.is_empty() || built.is_empty() || commit.contains(' ') || built.contains(' ') {
        return None;
    }
    Some(Stamp {
        commit: commit.into(),
        built: built.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpKind {
    /// exit 0 with output
    Ok,
    /// exit 0, nothing on stdout or stderr
    Silent,
    /// exited non-zero (flag unknown, usage on stderr, ...)
    Nonzero,
    /// still running at the deadline: ignored --help and started work
    Timeout,
}

impl HelpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HelpKind::Ok => "ok",
            HelpKind::Silent => "silent",
            HelpKind::Nonzero => "nonzero",
            HelpKind::Timeout => "TIMEOUT",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Help {
    pub kind: HelpKind,
    pub line: String,
    pub millis: u128,
}

pub fn classify_help(success: bool, stdout: &str, stderr: &str) -> (HelpKind, String) {
    let first = |s: &str| {
        s.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string()
    };
    if stderr.starts_with("timeout after ") {
        let rest = stderr.split_once('\n').map(|x| x.1).unwrap_or("");
        let l = first(stdout);
        return (
            HelpKind::Timeout,
            if l.is_empty() { first(rest) } else { l },
        );
    }
    let l = first(stdout);
    let l = if l.is_empty() { first(stderr) } else { l };
    let k = match (success, l.is_empty()) {
        (true, false) => HelpKind::Ok,
        (true, true) => HelpKind::Silent,
        (false, _) => HelpKind::Nonzero,
    };
    (k, l)
}

pub struct Opts<'a> {
    pub expect_version: Option<&'a str>,
    pub expect_commit: Option<&'a str>,
    pub expect_built: Option<&'a str>,
    pub qemu_dir: Option<&'a Path>,
    pub timeout: Duration,
    /// `Some(t)`: also run `--help` with deadline `t`.
    pub help: Option<Duration>,
}

impl Default for Opts<'_> {
    fn default() -> Self {
        Opts {
            expect_version: None,
            expect_commit: None,
            expect_built: None,
            qemu_dir: None,
            timeout: Duration::from_secs(30),
            help: None,
        }
    }
}

pub struct Check {
    pub run: Run,
    pub version: String,
    pub stamp: Option<Stamp>,
    pub help: Option<Help>,
    /// binding problems for this binary (besides `run.ok == false`)
    pub problems: Vec<String>,
}

impl Check {
    pub fn ok(&self) -> bool {
        self.run.ok && self.problems.is_empty()
    }
}

pub struct Report {
    pub checks: Vec<Check>,
    /// binding cross-binary / cross-target problems
    pub parity: Vec<String>,
}

impl Report {
    pub fn failed(&self) -> usize {
        self.checks.iter().filter(|c| !c.ok()).count() + self.parity.len()
    }
    pub fn stamped(&self) -> (usize, usize) {
        let s = self.checks.iter().filter(|c| c.stamp.is_some()).count();
        (s, self.checks.len() - s)
    }
    pub fn help_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for c in &self.checks {
            if let Some(h) = &c.help {
                *m.entry(h.kind.as_str()).or_insert(0) += 1;
            }
        }
        m
    }
    /// (target, binary, % of budget) above [`HEADROOM_PCT`].
    pub fn headroom(&self, budget: u64) -> Vec<(String, String, f64)> {
        self.checks
            .iter()
            .map(|c| {
                (
                    c.run.target.clone(),
                    c.run.name.clone(),
                    c.run.size as f64 * 100.0 / budget as f64,
                )
            })
            .filter(|x| x.2 > HEADROOM_PCT)
            .collect()
    }
}

/// Stamp + parity problems over finished checks (pure; unit-tested).
pub fn cross_check(checks: &mut [Check], o: &Opts) -> Vec<String> {
    let mut parity = Vec::new();
    for c in checks.iter_mut() {
        if let Some(s) = &c.stamp {
            if let Some(w) = o.expect_commit {
                if s.commit != w {
                    c.problems
                        .push(format!("commit stamp {} != expected {w}", s.commit));
                }
            }
            if let Some(w) = o.expect_built {
                if s.built != w {
                    c.problems
                        .push(format!("build-time stamp {} != expected {w}", s.built));
                }
            }
        }
    }
    // all stamps of the release agree with each other
    let stamps: BTreeSet<(String, String)> = checks
        .iter()
        .filter_map(|c| c.stamp.as_ref())
        .map(|s| (s.commit.clone(), s.built.clone()))
        .collect();
    if stamps.len() > 1 {
        let v: Vec<String> = stamps.iter().map(|(a, b)| format!("{a}@{b}")).collect();
        parity.push(format!("mixed provenance stamps: {}", v.join(" ")));
    }
    // same binary set + same --version line on every target
    let mut by_target: BTreeMap<&str, BTreeMap<&str, &str>> = BTreeMap::new();
    for c in checks.iter() {
        by_target
            .entry(c.run.target.as_str())
            .or_default()
            .insert(c.run.name.as_str(), c.run.line.as_str());
    }
    let mut it = by_target.iter();
    if let Some((t0, m0)) = it.next() {
        for (t, m) in it {
            let a: BTreeSet<&&str> = m0.keys().collect();
            let b: BTreeSet<&&str> = m.keys().collect();
            for x in a.difference(&b) {
                parity.push(format!("{x}: in {t0} but not in {t}"));
            }
            for x in b.difference(&a) {
                parity.push(format!("{x}: in {t} but not in {t0}"));
            }
            for (n, l0) in m0 {
                if let Some(l) = m.get(n) {
                    if l != l0 {
                        parity.push(format!("{n}: --version differs: {t0} '{l0}' vs {t} '{l}'"));
                    }
                }
            }
        }
    }
    parity
}

/// Verify-extract-run (`--version`), then `--help` (optional), stamps and
/// parity. `Err` only for setup failures (extract, missing qemu).
pub fn rehearse_full(v: &Verified, work: &Path, o: &Opts) -> Result<Report, String> {
    let runs = rehearse(v, work, o.expect_version, o.qemu_dir, o.timeout)?;
    let mut checks = Vec::new();
    for run in runs {
        let arch = v
            .archives
            .iter()
            .find(|a| a.manifest.target == run.target)
            .ok_or_else(|| format!("no archive for {}", run.target))?;
        let version = o
            .expect_version
            .unwrap_or(&arch.manifest.version)
            .to_string();
        let stamp = parse_stamp(&run.line, &run.name, &version);
        let help = match o.help {
            None => None,
            Some(t) => {
                let root = work.join(&arch.stem);
                let runner = runner_for(&run.target, o.qemu_dir)?;
                let (ok, so, se, el) = run_bounded(
                    runner.as_deref(),
                    &root.join(&run.name),
                    &["--help"],
                    &root,
                    t,
                )?;
                let (kind, mut line) = classify_help(ok, &so, &se);
                if line.chars().count() > 100 {
                    line = line.chars().take(100).collect::<String>() + "...";
                }
                Some(Help {
                    kind,
                    line,
                    millis: el.as_millis(),
                })
            }
        };
        checks.push(Check {
            run,
            version,
            stamp,
            help,
            problems: vec![],
        });
    }
    let parity = cross_check(&mut checks, o);
    Ok(Report { checks, parity })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SIZE_BUDGET;

    fn chk(t: &str, n: &str, line: &str) -> Check {
        Check {
            run: Run {
                target: t.into(),
                name: n.into(),
                size: 1,
                runner: "native".into(),
                ok: true,
                line: line.into(),
                millis: 0,
            },
            version: "0.1.0".into(),
            stamp: parse_stamp(line, n, "0.1.0"),
            help: None,
            problems: vec![],
        }
    }

    #[test]
    fn stamps() {
        let s = parse_stamp(
            "tuner 0.1.0 (commit 8d93ad5456af, built 2026-10-21T21:30:41+02:00)",
            "tuner",
            "0.1.0",
        )
        .unwrap();
        assert_eq!(s.commit, "8d93ad5456af");
        assert_eq!(s.built, "2026-10-21T21:30:41+02:00");
        assert_eq!(parse_stamp("tuner 0.1.0", "tuner", "0.1.0"), None);
        assert_eq!(parse_stamp("tuner 0.1.0 (oops)", "tuner", "0.1.0"), None);
        assert_eq!(
            parse_stamp("tuner 0.1.0 (commit , built x)", "tuner", "0.1.0"),
            None
        );
        // the live VPS tuner prints this (built without provenance)
        let u = parse_stamp(
            "tuner 0.1.0 (commit unknown, built unknown)",
            "tuner",
            "0.1.0",
        )
        .unwrap();
        assert_eq!(u.commit, "unknown");
    }

    #[test]
    fn help_classes() {
        assert_eq!(classify_help(true, "usage: x\n", "").0, HelpKind::Ok);
        assert_eq!(classify_help(true, "", "").0, HelpKind::Silent);
        let (k, l) = classify_help(false, "", "\nerror: unknown arg --help\n");
        assert_eq!(
            (k, l.as_str()),
            (HelpKind::Nonzero, "error: unknown arg --help")
        );
        let (k, l) = classify_help(false, "", "timeout after 3s\nlistening on :8420\n");
        assert_eq!((k, l.as_str()), (HelpKind::Timeout, "listening on :8420"));
        assert_eq!(classify_help(true, "", "usage on stderr").0, HelpKind::Ok);
    }

    #[test]
    fn parity_and_stamp_mismatch() {
        let x = "x86_64-unknown-linux-musl";
        let a = "aarch64-unknown-linux-musl";
        let good = "tuner 0.1.0 (commit abc, built T)";
        let mut v = vec![
            chk(x, "tuner", good),
            chk(a, "tuner", good),
            chk(x, "gaia", "gaia 0.1.0"),
            chk(a, "gaia", "gaia 0.1.0"),
        ];
        let o = Opts {
            expect_commit: Some("abc"),
            expect_built: Some("T"),
            ..Opts::default()
        };
        assert!(cross_check(&mut v, &o).is_empty());
        assert!(v.iter().all(|c| c.ok()));

        let o2 = Opts {
            expect_commit: Some("def"),
            ..Opts::default()
        };
        let p = cross_check(&mut v, &o2);
        assert!(p.is_empty());
        assert_eq!(v.iter().filter(|c| !c.ok()).count(), 2);

        // set + line drift + mixed stamps
        let mut w = vec![
            chk(x, "tuner", good),
            chk(a, "tuner", "tuner 0.1.0 (commit zzz, built T)"),
            chk(x, "gaia", "gaia 0.1.0"),
            chk(a, "room", "room 0.1.0"),
        ];
        let p = cross_check(&mut w, &Opts::default());
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p.iter().any(|s| s.starts_with("mixed provenance")));
        assert!(p.iter().any(|s| s
            == "gaia: in aarch64-unknown-linux-musl but not in x86_64-unknown-linux-musl"
            || s.contains("gaia: in x86_64")));
        assert!(p.iter().any(|s| s.contains("room: in aarch64")));
        assert!(p.iter().any(|s| s.contains("tuner: --version differs")));
    }

    #[test]
    fn headroom_list() {
        let mut c = chk("x", "hermes-gateway", "hermes-gateway 0.1.0");
        c.run.size = 2_082_048;
        let r = Report {
            checks: vec![c, chk("x", "tuner", "tuner 0.1.0")],
            parity: vec![],
        };
        let h = r.headroom(SIZE_BUDGET);
        assert_eq!(h.len(), 1);
        assert!((h[0].2 - 99.28).abs() < 0.01);
        assert_eq!(r.stamped(), (0, 2));
    }
}
