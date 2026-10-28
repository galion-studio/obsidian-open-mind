<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cortex-t42-hades-plans: third-party review (slot 192, branch `swarm/cortex-t42-hades-plans`)

Reviewed 2026-10-21 14:55 +02:00 (Europe/Zagreb).

## Result: no new runtime dependency, no copied code, no new package in Cargo.lock

`openmind-nerve`'s runtime dependencies are unchanged (serde, serde_json, semver, zagreb-time,
galion-common). The plan generator (`src/plans.rs`) is std + serde_json only; it does not link
hades-gate, so the nerve binary gains no crypto code.

Two **dev-dependencies** (tests only, not in the binary):

| crate | version | licence (SPDX) | source | used for | why not write it |
|---|---|---|---|---|---|
| `hades-gate` | workspace | MIT (ours) | `crates/hades-gate` | every plan template is checked by the real admin-safety review, the real single-use approval path and `valid_target` | the point of the test is to use the gate itself, not a copy of its rules |
| `ed25519-dalek` | 2.2.0 (already in Cargo.lock via hades-gate) | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek | sign throwaway test approvals (fixed test seed) for the approval test | same crate, version and features as hades-gate (`default-features = false`, `std`, `zeroize`); never write crypto ourselves |

Transitive (already in the lock through hades-gate): curve25519-dalek 4.1.3 BSD-3-Clause,
sha2 0.10.9 MIT OR Apache-2.0, zeroize 1.9.1 Apache-2.0 OR MIT. `Cargo.lock` change: one line
(`ed25519-dalek` added to openmind-nerve's dependency list); no new `[[package]]`.
