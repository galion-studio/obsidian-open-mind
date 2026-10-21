<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cortex-t31-band fixture (synthetic)

Synthesised on 2026-10-21 19:55 CEST (+02:00) for cortex T3.1 (threshold calibration, slot 207); no host data.
Addresses are documentation ranges (192.0.2.0/24, RFC 5737); names are made up.

It sits in the band between the old and the calibrated thresholds, so it fires only because of T3.1
(docs/cortex/calibration.md):

| input | when | before T3.1 | after T3.1 |
|---|---|---|---|
| 25 `Invalid user` lines (`ssh.service`), one every 8 s | 18:56:40 .. 18:59:52 +02:00 | quiet (warn at 30) | `sshd-invalid-user` warn (warn at 20), clears once the lines leave the 300 s window |
| `galion-desk` resident 300 MiB, 3 snapshots | 19:00:00, 19:00:30, 19:01:00 +02:00 | quiet (memory-heavy above 512 MiB) | `memory-heavy-galion-desk` info (above 256 MiB) |

Replay: `NERVE_SSHD=1 openmind-nerve replay tests/fixtures/cortex-t31-band --verdict` (corpus entry
`fixture-cortex-t31-band`).
