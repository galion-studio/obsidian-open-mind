<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: cortex-t52-fuzz (slot 185)

**No new third-party crates and no copied code.**

| Name | Version | Licence | Source | Used for | Why |
|---|---|---|---|---|---|
| `tunnel-lint` (workspace path crate, dev-dependency of `openmind-nerve`) | 0.1.0 | MIT | `crates/tunnel-lint` (from `swarm/tunnel-audit-2` 988e47f, merged here) | the `ingress-yaml` fuzz target and `gen-ingress` property drive its parser | ours; zero dependencies |

Algorithms written from public descriptions, no code copied:
- SplitMix64 (Sebastiano Vigna, public domain), the fuzz PRNG.
- FNV-1a (public domain), output signatures.
- civil-from-days (Howard Hinnant's public-domain description), the
  test-side inverse used by `gen-event`.

Not used on purpose: `cargo-fuzz` / `libfuzzer-sys` (nightly + C++ via
`cc`, the task says no new asm/C deps), `proptest`, `quickcheck`,
`arbitrary` (extra crates for what ~400 lines of std do here).
`Cargo.lock` change: only the new `tunnel-lint` dev-dependency edge.
