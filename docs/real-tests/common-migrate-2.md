<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: common-migrate-2 (wave 2 slot 28, ports 19280-19289)

All times are Europe/Zagreb (CEST, +02:00).

## 1. Migrated vs pre-migration binaries on the box

Run: `GALION_REAL=1 nice -n 10 cargo test -p galion-common --release --test
real_migrate -- --ignored --nocapture` (Rust:
`crates/galion-common/tests/real_migrate.rs`, helpers in
`tests/realsupport/mod.rs`, hammer in `tests/real_hammer.rs`). It replaces
`scripts/real/common-migrate-2.sh`, which is deleted. Run from 2026-10-20
21:26:18 to 21:30:10 +02:00 on the final code of this branch; 231.9 s, exit 0.
`GALION_REAL_OLD_REV` (default 26d98b0) and `GALION_REAL_PORT_BASE`
(default 19280) can be overridden. The test:
- builds release binaries of the migrated tree, and the same packages from
  26d98b0 (swarm/common, before this migration) in a temporary worktree;
- runs old and new **hermes-rs** (`--mock`, 19280/19281), **openmind-memory**
  (19282/19283), **galion-desk** (19284/19285, Ollama pointed at a recording
  fixture on 19288) and **room** (`--listen`, 19286/19287) from empty scratch
  state;
- sends the same valid requests to both sides of each pair and compares the
  raw replies byte for byte. Only when they differ is a masked comparison
  done. The mask covers timestamps, pids, durations, uptimes, generated ids,
  host load/RAM figures in desk's board, and the content-length line those
  change;
- runs the migrated clients (old and new builds) against the fixture and
  compares the exact request bytes and their stdout: guardian-tick /
  openmind-survival `check_http`, link-watch (plain and chunked replies),
  desk → Ollama (`wake`/`rest`) and the openmind-memory parity tool
  (`post_json`);
- then does abuse in the style of `scripts/real/common.sh`: hermes
  chunked-body abuse (`real_chunked_abuse`), then `real_hammer` against all
  8 servers. The hammer sends:
  - 2000 sequential and 16x200 concurrent GETs;
  - 416 malformed requests;
  - a 100 KB header, 1000 headers, a 10 GB Content-Length and a real 5 MB
    body;
  - 80 slowloris connections, with 60 legit GETs during the wave;
- samples memory and threads, and checks every process is still alive;
- phase 4 (new): restarts each migrated server with
  `<PREFIX>_HTTP_HEAD_TIMEOUT_MS=1000` and `<PREFIX>_HTTP_MAX_CONNECTIONS=2`,
  and checks that a stalled head is cut at 1 s instead of the default and
  that a 3rd connection gets 503.

Every started process is a `Proc` guard that is killed on drop, also on
panic. The old-revision git worktree and the scratch dir are removed on drop.
The nested release builds go to `$CARGO_TARGET_DIR/real-new` and
`real-old`, so they never wait on the outer cargo lock.

Result: **87 byte-identical, 11 identical after masking, 0 different**.
**All 8 servers alive** at the end; exit 0.

**Same as the shell-script run (20:59:15 to 21:02:30, 88/10/0).** The
98 probe names are identical. The only change is `desk POST /desk rest`,
which was byte-identical in one run and identical after masking in the
other: desk's board contains host load and RAM figures. Abuse outcome counts
are identical for all 8 servers, apart from timings and one case:
old-hermes slowloris was `503:16, survived:64` before and is
`503:4, survived:64, write cut:12` now. That is the old code's 503 being
reset before the client read it. It is the TCP RST issue under
"Open issue" in the approval packet, and it is not a migrated-server change.

The memory parity tool reports `diff: 2` for old-server vs new-server, from
**both** the old and the new parity build, with identical stdout. Those two
diffs are stored timestamps from the two servers' clocks, not protocol
differences. That comparison is informational; the check is that the two
parity builds print the same thing.

Abuse, migrated vs old, in short:

