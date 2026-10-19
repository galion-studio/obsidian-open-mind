// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use crate::load::LoadError;

/// Which record this seat is. One seat wears one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A soul file. Not a process.
    Persona,
    /// A persona bound to one home for one bout.
    Worker,
    /// A board of seats. Not a blended person.
    Team,
}

impl Kind {
    /// Stable name stored next to the id: `persona`, `worker`, or `team`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Persona => "persona",
            Kind::Worker => "worker",
            Kind::Team => "team",
        }
    }
}

/// One seat the arena can load or score.
///
/// Paths are repo-relative when the file sits under `stack/agents/`. They are
/// empty when that file does not apply. This type does not open them again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gladiator {
    pub id: String,
    pub kind: Kind,
    pub soul_path: String,
    pub task_path: String,
    pub config_path: String,
    pub voice: String,
    pub pinned_skill: Option<String>,
    /// Trimmed `TASK.md` body. Empty when this seat has no task file.
    pub task: String,
    pub members: Vec<String>,
}

impl Gladiator {
    /// A team gladiator. Members keep their own score lines.
    ///
    /// # Errors
    ///
    /// Empty id, empty member list, or an empty member id.
    pub fn team(id: impl Into<String>, members: Vec<String>) -> Result<Self, LoadError> {
        let id = id.into();
        if id.is_empty() || members.iter().any(String::is_empty) {
            return Err(LoadError::EmptyId);
        }
        if members.is_empty() {
            return Err(LoadError::EmptyMembers);
        }
        Ok(Self {
            id,
            kind: Kind::Team,
            soul_path: String::new(),
            task_path: String::new(),
            config_path: String::new(),
            voice: String::new(),
            pinned_skill: None,
            task: String::new(),
            members,
        })
    }
}
