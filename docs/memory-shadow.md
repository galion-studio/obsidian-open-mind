<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Memory shadow (not run)

- DESK-ID: openmind.B60
- Written: 2026-10-19T17:12:10+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Candidate: `crates/openmind-memory` (B59, fixture store and search)
- Live: memory-gateway on `127.0.0.1:8420` (not dialed by this ticket)

## Status

The switch is staged and not done.

This file is only the procedure a later shadow would follow. It was not executed. No request was sent to the live memory-gateway. No server was started. `crates/openmind-memory` was not edited. No unit was added under `deploy/`. Port 8420 stays with the live gateway, and Hermes still calls that gateway. The candidate binary refuses to bind `127.0.0.1:8420`. A cutover needs a later owner approval. This ticket does not grant it.

## What a future shadow would compare

One fixture corpus. The same HTTP request bodies. Two listeners. A field projection, not a raw byte diff.

Do this only after an owner says to run it. Until then, stop at this document.

Routes, the Hermes subset the candidate already implements:

| Method | Path | Pass |
| --- | --- | --- |
| GET | `/health` | read-only, both listeners |
| POST | `/search/memories` | read-only, both listeners |
| POST | `/search/conversations` | read-only, both listeners |
| POST | `/v2/atomic/search` | read-only, both listeners |
| POST | `/v2/conversation/search` | read-only, both listeners |
| POST | `/capture` | candidate only |
| POST | `/v2/conversation/add` | candidate only |

Any other path is out of scope and is 404 on the candidate, except the five MEM-2 fixture paths in the next section. Those handlers exist on the candidate only. This ticket does not run them and does not send them to the live gateway. Do not add embeddings, an extractor, or pyramid.

## MEM-2 fixture handlers

- DESK-ID: openmind.SHADOW-2
- Written: 2026-10-19T18:11:22+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Crate commit already present: `c3831fd`. This ticket does not edit `crates/openmind-memory`.

MEM-2 now has fixture handlers for `POST /recall`, `POST /session/end`, `POST /v2/offload/query-mmd`, `POST /v2/offload/ingest`, and `POST /v2/offload/read-ref`. The shadow is still not run.

No request was sent to the live memory-gateway on `127.0.0.1:8420`. No server was started. These five routes are candidate fixtures only. A cutover still needs a later owner approval. This ticket does not grant it.

| Method | Path | Pass |
| --- | --- | --- |
| POST | `/recall` | candidate only, not run |
| POST | `/session/end` | candidate only, not run |
| POST | `/v2/offload/query-mmd` | candidate only, not run |
| POST | `/v2/offload/ingest` | candidate only, not run |
| POST | `/v2/offload/read-ref` | candidate only, not run |

Shapes below are read from the crate. They are not a transcript of a call.

- `POST /recall` needs `query` and `session_key`. Status 200 returns `context`, `strategy` `fts`, `memory_count`, `code` 0, `message` `ok`, `retryable` false. `user_id` is optional and is not copied. Embeddings stay off. A blank field is 400.
- `POST /session/end` needs `session_key` and returns `flushed` true. The stored flush time is Europe/Zagreb RFC 3339 with `+01:00` or `+02:00`. Never `Z`. Never `+00:00`.
- Offload routes use the same v2 gate as the other v2 fixtures: `Authorization: Bearer local` and `x-tdai-service-id: default`. Presence only. No key file. No model call. No dial to port 8420.
- `POST /v2/offload/query-mmd` with no canvas returns `data.mmds` empty and `data.currentMmd` null. After a pair ingest, the canvas file name is `canvas.mmd`.
- `POST /v2/offload/ingest` with tool pairs stores one markdown ref per pair and returns `data.node_ids`. A prompt with no pairs does not invent a canvas. Do not send this write to port 8420.
- `POST /v2/offload/read-ref` returns content only when `result_ref` is owned by that `session_id`. Another session, or a missing ref, is 404. `full` must be a boolean when present.

Do not replay these bodies against the live gateway. Writes would mix fixture text into Hermes memory.

Listeners, when that later run is approved:

- Live side: the gateway that is already on `127.0.0.1:8420`. Read a response. Do not restart it. Do not change its environment. Do not open its env file or any key file.
- Candidate side: `openmind-memory --serve --addr 127.0.0.1:0 --fixture crates/openmind-memory/tests/fixtures/notes.json`. Port 0 is ephemeral. Port 8420 is refused. Stop the process when the diff is written. Without `--serve` the binary opens no socket. This ticket does not start it.

