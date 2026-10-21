<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party code: galion-realtest (swarm/real-harness)

Written 2026-10-20T19:30:00+02:00 (Europe/Zagreb).

**No new third-party dependency.** `crates/galion-realtest` depends only on:

| Crate | Version | Licence (SPDX) | Source | Use | Why |
|---|---|---|---|---|---|
| serde_json | 1 (workspace, already in Cargo.lock) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parse service JSON responses and reports | already used by most workspace crates; no new transitive crates |
| zagreb-time | workspace crate | MIT | crates/zagreb-time (this repo) | Europe/Zagreb RFC 3339 stamps in every report | the stack's shared timestamp rule |

The `Cargo.lock` diff is only the new `galion-realtest` package entry (8 lines); no new external package was added.

**Copied code (same repo, same MIT licence):** `crates/galion-realtest/src/sha256.rs` is a copy of `crates/galion-one/src/sha256.rs` at 13a296c (header kept, provenance comment added). It is used to write the fixture pairing store with the same code hash galion-one checks. A shared helper crate would be better; that is the existing "NIH helpers copied three times" finding in review CODE.md, not new here.

Tools the harness *runs* (not linked): `galion-boot` (stack supervisor), `sh` (signal builtin), `nice`/`cargo` (only with `--build`), `ssh` and `curl` (live mode only, GET-only script). Optional: `git`, `uname` for the report header.
