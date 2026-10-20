// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! T2.5 source-vs-live drift: the SHA-256 of each deployed binary / unit file
//! against an expected release manifest.
//!
//! Manifest: `SHA256SUMS` format (`<64 hex>  <path>`, `*path` binary marker
//! allowed, `#` comments), the pattern of `dist/bin/SHA256SUMS` and
//! `artifacts/tuner/SHA256SUMS`. Relative paths resolve against
//! `NERVE_DRIFT_ROOT`, else the manifest's directory. List the binaries and the
//! systemd unit files you want watched, one line each.
//!
//! Reads files only when [`Input::live`] is true, read-only. Hashing is
//! budgeted: at most `NERVE_DRIFT_BUDGET_KIB` (default 2048) bytes are hashed
//! per cycle, resumed next cycle (`&mut self` keeps the partial hash), and a
//! file is re-hashed only when its size / mtime / inode change. A 20 MiB binary
//! is therefore verified over ~10 cycles once, then costs one `stat` per cycle.
//! Binaries are never executed (no `--version`).
//!
//! Env: `NERVE_DRIFT_MANIFEST=<path>` turns the sensor on (unset: off);
//! `NERVE_DRIFT_ROOT`, `NERVE_DRIFT_BUDGET_KIB`.
//!
//! Findings (kind vulnerability, source `drift`), ids only in events:
//! - `drift-<name>` error: hash differs from the manifest (unreviewed code runs).
//! - `drift-missing-<name>` warn: listed file absent / unreadable.
//! - `drift-manifest` warn: manifest unreadable, empty, or has invalid lines.

use super::{Env, Sensor, Sink};
use crate::cortex::{Finding, FindingKind, Input};
use crate::incident::Sev;
use crate::state::CompState;
use galion_common::hex::encode as hex;
use galion_common::sha256::Sha256;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Default hashing budget per cycle (bytes).
pub const DEFAULT_BUDGET: u64 = 2048 * 1024;

/// Manifests above this size are refused (a manifest lists a few dozen files).
const MAX_MANIFEST: u64 = 1024 * 1024;

/// File identity used to decide "changed since hashed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Meta {
    len: u64,
    mtime: i64,
    mtime_ns: i64,
    /// Change time: catches an in-place rewrite with a restored mtime.
    ctime: i64,
    ctime_ns: i64,
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
            mtime: m.mtime(),
            mtime_ns: m.mtime_nsec(),
            ctime: m.ctime(),
            ctime_ns: m.ctime_nsec(),
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
            mtime: t.as_secs() as i64,
            mtime_ns: t.subsec_nanos() as i64,
            ctime: 0,
            ctime_ns: 0,
            ino: 0,
        })
    }
}

enum State {
    /// Not stat'ed yet, or changed: hash from 0.
    Fresh,
    Hashing {
        h: Sha256,
        off: u64,
        meta: Meta,
    },
    Done {
        meta: Meta,
        actual: String,
    },
    Missing(String),
}

struct Entry {
    /// Basename shown in titles/ids (never the full path).
    name: String,
    id: String,
    path: PathBuf,
    expected: String,
    state: State,
}

/// One parsed manifest line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub sha256: String,
    pub path: String,
}

/// Parse SHA256SUMS text. Returns the valid lines and the count of invalid
/// non-blank, non-comment lines.
pub fn parse_manifest(text: &str) -> (Vec<Line>, usize) {
    let mut ok = vec![];
    let mut bad = 0;
    for l in text.lines() {
        let l = l.trim_end();
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        let Some((h, p)) = l.split_once(char::is_whitespace) else {
            bad += 1;
            continue;
        };
        let p = p.trim_start();
        let p = p.strip_prefix('*').unwrap_or(p);
        if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) || p.is_empty() {
            bad += 1;
            continue;
        }
        ok.push(Line {
            sha256: h.to_ascii_lowercase(),
            path: p.to_string(),
        });
    }
    (ok, bad)
}

