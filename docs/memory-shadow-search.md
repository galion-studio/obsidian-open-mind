<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Memory shadow search

- DESK-ID: openmind.MEM-SEARCH
- Written: 2026-10-20T10:29:57+02:00
- Repo: /opt/galion/openmind
- Crate under test: crates/openmind-memory (not edited)
- This was not a switch.

## Not a switch

This was not a switch. The live listener on 127.0.0.1:8420 was not restarted. Its pid stayed 1374875. Its binary stayed /opt/galion/rust/bin/openmind-memory. Its start time stayed Sun Oct 18 20:14:40 2026. No unit was changed. systemctl was not used. Nothing was committed or pushed. git checkout was not used. No POST was sent to port 8420. The candidate never bound port 8420. No route was added.

## Selected provider

docs/memory-api.md names memory_tencentdb (v1) as the selected Hermes provider. That provider calls POST /search/memories and POST /search/conversations. Both handlers already exist in crates/openmind-memory. The v2 search routes are not what that provider calls. No route was added. No fixture handler was added. No test was added.

## Candidate probe

One process, then it was killed.

- Binary: /tmp/om-memsearch/debug/openmind-memory
- Args: --serve --addr 127.0.0.1:0 --fixture crates/openmind-memory/tests/fixtures/notes.json
- Bound: 127.0.0.1:35941
- Child pid: 3144291
- Stdout: loaded 2 fixture rows, listening 127.0.0.1:35941, stopped
- Port 35941 is not 8420. The child is gone. That port is no longer listening.

One request, and only one:

- URL: http://127.0.0.1:35941/search/memories
- Method: POST
- Body fields sent: query, limit
- The phrase is the checked-in fixture already named in docs/memory-shadow.md. The response body was not copied into this file.
- Status: 200 (HTTP/1.1 200 OK)
- Content-Type: application/json
- Content-Length: 172

Root keys, in response order:

- results
- strategy
- total

No nested keys. The body was a JSON object. Values were not stored.

## Documented live client shape

Not read from port 8420. No POST was sent there.

The selected provider client method search_memories posts to /search/memories and returns that JSON object unchanged. The tool memory_tencentdb_memory_search then serializes that object. The documented object is MemorySearchResponse in the package docs/memory-api.md names (@tencentdb-agent-memory/memory-tencentdb 1.0.3). The type is in src/gateway/types.ts. handleSearchMemories in src/gateway/server.ts fills it.

Keys, in that type field order:

- results
- total
- strategy

The HTTP field results is a string (the gateway assigns result.text). It is not the inner item list (id, content, type, priority, scene_name, score, created_at, updated_at) and it is not the optional inner message field. Those names are not keys of the JSON the client forwards.

POST /search/conversations was not requested. Its documented HTTP keys, ConversationSearchResponse, are results and total. They were not taken from a socket.

## Keys

Only on the candidate: none.

Only in the documented live client shape: none.

On both: results, strategy, total.

Same three names. Wire order on the candidate was results, strategy, total. The documented literal order is results, total, strategy. This was not a switch.

## Offline tests

- Copy of the lock before cargo: /tmp/om-lock-MEMSEARCH-before
- Lock sha256 before and after: 2098030715ceb1363634d495e96d5d9231ce46e9c45e3faafcbfc4da4f1fefea
- Cargo did not rewrite Cargo.lock. No restore was required. No git checkout.
- Command: CARGO_TARGET_DIR=/tmp/om-memsearch PATH=/root/.cargo/bin:$PATH nice -n 19 cargo test -j 1 -p openmind-memory --offline
- Exit: 0
- Compile of the test profile finished in 0.76s. Integration tests: 6 passed, 0 failed, 0.19s.
- Names: refuses_public_and_live_port, search, put, put_then_search_temp_file, http_put_and_search_on_ephemeral_port, http_selected_provider_fixture_routes.
- Lib, bin, and doc tests: 0 tests each, ok.

## Left in place

- Live listener not restarted.
- crates/openmind-memory not edited.
- Only this doc was written in the repo.
- This was not a switch.