- **Slowloris**: old hermes/memory let 64/32 drippers hold slots for the
  whole 37 s drip, and 0 of 60 legit GETs got through. Migrated: total head
  deadline (408 or quiet close at 10 s), and legit GETs get 200 again from
  10.3 s.
- **desk/room, old**: did a single `read()` and answered at once, so
  slowloris never held a slot. Migrated, they hold up to 32 slow heads for
  3 s (desk) or 8 s (room). During that window, legit GETs over the cap get
  503: 11/60 for desk, 31/60 for room. See the approval items.
- **Oversized input, old desk/room**: routed it (200/404/405). Migrated:
  431/413.
- **Malformed input, old desk/room**: routed it as well (404/405/200).
  Migrated: 400/501/505.
- **Peak RSS** is unchanged within noise (hermes 12.0 MB both, the others
  2.9 to 4.9 MB).

### Phase 4: limits from the environment (migrated binaries only)

| server | stalled head, default | with `HEAD_TIMEOUT_MS=1000` | 3rd connection with `MAX_CONNECTIONS=2` |
|---|---|---|---|
| hermes-rs (`HERMES_RS_`) | cut at 10.2 s | 1.0 s, 408 | 503 |
| openmind-memory (`OPENMIND_MEMORY_`) | 10.0 s | 1.0 s, quiet close (as before) | 503 |
| galion-desk (`DESK_`) | 3.1 s | 1.0 s, 408 | 503 |
| room (`ROOM_`) | 8.0 s | 1.0 s, 408 | 503 |

The first run of the port (started 21:17:44 +02:00) failed here for desk.
The cause was a race in the test, not in desk: the 1 s stalled-head probe's
handler thread had not yet freed its slot when the cap probe began. The
test now waits 300 ms and prints the state of the held connections.

### Output (verbatim)

