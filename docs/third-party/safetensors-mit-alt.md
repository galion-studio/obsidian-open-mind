<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# safetensors-mit-alt — third-party notes (slot 295)

Branch `swarm/safetensors-mit-alt`, base `swarm/cargo-deny-gate` 400e93a5. 2026-10-27 CET (+01:00).

## New code
| Item | Licence | Source | Use |
|---|---|---|---|
| `vendor/safetensors-mit` (package `safetensors` 0.7.0, Galion-written) | MIT | this repo | Clean-room reader/writer for the safetensors format; replaces registry `safetensors` 0.7.0 under candle via `[patch.crates-io]`. |

No code copied. Written from the public format description; upstream `safetensors` 0.7.0
(https://github.com/huggingface/safetensors, Apache-2.0) public item names/signatures were read so
candle-core / candle-nn compile unchanged. Its source is not vendored or linked on this branch.

## Dependencies of the new crate (no new crates in Cargo.lock)
| Name | Version in lock | Licence | Source | Why |
|---|---|---|---|---|
| serde | 1.0.229 | MIT OR Apache-2.0 (taken as MIT) | https://github.com/serde-rs/serde | Streaming map visitor so duplicate header keys can be rejected. Already in the lock. |
| serde_json | 1.0.154 | MIT OR Apache-2.0 (taken as MIT) | https://github.com/serde-rs/json | JSON header parse + string escaping on write. Already in the lock. |

## Removed from Cargo.lock
| Name | Version | Licence | Note |
|---|---|---|---|
| safetensors (registry) | 0.7.0 | Apache-2.0 | AP-1 offender; replaced. |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | Only the registry safetensors used this version. |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | Only via hashbrown 0.16.1. |

## Alternatives researched (crates.io API + docs.rs, 2026-10-27 19:16 CET)
| Crate | Latest | Licence | Downloads (all time) | Verdict |
|---|---|---|---|---|
| safetensors | 0.8.0 | Apache-2.0 | 33.3 M | Upstream; still Apache-only, so bumping does not fix AP-1. |
| ndarray-safetensors | 0.3.0 | MIT | 32.8 k | Wrapper that depends on `safetensors` itself; no help. |
| tpt-safetensors-io | 0.1.0 | MIT OR Apache-2.0 | 71 | New (2026-07), own API (builder + memmap2), not candle-compatible, tiny user base. |
| proxima-safetensors | 0.1.0 | MIT OR Apache-2.0 | 17 | Sans-IO parser, pulls 4 proxima-* crates + bon/conflaguration; not candle-compatible. |
| ferrox-safetensors | 0.25.0 | Apache-2.0 | 759 | Apache-only; no help. |
None is a drop-in for candle's `safetensors::tensor::*` + `slice::IndexOp` API, and the MIT ones are weeks old with
under 100 downloads (supply-chain risk). A ~1000-line (src) in-repo shim with tests was chosen instead (same pattern as
`vendor/paste-pastey`).

## Tools used, not shipped
Scratch parity harness `/workspace/swarm/target/safetensors-mit-alt/compare` (not committed) linked both upstream
`safetensors` 0.7.0 and this crate to compare them on real model files; see docs/real-tests/safetensors-mit-alt.md.
cargo-deny 0.20.2 (MIT OR Apache-2.0), box-local `/workspace/swarm/tools/bin/cargo-deny`.
