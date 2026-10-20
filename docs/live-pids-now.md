<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Live pids now

- DESK-ID: openmind.LIVE-PIDS-NOW
- Stamp: 2026-10-20T09:58:29+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: for each asked pid, one read of `/proc/<pid>/cmdline` and one `readlink` of `/proc/<pid>/exe`. For port 8420 and port 8080, one `ss -ltnp` each, then one `readlink` of the listener `/proc/<pid>/exe`. No process was killed. `systemctl` was not used. `cargo` was not run.

The stamp offset is numeric. It is not a trailing UTC designator.

## Records

Each row is one asked pid. `exists` is yes when `/proc/<pid>` is a directory at the read. `exe` is the `readlink` text of `/proc/<pid>/exe`. `cmdline` is `/proc/<pid>/cmdline` with NUL bytes shown as spaces. No process was killed.

| pid | exists | exe | cmdline |
| --- | --- | --- | --- |
| 1373612 | yes | /opt/galion/openmind/dist/bin-ws/galion-one | /opt/galion/openmind/dist/bin-ws/galion-one |
| 231 | yes | /opt/galion/bin/galion-desk | /opt/galion/bin/galion-desk |
| 1359406 | yes | /opt/galion/rust/bin/link-watch | /opt/galion/rust/bin/link-watch |
| 1196273 | yes | /opt/galion/tuner/bin/tuner | /opt/galion/tuner/bin/tuner |

All four asked pids exist. None was killed.

## Listeners

One `ss -ltnp` per port. The listener is named by pid and by one `readlink` of `/proc/<pid>/exe`. No process was killed.

| port | pid | exe |
| --- | --- | --- |
| 8420 | 1374875 | /opt/galion/rust/bin/openmind-memory |
| 8080 | 2501884 | /opt/galion/tooling/node/bin/node |

Port 8420 is LISTEN on 127.0.0.1:8420. Port 8080 is LISTEN on 127.0.0.1:8080. Each port had one LISTEN row.

## Not claimed

No process was killed. Nothing was committed. Nothing was pushed. `systemctl` was not used. `cargo` was not run.
