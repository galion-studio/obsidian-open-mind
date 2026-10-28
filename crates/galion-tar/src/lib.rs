// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! galion-tar: the one tar implementation of the Galion tools, std only.
//!
//! * [`gnu::TarWriter`]: GNU-format tree writer with GNU tar 1.35 `tar -c`
//!   semantics (readdir order, hard links, `././@LongLink`, base-256 numbers,
//!   10240-byte record). galion-backup's `files.tar` and `.tar.gz` are
//!   byte-identical to GNU tar's (proven by galion-backup's real parity test
//!   and by `tests/gnu_oracle.rs` here).
//! * [`ustar`]: POSIX.1-1988 ustar entry writer and whole-archive reader for
//!   in-memory entries (regular files, directories, symlinks). galion-deploy's
//!   pre-cutover `backup.tar` is written and verified with it.
//! * [`read::Reader`]: streaming reader for both formats (GNU long names,
//!   base-256 numbers, hard links, devices, fifos, minimal pax `x` records),
//!   checksums verified. It can read either tool's archives.
//!
//! uid/gid -> name lookups go through [`names::Names`]; the default
//! [`names::EtcNames`] parses `/etc/passwd` and `/etc/group` in pure std.
//! galion-backup plugs a `getpwuid_r`/`getgrgid_r` lookup instead so NSS
//! sources other than files match GNU tar exactly.

pub mod gnu;
pub mod names;
pub mod num;
pub mod read;
pub mod ustar;

/// Size of one tar block.
pub const BLOCK: usize = 512;
/// GNU tar default record size (blocking factor 20).
pub const RECORD: u64 = 10240;
