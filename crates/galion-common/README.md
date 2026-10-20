<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# galion-common

Shared std-only helpers that used to be copied crate to crate. No asm, no
SIMD, `#![forbid(unsafe_code)]`, zero dependencies by default.

| module | what | replaces |
|---|---|---|
| `sha256` | FIPS 180-4 SHA-256, streaming (`Sha256`) and one-shot (`digest`, `hex`); NIST vectors in `tests/nist.rs` | hermes-rs `mcp::sha256`, openmind-memory `hash::sha256_hex`, galion-one `sha256.rs` |
| `hex` | lower-case encode, strict decode | small local copies |
| `base64` | RFC 4648 `encode` (std, padded), `encode_url` (URL-safe, no pad), strict `decode` | galion-one `b64`/`b64url`, one-talk `b64_encode` |
| `json` | JSON string escaping; `string_ascii` = CPython `ensure_ascii` | hand-built JSON in several crates |
| `jsonout` (feature `serde-json`) | CPython `json.dumps` formatter over `serde_json::Value` | desk-hermes and link-watch `jsonout.rs` |
| `time` | Europe/Zagreb RFC 3339 (`now_rfc3339`, `from_unix`, `label`, `require_rfc3339`) without chrono; same output as `zagreb-time` | alternative to zagreb-time for dependency-free crates |
| `http` | strict request parser (opt-in chunked bodies), bounded one-request-per-connection server, one-shot client | tuner, galion-origin, hermes-rs, openmind-memory, galion-desk and room accept loops; galion-boot, openmind-memory `post_json`, galion-desk, link-watch, openmind-survival and one-talk clients |

## HTTP server in one paragraph

`http::serve(listener, Limits, Arc<handler>)`: one thread per connection up
to `max_connections` (the rest get a fast 503), one request per connection,
`connection: close` always. The request head must arrive within
`head_timeout` in total (anti-slowloris), the body within `body_timeout`.
Heads over `max_head_bytes` or `max_headers` get 431, bodies over
`max_body_bytes` get 413 before any body byte is read, `Transfer-Encoding`
gets 501, malformed lines get 400, other versions 505. Handlers return a
structured `Response` or `Response::raw(bytes)` for byte-exact ports.

## Behaviour changes in migrated crates

Valid requests get byte-identical replies (unit and golden tests unchanged,
plus a live comparison with the VPS tuner in `docs/real-tests/common.md`).
Only invalid or abusive traffic is treated differently:

- tuner, galion-origin: malformed requests now get the shared 400/431/413/
  501/505 replies instead of being routed; the head deadline is total, not
  per read; connections over the cap get 503 (origin used to drop them
  silently, tuner had no cap at all: now 64).
- galion-boot `post_json`: reads the reply up to 64 KiB (or `Content-Length`)
  instead of a single 256-byte read; the status and error texts are the same.

## Additions for the second migration (`swarm/common-migrate-2`)

- `Limits::accept_chunked` (default `false`): accept exactly one
  `Transfer-Encoding: chunked` on requests. Bad chunk framing is
  `Error::BadChunk` (400); the decoded body still obeys `max_body_bytes`
  (413), trailers obey `max_head_bytes`, a chunk-size line is at most 1024
  bytes and 15 hex digits. `Content-Length` plus `Transfer-Encoding` is
  still 400, any other coding still 501. `ChunkDecoder` is the incremental
  decoder (also used by the client).
- `serve_connections(listener, limits, busy_bytes, Arc<ConnHandler>)`: the
  same bounded accept loop with custom "too busy" bytes and a per-connection
  callback, for servers that stream (hermes SSE) or that keep their own
  error envelopes. `handle_connection_with` takes a custom error mapper;
  `drain` and `error_reply` are public.
- `send_prefix(addr, req, timeout, max_bytes)`: read at most `max_bytes`
  of the reply and stop quietly (`complete == false`), for probes that only
  look at the status line or the first bytes.
- `ClientError::BadStatus(String)` now carries the first reply line
  (Display is still `bad status line`).