Corpus: the checked-in fixture only. Do not read live memory rows into the candidate. Do not copy a live session, bearer token, or service id into the fixture or into this file. v2 examples use the placeholder `Authorization: Bearer local` and `x-tdai-service-id: default` already used by `crates/openmind-memory/tests/put_search.rs`. Those placeholders are not a live credential. If a later run must pass the live v2 gate, the operator supplies the real header outside this repo and does not write it down.

Writes are not sent to port 8420. `POST /capture` and `POST /v2/conversation/add` change the store. Replaying them against the live gateway would mix fixture text into Hermes memory. That is not part of this procedure. The write shapes below are the candidate responses, taken from the crate, so a later diff has a target. They are not transcripts of a live call. The candidate may accept those writes only because its index is in-memory (or a temp file) and dies with the process.

## Fixture corpus

File: `crates/openmind-memory/tests/fixtures/notes.json`.

```json
[
  {
    "kind": "memory",
    "content": "The desk keeps notes in Zagreb time.",
    "mem_type": "episodic",
    "scene": "desk",
    "session_key": "fixture"
  },
  {
    "kind": "conversation",
    "role": "user",
    "content": "Where is the spare memory port?",
    "session_key": "fixture-session"
  }
]
```

Load that file on the candidate only, with `--fixture`. Do not POST it to port 8420.

A future read-only pass therefore expects a hit on the candidate and a miss on the live gateway, unless those phrases already happen to exist there. A live miss is `live_miss_expected`. Record the live status and `total` (or item count). Do not require the live `total` to equal the candidate `total`. Do not treat a live miss as a failed gateway.

## Projection

Ignore these on both sides. They are volatile or local to the implementation:

- `request_id` (candidate values look like `om-N`)
- `uptime`
- `id`, `accepted_ids`
- `created_at`, `updated_at`, `timestamp`, and any clock inside a v1 results string
- `score` (candidate score is the fraction of ASCII query tokens found in the content; the live score is not that)
- `embeddingService` (candidate is false; the live value may be true)
- `implementation` (candidate is `openmind-memory`)
- `strategy` (candidate is `fts`)
- `stores.vectorStore` and `services.*` (candidate shape flags; `vectorStore: true` does not mean an embedding ran)

Compare these:

- HTTP status.
- Health: both bodies have `status` equal to `ok`.
- v1 search: `total` is a number and `results` is a string. On the candidate, a hit string contains the fixture phrase. An empty candidate memory search is exactly `No matching memories found.` An empty candidate conversation search is exactly `No matching conversation messages found.`
- v1 error: status 400 and an `error` string. A blank `query` says `Missing required field: query`.
- v2 success: status 200 and `code` 0. On the candidate, `data.items[].content` or `data.messages[].content` contains the fixture phrase, and `role` matches when the route returns one.
- v2 auth failure on the candidate: status 401 and `code` 401 when `Authorization: Bearer <non-empty>` or `x-tdai-service-id` is missing. The candidate checks presence only. It does not open a key file.
- Candidate clocks, wherever they appear, are Europe/Zagreb RFC 3339 with `+01:00` or `+02:00`. Never `Z`. Never `+00:00`.

Known candidate gaps. Record them. They are not a reason to flip the switch:

- No embedding model. `embeddingService` stays false. Search tokens are ASCII letters and digits; other characters are separators. A query that yields no ASCII token returns an empty hit list and status 200, not 400.
- The HTTP strategy label is `fts`. FTS5 is the lookup. The rank used in the response is token overlap, then id. Do not require score equality with the live gateway.
- v1 `/search/memories` and `/search/conversations` return a rendered string in `results`, not a JSON list. Memory lines include the type, a priority of 0, an optional scene, and a score. Conversation lines include the role, the session, and a clock.
- `limit` missing or 0 means 5. `limit` above 100 is clamped to 100.
- `query` longer than 2048 bytes is 400. Content longer than 8192 bytes is 400. `session_key` longer than 512 bytes is 400.
- `POST /capture` answers `l0_recorded: 2` and `scheduler_notified: false`. The index also stores a third memory row (user text, a newline, assistant text) that the count does not include. A missing `user_content`, `assistant_content`, or `session_key` is 400.
- `POST /v2/conversation/add` stores a conversation row and a memory row per message. `accepted_ids` lists the conversation ids only (prefix `c`). Memory ids use prefix `m`. `data.total_count` is the number of messages, not the number of rows. Incoming message clocks are ignored. `session_id` is required. `messages` must hold 1 to 100 items. Roles on a conversation row must be `user`, `assistant`, or `system`.
- v1 memory search reads `type` and `scene`. v1 conversation search reads `session_key`. v2 atomic search reads `type`. v2 conversation search reads `session_id` (not `session_key`). Scene match is a case-insensitive substring of the stored scene.

