<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: realscript-port-2

No new third-party crate and no copied code.

New dev-dependency edges, all in-workspace (MIT, this repository):

| crate | added as dev-dependency of | used for |
|---|---|---|
| `galion-realtest` (path) | openmind-discovery, repo-check, openmind-nerve, galion-one, openmind-queue | `galion_realtest::realrun` plumbing and its HTTP client |
| `serde_json` (workspace, already in Cargo.lock) | galion-one | parsing JSON responses in `real_one_static.rs` |

`Cargo.lock` gains only these in-workspace edges (5 lines).

External programs the real tests call (not linked): `nice`, `kill`, `ps`,
`date`, `git`, `readelf` (binutils), `ssh` (read-only VPS test only), and
`scripts/public-export.sh` with its scanners (`cargo-deny`, `comply`,
optional `gitleaks`, `cargo-cyclonedx`), as documented in
`docs/launch/PUBLIC-EXPORT.md`. `curl` and `jq` are no longer needed by these
tests.