```
== 2026-10-20T21:26:18+02:00 build migrated tree and 26d98b055bd980045e718e3ab63a0d26888f0256
== 2026-10-20T21:26:29+02:00 valid traffic, old vs new
== hermes: old 127.0.0.1:19280 vs new 127.0.0.1:19281
  same   hermes GET /health (261 bytes)
  same   hermes GET /v1/health HTTP/1.0 (Galion One) (261 bytes)
  masked hermes GET /health/detailed (old 489 / new 489 bytes)
  same   hermes GET /v1/models (297 bytes)
  same   hermes GET /v1/capabilities (377 bytes)
  same   hermes GET /v1/models no key (401) (270 bytes)
  same   hermes GET /v1/models wrong key (401) (270 bytes)
  masked hermes POST /v1/chat/completions (old 477 / new 477 bytes)
  masked hermes POST /v1/chat/completions HTTP/1.0 (Galion One) (old 477 / new 477 bytes)
  masked hermes POST /v1/chat/completions stream (SSE) (old 1195 / new 1195 bytes)
  masked hermes POST /v1/chat/completions chunked upload (old 477 / new 477 bytes)
  same   hermes POST /v1/chat/completions bad JSON (239 bytes)
  masked hermes GET /api/sessions (old 1459 / new 1455 bytes)
  same   hermes GET /api/jobs (149 bytes)
  same   hermes GET /api/spend (291 bytes)
  same   hermes GET /p/default/health (261 bytes)
  same   hermes GET /p/other/health (404) (263 bytes)
  same   hermes GET /nope (404) (253 bytes)
  same   hermes GET /v1/runs/missing (259 bytes)
  same   hermes HEAD /health (270 bytes)
  same   hermes LF-only GET /health (261 bytes)
  masked hermes Expect 100-continue with body sent at once (old 477 / new 477 bytes)
== memory: old 127.0.0.1:19282 vs new 127.0.0.1:19283
  same   memory GET /health (364 bytes)
  same   memory GET /health?x=1 (364 bytes)
  same   memory GET absolute-form (364 bytes)
  same   memory lower-case method (364 bytes)
  same   memory POST /capture (134 bytes)
  same   memory POST /recall (192 bytes)
  same   memory POST /search/memories (158 bytes)
  masked memory POST /search/conversations (old 254 / new 254 bytes)
  same   memory POST /session/end (106 bytes)
  same   memory POST /recall bad JSON (128 bytes)
  same   memory POST /recall chunked (refused, 400) (164 bytes)
  masked memory POST /v2/pipeline/status (old 229 / new 229 bytes)
  same   memory GET /nope (404) (129 bytes)
  same   memory GET /recall (wrong method) (131 bytes)
== desk: old 127.0.0.1:19284 vs new 127.0.0.1:19285
  same   desk GET /health (101 bytes)
  same   desk GET /health HTTP/1.0 (101 bytes)
  same   desk GET /desk (824 bytes)
  same   desk GET /desk -> fixture request 1 (66 bytes)
  same   desk GET /desk?x=1 (824 bytes)
  same   desk GET /desk?x=1 -> fixture request 1 (66 bytes)
  same   desk POST /desk note (865 bytes)
  same   desk POST /desk note -> fixture request 1 (66 bytes)
  same   desk GET /desk after note (865 bytes)
  same   desk GET /desk after note -> fixture request 1 (66 bytes)
  same   desk POST /health (101 bytes)
  same   desk GET /nope (404) (118 bytes)
  same   desk HEAD /health (101 bytes)
  same   desk LF-only GET /desk (865 bytes)
  same   desk LF-only GET /desk -> fixture request 1 (66 bytes)
== room: old 127.0.0.1:19286 vs new 127.0.0.1:19287
  same   room GET /health (111 bytes)
  same   room GET / (129 bytes)
  same   room GET /index.html (129 bytes)
  same   room GET /api/objects (3105 bytes)
  same   room GET /api/read held (113 bytes)
  same   room GET /api/read traversal (113 bytes)
  same   room POST / (405) (111 bytes)
  same   room HEAD /health (405) (111 bytes)
  same   room GET /nope (404) (111 bytes)
  same   room GET /health HTTP/1.0 (111 bytes)
  masked desk POST /desk wake (old 864 / new 864 bytes)
  same   desk wake -> Ollama fixture request 1 (66 bytes)
  same   desk wake -> Ollama fixture request 2 (207 bytes)
  masked desk POST /desk rest (old 864 / new 864 bytes)
  same   desk rest -> Ollama fixture request 1 (66 bytes)
  same   desk rest -> Ollama fixture request 2 (66 bytes)
  same   desk rest -> Ollama fixture request 3 (192 bytes)
== guardian-tick (openmind-survival check_http)
  exit old 0 new 0
  same   guardian-tick report (868 bytes)
  same   guardian-tick stdout (72 bytes)
  same   guardian-tick request 1 (64 bytes)
  same   guardian-tick request 2 (66 bytes)
  same   guardian-tick request 3 (61 bytes)
  same   guardian-tick request 4 (56 bytes)
  same   guardian-tick request 5 (62 bytes)
== link-watch (LINK_WATCH_ONCE)
  plain: exit old 0 new 0
  same   link-watch plain link.json (218 bytes)
  same   link-watch plain request 1 (103 bytes)
  same   link-watch plain request 2 (104 bytes)
  same   link-watch plain request 3 (103 bytes)
  chunked: exit old 0 new 0
  same   link-watch chunked link.json (216 bytes)
  same   link-watch chunked request 1 (109 bytes)
  same   link-watch chunked request 2 (111 bytes)
  same   link-watch chunked request 3 (110 bytes)
  same   link-watch chunked request 4 (109 bytes)
== openmind-memory parity (post_json client)
  old parity old-server vs new-server: exit 2 {"summary":{"match":4,"diff":2}}
  new parity old-server vs new-server: exit 2 {"summary":{"match":4,"diff":2}}
  same   memory parity stdout (fixture) (1983 bytes)
  same   memory parity stdout (old vs new server) (2089 bytes)
  same   memory parity request 1 (158 bytes)
  same   memory parity request 2 (158 bytes)
  same   memory parity request 3 (160 bytes)
  same   memory parity request 4 (160 bytes)
  same   memory parity request 5 (159 bytes)
  same   memory parity request 6 (159 bytes)
  same   memory parity request 7 (161 bytes)
  same   memory parity request 8 (161 bytes)
  same   memory parity request 9 (154 bytes)
  same   memory parity request 10 (154 bytes)
  same   memory parity request 11 (156 bytes)
  same   memory parity request 12 (156 bytes)
== summary: 87 byte-identical, 11 identical after masking, 0 different
== 2026-10-20T21:26:32+02:00 abuse
before new-hermes: VmHWM:4052 VmRSS:4052 Threads:2
before new-memory: VmHWM:4040 VmRSS:4040 Threads:1
before new-desk: VmHWM:2544 VmRSS:2544 Threads:1
before new-room: VmHWM:2220 VmRSS:2220 Threads:1
before old-hermes: VmHWM:4088 VmRSS:4088 Threads:2
before old-memory: VmHWM:3976 VmRSS:3976 Threads:1
before old-desk: VmHWM:2468 VmRSS:2468 Threads:1
before old-room: VmHWM:2172 VmRSS:2172 Threads:2
new-hermes chunked size ffffffffffffffff: HTTP/1.1 400 in 0.6ms
new-hermes chunked size line 2000 bytes: HTTP/1.1 400 in 0.6ms
new-hermes chunked non-hex size: HTTP/1.1 400 in 0.2ms
new-hermes chunked negative size: HTTP/1.1 400 in 0.2ms
new-hermes chunked missing CRLF after data: HTTP/1.1 400 in 0.6ms
new-hermes chunked trailer 20 KB: HTTP/1.1 400 in 0.4ms
new-hermes chunked 9 MiB chunked body: HTTP/1.1 413 in 9.7ms
new-hermes chunked TE gzip: HTTP/1.1 501 in 0.6ms
new-hermes chunked TE chunked + CL: HTTP/1.1 400 in 0.4ms
new-hermes chunked TE twice: HTTP/1.1 501 in 0.2ms
new-hermes chunked truncated chunked: HTTP/1.1 400 in 0.1ms
new-hermes chunked slow drip: Some("dripped 4 s, client gave up") after 3.9s
new-hermes after chunked abuse: /health -> HTTP/1.1 200
old-hermes chunked size ffffffffffffffff: HTTP/1.1 413 in 0.2ms
old-hermes chunked size line 2000 bytes: HTTP/1.1 400 in 0.1ms
old-hermes chunked non-hex size: HTTP/1.1 400 in 1.3ms
old-hermes chunked negative size: HTTP/1.1 400 in 0.4ms
old-hermes chunked missing CRLF after data: HTTP/1.1 400 in 0.2ms
old-hermes chunked trailer 20 KB: HTTP/1.1 400 in 0.1ms
old-hermes chunked 9 MiB chunked body: HTTP/1.1 413 in 13.2ms
old-hermes chunked TE gzip: HTTP/1.1 400 in 0.2ms
old-hermes chunked TE chunked + CL: HTTP/1.1 400 in 0.2ms
old-hermes chunked TE twice: HTTP/1.1 400 in 0.5ms
old-hermes chunked truncated chunked: HTTP/1.1 400 in 0.2ms
old-hermes chunked slow drip: Some("dripped 4 s, client gave up") after 3.9s
old-hermes after chunked abuse: /health -> HTTP/1.1 200
new-hermes sequential GET /health: n=2000 ok=2000 p50=0.07ms p99=0.93ms max=8.85ms
new-hermes concurrent: 18361 req/s over 0.17s
new-hermes concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.31ms p99=6.12ms max=15.75ms
new-hermes malformed: 416 requests in 0.05s -> {"400": 412, "401": 1, "505": 1, "closed": 2}
new-hermes after malformed: /health -> Some(200)
new-hermes oversized 100 KB header: Some(413) in 0.6ms
new-hermes oversized 1000 headers: Some(413) in 0.1ms
new-hermes oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-hermes oversized 5 MB body: Some(401) in 19.7ms
new-hermes slowloris x80: outcomes {"HTTP/1.1 408": 64, "HTTP/1.1 503": 16}, longest held 10.1s
new-hermes legit GETs during slowloris (60 over 15s): {"Some(200)": 21, "Some(503)": 39}, first 200 at Some("10.3s")
new-hermes final /health -> Some(200)
new-memory sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.24ms max=3.35ms
new-memory concurrent: 23773 req/s over 0.13s
new-memory concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.26ms p99=5.93ms max=8.34ms
new-memory malformed: 416 requests in 0.02s -> {"400": 15, "closed": 401}
new-memory after malformed: /health -> Some(200)
new-memory oversized 100 KB header: Some(400) in 0.6ms
new-memory oversized 1000 headers: Some(400) in 0.2ms
new-memory oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-memory oversized 5 MB body: Some(413) in 1.0ms
new-memory slowloris x80: outcomes {"HTTP/1.1 503": 48, "closed": 32}, longest held 10.1s
new-memory legit GETs during slowloris (60 over 15s): {"Some(200)": 21, "Some(503)": 39}, first 200 at Some("10.3s")
new-memory final /health -> Some(200)
new-desk sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.46ms max=3.71ms
new-desk concurrent: 36479 req/s over 0.09s
new-desk concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.30ms p99=3.34ms max=7.82ms
new-desk malformed: 416 requests in 0.04s -> {"400": 411, "501": 2, "505": 1, "closed": 2}
new-desk after malformed: /health -> Some(200)
new-desk oversized 100 KB header: Some(431) in 0.1ms
new-desk oversized 1000 headers: Some(431) in 0.1ms
new-desk oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-desk oversized 5 MB body: Some(413) in 0.6ms
test real_migrate has been running for over 60 seconds
new-desk slowloris x80: outcomes {"HTTP/1.1 408": 32, "HTTP/1.1 503": 48}, longest held 3.0s
new-desk legit GETs during slowloris (60 over 15s): {"Some(200)": 49, "Some(503)": 11}, first 200 at Some("3.3s")
new-desk final /health -> Some(200)
new-room sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.25ms max=1.53ms
new-room concurrent: 25268 req/s over 0.13s
new-room concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.23ms p99=5.20ms max=7.38ms
new-room malformed: 416 requests in 0.03s -> {"400": 411, "501": 2, "505": 1, "closed": 2}
new-room after malformed: /health -> Some(200)
new-room oversized 100 KB header: Some(431) in 0.3ms
new-room oversized 1000 headers: Some(431) in 0.1ms
new-room oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-room oversized 5 MB body: Some(413) in 0.6ms
new-room slowloris x80: outcomes {"HTTP/1.1 408": 32, "HTTP/1.1 503": 48}, longest held 8.1s
new-room legit GETs during slowloris (60 over 15s): {"Some(200)": 29, "Some(503)": 31}, first 200 at Some("8.3s")
new-room final /health -> Some(200)
old-hermes sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.97ms max=6.00ms
old-hermes concurrent: 11758 req/s over 0.27s
old-hermes concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.29ms p99=9.60ms max=14.27ms
old-hermes malformed: 416 requests in 0.05s -> {"400": 405, "401": 11}
old-hermes after malformed: /health -> Some(200)
old-hermes oversized 100 KB header: Some(413) in 0.3ms
old-hermes oversized 1000 headers: Some(401) in 0.4ms
old-hermes oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
old-hermes oversized 5 MB body: Some(401) in 8.6ms
old-hermes slowloris x80: outcomes {"HTTP/1.1 503": 4, "survived whole drip": 64, "write cut": 12}, longest held 37.4s
old-hermes legit GETs during slowloris (60 over 15s): {"Some(503)": 60}, first 200 at None
old-hermes final /health -> Some(200)
old-memory sequential GET /health: n=2000 ok=2000 p50=0.07ms p99=0.39ms max=0.62ms
old-memory concurrent: 33236 req/s over 0.10s
old-memory concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.30ms p99=3.39ms max=5.40ms
old-memory malformed: 416 requests in 0.03s -> {"400": 5, "404": 11, "closed": 400}
old-memory after malformed: /health -> Some(200)
old-memory oversized 100 KB header: Some(400) in 0.4ms
old-memory oversized 1000 headers: Some(404) in 0.6ms
old-memory oversized Content-Length 10 GB (no body): Some(413) in 0.3ms
old-memory oversized 5 MB body: Some(413) in 0.3ms
old-memory slowloris x80: outcomes {"HTTP/1.1 503": 48, "survived whole drip": 32}, longest held 37.4s
old-memory legit GETs during slowloris (60 over 15s): {"Some(503)": 60}, first 200 at None
old-memory final /health -> Some(200)
old-desk sequential GET /health: n=2000 ok=2000 p50=0.09ms p99=0.61ms max=4.13ms
old-desk concurrent: 20258 req/s over 0.16s
old-desk concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.36ms p99=5.42ms max=9.12ms
old-desk malformed: 416 requests in 0.05s -> {"404": 414, "closed": 2}
old-desk after malformed: /health -> Some(200)
old-desk oversized 100 KB header: Some(404) in 0.2ms
old-desk oversized 1000 headers: Some(404) in 0.1ms
old-desk oversized Content-Length 10 GB (no body): Some(200) in 0.1ms
old-desk oversized 5 MB body: Some(200) in 0.2ms
old-desk slowloris x80: outcomes {"HTTP/1.1 404": 80}, longest held 0.0s
old-desk legit GETs during slowloris (60 over 15s): {"Some(200)": 60}, first 200 at Some("0.5s")
old-desk final /health -> Some(200)
old-room sequential GET /health: n=2000 ok=2000 p50=0.07ms p99=0.42ms max=3.77ms
old-room concurrent: 30304 req/s over 0.11s
old-room concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.31ms p99=4.70ms max=6.21ms
old-room malformed: 416 requests in 0.04s -> {"200": 7, "404": 1, "405": 408}
old-room after malformed: /health -> Some(200)
old-room oversized 100 KB header: Some(200) in 0.2ms
old-room oversized 1000 headers: Some(200) in 0.1ms
old-room oversized Content-Length 10 GB (no body): Some(405) in 0.2ms
old-room oversized 5 MB body: Some(405) in 0.2ms
old-room slowloris x80: outcomes {"HTTP/1.1 405": 80}, longest held 0.0s
old-room legit GETs during slowloris (60 over 15s): {"Some(200)": 60}, first 200 at Some("0.5s")
old-room final /health -> Some(200)
== 2026-10-20T21:29:30+02:00 done
after new-hermes: VmHWM:12228 VmRSS:10676 Threads:2
after new-memory: VmHWM:4964 VmRSS:4964 Threads:1
after new-desk: VmHWM:3316 VmRSS:3316 Threads:1
after new-room: VmHWM:3116 VmRSS:3116 Threads:1
after old-hermes: VmHWM:12068 VmRSS:10648 Threads:2
after old-memory: VmHWM:4812 VmRSS:4812 Threads:1
after old-desk: VmHWM:2956 VmRSS:2956 Threads:1
after old-room: VmHWM:2740 VmRSS:2740 Threads:1
peak threads seen by the 1 s sampler:
  new-desk 33
  new-hermes 66
  new-memory 33
  new-room 33
  old-desk 1
  old-hermes 66
  old-memory 33
  old-room 1
new-hermes: alive
new-memory: alive
new-desk: alive
new-room: alive
old-hermes: alive
old-memory: alive
old-desk: alive
old-room: alive
== 2026-10-20T21:29:31+02:00 limits from environment (migrated binaries)
new-hermes stalled head closed after 10.2s by default, 1.0s with HERMES_RS_HTTP_HEAD_TIMEOUT_MS=1000 (HTTP/1.1 408); HERMES_RS_HTTP_MAX_CONNECTIONS=2: 3rd connection -> HTTP/1.1 503 (held: ["held", "held"])
new-memory stalled head closed after 10.0s by default, 1.0s with OPENMIND_MEMORY_HTTP_HEAD_TIMEOUT_MS=1000 (closed); OPENMIND_MEMORY_HTTP_MAX_CONNECTIONS=2: 3rd connection -> HTTP/1.1 503 (held: ["held", "held"])
new-desk stalled head closed after 3.1s by default, 1.0s with DESK_HTTP_HEAD_TIMEOUT_MS=1000 (HTTP/1.1 408); DESK_HTTP_MAX_CONNECTIONS=2: 3rd connection -> HTTP/1.1 503 (held: ["held", "held"])
new-room stalled head closed after 8.0s by default, 1.0s with ROOM_HTTP_HEAD_TIMEOUT_MS=1000 (HTTP/1.1 408); ROOM_HTTP_MAX_CONNECTIONS=2: 3rd connection -> HTTP/1.1 503 (held: ["held", "held"])
.
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 231.89s
```

