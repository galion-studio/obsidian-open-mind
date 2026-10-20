<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real life room

- DESK-ID: openmind.REAL-ROOM
- Stamp: 2026-10-20T10:00:48+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Binary: /opt/galion/openmind/dist/bin/room
- Result: RAN
- Exit code: 143
- Child: stopped after the check. None kept.

The stamp offset is numeric. It is not a trailing UTC designator.

## Run

`unshare -n true` exited 0, so the binary was started in a new network namespace. The command was `unshare -n -- /opt/galion/openmind/dist/bin/room`. No flags were passed. `--listen` was not passed. `ROOM_BIND` was unset.

The process did not exit on its own. It stayed up and listened on 127.0.0.1:4020 inside that namespace only. `nsenter` on the child network namespace showed LISTEN 127.0.0.1:4020. Host `ss` showed no listener on 4020 before the start, while the child was up, and after it was stopped. Port 4020 was not bound on the host. This is not FAIL.

The child was then stopped with SIGTERM. `wait` reported 143. No room process was left. No other process was signaled.

| Item | Value |
| --- | --- |
| unshare -n | works, exit 0 |
| flags | none |
| stdout bytes | 0 |
| stderr bytes | 18 |
| stderr text | room: 689 objects |
| namespace listen | 127.0.0.1:4020 |
| host listen 4020 | no |
| wait status | 143 |

Stderr is that one line, printed after the bind. Stdout was empty. The object list was not copied. No secrets were copied.

## Binary versus source

The dist binary is older than the current room source. It was not rebuilt.

| Path | When | Bytes |
| --- | --- | --- |
| dist/bin/room | 2026-10-18T19:43:17+02:00 | 344304 |
| crates/room/src/main.rs | 2026-10-19T19:53:18+02:00 | not the binary |

Commit 25ffe16 recorded this binary. Its `main` always binds `127.0.0.1:4020` (or `ROOM_BIND` when that env is set). Commit 84cb4df (2026-10-19T18:43:27+02:00) later made `main` listen only when the first argument is `--listen`. That source is not what this file ran.

## Not done

Nothing was committed. Nothing was pushed. cargo was not run. systemctl was not used. Python was not used. The binary was not rebuilt. Port 4020 was not bound on the host.