fn slug(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    s.trim_matches('-').to_string()
}

/// Component owning a manifest file: unit file name, unit stem or component
/// name equal to the file's basename; else `host`.
fn component_of(comps: &[CompState], name: &str) -> String {
    comps
        .iter()
        .find(|c| {
            let unit = c.spec.unit.unwrap_or("");
            name == unit || name == unit.trim_end_matches(".service") || name == c.spec.name
        })
        .map(|c| c.spec.name.to_string())
        .unwrap_or_else(|| "host".into())
}

/// Sensor `drift`.
pub struct Drift {
    manifest: PathBuf,
    root: Option<PathBuf>,
    budget: u64,
    manifest_seen: Option<Result<String, String>>,
    manifest_problem: Option<String>,
    entries: Vec<Entry>,
}

impl Drift {
    /// Watch `manifest`; relative paths resolve against `root` (else the
    /// manifest directory); hash at most `budget` bytes per cycle.
    pub fn new(manifest: impl Into<PathBuf>, root: Option<PathBuf>, budget: u64) -> Self {
        Drift {
            manifest: manifest.into(),
            root,
            budget: budget.max(4096),
            manifest_seen: None,
            manifest_problem: None,
            entries: vec![],
        }
    }

    /// `NERVE_DRIFT_MANIFEST` (on), `NERVE_DRIFT_ROOT`, `NERVE_DRIFT_BUDGET_KIB`.
    pub fn from_env(env: Env) -> Option<Self> {
        let m = env("NERVE_DRIFT_MANIFEST").filter(|s| !s.trim().is_empty())?;
        let root = env("NERVE_DRIFT_ROOT")
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        let budget = env("NERVE_DRIFT_BUDGET_KIB")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|k| k.saturating_mul(1024))
            .unwrap_or(DEFAULT_BUDGET);
        Some(Drift::new(m.trim(), root, budget))
    }

    /// Entries verified so far / total (for tests and the real test).
    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .entries
            .iter()
            .filter(|e| matches!(e.state, State::Done { .. } | State::Missing(_)))
            .count();
        (done, self.entries.len())
    }

    /// Re-read the manifest (small; cheaper and safer than trusting a
    /// coarse mtime) and rebuild the entry list only when its text changed.
    fn reload_manifest(&mut self) {
        let read = std::fs::metadata(&self.manifest).and_then(|m| {
            if m.len() > MAX_MANIFEST {
                Err(std::io::Error::other("larger than 1 MiB"))
            } else {
                std::fs::read_to_string(&self.manifest)
            }
        });
        let seen = read.map_err(|e| e.to_string().chars().take(60).collect::<String>());
        if self.manifest_seen.as_ref() == Some(&seen) {
            return;
        }
        self.manifest_seen = Some(seen.clone());
        self.entries.clear();
        let text = match seen {
            Ok(t) => t,
            Err(e) => {
                self.manifest_problem = Some(format!("manifest unreadable ({e})"));
                return;
            }
        };
        let (lines, bad) = parse_manifest(&text);
        let base = self
            .root
            .clone()
            .or_else(|| self.manifest.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        for l in lines {
            let p = Path::new(&l.path);
            let path = if p.is_absolute() {
                p.to_path_buf()
            } else {
                base.join(p)
            };
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| l.path.clone());
            let mut id = slug(&name);
            let mut k = 2;
            while self.entries.iter().any(|e| e.id == id) {
                id = format!("{}-{k}", slug(&name));
                k += 1;
            }
            self.entries.push(Entry {
                name,
                id,
                path,
                expected: l.sha256,
                state: State::Fresh,
            });
        }
        self.manifest_problem = match (self.entries.len(), bad) {
            (0, _) => Some(format!("manifest lists no file ({bad} invalid line(s))")),
            (_, 0) => None,
            (_, b) => Some(format!("manifest has {b} invalid line(s)")),
        };
    }

    /// Stat every entry; restart hashing of changed files.
    fn restat(&mut self) {
        for e in &mut self.entries {
            match meta_of(&e.path) {
                Err(err) => e.state = State::Missing(format!("{:?}", err.kind())),
                Ok(m) => {
                    let same = match &e.state {
                        State::Hashing { meta, .. } | State::Done { meta, .. } => *meta == m,
                        _ => false,
                    };
                    if !same {
                        e.state = State::Hashing {
                            h: Sha256::new(),
                            off: 0,
                            meta: m,
                        };
                    }
                }
            }
        }
    }

    /// Spend the byte budget on entries still hashing, in manifest order.
    fn hash_some(&mut self) {
        let mut left = self.budget;
        let mut buf = vec![0u8; 64 * 1024];
        for e in &mut self.entries {
            if left == 0 {
                break;
            }
            let State::Hashing { h, off, meta } = &mut e.state else {
                continue;
            };
            let mut f = match File::open(&e.path)
                .and_then(|mut f| f.seek(SeekFrom::Start(*off)).map(|_| f))
            {
                Ok(f) => f,
                Err(err) => {
                    e.state = State::Missing(format!("{:?}", err.kind()));
                    continue;
                }
            };
            let mut eof = false;
            let mut failed = None;
            while left > 0 {
                let want = (left as usize).min(buf.len());
                match f.read(&mut buf[..want]) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        h.update(&buf[..n]);
                        *off += n as u64;
                        left -= n as u64;
                    }
                    Err(err) => {
                        failed = Some(format!("{:?}", err.kind()));
                        break;
                    }
                }
            }
            if let Some(why) = failed {
                e.state = State::Missing(why);
                continue;
            }
            if !eof && *off >= meta.len {
                // Budget ran out exactly at the end: confirm EOF cheaply.
                eof = matches!(f.read(&mut buf[..1]), Ok(0));
            }
            if eof {
                if let State::Hashing { h, meta, .. } =
                    std::mem::replace(&mut e.state, State::Fresh)
                {
                    e.state = State::Done {
                        meta,
                        actual: hex(&h.finalize()),
                    };
                }
            }
        }
    }
}

