// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! Persistent state: bans, ban history, recent logins, log position.
//! Written atomically (temp file, fsync, rename, fsync dir), mode 0600.

use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::jail::{Ban, History, Totals};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilePos {
    pub dev: u64,
    pub ino: u64,
    pub pos: u64,
    /// Fingerprint of the first line (0 = unknown).
    #[serde(default)]
    pub head: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub version: u32,
    /// Unix seconds of the last save.
    pub saved_at: i64,
    pub bans: Vec<Ban>,
    pub history: Vec<History>,
    pub accepted: Vec<(IpAddr, i64)>,
    pub totals: Totals,
    /// journald cursor of the last processed entry.
    pub journal_cursor: Option<String>,
    /// auth.log position of the last processed line.
    pub file_pos: Option<FilePos>,
}

#[derive(Debug)]
pub enum Load {
    Fresh,
    Loaded(Box<State>),
    /// Unreadable state was moved aside; start empty (fail open: no bans).
    Corrupt {
        moved_to: PathBuf,
        error: String,
    },
}

pub fn load(path: &Path) -> std::io::Result<Load> {
    let data = match fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Load::Fresh),
        Err(e) => return Err(e),
    };
    match serde_json::from_slice::<State>(&data) {
        Ok(s) if s.version == VERSION => Ok(Load::Loaded(Box::new(s))),
        Ok(s) => corrupt(path, format!("unsupported state version {}", s.version)),
        Err(e) => corrupt(path, e.to_string()),
    }
}

fn corrupt(path: &Path, error: String) -> std::io::Result<Load> {
    let mut moved = path.as_os_str().to_owned();
    moved.push(format!(".corrupt-{}", now_secs()));
    let moved = PathBuf::from(moved);
    fs::rename(path, &moved)?;
    Ok(Load::Corrupt {
        moved_to: moved,
        error,
    })
}

pub fn save(path: &Path, st: &State) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    if !dir.as_os_str().is_empty() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let body = serde_json::to_vec_pretty(st).map_err(std::io::Error::other)?;
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&body)?;
        f.write_all(b"\n")?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Ok(d) = fs::File::open(if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    }) {
        let _ = d.sync_all();
    }
    Ok(())
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("galion-guard-state-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn roundtrip_and_mode() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmpdir("rt");
        let p = d.join("state.json");
        assert!(matches!(load(&p).unwrap(), Load::Fresh));
        let st = State {
            version: VERSION,
            saved_at: 5,
            bans: vec![Ban {
                ip: "203.0.113.1".parse().unwrap(),
                start: 1,
                end: 2,
                count: 1,
                failures: 5,
                manual: false,
            }],
            journal_cursor: Some("s=abc".into()),
            ..State::default()
        };
        save(&p, &st).unwrap();
        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        match load(&p).unwrap() {
            Load::Loaded(s) => assert_eq!(*s, st),
            o => panic!("{o:?}"),
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_is_moved_aside() {
        let d = tmpdir("bad");
        let p = d.join("state.json");
        fs::write(&p, b"{not json").unwrap();
        match load(&p).unwrap() {
            Load::Corrupt { moved_to, .. } => assert!(moved_to.exists()),
            o => panic!("{o:?}"),
        }
        assert!(!p.exists());
        let _ = fs::remove_dir_all(&d);
    }
}
