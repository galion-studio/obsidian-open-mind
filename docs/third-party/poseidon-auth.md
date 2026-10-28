<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: poseidon-auth (slot 205)

No new dependencies and no copied code. The auth layer
(`crates/openmind-queue/src/auth.rs`) is std only: constant-time compare,
`/proc/self` owner for the effective uid (no libc), `MetadataExt` for mode and
owner. Cargo.lock is unchanged.
