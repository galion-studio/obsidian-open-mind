<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: sec-ledger-race (slot 219)

No new crate dependencies and no copied code. `Cargo.lock` is unchanged.

The real test `crates/hades-gate/tests/real_ledger_race.rs` runs two system
programs as separate processes (not linked, not vendored):

| Program | Version on the box | Licence | Used for |
|---|---|---|---|
| `mkfifo` | GNU coreutils 9.7 | GPL-3.0-or-later | one FIFO per `check` process as the start barrier |
| `prlimit` | util-linux 2.41.5 | GPL-2.0-or-later | `--fsize` limit to make the kernel cut a ledger write short |

Why not in Rust: both need `libc` calls (`mkfifo(3)`, `setrlimit(2)` in
`pre_exec`), which would add a `libc` dev-dependency and `unsafe` in tests.
Owner question D-219-4 (same as slot 165's D-A6).
