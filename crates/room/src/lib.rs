// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! # room
//!
//! A read-only view of the operator's files as one page.
//!
//! Room is not a sandbox and runs no commands. It lists a fixed set of live
//! services, a set of held host names, and the docs, papers, library manifests
//! and API/visual sources it finds under a data root (default `/workspace`),
//! and prints that list as JSON. With `--listen` it serves the same list over
//! a small GET-only HTTP server (default `127.0.0.1:4020`) plus a guarded
//! reader for `.md`/`.txt` files.
//!
//! ## Modes
//! - `room`: print the page JSON for the data root, exit 0. No socket.
//! - `room --fixture PATH`: print the page JSON for one file or tree. No socket.
//! - `room --listen`: serve HTTP on `ROOM_BIND` (default `127.0.0.1:4020`).
//!
//! ## Environment
//! - `ROOM_BIND`: listen address for `--listen` (trimmed; blank means unset).
//! - `ROOM_ROOT`: data root (default `/workspace`).
//!
//! ## Safety properties (all tested)
//! - Only `--listen` opens a socket (`tests/no_bind.rs` checks this with
//!   `strace` inside an empty network namespace).
//! - Paths hinting at secrets (`secret`, `.env`, `token`, `id_ed25519`) are
//!   never listed or served.
//! - Walking never follows symbolic links and stops at [`index::MAX_DEPTH`].
//! - `/api/read` serves only `.md`/`.txt` regular files up to
//!   [`http::MAX_READ_BYTES`] whose canonical path stays inside a read root.
//! - Request parsing never panics (the release profile aborts on panic).

#![warn(missing_docs)]

pub mod cli;
pub mod config;
pub mod error;
pub mod http;
pub mod index;
pub mod json;

pub use cli::{parse, Mode, USAGE};
pub use config::Config;
pub use error::RoomError;
pub use index::{index, index_at, Obj};
pub use json::to_json;

/// The page JSON for a non-listening mode, or `None` for modes that print
/// nothing of the page (`--listen`, `--help`, `--version`). Opens no socket.
#[must_use]
pub fn page(mode: &Mode, cfg: &Config) -> Option<String> {
    match mode {
        Mode::Page => Some(to_json(&index(cfg))),
        Mode::Fixture(path) => Some(to_json(&index_at(path))),
        Mode::Listen | Mode::Help | Mode::Version => None,
    }
}

/// Runs the server for `cfg`: builds the page once, binds, reports on
/// stderr, then serves forever. Returns only on a bind or accept error.
///
/// # Errors
/// [`RoomError::Bind`] when `cfg.bind` cannot be bound.
pub fn listen(cfg: Config) -> Result<(), RoomError> {
    let objects = index(&cfg);
    let server = http::Server::bind(cfg, to_json(&objects))?;
    eprintln!("room: {} objects", objects.len());
    eprintln!("room: listening on {}", server.local_addr()?);
    server.run()
}

#[cfg(test)]
mod tests {
    use super::{page, Config, Mode};
    use std::fs;
    use std::path::PathBuf;

    fn socket_fds() -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir("/proc/self/fd")
            .expect("fd")
            .flatten()
            .filter_map(|e| fs::read_link(e.path()).ok())
            .map(|t| t.to_string_lossy().into_owned())
            .filter(|t| t.starts_with("socket:"))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn default_and_fixture_do_not_open_a_socket() {
        let before = socket_fds();
        let cfg = Config::with_root("/nonexistent-room-root");
        let page_default = page(&Mode::Page, &cfg).expect("default page");
        assert!(page_default.starts_with("{\"objects\":["));
        assert!(page_default.contains("\"name\":\"room\""));
        assert_eq!(socket_fds(), before);

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir: PathBuf =
            std::env::temp_dir().join(format!("room-fix-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        fs::write(dir.join("note.md"), "# Desk note\n").expect("note");
        let before_fix = socket_fds();
        let page_fix = page(&Mode::Fixture(dir.clone()), &cfg).expect("fixture page");
        let after_fix = socket_fds();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(before_fix, after_fix);
        assert!(page_fix.contains("\"kind\":\"doc\""));
        assert!(page_fix.contains("Desk note"));
    }

    #[test]
    fn non_page_modes_print_no_page() {
        let cfg = Config::default();
        assert!(page(&Mode::Listen, &cfg).is_none());
        assert!(page(&Mode::Help, &cfg).is_none());
        assert!(page(&Mode::Version, &cfg).is_none());
    }
}
