// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! `galleon-arena list`: print the seed gladiators.
//!
//! The clock is `TZ=Europe/Zagreb date -Iseconds`. The text is a roster, not a
//! bout. Scoring in this crate stays paper integers. This module does not open
//! a socket and does not transfer a value.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bout::zagreb_stamp;
use crate::gladiator::Gladiator;
use crate::load::{load_seed, redacted_homes, LoadError, SEED_IDS, SEED_TEAM_ID};

/// Usage text for stdout (`--help`) and stderr (bad arguments).
pub const USAGE: &str = "\
usage: galleon-arena list [--homes PATH]
       galleon-arena --help

list prints the seed gladiators g2-0 g2-1 g2-2 g2-3 and the arena-compare team.
The timestamp is Europe/Zagreb RFC 3339 from TZ=Europe/Zagreb date -Iseconds.
It carries +01:00 or +02:00 and never a Zulu suffix. Paper score only. No network.
";

/// What a successful parse prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliOut {
    /// Help text. The caller prints [`USAGE`].
    Help,
    /// Roster text, already ending in a newline.
    List(String),
}

/// A bad argument line, or a list that could not be built.
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    /// The process should exit 2 and print [`USAGE`].
    Usage(String),
    /// The process should exit 1. Nothing was written.
    Failed(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(msg) | CliError::Failed(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for CliError {}

/// Why `list` refused a roster. Nothing was written.
#[derive(Debug)]
pub enum ListError {
    BadStamp(String),
    Load(LoadError),
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListError::BadStamp(at) => write!(
                f,
                "timestamp is not Europe/Zagreb RFC 3339 with a numeric offset: {at}"
            ),
            ListError::Load(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ListError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ListError::Load(err) => Some(err),
            ListError::BadStamp(_) => None,
        }
    }
}

impl From<LoadError> for ListError {
    fn from(err: LoadError) -> Self {
        ListError::Load(err)
    }
}

/// Current Europe/Zagreb time from `TZ=Europe/Zagreb date -Iseconds`.
///
/// Seconds precision, explicit offset. `Z` and `+00:00` are rejected.
///
/// # Errors
///
/// `date` cannot be run, or its stdout is not a Zagreb RFC 3339 stamp.
#[must_use = "the stamp is the only clock the list command may print"]
pub fn read_zagreb_now() -> Result<String, CliError> {
    let output = Command::new("date")
        .arg("-Iseconds")
        .env("TZ", "Europe/Zagreb")
        .output()
        .map_err(|err| CliError::Failed(format!("date: {err}")))?;
    if !output.status.success() {
        return Err(CliError::Failed("date -Iseconds failed".to_string()));
    }
    let stamp = String::from_utf8(output.stdout)
        .map_err(|_| CliError::Failed("date output is not utf-8".to_string()))?;
    let stamp = stamp.trim().to_string();
    if !zagreb_stamp(&stamp) {
        return Err(CliError::Failed(format!(
            "date did not return a Europe/Zagreb RFC 3339 stamp: {stamp}"
        )));
    }
    Ok(stamp)
}

/// Parse arguments after the program name.
///
/// `at` is used only for `list`. Pass the string from [`read_zagreb_now`].
/// Help does not read it.
///
/// # Errors
///
/// Unknown arguments, a missing homes directory, or a stamp that is not
/// Europe/Zagreb RFC 3339.
pub fn dispatch(args: &[String], at: &str) -> Result<CliOut, CliError> {
    match args.first().map(String::as_str) {
        None => Err(CliError::Usage("missing command".to_string())),
        Some("-h" | "--help" | "help") => {
            if args.len() != 1 {
                return Err(CliError::Usage("help takes no arguments".to_string()));
            }
            Ok(CliOut::Help)
        }
        Some("list") => list_cmd(&args[1..], at),
        Some(other) => Err(CliError::Usage(format!("unknown command `{other}`"))),
    }
}

/// Load `homes` and render the roster at `at`.
///
/// Reads `SOUL.md` and `TASK.md` for `g2-0` through `g2-3`. Adds the
/// `arena-compare` team in memory. Does not create a path.
///
/// # Errors
///
/// `at` is not Europe/Zagreb RFC 3339, or a seed home cannot be read.
pub fn render_list(homes: &Path, at: &str) -> Result<String, ListError> {
    if !zagreb_stamp(at) {
        return Err(ListError::BadStamp(at.to_string()));
    }
    let seats = load_seed(homes)?;
    let team = Gladiator::team(
        SEED_TEAM_ID,
        SEED_IDS.iter().map(|id| (*id).to_string()).collect(),
    )?;
    Ok(format_list(at, &seats, &team))
}

fn list_cmd(args: &[String], at: &str) -> Result<CliOut, CliError> {
    let mut rest = args.iter();
    let mut homes: Option<&str> = None;
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                if homes.is_some() || rest.next().is_some() {
                    return Err(CliError::Usage(
                        "list --help takes no other arguments".to_string(),
                    ));
                }
                return Ok(CliOut::Help);
            }
            "--homes" => {
                if homes.is_some() {
                    return Err(CliError::Usage("--homes given twice".to_string()));
                }
                let path = rest
                    .next()
                    .ok_or_else(|| CliError::Usage("--homes needs a path".to_string()))?;
                homes = Some(path.as_str());
            }
            other => {
                return Err(CliError::Usage(format!("unknown argument `{other}`")));
            }
        }
    }
    let path = match homes {
        Some(path) => PathBuf::from(path),
        None => redacted_homes().ok_or_else(|| {
            CliError::Failed(
                "seed homes not found under stack/agents/personas/arena/homes".to_string(),
            )
        })?,
    };
    let text = render_list(&path, at).map_err(|err| CliError::Failed(err.to_string()))?;
    Ok(CliOut::List(text))
}

fn format_list(at: &str, seats: &[Gladiator], team: &Gladiator) -> String {
    let mut out = String::new();
    push_line(&mut out, &["at", at]);
    for seat in seats {
        let mut parts = vec![seat.id.as_str(), seat.kind.as_str()];
        if let Some(skill) = seat.pinned_skill.as_deref() {
            parts.push(skill);
        }
        push_line(&mut out, &parts);
    }
    let mut parts = Vec::with_capacity(2 + team.members.len());
    parts.push(team.id.as_str());
    parts.push(team.kind.as_str());
    for member in &team.members {
        parts.push(member.as_str());
    }
    push_line(&mut out, &parts);
    out
}

fn push_line(out: &mut String, parts: &[&str]) {
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(part);
    }
    out.push('\n');
}