impl Sensor for Drift {
    fn name(&self) -> &'static str {
        "drift"
    }

    fn sense(&mut self, i: &Input<'_>, out: &mut Sink<'_>) {
        if !i.live {
            return; // replay/tests: never read the host
        }
        self.reload_manifest();
        if let Some(p) = &self.manifest_problem {
            out.finding(
                Finding::new(FindingKind::Vulnerability, "drift-manifest", Sev::Warn, "host", format!("drift check degraded: {p}"))
                    .with_evidence(vec![format!("manifest {}", self.manifest.display())])
                    .with_fix("write the release SHA256SUMS (`sha256sum <files> > SHA256SUMS`) where NERVE_DRIFT_MANIFEST points"),
            );
        }
        self.restat();
        self.hash_some();
        for e in &self.entries {
            let comp = component_of(i.comps, &e.name);
            match &e.state {
                State::Missing(why) => out.finding(
                    Finding::new(
                        FindingKind::Vulnerability,
                        format!("drift-missing-{}", e.id),
                        Sev::Warn,
                        comp,
                        format!("{}: listed in the release manifest but not readable on the host", e.name),
                    )
                    .with_evidence(vec![format!("path {} ({why})", e.path.display())])
                    .with_fix("deploy the release file, or drop the line from the manifest (owner)"),
                ),
                State::Done { meta, actual } if *actual != e.expected => out.finding(
                    Finding::new(
                        FindingKind::Vulnerability,
                        format!("drift-{}", e.id),
                        Sev::Error,
                        comp,
                        format!("{}: deployed file differs from the release manifest", e.name),
                    )
                    .with_evidence(vec![
                        format!("path {}", e.path.display()),
                        format!("expected sha256 {}.. live {}..", &e.expected[..12], &actual[..12]),
                        format!("size {} B, mtime {}", meta.len, zagreb_time::from_unix(meta.mtime)),
                    ])
                    .with_fix("redeploy the released build (owner-approved cutover), or regenerate the manifest if this build is the intended release")
                    .with_cause("drift"),
                ),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logscan::LogInsights;
    use crate::sensors::Sensors;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nerve-drift-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sha(b: &[u8]) -> String {
        hex(&galion_common::sha256::digest(b))
    }

    fn live(s: &mut Sensors, comps: &[CompState]) -> crate::cortex::Cortex {
        let logs = LogInsights::default();
        let mut i = Input::new(1_792_000_000, comps, &logs);
        i.live = true;
        s.run(&i)
    }

    fn ids(cx: &crate::cortex::Cortex) -> Vec<(String, Sev)> {
        cx.vulnerabilities
            .iter()
            .map(|f| (f.id.clone(), f.sev))
            .collect()
    }

    #[test]
    fn manifest_parse() {
        let t = format!(
            "# release 1\n{}  galion-one\n{} *bin/room\n\nnot-a-line\n{}  \nABC  x\n",
            "a".repeat(64),
            "B".repeat(64),
            "c".repeat(64)
        );
        let (ok, bad) = parse_manifest(&t);
        assert_eq!(ok.len(), 2);
        assert_eq!(
            ok[1],
            Line {
                sha256: "b".repeat(64),
                path: "bin/room".into()
            }
        );
        assert_eq!(bad, 3);
    }

    #[test]
    fn match_mismatch_missing_and_clear() {
        let d = tmp("mm");
        std::fs::write(d.join("good"), b"good binary").unwrap();
        std::fs::write(d.join("bad"), b"patched by hand").unwrap();
        std::fs::write(
            d.join("SHA256SUMS"),
            format!(
                "{}  good\n{}  bad\n{}  gone\n",
                sha(b"good binary"),
                sha(b"released"),
                sha(b"x")
            ),
        )
        .unwrap();
        let mut s = Sensors::empty();
        s.add(Drift::new(d.join("SHA256SUMS"), None, DEFAULT_BUDGET));
        let cx = live(&mut s, &[]);
        assert_eq!(
            ids(&cx),
            [
                ("drift-bad".to_string(), Sev::Error),
                ("drift-missing-gone".into(), Sev::Warn)
            ]
        );
        let f = &cx.vulnerabilities[0];
        assert_eq!(
            (f.source.as_str(), f.component.as_str(), f.cause.as_deref()),
            ("drift", "host", Some("drift"))
        );
        assert_eq!(
            f.title,
            "bad: deployed file differs from the release manifest"
        );
        assert!(
            !f.title.contains(&d.display().to_string()),
            "no path in titles"
        );
        let ev = f.to_nerve_event(1_792_000_000).to_string();
        assert!(
            !ev.contains(&d.display().to_string()),
            "no path in events: {ev}"
        );
        // Redeploy the released file: finding clears next cycle.
        std::fs::write(d.join("bad"), b"released").unwrap();
        std::fs::write(d.join("gone"), b"x").unwrap();
        assert!(ids(&live(&mut s, &[])).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn budget_resumes_across_cycles_and_rehash_on_change() {
        let d = tmp("bud");
        let big: Vec<u8> = (0..20_000u32).map(|k| (k % 251) as u8).collect();
        std::fs::write(d.join("big"), &big).unwrap();
        std::fs::write(d.join("SHA256SUMS"), format!("{}  big\n", sha(b"other"))).unwrap();
        let mut drift = Drift::new(d.join("SHA256SUMS"), None, 4096);
        let logs = LogInsights::default();
        let mut i = Input::new(1, &[], &logs);
        i.live = true;
        let mut cycles = 0;
        loop {
            let mut cx = crate::cortex::Cortex::default();
            drift.sense(&i, &mut Sink::new(&mut cx, "drift"));
            cycles += 1;
            if !cx.vulnerabilities.is_empty() {
                assert_eq!(cx.vulnerabilities[0].id, "drift-big");
                break;
            }
            assert!(cycles < 10);
        }
        assert_eq!(cycles, 5, "20000 B at 4096 B/cycle = 5 cycles");
        assert_eq!(drift.progress(), (1, 1));
        // Matching manifest now (manifest change reloads), same file.
        std::fs::write(d.join("SHA256SUMS"), format!("{}  big\n", sha(&big))).unwrap();
        let mut n = 0;
        let clean = loop {
            let mut cx = crate::cortex::Cortex::default();
            drift.sense(&i, &mut Sink::new(&mut cx, "drift"));
            n += 1;
            if drift.progress().0 == 1 || n > 10 {
                break cx.vulnerabilities.is_empty();
            }
        };
        assert!(clean);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn manifest_problems_component_mapping_and_not_live() {
        let d = tmp("mp");
        let mut s = Sensors::empty();
        s.add(Drift::new(d.join("absent"), None, DEFAULT_BUDGET));
        assert_eq!(
            ids(&live(&mut s, &[])),
            [("drift-manifest".to_string(), Sev::Warn)]
        );
        // Not live: nothing at all (not even the manifest warning).
        let logs = LogInsights::default();
        assert!(s
            .run(&Input::new(1, &[], &logs))
            .findings()
            .next()
            .is_none());
        // Unit-file name maps to its component; root override.
        std::fs::write(d.join("tuner.service"), b"[Unit]\n").unwrap();
        std::fs::write(
            d.join("M"),
            format!("{}  tuner.service\nbogus\n", sha(b"[Unit]\nold\n")),
        )
        .unwrap();
        let comps: Vec<CompState> = crate::config::default_specs(None, None)
            .into_iter()
            .map(CompState::new)
            .collect();
        let mut s = Sensors::empty();
        s.add(Drift::new(d.join("M"), Some(d.clone()), DEFAULT_BUDGET));
        let cx = live(&mut s, &comps);
        assert_eq!(
            ids(&cx),
            [
                ("drift-tuner.service".to_string(), Sev::Error),
                ("drift-manifest".into(), Sev::Warn)
            ]
        );
        assert_eq!(cx.vulnerabilities[0].component, "tuner");
        assert!(cx.vulnerabilities[1].title.contains("1 invalid line"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn env() {
        assert!(Drift::from_env(&|_| None).is_none());
        let d = Drift::from_env(&|k| match k {
            "NERVE_DRIFT_MANIFEST" => Some("/x/SHA256SUMS".into()),
            "NERVE_DRIFT_BUDGET_KIB" => Some("1".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            (d.budget, d.manifest.as_path()),
            (4096, Path::new("/x/SHA256SUMS"))
        );
    }

    /// Replay fixture tests/fixtures/drift/: a release manifest and the
    /// "deployed" files (one drifted, one missing). See its README.
    #[test]
    fn replay_fixture() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/drift");
        let mut s = Sensors::empty();
        s.add(Drift::new(
            dir.join("SHA256SUMS"),
            Some(dir.join("deployed")),
            DEFAULT_BUDGET,
        ));
        let cx = live(&mut s, &[]);
        assert_eq!(
            ids(&cx),
            [
                ("drift-galion-desk".to_string(), Sev::Error),
                ("drift-missing-room".into(), Sev::Warn)
            ]
        );
        let ev = cx.nerve_events(1_792_000_000);
        assert_eq!(ev.len(), 1);
        assert_eq!(
            (ev[0]["source"].as_str(), ev[0]["severity"].as_str()),
            (Some("drift"), Some("high"))
        );
        assert_eq!(ev[0]["data"]["findings"], 2);
    }
}
