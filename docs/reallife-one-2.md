<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life one 2

- DESK-ID: openmind.REAL-ONE-2
- Stamp: 2026-10-20T09:59:58+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- HEAD: 76bba82fa908039a64f7d3790cb16f275fdb7945
- Binary: /tmp/om-realone2/debug/galion-one
- Result: PASS
- Child: 3120533, started, then killed
- pid 1373612: left running. Not signaled.

The stamp offset is numeric. It is not a trailing UTC designator.

## Why a start was allowed

Source at that HEAD reads all four controls, and it has a no-warm switch.

| Role | Control | Forced value |
| --- | --- | --- |
| listen bind | `ONE_BIND` | 127.0.0.1:18091 |
| mouth listen | `ONE_MOUTH_BIND` | 127.0.0.1:18002 |
| page and local state root | `GALION_HOME` | /tmp/om-realone2-home |
| Hermes state directory | `ONE_HERMES_DIR` | /tmp/om-realone2-hermes |
| warm loop | `ONE_NO_WARM` | 1 |

`/etc/openmind/openmind.toml` is absent. A missing file keeps the built-in defaults, then `ONE_BIND`, `ONE_MOUTH_BIND`, `GALION_HOME`, and `ONE_HERMES_DIR` win. `ONE_NO_WARM=1` skips `keep_warm`. Nothing in this start was left unforced, so the new binary was started.

## Build

Working directory: /opt/galion/openmind

Cargo.lock was copied to /tmp/om-lock-REALONE2-before before cargo. The build was:

`PATH=/root/.cargo/bin:$PATH CARGO_TARGET_DIR=/tmp/om-realone2 nice -n 19 cargo build -j 1 -p galion-one --offline`

Finished dev profile in 11.30s. Exit 0. The binary is /tmp/om-realone2/debug/galion-one. Cargo.lock matched the snapshot after the build, so the snapshot was not copied back. git checkout was not used.

The new binary was not copied onto dist/bin, dist/bin-ws, or /opt/galion/bin. Those files were not replaced.

## Run

Started once, loopback only, from /tmp/om-realone2/debug/galion-one. The five variables above were set in a clean environment. Log lines were `one: listening` and `voice: listening`.

Listen sockets for pid 3120533:

- 127.0.0.1:18091
- 127.0.0.1:18002

That pid did not listen on 8091 or 8002. pid 1373612 kept 127.0.0.1:8091 and 127.0.0.1:8002 for the whole run.

The sampler still opens short client connections, including to 127.0.0.1:8091 and the mouth client port 8002. Those are not listen sockets. The mouth listen address and the mouth client port are separate. `ONE_MOUTH_BIND` moved only the listen address.

State dirs /tmp/om-realone2-home and /tmp/om-realone2-hermes were created empty and were still empty after the stop.

## Health curls

One GET each. No retries. Loopback only. Taken while pid 3120533 was listening.

| URL | HTTP status |
| --- | --- |
| http://127.0.0.1:18091/health | 200 |
| http://127.0.0.1:8091/health | 200 |

Each body was 3 bytes: the word ok and a newline. The 8091 listener was pid 1373612.

## Stop

Only pid 3120533 was signaled. Its process group was its own and was not the group of pid 1373612. After the stop, 18091 and 18002 were not listening. pid 1373612 was still listening on 8091 and 8002.

Nothing was committed. Nothing was pushed. No unit was started, stopped, or restarted.
