<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Dist switch (staged checklist, not executed)

- DESK-ID: openmind.B51
- Written: 2026-10-19T17:12:40+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: galion-one, galion-desk, link-watch
- This file is a checklist. It does not switch, copy, install, enable, or restart anything.

## Rollback

The live process stays until Slavik approves.

`deploy/backup/` does not exist. `deploy/rollback.sh` is dry-run by default. After a later approved apply it would restore the newest `deploy/backup/<unit>.<stamp>.service`. It does not restart a process. Until Slavik approves, do not run it with `--apply`. The three live processes below stay on their current executables.

## Stage 0 — hold

- Do not copy any binary into `/opt/galion/bin` or `/usr`.
- Do not replace `/opt/galion/rust/bin/link-watch`.
- Do not install `deploy/*.service` over `/etc/systemd/system`.
- Do not treat `deploy/boot/rendered/` as this switch. Those files are renderer output from another ticket and are not installed.
- Do not build the workspace. The sizes below are files already on disk. No service was started to measure them.

## Stage 1 — paths and sizes (read-only snapshot)

Sizes are bytes from `stat`. mtimes are `stat` `%y` truncated to the second, zone `+0000`. Process paths are `/proc/<pid>/exe` at this snapshot. Pids are not a lock.

### Live (leave running)

| Name | Running executable | Unit file on disk | Size | mtime (UTC) | pid | cwd |
| --- | --- | --- | --- | --- | --- | --- |
| galion-one | `/opt/galion/openmind/dist/bin-ws/galion-one` | `/etc/systemd/system/galion-one.service` ExecStart is that same path | 828640 | 2026-10-18 20:13:56 +0000 | 1373612 | `/opt/galion/galion-light` |
| galion-desk | `/opt/galion/bin/galion-desk` | `/etc/systemd/system/galion-desk.service` ExecStart is that same path. Has `[Install]`. | 4735088 | 2026-10-10 09:02:43 +0000 | 231 | `/` |
| link-watch | `/opt/galion/rust/bin/link-watch` | `/etc/systemd/system/link-watch.service` ExecStart is `/usr/bin/python3 /opt/galion/galion-light/scripts/link-watch.py` (script 3232 bytes). The process and the unit text do not match. Has `[Install]`. | 508248 | 2026-10-18 19:57:47 +0000 | 1359406 | `/` |

On disk and not running: `/opt/galion/galion-light/one/one` size 1060288. Do not start it.

Health, one request only, not repeated: `GET http://127.0.0.1:8091/health` returned `HTTP/1.1 200 OK`, `Content-Type: text/plain`, `Content-Length: 3`, `Cache-Control: no-store`, body `ok`. That port is the live galion-one above (`dist/bin-ws`), not `dist/bin/galion-one`.

### Openmind dist builds (the switch target, not started)

`dist/bin/SHA256SUMS` matches the four hashes below. `room` is in that sums file (size 344304) and is out of scope.

| File | Size | mtime (UTC) | sha256 |
| --- | --- | --- | --- |
| `dist/bin/galion-one` | 616600 | 2026-10-18 17:43:17 +0000 | `3357e2648e57ab0fe2d85e27226a9b50002c08897b64f6c06c86ae900bf6be9f` |
| `dist/bin/galion-desk` | 403056 | 2026-10-18 17:43:17 +0000 | `889737d0337e7a3a087348566be29f0c2c57ee79db6eece61ea5463ff72adb5f` |
| `dist/bin/link-watch` | 394392 | 2026-10-18 17:43:18 +0000 | `6b9d04de6bcaaae86a5d3a411eb29793fb242ef9e71e4ccfbe7dee69218db92b` |
| `dist/bin/desk-hermes` | 1427792 | 2026-10-18 17:43:18 +0000 | `89bc45cd42db326c0ce1ff0cadd3be21649b641fa7ada232c9af3b5aa4889762` |

`desk-hermes` is not its own service. The staged desk unit names it via `DESK_HERMES_BIN`.

Not the dist target:

