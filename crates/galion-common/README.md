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
| `http` | strict request parser, bounded one-request-per-connection server, one-shot client | tuner and galion-origin accept loops, galion-boot `post_json` |

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
