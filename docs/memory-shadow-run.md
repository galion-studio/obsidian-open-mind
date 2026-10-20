<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Memory shadow run

- DESK-ID: openmind.MEM-SHADOW-RUN
- Written: 2026-10-20T10:02:29+02:00
- Repo: /opt/galion/openmind
- Crate under test: `crates/openmind-memory` (not edited)
- This was not a switch.

## Not a switch

This was not a switch. The live listener on `127.0.0.1:8420` was not restarted. Its pid stayed `1374875`. Its binary stayed `/opt/galion/rust/bin/openmind-memory`. No unit was changed. `systemctl` was not used. Nothing was committed or pushed. `git checkout` was not used. No POST was sent to port 8420. The candidate never bound port 8420.

## Live health

One GET, and only one:

- URL: `http://127.0.0.1:8420/health`
- Method: GET
- Status: `200` (`HTTP/1.1 200 OK`)
- Body was not copied into this file (278 bytes on the wire)

Root keys, in response order:

- status
- version
- uptime
- stores
- services
- implementation
- implementation_version
- read_only
- fts

Leaf paths kept by a filter that skips false and null:

- fts
- implementation
- implementation_version
- services.stateBackend
- status
- stores.vectorStore
- uptime
- version

`read_only` is a root key and was not in that leaf list, so its value is false or null. The value was not recorded. Nested keys under `stores` or `services` that are false or null, if any, were not listed. A second GET was not made.

## Candidate health

`/tmp/om-memshadow/debug/openmind-memory` was started with `--serve --addr 127.0.0.1:0`. Each process bound `127.0.0.1` and an ephemeral port, then was stopped. Ports used: `34227`, `35973`, `41355`. None was `8420`. All three processes are gone.

Keys below are from the last listener (`127.0.0.1:41355`), status `200` (`HTTP/1.1 200 OK`). Values were not stored.

- embeddingService
- implementation
- services
- services.pipelineWorker
- services.stateBackend
- services.timerScanner
- status
- stores
- stores.embeddingService
- stores.vectorStore
- uptime
- version

The first candidate GET (`127.0.0.1:34227`) was also status `200`. Its body was 236 bytes and was not copied here. The middle start only checked the bind.

## Key difference, not a cutover

Shared root keys: `status`, `version`, `uptime`, `stores`, `services`, `implementation`.

Only on the live root: `implementation_version`, `read_only`, `fts`.

Only on the candidate root: `embeddingService`.

Shared nested paths observed on the live side: `stores.vectorStore`, `services.stateBackend`.

Seen on the candidate and not in the live leaf list above: `stores.embeddingService`, `services.timerScanner`, `services.pipelineWorker`. Those three may be absent on the live side, or present with a false or null value. This file does not decide that. No second live GET was sent.

This was not a switch. Port 8420 stayed with pid `1374875`.

## Offline tests

- Copy of the lock before the run: `/tmp/om-lock-MEMSHADOW-before`
- Lock sha256 before and after: `2098030715ceb1363634d495e96d5d9231ce46e9c45e3faafcbfc4da4f1fefea`
- Cargo did not rewrite `Cargo.lock`. No restore was required. No `git checkout`.
- Command: `CARGO_TARGET_DIR=/tmp/om-memshadow PATH=/root/.cargo/bin:$PATH nice -n 19 cargo test -j 1 -p openmind-memory --offline`
- Exit: 0
- Compile finished in 23.76s. Integration tests: 6 passed, 0 failed, 0.20s.
- Names: `refuses_public_and_live_port`, `search`, `put`, `put_then_search_temp_file`, `http_put_and_search_on_ephemeral_port`, `http_selected_provider_fixture_routes`.
- Lib, bin, and doc tests: 0 tests each, ok.

## Left in place

- Live listener not restarted.
- `crates/openmind-memory` not edited.
- Only this doc was written in the repo.
- This was not a switch.
