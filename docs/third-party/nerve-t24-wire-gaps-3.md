<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party: nerve-t24-wire-gaps-3 (wave-4 slot 289)

No new third-party crates.

All five wirings depend only on the in-workspace `nerve-client` path
dependency (`crates/nerve-client`). Existing third-party pins already in
`Cargo.lock` for the touched crates (e.g. `rusqlite` in desk-hermes,
`socket2` in openmind-discovery, optional candle features in
galion-voice-cpu) were not changed.
