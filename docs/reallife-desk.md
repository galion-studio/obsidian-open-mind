<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real-life desk

- DESK-ID: openmind.REAL-DESK
- Stamp: 2026-10-19T22:30:31+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Binary: /opt/galion/openmind/dist/bin/galion-desk
- Result: FAIL
- The spare process was not started.

Current source can point the listen address at `127.0.0.1:18646` with `DESK_BIND`. It can point state at `/tmp/om-real-desk` with `DESK_STATE_DIR`, `DESK_ONE_DIR`, and `DESK_HERMES_DIR`. Those names win over a config file.

The dist binary is older than that source. Its mtime is 2026-10-18 17:43:17 +0000. `DESK_STATE_DIR` arrived in source later (commit ae72938, 2026-10-19). Binary strings include `DESK_BIND` and `DESK_HERMES_BIN`. They do not include `DESK_STATE_DIR`, `DESK_ONE_DIR`, `DESK_HERMES_DIR`, `DESK_OLLAMA`, or `OPENMIND_CONFIG`.

Live path strings are compiled in:

- `/opt/galion/galion-light/state`
- `/workspace/galion-light/one`
- `/opt/galion/hermes`

`DESK_BIND` alone can avoid port 8646. No setting in this binary can move state off those live paths. The spare listener was not started. Port 18646 was not opened. Nothing was bound on 8646 or 8091 by this check. pid 231 was not killed.

Before the path check, two runs of the same binary with no `DESK_BIND` hit the default listen address and exited at once. The message was address already in use. Both processes are gone. They did not stay listening. pid 231 was not touched.

## Reads

| URL | HTTP status | Note |
| --- | --- | --- |
| http://127.0.0.1:18646/health | none | spare process not started, no curl |
| http://127.0.0.1:8646/health | 200 | one GET, 11 bytes, fixed health JSON, body not copied |

The live listener stayed pid 231, `/opt/galion/bin/galion-desk`, on `127.0.0.1:8646`. Port 8091 stayed the other live listener. No child was left behind. No unit was changed. Nothing was committed or pushed.

The stamp offset is numeric. It is not a trailing UTC designator.
