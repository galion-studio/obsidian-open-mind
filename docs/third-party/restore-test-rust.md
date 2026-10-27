<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: restore-test-rust (wave-4 slot 326)

**No new dependencies and no copied code.** `Cargo.toml` of galion-backup is
unchanged; `Cargo.lock` gains no package.

What the new code uses, all already direct dependencies of galion-backup:

| Crate | Version | Licence (SPDX) | Source | New use |
|---|---|---|---|---|
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide | `inflate::stream::inflate` (raw inflate) for the streaming gzip reader `src/gunzip.rs`; gzip framing, header flags and CRC32/ISIZE trailer checks are ours (CRC32 from `src/gzip.rs`) |
| galion-tar | workspace | MIT | in-repo `crates/galion-tar` | `read::Reader` to walk the outer archive and `files/files.tar` |
| rusqlite | 0.32 (bundled SQLite) | MIT | https://github.com/rusqlite/rusqlite | unchanged `sqlite::verify` (integrity_check + table count), now in-process |
| libc | 0.2 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | `umask`, `mkfifo` (restore of fifos), `kill` in a test |

Why not the `flate2` / `tar` crates: both would add dependencies for what
~200 lines on top of crates we already ship do; `tar` also follows symlinks
during extraction unless configured, which is exactly what the restore must
refuse (see `parent_dirs` in `src/restore.rs`).
