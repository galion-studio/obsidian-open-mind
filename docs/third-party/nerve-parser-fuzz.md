<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-parser-fuzz (slot 212)

No new dependencies (Cargo.toml and Cargo.lock unchanged). std + the
crate's existing serde_json only.

Copied code: `crates/openmind-nerve/tests/fuzz/mod.rs` comes from this
repository's own `swarm/cortex-t52-fuzz` c2f7cda (MIT, Galion /
galion-studio). It implements SplitMix64 (public-domain algorithm by
Sebastiano Vigna, re-derived there) and FNV-1a (public-domain hash).
`civil()` in `tests/fuzz/nerve.rs` is H. Hinnant's public-domain
`civil_from_days` algorithm, re-derived (inverse of the crate's own
`util::days_from_civil`).
