// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The rollback journal, `$BACKUP_DIR/<unit>.journal` (F6). It exists exactly while a
//! deploy is in flight and is also the per-unit lock: it is created with link(2), so
//! two deploys of the same unit cannot both own it. Every phase change rewrites it
//! atomically *before* the step it announces, so a deploy killed at any point
//! (SIGKILL, power loss) leaves enough on disk for the next run to put the old
//! binary back. Plain `key=value` lines, readable with `cat`.

use crate::files;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Version probe and backup copy; nothing installed yet. Recovery: discard.
    Prepare,
    /// About to replace (or create) the target. Recovery: restore.
    Install,
    /// New binary is in place, restart and health check running. Recovery: restore.
    Verify,
    /// Restoring the backup. Recovery: restore again (idempotent).
    Rollback,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Prepare => "prepare",
            Phase::Install => "install",
            Phase::Verify => "verify",
            Phase::Rollback => "rollback",
        }
    }
    fn parse(s: &str) -> Option<Phase> {
        Some(match s {
            "prepare" => Phase::Prepare,
            "install" => Phase::Install,
            "verify" => Phase::Verify,
            "rollback" => Phase::Rollback,
            _ => return None,
        })
    }
    /// True once the target may differ from what was there before the deploy.
    pub fn touched_target(self) -> bool {
        self != Phase::Prepare
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub pid: u32,
    pub started: String,
    pub phase: Phase,
    pub unit: String,
    pub target: PathBuf,
    /// `None` = first install (there was no binary before).
    pub backup: Option<PathBuf>,
    pub new: PathBuf,
    pub sha_old: String,
    pub sha_new: String,
    pub health: String,
}

impl Entry {
    pub fn render(&self) -> String {
        format!(
            "# galion-deploy rollback journal: a deploy of {u} is in flight (or was killed).\n\
             # The next `galion-deploy {u} ...` or `galion-deploy --recover {u}` restores the backup.\n\
             pid={}\nstarted={}\nphase={}\nunit={u}\ntarget={}\nbackup={}\nnew={}\nsha_old={}\nsha_new={}\nhealth={}\n",
            self.pid,
            self.started,
            self.phase.as_str(),
            self.target.display(),
            self.backup
                .as_ref()
                .map(|b| b.display().to_string())
                .unwrap_or_else(|| "none".into()),
            self.new.display(),
            self.sha_old,
            self.sha_new,
            self.health,
            u = self.unit,
        )
    }

    pub fn parse(text: &str) -> Option<Entry> {
        let get = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix('=')))
                .map(str::to_string)
        };
        Some(Entry {
            pid: get("pid")?.parse().ok()?,
            started: get("started")?,
            phase: Phase::parse(&get("phase")?)?,
            unit: get("unit")?,
            target: PathBuf::from(get("target")?),
            backup: match get("backup")?.as_str() {
                "none" => None,
                b => Some(PathBuf::from(b)),
            },
            new: PathBuf::from(get("new")?),
            sha_old: get("sha_old")?,
            sha_new: get("sha_new")?,
            health: get("health")?,
        })
    }
}

pub fn path(bdir: &Path, unit: &str) -> PathBuf {
    bdir.join(format!("{unit}.journal"))
}

pub enum Found {
    None,
    /// A journal whose owner is still running: another deploy is in progress.
    Busy(u32),
    /// A journal left by a deploy that died.
    Stale(Box<Entry>),
    /// A journal that cannot be parsed (never written by us, or torn by hand).
    Corrupt(String),
}

pub fn look(bdir: &Path, unit: &str) -> Found {
    let p = path(bdir, unit);
    let Ok(text) = fs::read_to_string(&p) else {
        return Found::None;
    };
    match Entry::parse(&text) {
        Some(e)
            if e.pid != std::process::id() && crate::sys::alive_with(e.pid, "galion-deploy") =>
        {
            Found::Busy(e.pid)
        }
        Some(e) => Found::Stale(Box::new(e)),
        None => Found::Corrupt(text),
    }
}

/// Take the lock: create the journal; fails if one exists.
pub fn create(bdir: &Path, e: &Entry) -> std::io::Result<()> {
    files::create_exclusive(&path(bdir, &e.unit), e.render().as_bytes(), 0o600)
}

pub fn set_phase(bdir: &Path, e: &mut Entry, ph: Phase) -> std::io::Result<()> {
    e.phase = ph;
    files::write_atomic(&path(bdir, &e.unit), e.render().as_bytes(), 0o600)
}

pub fn remove(bdir: &Path, unit: &str) {
    let p = path(bdir, unit);
    let _ = fs::remove_file(&p);
    files::sync_dir(&p);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e() -> Entry {
        Entry {
            pid: 4_000_000,
            started: "2026-10-20T23:00:00+02:00".into(),
            phase: Phase::Verify,
            unit: "tuner".into(),
            target: "/opt/galion/tuner/bin/tuner".into(),
            backup: Some("/var/backups/galion/deploy/tuner-20261020-230000+0200.bin".into()),
            new: "/var/tmp/galion-cutover/tuner".into(),
            sha_old: "aa".into(),
            sha_new: "bb".into(),
            health: "http://127.0.0.1:8650/health".into(),
        }
    }

    #[test]
    fn roundtrip() {
        let a = e();
        assert_eq!(Entry::parse(&a.render()), Some(a.clone()));
        let mut b = a;
        b.backup = None;
        b.phase = Phase::Prepare;
        assert_eq!(Entry::parse(&b.render()), Some(b.clone()));
        assert!(!b.phase.touched_target());
        assert!(Entry::parse("pid=1\n").is_none());
    }

    #[test]
    fn lock_and_stale() {
        let d = std::env::temp_dir().join(format!("gd-journal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        assert!(matches!(look(&d, "tuner"), Found::None));
        let mut a = e(); // pid 4_000_000: not running
        create(&d, &a).unwrap();
        assert!(create(&d, &a).is_err(), "second create must fail (lock)");
        assert!(matches!(look(&d, "tuner"), Found::Stale(_)));
        set_phase(&d, &mut a, Phase::Rollback).unwrap();
        match look(&d, "tuner") {
            Found::Stale(x) => assert_eq!(x.phase, Phase::Rollback),
            _ => panic!("expected stale"),
        }
        fs::write(path(&d, "tuner"), "junk").unwrap();
        assert!(matches!(look(&d, "tuner"), Found::Corrupt(_)));
        remove(&d, "tuner");
        assert!(matches!(look(&d, "tuner"), Found::None));
        fs::remove_dir_all(&d).unwrap();
    }
}
