// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::gladiator::{Gladiator, Kind};

/// Seed home directory names, in rank-tie order.
pub const SEED_IDS: [&str; 4] = ["g2-0", "g2-1", "g2-2", "g2-3"];

/// Design name of the four-seat board. There is no directory with this name.
pub const SEED_TEAM_ID: &str = "arena-compare";

/// Why a seed home could not be loaded. Nothing was written.
#[derive(Debug)]
pub enum LoadError {
    EmptyId,
    EmptyMembers,
    NotADirectory(PathBuf),
    MissingVoice(String),
    EmptyTask(String),
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::EmptyId => write!(f, "gladiator id is empty"),
            LoadError::EmptyMembers => write!(f, "team has no members"),
            LoadError::NotADirectory(path) => {
                write!(f, "homes path is not a directory: {}", path.display())
            }
            LoadError::MissingVoice(id) => write!(f, "home {id} has no Voice section"),
            LoadError::EmptyTask(id) => write!(f, "home {id} has an empty task"),
            LoadError::Read { path, source } => {
                write!(f, "read {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LoadError::Read { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Redacted seed homes in this checkout, if `SOUL.md` for `g2-0` is there.
///
/// The path is `stack/agents/personas/arena/homes` relative to the repo root
/// two directories above this crate. Returns `None` when it is absent. Does
/// not create the directory.
#[must_use]
pub fn redacted_homes() -> Option<PathBuf> {
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("stack")
        .join("agents")
        .join("personas")
        .join("arena")
        .join("homes");
    let soul = candidate.join("g2-0").join("SOUL.md");
    if soul.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Load `g2-0` through `g2-3` from `homes`.
///
/// Each home is opened read-only. `SOUL.md` supplies the voice and, when the
/// file says so, one pinned skill. `TASK.md` supplies the shared task text.
/// The seat id is the directory name, so two homes with the same soul bytes
/// stay distinct. Missing homes are an error. This function never creates a
/// file or a directory.
///
/// # Errors
///
/// The path is not a directory, a seed file cannot be read, the voice section
/// is missing, or the task is empty.
pub fn load_seed(homes: &Path) -> Result<Vec<Gladiator>, LoadError> {
    if !homes.is_dir() {
        return Err(LoadError::NotADirectory(homes.to_path_buf()));
    }
    let mut loaded = Vec::with_capacity(SEED_IDS.len());
    for id in SEED_IDS {
        loaded.push(load_one(homes, id)?);
    }
    Ok(loaded)
}

fn load_one(homes: &Path, id: &str) -> Result<Gladiator, LoadError> {
    let dir = homes.join(id);
    let soul_path = dir.join("SOUL.md");
    let task_path = dir.join("TASK.md");
    let soul = read_only(&soul_path)?;
    let task = read_only(&task_path)?;
    let voice = voice_of(&soul).ok_or_else(|| LoadError::MissingVoice(id.to_string()))?;
    let task_body = task.trim().to_string();
    if task_body.is_empty() {
        return Err(LoadError::EmptyTask(id.to_string()));
    }
    Ok(Gladiator {
        id: id.to_string(),
        kind: Kind::Persona,
        soul_path: display_rel(&soul_path),
        task_path: display_rel(&task_path),
        config_path: String::new(),
        voice,
        pinned_skill: pinned_skill_of(&soul),
        task: task_body,
        members: Vec::new(),
    })
}

fn read_only(path: &Path) -> Result<String, LoadError> {
    let mut file = File::open(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|source| LoadError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(buf)
}

fn display_rel(path: &Path) -> String {
    let full = path.to_string_lossy().replace('\\', "/");
    const MARK: &str = "stack/agents/personas/arena/homes/";
    if let Some(index) = full.find(MARK) {
        return full[index..].to_string();
    }
    full
}

fn voice_of(text: &str) -> Option<String> {
    let mut found = false;
    let mut body = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if !found {
            found = line.trim() == "## Voice";
            continue;
        }
        if line.starts_with("## ") {
            break;
        }
        body.push(line);
    }
    if !found {
        return None;
    }
    let joined = body.join("\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn pinned_skill_of(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim().trim_end_matches('\r');
        if !line.to_ascii_lowercase().starts_with("pinned skill") {
            continue;
        }
        let Some((_, value)) = line.rsplit_once(':') else {
            continue;
        };
        let skill = value.trim().trim_end_matches('.').trim();
        if !skill.is_empty() {
            return Some(skill.to_string());
        }
    }
    None
}
