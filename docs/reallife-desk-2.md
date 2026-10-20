<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real life desk 2

- DESK-ID: openmind.REAL-DESK-2
- Stamp: 2026-10-20T10:00:17+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Binary: /tmp/om-realdesk2/debug/galion-desk
- Result: PASS
- Child pid 3120697 was started, then stopped. pid 231 was not signaled.

The stamp offset is numeric. It is not a trailing UTC designator.

## Build

Source was not edited. Build command: `PATH=/root/.cargo/bin:$PATH CARGO_TARGET_DIR=/tmp/om-realdesk2 nice -n 19 cargo build -j 1 -p galion-desk --offline`. It finished the dev profile in 10.45s. The new binary was run from `/tmp/om-realdesk2/debug/galion-desk` only. It was not copied onto `dist/bin` or `/opt/galion/bin`.

Before cargo, `Cargo.lock` was copied to `/tmp/om-lock-REALDESK2-before`. After the build the file matched that copy. Cargo did not rewrite it, so the copy was not put back. `git checkout` was not used.

## Why it was started

Current `galion-desk` source can move every state dir it reads:

- `DESK_STATE_DIR` (default `/opt/galion/galion-light/state`)
- `DESK_ONE_DIR` (default `/workspace/galion-light/one`)
- `DESK_HERMES_DIR` (default `/opt/galion/hermes`)

`DESK_*` wins over a config file. `/etc/openmind` is absent, so no config file supplied another root. `DESK_HERMES_BIN` names a program, not a state dir. It was left unset. `/health` does not run that program.

The child env was only:

- `DESK_BIND=127.0.0.1:18646`
- `DESK_STATE_DIR=/tmp/om-realdesk2/state`
- `DESK_ONE_DIR=/tmp/om-realdesk2/one`
- `DESK_HERMES_DIR=/tmp/om-realdesk2/hermes`

Plus `PATH`. The log line was `galion-desk 127.0.0.1:18646`. The listen socket was only `127.0.0.1:18646`. It was not on 8646 or 8091. The three state dirs gained no files.

## Reads

One GET each. No retry. Loopback only. Bodies were not copied into this file.

| URL | HTTP status | content-type | bytes |
| --- | --- | --- | --- |
| http://127.0.0.1:18646/health | 200 | application/json | 11 |
| http://127.0.0.1:8646/health | 200 | application/json | 11 |

## After

pid 3120697 is gone. Port 18646 is not listening. pid 231 is still `/opt/galion/bin/galion-desk` on `127.0.0.1:8646` (started Mon Oct 12 05:01:35 2026, same as before this run). Port 8091 is still pid 1373612. No unit was changed. Nothing was committed. Nothing was pushed.
