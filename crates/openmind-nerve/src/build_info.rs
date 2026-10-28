// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Build identity (F-377-1): the source commit and commit date stamped by
//! `build.rs`, shown by `openmind-nerve --version`, the startup log line and
//! `/health`. Ported from the cortex (swarm/cortex-version-commit b36dc358).
//!
//! Deterministic per commit (no wall clock), so two builds of one commit stay
//! byte-identical. For a `git archive` build (no `.git`), pass
//! `GALION_BUILD_COMMIT=<sha>` and
//! `SOURCE_DATE_EPOCH=$(git show -s --format=%ct <sha>)`; then the archive
//! build and a checkout build of that commit are byte-identical.

use serde_json::{json, Value};

/// Full source commit (hex), or `unknown`.
pub const COMMIT: &str = env!("OPENMIND_NERVE_BUILD_COMMIT");
const EPOCH_STR: &str = env!("OPENMIND_NERVE_BUILD_EPOCH");

/// Commit (committer) date as unix seconds; 0 = unknown.
pub fn epoch() -> i64 {
    EPOCH_STR.parse().unwrap_or(0)
}

/// Short commit (8 hex) or `unknown`.
pub fn short_commit() -> &'static str {
    if COMMIT == "unknown" {
        COMMIT
    } else {
        &COMMIT[..COMMIT.len().min(8)]
    }
}

/// Build date in Europe/Zagreb RFC 3339 (explicit offset), or `unknown`.
pub fn date() -> String {
    match epoch() {
        e if e > 0 => zagreb_time::from_unix(e),
        _ => "unknown".to_string(),
    }
}

/// Compiled-in cargo features that change behaviour.
pub fn features() -> Vec<&'static str> {
    let mut f = Vec::new();
    if cfg!(feature = "journal-native") {
        f.push("journal-native");
    }
    f
}

/// The `--version` line, e.g.
/// `openmind-nerve 0.1.0 (commit 9baa450c, built 2026-10-28T05:10:00+01:00, features none)`.
pub fn version_line() -> String {
    let feats = features();
    let feats = if feats.is_empty() {
        "none".to_string()
    } else {
        feats.join(",")
    };
    format!(
        "openmind-nerve {} (commit {}, built {}, features {})",
        env!("CARGO_PKG_VERSION"),
        short_commit(),
        date(),
        feats
    )
}

/// The `build` object in `/health`.
pub fn json() -> Value {
    json!({"commit": COMMIT, "date": date(), "features": features()})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_is_well_formed() {
        assert!(COMMIT == "unknown" || COMMIT.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(COMMIT == "unknown" || (7..=64).contains(&COMMIT.len()));
        let d = date();
        assert!(
            d == "unknown" || zagreb_time::require_rfc3339(&d).is_ok(),
            "{d}"
        );
        assert!(!d.ends_with('Z'));
        assert_eq!(d == "unknown", epoch() == 0);
    }

    #[test]
    fn version_line_shape() {
        let v = version_line();
        assert!(v.starts_with(concat!(
            "openmind-nerve ",
            env!("CARGO_PKG_VERSION"),
            " (commit "
        )));
        assert!(v.contains(short_commit()) && v.contains(", built ") && v.contains(", features "));
        assert!(short_commit().len() <= 8 || short_commit() == "unknown");
        assert!(!v.contains('Z'), "{v}");
    }

    #[test]
    fn json_shape() {
        let j = json();
        assert_eq!(j["commit"], COMMIT);
        assert!(j["features"].is_array());
        assert_eq!(j["date"], date());
        assert_eq!(
            j["features"].as_array().unwrap().len(),
            usize::from(cfg!(feature = "journal-native"))
        );
    }
}