## Fixture examples

These are the request bodies and the candidate shapes. They are not a log of a run. Nothing here was sent to port 8420.

### 1. Health (both listeners, later)

`GET /health`

Candidate shape:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "uptime": 0,
  "stores": { "vectorStore": true, "embeddingService": false },
  "services": {
    "timerScanner": null,
    "pipelineWorker": null,
    "stateBackend": "none"
  },
  "embeddingService": false,
  "implementation": "openmind-memory"
}
```

Future diff: both status codes 200, both `status` equal `ok`. Do not diff `uptime`, `implementation`, or `embeddingService`.

### 2. Memory search, fixture hit (both listeners, later)

`POST /search/memories`

```json
{"query":"Zagreb notes","limit":5}
```

Candidate: status 200, `strategy` `fts`, `total` 1. `results` contains `episodic`, `[scene: desk]`, and `The desk keeps notes in Zagreb time.`

Live: record status and `total`. A miss is expected. Do not require `total` 1.

### 3. Scene filter, candidate miss

`POST /search/memories`

```json
{"query":"Zagreb notes","scene":"kitchen","limit":5}
```

Candidate: status 200, `total` 0, `results` exactly `No matching memories found.`

The same query with `"scene":"desk"` stays a hit (`total` 1) on the candidate.

### 4. Conversation search (both listeners, later)

`POST /search/conversations`

```json
{"query":"spare port","session_key":"fixture-session","limit":5}
```

Candidate: status 200, `total` 1. `results` contains `[user]`, `Session: fixture-session`, and `Where is the spare memory port?`

Same body with `"session_key":"other-session"`: candidate status 200, `total` 0, `results` exactly `No matching conversation messages found.`

Live: record status and `total`. A miss is expected.

### 5. Blank query

`POST /search/conversations`

```json
{}
```

Candidate: status 400, `error` contains `query`.

### 6. Capture shape (candidate only)

Do not send this to port 8420.

`POST /capture`

```json
{"user_content":"Store the cedar key.","assistant_content":"Cedar key stored.","session_key":"shadow-fixture"}
```

Candidate: status 200.

```json
{"l0_recorded":2,"scheduler_notified":false}
```

Then `POST /search/memories` with `{"query":"cedar","limit":5}` on that same candidate process: status 200, `total` 1, `results` contains `cedar` and `episodic`.

`POST /search/conversations` with `{"query":"cedar","session_key":"shadow-fixture","limit":5}`: status 200, `total` 2, `results` contains `Store the cedar key.` and `Cedar key stored.`

### 7. v2 shape (candidate only for the write)

Do not send the add to port 8420. A later read of `/v2/atomic/search` or `/v2/conversation/search` against the live gateway needs a header supplied outside this file. Do not paste a real bearer token here.

Headers on the candidate:

```
Authorization: Bearer local
x-tdai-service-id: default
```

`POST /v2/conversation/add`

```json
{"session_id":"shadow-fixture","messages":[{"role":"user","content":"Remember the linen map."}]}
```

Candidate: status 200, `code` 0, `data.total_count` 1, `data.accepted_ids` length 1. The accepted id starts with `c`.

`POST /v2/atomic/search`

```json
{"query":"linen","limit":5}
```

Candidate: status 200, `code` 0, `data.items` length 1, `content` contains `linen`, `type` is `episodic`, `created_at` and `updated_at` use a Zagreb offset.

`POST /v2/conversation/search`

```json
{"query":"linen","session_id":"shadow-fixture"}
```

Candidate: status 200, `code` 0, `data.messages` length 1, `role` is `user`, `content` contains `linen`, `timestamp` uses a Zagreb offset.

Same atomic search with no `Authorization` header: status 401, `code` 401.

## Diff a later run would keep

Not written by this ticket. A later run would append a table with one row per case: name, live status, candidate status, projection (equal / live_miss_expected / differ), and a short note. No response body that holds a secret. No bearer token. Fixture phrases only. A `differ` row blocks any cutover. A `live_miss_expected` row does not.

## Switch

Staged and not done.

- The candidate crate is already in the tree. This ticket does not change it.
- The live memory-gateway remains the process Hermes uses.
- There is no memory unit under `deploy/`. Nothing was installed or enabled.
- The candidate will not listen on 8420.
- This document is not a cutover.
