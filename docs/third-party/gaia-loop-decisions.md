<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: gaia-loop-decisions (wave-4 slot 175)

**No new dependencies and no copied code.** Cargo.lock and every Cargo.toml are unchanged. The work uses only std plus crates already in the workspace:

| Crate (already present) | Used for |
|---|---|
| `hades-gate` (workspace) | `token::{Claims, from_hex, plan_hash, to_hex, verify_signature}`, `keys::{load_seed, load_public, os_random}`, `ledger::Ledger::is_consumed`, `config::Config` for `gaia tokens` (same signing code as `hades-gate mint`). |
| `serde_json` (workspace, MIT OR Apache-2.0) | Nerve `/state` parsing, request files, CLI output. |
| `zagreb-time` (workspace) | Europe/Zagreb stamps in request files, status and events. |

`/proc/net/tcp{,6}` and `/proc/<pid>/fd` are read with std (`heal::listener_pids`) to name a hung service's listener. Nothing is signalled.
