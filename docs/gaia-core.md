<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# gaia-core

> **Superseded (2026-10-20 19:00 CEST (+02:00), swarm/gaia-land).** This page describes the older VPS dry-run gaia-core (with gaia-registry), which is not in main 13a296c. The Gaia architecture is now the ws/gaia orchestrator: `crates/gaia-core`, the nine function crates and the `gaia` CLI. See `gaia/README.md`, `gaia/docs/gaia-design.md` and `docs/gaia-ownership.md`.

Dry-run only. Written 2026-10-19T16:42:28+02:00 (Europe/Zagreb).

`gaia status` prints a Zagreb timestamp and `idle`. `gaia tick` is one pure step and does not loop. `gaia simulate-faro` is text only: no power change, no GPIO, no network. `--master-override` is refused unless `GAIA_OVERRIDE=1`, and even then it prints `refused: not approved` and exits 2. It never restarts a service.