## 2. Live VPS, read-only GETs (status, headers, JSON shape only)

2026-10-20 20:53:45 +02:00. These were plain `GET`s over ssh, using bash
`/dev/tcp` on the VPS to 127.0.0.1:8420 (openmind-memory) and 127.0.0.1:8646
(galion-desk). No file was written on the VPS, and nothing was restarted or
posted. On the box, only the status line, header names, content-type and
connection values, and the JSON key paths (arrays collapsed to `[]`) were
printed. No body values were printed or stored.

The same probes were then run at 20:54:07 against the migrated binaries on
19283 (memory, with a test API key so auth behaves like the VPS) and 19285
(desk).

```
--- VPS 127.0.0.1:8420 GET /health
status: HTTP/1.1 200 OK
  Content-Type: application/json
  content-length: <n>
  Connection: close
  keys(14): fts implementation implementation_version read_only services services.pipelineWorker services.stateBackend services.timerScanner status stores stores.embeddingService stores.vectorStore uptime version
--- box new 127.0.0.1:19283 GET /health
status: HTTP/1.1 200 OK
  Content-Type: application/json
  content-length: <n>
  Connection: close
  keys(14): fts implementation implementation_version read_only services services.pipelineWorker services.stateBackend services.timerScanner status stores stores.embeddingService stores.vectorStore uptime version
--- VPS 127.0.0.1:8420 GET /nope
status: HTTP/1.1 401 Unauthorized
  Content-Type: application/json
  content-length: <n>
  Connection: close
  keys(1): error
--- box new 127.0.0.1:19283 GET /nope
status: HTTP/1.1 401 Unauthorized
  Content-Type: application/json
  content-length: <n>
  Connection: close
  keys(1): error
--- VPS 127.0.0.1:8646 GET /health
status: HTTP/1.1 200 OK
  content-type: application/json
  content-length: <n>
  connection: close
  keys(1): ok
--- box new 127.0.0.1:19285 GET /health
status: HTTP/1.1 200 OK
  content-type: application/json
  content-length: <n>
  connection: close
  keys(1): ok
--- VPS 127.0.0.1:8646 GET /nope
status: HTTP/1.1 404 Not Found
  content-type: application/json
  content-length: <n>
  connection: close
  keys(1): error
--- box new 127.0.0.1:19285 GET /nope
status: HTTP/1.1 404 Not Found
  content-type: application/json
  content-length: <n>
  connection: close
  keys(1): error
--- VPS 127.0.0.1:8646 GET /desk   (same headers as below)
  top-level keys (22): at board ctx events focus foundation heard hermes hold line mindpack model notes one resting said services tunnel turns voice vram waking
  board keys: agents cpu disk france gpu hermes_ms laptop_ram load model_ms pod pod_ram studio version
--- box new 127.0.0.1:19285 GET /desk
status: HTTP/1.1 200 OK
  content-type: application/json
  content-length: <n>
  connection: close
  top-level keys (22): same list as the VPS
  board keys: same list as the VPS
```

