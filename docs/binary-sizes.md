<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Binary sizes

- DESK-ID: openmind.SIZE-2
- Written: 2026-10-19T18:43:53+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: read `/proc/<pid>/exe` with `readlink`, then byte size with `stat` when that file exists. Dist paths were measured the same way. Nothing was copied, installed, or restarted. `cargo` was not run. `systemctl` was not used.

## Live processes

Each row is one named pid. The exe column is the `readlink` text of `/proc/<pid>/exe`. Bytes are the size of that file when it exists on disk.

| Name | pid | /proc/pid/exe | Bytes | File |
| --- | --- | --- | --- | --- |
| galion-one | 1373612 | /opt/galion/openmind/dist/bin-ws/galion-one | 828640 | exists |
| galion-desk | 231 | /opt/galion/bin/galion-desk | 4735088 | exists |
| link-watch | 1359406 | /opt/galion/rust/bin/link-watch | 508248 | exists |
| tuner | 1196273 | /opt/galion/tuner/bin/tuner | 508680 | exists |

## dist/bin

Paths are under the repo. Absent means the path is not on disk. No file was created to fill a gap.

| Path | Bytes | Status |
| --- | --- | --- |
| /opt/galion/openmind/dist/bin/galion-one | 616600 | present |
| /opt/galion/openmind/dist/bin/galion-desk | 403056 | present |
| /opt/galion/openmind/dist/bin/link-watch | 394392 | present |
| /opt/galion/openmind/dist/bin/tuner | n/a | absent |

## Note

The live galion-one exe is not `dist/bin/galion-one`. Those two paths are different files, and their byte sizes differ. `dist/bin/tuner` is absent. The live tuner file is the path in the live table.
