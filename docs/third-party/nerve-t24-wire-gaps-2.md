<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party: nerve-t24-wire-gaps-2 (wave-4 slot 277)

No new third-party crates.

All six wirings depend only on the in-workspace `nerve-client` path
dependency (`crates/nerve-client`). Existing third-party pins already in
`Cargo.lock` for the touched crates (e.g. `ed25519-dalek` / `sha2` in
hades-gate, `regex` in galion-guard, `rusqlite` in galion-backup) were
not changed.
