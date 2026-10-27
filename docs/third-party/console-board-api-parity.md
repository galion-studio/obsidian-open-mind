<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party / provenance: console board API parity (slot 303)

No new crates. `console-static` stays std-only (`Cargo.lock` unchanged).

| Item | Source | License | Notes |
|---|---|---|---|
| `crates/console-static/src/sha256.rs` | in-tree copy of `crates/galion-one/src/sha256.rs` | MIT (Galion) | FIPS 180-4, known-vector tests kept |
| `crates/console-static/board/fleet-board.json` | converted from Galion `console-ide/src/lib/fleet.ts` + `lib/api-catalog.ts` (VPS, read-only copy) | MIT (Galion), `.license` sidecar | machine IPs, SSH keys and `machine.ts` left out |
| `src/admin.rs` | behaviour port of Galion `console-ide/src/lib/admin-device.ts` | MIT (Galion) | no code copied verbatim |

GitHub search for reuse (rule t220): a JSON-file pairing store plus board
catalog is ~600 lines of std Rust; an HTTP/JSON/crypto dependency would add
far more weight than it saves, and the crate already had its own JSON and
HTTP code, so nothing external was pulled in.
