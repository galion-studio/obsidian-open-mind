<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Staged, not live

- DESK-ID: openmind.STAGED-NOW
- Written: 2026-10-19T18:13:12+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Directory: `deploy/boot/rendered/`
- This file is a read-only record. It does not install, enable, start, stop, or kill anything. `systemctl` was not used.

## Rendered files

These four files are the whole of `deploy/boot/rendered/`. Each one is staged, not installed. They are not the unit files in `/etc/systemd/system/`. A byte compare against `/etc/systemd/system/<name>.service` differs for every name. The rendered files have no install section.

| File | Bytes | Status |
| --- | --- | --- |
| deploy/boot/rendered/galion-desk.service | 580 | staged, not installed |
| deploy/boot/rendered/galion-one.service | 584 | staged, not installed |
| deploy/boot/rendered/link-watch.service | 485 | staged, not installed |
| deploy/boot/rendered/tuner.service | 514 | staged, not installed |

### deploy/boot/rendered/galion-desk.service

staged, not installed. Its `ExecStart` text names `/opt/galion/openmind/dist/bin/galion-desk`. That path is not the live binary.

### deploy/boot/rendered/galion-one.service

staged, not installed. Its `ExecStart` text names `/opt/galion/openmind/dist/bin/galion-one`. That path is not the live binary.

### deploy/boot/rendered/link-watch.service

staged, not installed. Its `ExecStart` text names `/opt/galion/openmind/dist/bin/link-watch`. That path is not the live binary.

### deploy/boot/rendered/tuner.service

staged, not installed. Its `ExecStart` text names `/opt/galion/openmind/dist/bin/tuner`. That path is not on disk. The live binary is `/opt/galion/tuner/bin/tuner`.

## Live binaries

Confirmed with `ps` and `/proc/<pid>/exe` at the stamp above. Each running executable is the live path below. None of them is a `dist/bin` copy. No `dist/bin` path was the `exe` of any process. No process was killed.

| Name | pid | ps args | live executable | not this |
| --- | --- | --- | --- | --- |
| galion-one | 1373612 | /opt/galion/openmind/dist/bin-ws/galion-one | /opt/galion/openmind/dist/bin-ws/galion-one | /opt/galion/openmind/dist/bin/galion-one |
| galion-desk | 231 | /opt/galion/bin/galion-desk | /opt/galion/bin/galion-desk | /opt/galion/openmind/dist/bin/galion-desk |
| link-watch | 1359406 | /opt/galion/rust/bin/link-watch | /opt/galion/rust/bin/link-watch | /opt/galion/openmind/dist/bin/link-watch |
| tuner | 1196273 | /opt/galion/tuner/bin/tuner | /opt/galion/tuner/bin/tuner | /opt/galion/openmind/dist/bin/tuner (absent) |

`galion-one` runs from `dist/bin-ws`, not from `dist/bin`. Those are different files.

## Hold

Leave `deploy/boot/rendered/*.service` staged. Do not install them. Do not replace the four live binaries with `dist/bin` copies.
