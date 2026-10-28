<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: hades-redteam (S3.1)

2026-10-20 23:00 CEST +02:00

**No new dependencies and no copied code.** `Cargo.toml` and `Cargo.lock` are unchanged from ccaab42.
The allow-list, the corpus, the example and both tests use only `std`, `hades-gate` itself and `serde_json`
(already a dependency of hades-gate, MIT OR Apache-2.0). A shell-word parser crate (e.g. `shlex`) was considered
and not used: the fix refuses shell metacharacters outright, so there is nothing left to parse.
