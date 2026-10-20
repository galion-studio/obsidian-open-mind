<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Live pids

- DESK-ID: openmind.LIVE-PIDS
- Stamp: 2026-10-19T19:54:12+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: read `/proc/<pid>/cmdline` and `/proc/<pid>/exe` once each. No process was killed. `systemctl` was not used. `cargo` was not run.

The stamp offset is numeric. It is not a trailing UTC designator.

## Records

Each row is one asked pid. `exists` is yes when `/proc/<pid>` is a directory at the stamp. `exe` is the `readlink` text of `/proc/<pid>/exe`. `cmdline` is `/proc/<pid>/cmdline` with NUL bytes shown as spaces. No process was killed.

| pid | exists | exe | cmdline |
| --- | --- | --- | --- |
| 1373612 | yes | /opt/galion/openmind/dist/bin-ws/galion-one | /opt/galion/openmind/dist/bin-ws/galion-one |
| 231 | yes | /opt/galion/bin/galion-desk | /opt/galion/bin/galion-desk |
| 1359406 | yes | /opt/galion/rust/bin/link-watch | /opt/galion/rust/bin/link-watch |
| 1196273 | yes | /opt/galion/tuner/bin/tuner | /opt/galion/tuner/bin/tuner |

All four asked pids still exist. None was killed.

## Not claimed

No process was killed. Nothing was committed. Nothing was pushed. `systemctl` was not used. `cargo` was not run.

