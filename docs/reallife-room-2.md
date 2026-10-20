<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real life room 2

- TICKET: openmind.REAL-ROOM-2
- DESK-ID: openmind.REAL-ROOM-2
- Stamp: 2026-10-20T10:39:17+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- HEAD: c417b6c5bfc3f0d9e47119a20a7c9cb5301c35b7
- Binary: /tmp/om-realroom2/debug/room
- RESULT: PASS
- Exit code: 0
- FILES: docs/reallife-room-2.md
- Child: pid 3152378 started, then exited. None kept.

The stamp offset is numeric. It is not a trailing UTC designator.

## Build

Source was not edited. Working directory: /opt/galion/openmind.

`PATH=/root/.cargo/bin:$PATH CARGO_TARGET_DIR=/tmp/om-realroom2 nice -n 19 cargo build -j 1 -p room --offline`

Finished dev profile in 0.48s. Build exit 0. Started 2026-10-20T10:37:10+02:00. Ended 2026-10-20T10:37:11+02:00.

Cargo.lock was copied to /tmp/om-lock-REALROOM2-before before cargo. After the build the repo lock and that snapshot compared equal. sha256 2098030715ceb1363634d495e96d5d9231ce46e9c45e3faafcbfc4da4f1fefea. Cargo did not rewrite the lock. The snapshot was not copied back. git checkout was not used.

The new binary was not copied onto dist/bin or any live path.

| Item | Value |
| --- | --- |
| Build exit code | 0 |
| Profile | dev |
| Cargo lock rewritten | no |
| New binary | /tmp/om-realroom2/debug/room |
| New binary bytes | 5545088 |
| New binary inode | 661283 |
| dist/bin/room bytes | 344304 |
| dist/bin/room inode | 497615 |

## Run

The new binary was started once, from /tmp/om-realroom2/debug/room, with no arguments. `--listen` was not passed. `ROOM_BIND` was unset. The cap was `timeout --signal=TERM 3s`. The cap was not hit. The process exited on its own.

Host `ss` for sport 4020 was empty before the start (2026-10-20T10:38:01+02:00), while pid 3152378 was alive, and after it exited. During that alive sample the child fd table had no socket. The new binary did not listen on the host. Port 4020 was not bound. RESULT is PASS.

| Item | Value |
| --- | --- |
| flags | none |
| ROOM_BIND | unset |
| before listen 4020 | no |
| during listen 4020 | no |
| after listen 4020 | no |
| stdout bytes | 103750 |
| stderr bytes | 0 |
| wait status | 0 |
| cap hit | no |
| parent pid | 3152376 |
| child pid | 3152378 |

Stdout is the page printed when room does not listen. It was not copied into this file. The page holds file notes and has non-ASCII bytes, so the body stays out of this ASCII note. Stderr was empty.

Parent pid 3152376 was the timeout wrapper. Child pid 3152378 was /tmp/om-realroom2/debug/room. Both are gone. No leftover child. No other process was signaled.

## Not done

Nothing was committed. Nothing was pushed. systemctl was not used. Port 4020 was not bound on the host. The live dist/bin/room file was not replaced.
