// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Runtime configuration: where room listens and which tree it shows.

use std::path::{Path, PathBuf};

/// Default port. Room has always used 4020.
pub const DEFAULT_PORT: u16 = 4020;
/// Default data root. All indexed and readable paths live under it.
pub const DEFAULT_ROOT: &str = "/workspace";

/// Doc roots walked for the page, relative to [`Config::root`].
pub const WALK_ROOTS: [&str; 5] = [
    "galion-light/docs",
    "galion-src/galion-docs",
    "galion-src/galion-workspace3d/src",
    "galion-src/galion-app/docs",
    "galion-src/galion-app/src",
];

/// Single library manifests shown on the page, relative to [`Config::root`].
pub const MANIFESTS: [&str; 10] = [
    "galion-src/galion-workspace3d/package.json",
    "galion-src/galion-app/package.json",
    "galion-src/galion-docs/package.json",
    "galion-src/galion-rs/Cargo.toml",
    "galion-src/galion-console/package.json",
    "galion-src/galion-api-docs/package.json",
    "galion-src/galion-knowledge/package.json",
    "galion-src/galion-studio-app/package.json",
    "galion-src/merlin-element/package.json",
    "galion-src/galion-shared/package.json",
];

/// Directories under [`Config::root`] whose `.md`/`.txt` files `/api/read` may return.
pub const READ_ROOTS: [&str; 2] = ["galion-src", "galion-light/docs"];

/// The HTML shell served at `/`, relative to [`Config::root`].
pub const INDEX_HTML: &str = "galion-light/room/index.html";

/// Where room listens and which tree it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Listen address for `--listen`, e.g. `127.0.0.1:4020`.
    pub bind: String,
    /// Data root; defaults to `/workspace`.
    pub root: PathBuf,
}

impl Config {
    /// Reads `ROOM_BIND` and `ROOM_ROOT` from the process environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds a config from any variable lookup (testable without touching the
    /// real environment). Values are trimmed; empty counts as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let get = |name: &str| {
            lookup(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Config {
            bind: get("ROOM_BIND").unwrap_or_else(|| format!("127.0.0.1:{DEFAULT_PORT}")),
            root: get("ROOM_ROOT").map_or_else(|| PathBuf::from(DEFAULT_ROOT), PathBuf::from),
        }
    }

    /// A config with the default bind address and the given data root.
    pub fn with_root(root: impl AsRef<Path>) -> Self {
        Config {
            bind: format!("127.0.0.1:{DEFAULT_PORT}"),
            root: root.as_ref().to_path_buf(),
        }
    }

    /// Absolute path of the HTML shell served at `/`.
    #[must_use]
    pub fn index_html(&self) -> PathBuf {
        self.root.join(INDEX_HTML)
    }

    /// Absolute directories that `/api/read` may serve from.
    #[must_use]
    pub fn read_roots(&self) -> Vec<PathBuf> {
        READ_ROOTS.iter().map(|r| self.root.join(r)).collect()
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::with_root(DEFAULT_ROOT)
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::path::PathBuf;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn defaults_match_the_historic_room() {
        let cfg = Config::from_lookup(lookup(&[]));
        assert_eq!(cfg.bind, "127.0.0.1:4020");
        assert_eq!(cfg.root, PathBuf::from("/workspace"));
        assert_eq!(cfg, Config::default());
        assert_eq!(
            cfg.index_html(),
            PathBuf::from("/workspace/galion-light/room/index.html")
        );
        assert_eq!(
            cfg.read_roots(),
            vec![
                PathBuf::from("/workspace/galion-src"),
                PathBuf::from("/workspace/galion-light/docs")
            ]
        );
    }

    #[test]
    fn env_values_are_trimmed_and_blank_is_unset() {
        let cfg = Config::from_lookup(lookup(&[
            ("ROOM_BIND", "  127.0.0.1:19080 "),
            ("ROOM_ROOT", "/srv/x"),
        ]));
        assert_eq!(cfg.bind, "127.0.0.1:19080");
        assert_eq!(cfg.root, PathBuf::from("/srv/x"));
        let blank = Config::from_lookup(lookup(&[("ROOM_BIND", "   "), ("ROOM_ROOT", "")]));
        assert_eq!(blank, Config::default());
    }
}