Migrated in this round: hermes-rs, openmind-memory (server and
`tools::post_json`), galion-desk (server, Ollama and hermes clients), room,
link-watch, openmind-survival probes, one-talk, and the base64 copy in
`edge/openmind-node` (now `galion_common::base64::encode_url`, checked
against the old local copy for 600 lengths).

### Behaviour changes (valid requests unchanged; see docs/real-tests/common-migrate-2.md)

- **hermes-rs**: head must arrive within 10 s total and the body within
  60 s (was 30 s per read). Strict-parser refusals use the OpenAI error
  envelope: header too large / too many headers 413, body 413, bad framing
  400, other transfer codings 501, HTTP versions other than 1.0/1.1 505,
  slow head 408. An empty connection is closed silently (was a 400 "empty
  request"). Sends `100 Continue` when the client waits on `Expect`.
  Over-cap connections still get the old 503 JSON. Connection threads are
  no longer named `hermes-conn`. Non-streamed replies go out in one write.
- **openmind-memory**: unparseable `Content-Length` is 400 (was read as 0);
  other HTTP versions 400; total head (10 s) and body (30 s) deadlines;
  slow or truncated heads are closed silently, as before for truncation.
  `post_json` decodes chunked replies, maps a non-HTTP reply to
  `Err("bad response")` (was `Ok((0, ""))`) and a bare status line to
  `Ok((503, ""))` (was an error); total 60 s deadline and 16 MiB cap.
- **galion-desk, room**: previously no connection cap and a single
  `read()`; now 32 connections (503 beyond), the whole body is read
  (`Content-Length` up to 64 KiB, else 413), strict 400/431/501/505, total
  deadlines (desk 3 s + 3 s, room 8 s + 8 s). desk `HEAD` still gets a body,
  to stay byte-identical. desk's Ollama/hermes clients: total timeouts
  (1.1 s / 92 s) instead of per-operation ones, size caps 1 MiB / 16 MiB.
- **link-watch**: replies must have a valid `HTTP/x.y NNN` status line;
  chunked is decoded by the shared client; incomplete replies, replies over
  8 MiB and replies cut by the deadline are probe failures.
- **openmind-survival**: probes read with `send_prefix`; chunked bodies are
  decoded, LF-only heads give their body, and a complete reply on a socket
  kept open returns at `Content-Length` instead of at the timeout. Error
  texts (`bad status line: ...`, `timeout`, `connect ...`) are unchanged.
- **one-talk**: chunked replies are decoded (a chunked `hear` now parses);
  a non-HTTP reply is `UnexpectedEof "no http header"` (warm used to return
  `Ok(0)`), a bare status line is a valid status (warm returns 503 instead
  of an error); 8 MB reply cap.

### Limits from the environment (`Limits::with_env`)

`Limits::with_env(prefix)` overrides a limit from
`<PREFIX>_HTTP_<KEY>`. The keys are `MAX_HEAD_BYTES`, `MAX_HEADERS`,
`MAX_BODY_BYTES`, `HEAD_TIMEOUT_MS`, `BODY_TIMEOUT_MS`, `WRITE_TIMEOUT_MS`
and `MAX_CONNECTIONS`. Values must be whole numbers above 0; anything else is
ignored, with a warning on stderr. An unset variable keeps the crate default.
`with_vars` takes the same keys from any lookup, so a config file can use it
too.

| crate | prefix | head bytes / headers | body bytes | head / body / write timeout | connections |
|---|---|---|---|---|---|
| hermes-rs | `HERMES_RS` | 64 KiB / 256 | 8 MiB | 10 s / 60 s / 30 s | 64 |
| openmind-memory | `OPENMIND_MEMORY` | 64 KiB / 256 | 1 MiB (`MEMORY_MAX_BODY_BYTES`; the HTTP_ variable wins) | 10 s / 30 s / 30 s | 32 (`OPENMIND_MEMORY_MAX_CONNS`) |
| galion-desk | `DESK` | 8 KiB / 64 | 64 KiB | 3 s / 3 s / 10 s | 32 |
| room | `ROOM` | 4 KiB / 64 | 64 KiB | 8 s / 8 s / 10 s | 32 |

Each strict limit can be approved or reverted separately with these
variables; see `docs/approvals/common-migrate-2-strict.md`. Parser strictness
is not configurable; that document explains why.
