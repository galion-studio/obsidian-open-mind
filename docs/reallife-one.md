<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real-life one

- DESK-ID: openmind.REAL-ONE
- Stamp: 2026-10-19T22:31:39+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Binary: /opt/galion/openmind/dist/bin/galion-one
- Result: NOT-STARTED
- Child: none kept
- pid 1373612: left running. Not signaled.

The stamp offset is numeric. It is not a trailing UTC designator.

## What sets bind, mouth, and state

Env names below are in the binary (`strings`) and in `crates/galion-one/src/main.rs` at git `4d4c488`. That commit is the tree from before the binary mtime `2026-10-18 17:43:17 +0000`.

| Role | Control | If unset |
| --- | --- | --- |
| listen bind | `ONE_BIND` via `bind_addr` | `127.0.0.1:8091` |
| mouth listen | `ONE_MOUTH_BIND` via `bind_addr` | `127.0.0.1:8002` |
| page and local state root | `GALION_HOME` via `home` / `p` | `/workspace/galion-light` |
| mouth client | `GALION_MOUTH` | `127.0.0.1:8002` |

Hermes state is not an env var in this binary. `strings` finds `ONE_HERMES_DIR` zero times. Fixed paths:

- `/workspace/hermes/channels/one.jsonl`
- `/workspace/hermes/channels/galion-one.md`
- `/workspace/hermes/modes.json`
- `create_dir_all` of `/workspace/hermes/channels` when a turn is remembered

`GALION_HOME` only prefixes paths built by `p(rel)`. It does not move the Hermes tree.

Current `main.rs` (mtime `2026-10-19 17:53:18 +0000`) does add `ONE_HERMES_DIR` and reads `OPENMIND_CONFIG`. That source is newer than this binary. The binary has no `OPENMIND_CONFIG` string and was not rebuilt. `/etc/openmind/openmind.toml` is absent. A newer binary could have been pointed at `/tmp/om-real-one` with `GALION_HOME` and `ONE_HERMES_DIR`. This binary cannot.

## One default invocation, then stop

This binary has no help flag. `main` binds immediately. One invocation was made with `--help` before that was known, so the defaults applied. It printed `one: bind failed: Address already in use (os error 98)` and exited. It did not listen. It did not take 8091 or 8002 from pid 1373612.

`sample`, `boot_worker`, `serve_tts`, and `keep_warm` are spawned before that bind. They end when the process exits. A later process list showed only pid 1373612 for `galion-one`, and no tts worker child.

No second start was made. The spare-port start was not done.

## Why the spare-port start was not done

Required: bind `127.0.0.1:18091`, mouth `127.0.0.1:18002`, state dirs under `/tmp/om-real-one`.

`ONE_BIND` and `ONE_MOUTH_BIND` can force those two listen addresses. `GALION_HOME` can place the page root under `/tmp/om-real-one`. Hermes state cannot be placed there. The rule was: if bind, mouth, and state dirs cannot all be forced, do not start. That start was not done. No child was killed.

A real start would also keep `sample()` running. That loop GETs `127.0.0.1:8091/health` and the mouth address (default port 8002) about once a second. `boot_worker` treats `/workspace/galion-light/tts/worker.py` as the gate. That file is present, and the gate does not follow `GALION_HOME`. `start_worker` then spawns `/workspace/tts/venv/bin/python` on `p("tts/worker.py")`.

## Health curls

One GET each. No retries. Loopback only. Taken while 18091 and 18002 were not listening.

| URL | HTTP status |
| --- | --- |
| http://127.0.0.1:18091/health | 000 |
| http://127.0.0.1:8091/health | 200 |

The 18091 curl failed to connect (curl exit 7). No body was saved. The 8091 curl returned 200 and 3 bytes: the word ok and a newline. That listener is pid 1373612 (`/opt/galion/openmind/dist/bin-ws/galion-one`). It was already listening on `127.0.0.1:8091` and `127.0.0.1:8002` before the curls and still was after them. `18091` and `18002` were not listening.

Nothing was committed. Nothing was pushed. No unit was started, stopped, or restarted.