Status lines, header names, header order, and content-type and connection
values match exactly. JSON key shapes match.

The only difference is inside desk's `mindpack` (and the `events`/`notes`
array items). On the VPS these are filled from the live desk-hermes and
stored history: 115 key paths. On the box the fixture has no desk-hermes
binary and an empty state, so they are `{}` / `[]`: 38 key paths. That is
data, not protocol.

## 3. Client golden captures (old tree 26d98b0 vs this branch)

The `tests/wire_golden.rs` files in openmind-survival, one-talk and
openmind-memory were run unchanged on both trees, with ports masked. Every
`WIRE` line (exact request bytes sent) is identical: survival 28, one-talk
22, memory 8. The valid cases assert the exact bytes and pass on both
trees.

Outcome differences appear only for odd servers:

```
openmind-memory post_json
< OUT  chunked Ok((200, "7\r\n{\"conte\r\n8\r\nxt\":\"c\"}\r\n0\r\n\r\n"))
> OUT  chunked Ok((200, "{\"context\":\"c\"}"))
< OUT  garbage Ok((0, ""))
< OUT  status line only Err("bad response")
> OUT  garbage Err("bad response")
> OUT  status line only Ok((503, ""))
  (complete reply on a socket held open 2 s: old test 2.00 s, new 0.20 s)
openmind-survival http_get
< OUT  http_get chunked Ok((200, "a\r\n{\"status\":\r\nb\r\n\"ok\",\"n\":1}\r\n0\r\n\r\n"))
> OUT  http_get chunked Ok((200, "{\"status\":\"ok\",\"n\":1}"))
< OUT  http_get LF-only head Ok((200, ""))
> OUT  http_get LF-only head Ok((200, "ok"))
< OUT  http_get complete but kept open Ok((200, "ok")) (slow)
> OUT  http_get complete but kept open Ok((200, "ok"))
one-talk
< OUT  hear chunked Ok("{\"audio\":\"\",\"say\":\"Again\"}")
> OUT  hear chunked Ok("{\"say\":\"\"}")
< OUT  warm garbage Ok(0)
> OUT  warm garbage Err(UnexpectedEof)
< OUT  warm status line only Err(UnexpectedEof)
> OUT  warm status line only Ok(503)
```

