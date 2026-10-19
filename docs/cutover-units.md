<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Cutover units

- DESK-ID: openmind.AUDIT-UNITS
- Stamp: 2026-10-19T22:29:21+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: read `deploy/boot/rendered/*.service`, read `/proc/<pid>/exe` and `/proc/<pid>/cgroup`, compare each rendered file to `/etc/systemd/system/<name>.service` with `cmp`. No process was killed. `systemctl` was not used. No binary was copied. No unit was installed.

The stamp offset is numeric. It is not a trailing UTC designator.

## Rendered units, installed or not

Installed is yes only when the rendered file is byte-identical to the host file of the same name. A same-named host file can exist and still be a different unit.

| Unit | Rendered ExecStart | Installed |
| --- | --- | --- |
| galion-one.service | /opt/galion/openmind/dist/bin/galion-one | no |
| galion-desk.service | /opt/galion/openmind/dist/bin/galion-desk | no |
| link-watch.service | /opt/galion/openmind/dist/bin/link-watch | no |
| tuner.service | /opt/galion/openmind/dist/bin/tuner | no |

Host files of those four names are present under `/etc/systemd/system/`. `cmp` says each one differs from the rendered file. The rendered marker text is not in the host unit directory. No rendered `ExecStart` under `dist/bin/` appears in a host unit. Nothing from `deploy/boot/rendered/` was copied.

## Live processes

Each asked pid was read at the stamp. `gone` means `/proc/<pid>` was absent. None was killed.

| Name | pid | status | exe | Same path as rendered ExecStart |
| --- | --- | --- | --- | --- |
| galion-one | 1373612 | not gone | /opt/galion/openmind/dist/bin-ws/galion-one | no |
| galion-desk | 231 | not gone | /opt/galion/bin/galion-desk | no |
| link-watch | 1359406 | not gone | /opt/galion/rust/bin/link-watch | no |
| tuner | 1196273 | not gone | /opt/galion/tuner/bin/tuner | no |

Cgroup text is from `/proc/<pid>/cgroup`, not from `systemctl`.

| pid | cgroup |
| --- | --- |
| 1373612 | /system.slice/galion-one.service |
| 231 | /system.slice/galion-desk.service |
| 1359406 | /system.slice/link-watch-rs.service |
| 1196273 | /system.slice/tuner.service |

The live galion-one exe is `/opt/galion/openmind/dist/bin-ws/galion-one`. The live galion-desk exe is `/opt/galion/bin/galion-desk`. The live link-watch exe is `/opt/galion/rust/bin/link-watch`. The live tuner exe is `/opt/galion/tuner/bin/tuner`. Those four paths are the ones that were already running. They are not the rendered `ExecStart` paths.

Host `galion-one.service` ExecStart is the live galion-one path. Host `galion-desk.service` ExecStart is the live galion-desk path. Host `tuner.service` ExecStart is the live tuner path. The live link-watch path is the ExecStart of host `link-watch-rs.service`, not of rendered `link-watch.service`, and not of host `link-watch.service` (that host file starts a script under `/opt/galion/galion-light/scripts/`).

## Not done

No unit was installed. No binary was copied. `systemctl` was not used. No process was killed. Nothing was committed. Nothing was pushed.
