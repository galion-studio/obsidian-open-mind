<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: poseidon-refusal-persist

No new dependencies and no copied code. The sidecar uses std
(`std::os::unix::fs::FileExt` positioned writes) and the crate's existing
serde / serde_json and its own CRC-32 (`store::crc32`). Cargo.lock is
unchanged.
