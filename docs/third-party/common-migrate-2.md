<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# common-migrate-2: third-party review (wave 2 slot 28, branch `swarm/common-migrate-2`)

Reviewed 2026-10-20 21:00 +02:00 (Europe/Zagreb).

## Result: no new third-party dependency, no copied code

- The migrated crates (hermes-rs, openmind-memory, galion-desk, room,
  link-watch, openmind-survival, one-talk) only gain or keep the in-repo
  path dependency `galion-common` (MIT, this repository). Its default
  features use std only.
- `edge/openmind-node` (its own lock file) gains the path dependency
  `galion-common = { path = "../../crates/galion-common" }`. Its
  `Cargo.lock` gains just that one local package: no registry crate,
  checksum or download.
- The workspace `Cargo.lock` gains no external package; only the dependency
  lists of galion-desk, room and openmind-survival change.
- The chunked decoder (`ChunkDecoder`) follows RFC 9112 section 7.1. It was
  written from the RFC, not copied from httparse, hyper or any other
  project. The parser candidates already reviewed in
  `docs/third-party/common.md` (httparse, tiny_http, ureq/minreq) were
  rejected again for the same reasons.
- The old `b64url` in `edge/openmind-node/src/util.rs` is kept, in the
  test module only, as a reference implementation that the shared encoder
  is checked against. It is our own code.
