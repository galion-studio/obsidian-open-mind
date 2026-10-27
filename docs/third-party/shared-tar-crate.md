<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: shared-tar-crate (slot 122)

No new dependencies and no copied code.

| item | licence | note |
|---|---|---|
| `crates/galion-tar` | MIT, this project | std only; code moved from galion-backup `src/tarw.rs` (rust-ports 26a2a8e) and galion-deploy `src/tar.rs` (cutover-prep a5ac49b), both MIT, this project; the reader is new |
| galion-deploy | — | still no third-party deps (now depends on our own `galion-tar`) |
| galion-backup | — | unchanged set (rusqlite, miniz_oxide, libc, zagreb-time); libc no longer used for `major/minor` |

Why not the `tar` crate (MIT/Apache-2.0): it pulls `filetime` and `xattr`
(libc), and byte parity with GNU tar's default `tar -c` output (readdir
order, GNU header layout, LongLink header fields, record padding) is the
backup's contract; the existing writer already proves it.
