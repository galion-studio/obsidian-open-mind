<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/cutover-prep (slot 38)

**No new dependencies.** `crates/galion-deploy` stays std-only (see deploy-rust.md),
and `Cargo.lock` is unchanged by this branch.

| need | done with | why not a crate |
|---|---|---|
| tar archive of the overlay | `src/tar.rs`, a ustar writer/reader (POSIX.1-1988 headers, checksum, prefix split), about 200 lines, MIT, this project | the `tar` crate (MIT/Apache-2.0) pulls `filetime` + `xattr`; only plain files and dirs are needed |
| the data backup | the `galion-backup` **binary** from swarm/rust-ports, called as a child process (`run`, `GALION_BACKUP_DEST`, `GALION_BACKUP_KEEP`) | linking its library would add rusqlite (bundled SQLite), miniz_oxide and libc to the deploy tool, and make this branch depend on an unmerged one |
| why not galion-backup's `tarw.rs` | checked (rust-ports 26a2a8e): `TarWriter::add_tree` is a GNU-format tree walker built for byte-parity with `tar -c`, with uname lookups through `libc`. It reads files on its own, so the manifest sha would not be of the same bytes, and it has no reader, which `backup-verify` needs. It is reachable only through the `galion-backup` crate (rusqlite bundled, miniz_oxide, libc), which is not on this base | **decision item:** after rust-ports merges, move `tarw` plus a reader into a std-only shared crate and switch both tools to it |
| SHA-256, Zagreb time | deploy-rust's `src/sha256.rs` and `src/zagreb.rs` | — |

Licences: everything in this branch is MIT, `Copyright (c) 3000 Galion / galion-studio`.