## 4. scripts/real/common.sh ported as well

The branch must not add or keep any `.sh` compared with main. Slot 16's
`scripts/real/common.sh` (tuner + galion-origin, old vs new hammer) was also
on this branch, so it is ported to `crates/galion-common/tests/real_common.rs`
(same probes; default port base 19160, the swarm/common slot). It is run with
`GALION_REAL=1 GALION_REAL_OLD=1 cargo test -p galion-common --release --test
real_common -- --ignored --nocapture`. Run on my ports
(`GALION_REAL_PORT_BASE=19280`) from 21:32:27 to 21:33:52 +02:00, exit 0,
92.8 s:

```
tuner slowloris x80: {"HTTP/1.1 408": 64, "HTTP/1.1 503": 16}, longest held 5.1s; legit 200:41 503:19, first 200 at 5.3s
origin slowloris x80: {"HTTP/1.1 408": 32, "HTTP/1.1 503": 48}, longest held 10.1s; legit 200:21 503:39, first 200 at 10.3s
old-tuner slowloris x80: {"HTTP/1.1 405": 80}; legit 200:60
old-origin slowloris x80: {"closed": 3, "survived whole drip": 32, "write cut": 45}, longest held 37.4s; legit: 60 no reply
peak RSS after: tuner 3.8 MB, origin 3.1 MB, old-tuner 2.5 MB, old-origin 3.0 MB; all 4 alive
```
