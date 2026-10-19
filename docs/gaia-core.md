<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# gaia-core

Dry-run only. Written 2026-10-19T16:42:28+02:00 (Europe/Zagreb).

`gaia status` prints a Zagreb timestamp and `idle`. `gaia tick` is one pure step and does not loop. `gaia simulate-faro` is text only: no power change, no GPIO, no network. `--master-override` is refused unless `GAIA_OVERRIDE=1`, and even then it prints `refused: not approved` and exits 2. It never restarts a service.
