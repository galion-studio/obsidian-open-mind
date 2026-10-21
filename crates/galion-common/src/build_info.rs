// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Build provenance for every binary's `--version`.
//!
//! The values come from `option_env!` at compile time: the release script
//! sets `GALION_BUILD_COMMIT` and `GALION_BUILD_TIME`; anything else builds
//! with `"unknown"`. There is deliberately no build.rs and no git call, so
//! one source tree always produces the same bytes (reproducible builds).
//!
//! `--version` handling is pure: no config, no sockets, no files, so it
//! returns instantly and can never hang a deploy probe.

/// Commit the binary was built from, or `"unknown"`.
pub const COMMIT: &str = match option_env!("GALION_BUILD_COMMIT") {
    Some(v) => v,
    None => "unknown",
};

/// Build time as set by the release script, or `"unknown"`.
pub const BUILD_TIME: &str = match option_env!("GALION_BUILD_TIME") {
    Some(v) => v,
    None => "unknown",
};

/// `"<name> <version> (commit <c>, built <t>)"`.
pub fn version_line(name: &str, version: &str) -> String {
    format!("{name} {version} (commit {COMMIT}, built {BUILD_TIME})")
}

/// True when the first argument (after argv\[0\]) is `--version` or `-V`.
pub fn version_requested<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .nth(1)
        .is_some_and(|a| matches!(a.as_ref(), "--version" | "-V"))
}

/// Call first thing in `main`: on `--version`/`-V` print [`version_line`]
/// and exit 0 before any config, bind or I/O happens.
pub fn exit_if_version(name: &str, version: &str) {
    if version_requested(std::env::args()) {
        println!("{}", version_line(name, version));
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_or_env() {
        assert_eq!(
            COMMIT,
            option_env!("GALION_BUILD_COMMIT").unwrap_or("unknown")
        );
        assert_eq!(
            BUILD_TIME,
            option_env!("GALION_BUILD_TIME").unwrap_or("unknown")
        );
        assert!(!COMMIT.is_empty() && !BUILD_TIME.is_empty());
    }

    #[test]
    fn line_shape() {
        let l = version_line("tuner", "0.1.0");
        assert!(l.starts_with("tuner 0.1.0 (commit "));
        assert!(l.contains(&format!("commit {COMMIT}, built {BUILD_TIME})")));
    }

    #[test]
    fn requested() {
        assert!(version_requested(["bin", "--version"]));
        assert!(version_requested(["bin", "-V"]));
        assert!(!version_requested(["bin"]));
        assert!(!version_requested(["bin", "--listen"]));
        assert!(!version_requested(["bin", "x", "--version"]));
        assert!(!version_requested(["bin", "--versions"]));
    }
}
