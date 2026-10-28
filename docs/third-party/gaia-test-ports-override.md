<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: gaia-test-ports-override (slot 423)

No new dependencies and no copied code. The override is std-only code in
`crates/gaia-live/src/heal.rs`; the new real test uses the existing
`galion-realtest` dev-dependency (path crate) and `serde_json` (workspace).
Cargo.lock unchanged.
