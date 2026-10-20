// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio

//! `history-purge`: drop listed paths from every commit of a **scratch
//! mirror clone**, using git's own `fast-export | filter | fast-import`.
//!
//! Safety model (see `docs/launch/SEC-1-HISTORY-PURGE.md`):
//! * `rewrite` only works on a bare repository (make one with
//!   `git clone --mirror`), refuses shallow clones, refuses paths under
//!   `/opt/galion`, and refuses any repository that still has a remote
//!   unless `--strip-remotes` is given, which deletes the remote config
//!   (never refs) *before* rewriting. The result cannot be pushed by
//!   accident; pushing is a separate, manual owner step.
//! * Nothing ever prints file contents: reports list paths, counts and
//!   object ids only.

pub mod pattern;
pub mod repo;
pub mod sec3;
pub mod stream;
pub mod zagreb;

pub use pattern::PathSet;
pub use stream::{filter, Stats};
