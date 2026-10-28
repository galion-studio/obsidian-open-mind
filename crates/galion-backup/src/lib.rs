// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! galion-backup: the periodic Galion backup (Rust port of
//! ws/foundation `deploy/foundation/galion-backup.sh`, VPS copy sha256
//! 0897efc0...) plus the SQLite helper subcommands the script called.
//! See docs/galion-backup.md for the line-by-line mapping.

pub mod backup;
pub mod gzip;
pub mod sha256;
pub mod sqlite;
pub mod tarw;