| File | Size | sha256 | Note |
| --- | --- | --- | --- |
| `dist/bin-ws/galion-one` | 828640 | `090c15d0fbb32d372edd87ea3cb154a5be40cbadbb3ea3d8bb4a7a5880e50191` | live galion-one; not listed in `dist/bin/SHA256SUMS` |
| `target/release/galion-one` | 642728 | `fe02f281555a8c36bb9709e90770630a90fb15f9f61f8491a96a57af86bbf340` | different bytes from dist |
| `target/release/galion-desk` | 403056 | same hash as `dist/bin/galion-desk` | matches dist |
| `target/release/link-watch` | 394936 | `23dcb44e76972d4622966ca199d281460d2259138efec222e7d3bd9a94f74023` | different bytes from dist |
| `target/release/desk-hermes` | 1427792 | `d784dcec85853cc3d2c948c779f472aed893caa5f9d0b888a258706a498afbe4` | same size as dist, different bytes |

## Stage 2 — staged unit text (repo only)

These files already exist under `deploy/`. This ticket did not copy them onto the host.

| Staged file | ExecStart | Environment this file sets | Install section |
| --- | --- | --- | --- |
| `deploy/galion-one.service` | `/opt/galion/openmind/dist/bin/galion-one` | `ONE_BIND=127.0.0.1:8091`, `ONE_MOUTH_BIND=127.0.0.1:8002` | none |
| `deploy/galion-desk.service` | `/opt/galion/openmind/dist/bin/galion-desk` | `DESK_BIND=127.0.0.1:8646`, `DESK_HERMES_BIN=/opt/galion/openmind/dist/bin/desk-hermes` | none |
| `deploy/link-watch.service` | `/opt/galion/openmind/dist/bin/link-watch` | none | none |

`deploy/switch.sh <unit>` prints a dry run unless `--apply` is passed. With `--apply` it would back up the live unit into `deploy/backup/`, install the staged unit, and reload the service manager. It does not start, stop, or restart the process. This ticket did not run `deploy/switch.sh`.

## Stage 3 — gates before any later apply

Do not clear these gates by editing units or binaries in this ticket.

1. Slavik approves the switch in writing. Until that approval the live process stays.
2. galion-one. The process answering `/health` is `dist/bin-ws/galion-one` (828640 bytes), not `dist/bin/galion-one` (616600 bytes). The installed unit sets a working directory, log paths, resource limits, and extra environment keys (a device store, hermes paths, a key file). Those values are not repeated here. The staged unit sets only the two bind addresses. Installing it as written changes more than the executable. Decide that before any apply.
3. galion-desk. A later switch is a new ExecStart path. It is not a copy over `/opt/galion/bin/galion-desk`. The staged unit adds bind and hermes-bin settings the live unit does not have, and it drops `[Install]`. `dist/bin/desk-hermes` is not byte-identical to `target/release/desk-hermes`.
4. link-watch. Three different things: the running file `/opt/galion/rust/bin/link-watch` (508248 bytes), the unit text (an interpreter plus `galion-light/scripts/link-watch.py`), and `dist/bin/link-watch` (394392 bytes). The dist file still has a France host baked into its strings. `target/release/link-watch` is a later hash; its strings include `unconfigured` and do not show that host. Do not point a unit at `dist/bin/link-watch` until Slavik accepts the baked host, or a later ticket replaces the dist file. Do not rebuild here.
5. `deploy/boot/rendered/*.service` names the same dist executables but different restart timing. Do not install that set as a substitute for `deploy/*.service`.

## Stage 4 — order, only after the gates

Not done. Suggested order once Slavik has approved:

1. link-watch only after gate 4 is closed.
2. galion-desk next. Point the unit at `dist/bin/galion-desk`. Leave `/opt/galion/bin/galion-desk` in place as the rollback binary.
3. galion-one last, and only after the staged unit's environment is reconciled with the live unit (no secrets in git). Then change ExecStart from `dist/bin-ws/galion-one` to `dist/bin/galion-one`. `/health` was `ok` before any switch. Check it again only after an approved restart.

Each later step is a read-only diff, then `deploy/switch.sh <unit>` with no `--apply`, then Slavik says apply, then `--apply`, then a separate operator restart. This checklist does not authorize that restart. Between unit install and a restart, the process already running keeps its original executable.

## Stage 5 — rollback line

The live process stays until Slavik approves.

After an approved apply, roll the unit file back with `deploy/rollback.sh <unit>` (dry run first). That restores the backup and does not restart. The process that was already running keeps its original executable until an operator restarts it, and that restart still needs Slavik's approval.
