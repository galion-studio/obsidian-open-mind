<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life test: common-migrate-2 (wave 2 slot 28, ports 19280-19289)

All times are Europe/Zagreb (CEST, +02:00).

## 1. Migrated vs pre-migration binaries on the box

Run: `scripts/real/common-migrate-2.sh` from 2026-10-20 20:59:15 to 21:02:30
+02:00, on the final code of this branch. The script:

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
- samples memory and threads, and checks every process is still alive. All
  processes are killed at the end (trap).

Result: **88 byte-identical, 10 identical after masking, 0 different**.
**All 8 servers alive** at the end; exit 0.

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

### Output (verbatim)

```
== 2026-10-20T20:59:15+02:00 valid traffic, old vs new
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
  masked hermes GET /api/sessions (old 1458 / new 1456 bytes)
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
  same   desk POST /desk rest (864 bytes)
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
== summary: 88 byte-identical, 10 identical after masking, 0 different
.
== 2026-10-20T20:59:25+02:00 abuse
before new-hermes: VmHWM:4056 VmRSS:4056 Threads:2
before new-memory: VmHWM:3980 VmRSS:3980 Threads:1
before new-desk: VmHWM:2536 VmRSS:2536 Threads:1
before new-room: VmHWM:2404 VmRSS:2404 Threads:1
before old-hermes: VmHWM:4092 VmRSS:4092 Threads:2
before old-memory: VmHWM:4096 VmRSS:4096 Threads:1
before old-desk: VmHWM:2440 VmRSS:2440 Threads:1
before old-room: VmHWM:2308 VmRSS:2308 Threads:1
new-hermes chunked size ffffffffffffffff: HTTP/1.1 400 in 0.4ms
new-hermes chunked size line 2000 bytes: HTTP/1.1 400 in 0.2ms
new-hermes chunked non-hex size: HTTP/1.1 400 in 0.2ms
new-hermes chunked negative size: HTTP/1.1 400 in 0.1ms
new-hermes chunked missing CRLF after data: HTTP/1.1 400 in 0.1ms
new-hermes chunked trailer 20 KB: HTTP/1.1 400 in 0.2ms
new-hermes chunked 9 MiB chunked body: HTTP/1.1 413 in 8.9ms
new-hermes chunked TE gzip: HTTP/1.1 501 in 0.3ms
new-hermes chunked TE chunked + CL: HTTP/1.1 400 in 0.2ms
new-hermes chunked TE twice: HTTP/1.1 501 in 0.1ms
new-hermes chunked truncated chunked: HTTP/1.1 400 in 0.1ms
new-hermes chunked slow drip: Some("dripped 4 s, client gave up") after 3.9s
new-hermes after chunked abuse: /health -> HTTP/1.1 200
old-hermes chunked size ffffffffffffffff: HTTP/1.1 413 in 0.2ms
old-hermes chunked size line 2000 bytes: HTTP/1.1 400 in 0.1ms
old-hermes chunked non-hex size: HTTP/1.1 400 in 0.1ms
old-hermes chunked negative size: HTTP/1.1 400 in 0.1ms
old-hermes chunked missing CRLF after data: HTTP/1.1 400 in 0.1ms
old-hermes chunked trailer 20 KB: HTTP/1.1 400 in 0.1ms
old-hermes chunked 9 MiB chunked body: HTTP/1.1 413 in 13.8ms
old-hermes chunked TE gzip: HTTP/1.1 400 in 0.3ms
old-hermes chunked TE chunked + CL: HTTP/1.1 400 in 0.3ms
old-hermes chunked TE twice: HTTP/1.1 400 in 0.1ms
old-hermes chunked truncated chunked: HTTP/1.1 400 in 0.4ms
old-hermes chunked slow drip: Some("dripped 4 s, client gave up") after 3.9s
old-hermes after chunked abuse: /health -> HTTP/1.1 200
.
new-hermes sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.42ms max=1.39ms
new-hermes concurrent: 31622 req/s over 0.10s
new-hermes concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.36ms p99=2.71ms max=6.34ms
new-hermes malformed: 416 requests in 0.03s -> {"400": 412, "401": 1, "505": 1, "closed": 2}
new-hermes after malformed: /health -> Some(200)
new-hermes oversized 100 KB header: Some(413) in 0.7ms
new-hermes oversized 1000 headers: Some(413) in 0.2ms
new-hermes oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-hermes oversized 5 MB body: Some(401) in 4.7ms
new-hermes slowloris x80: outcomes {"HTTP/1.1 408": 64, "HTTP/1.1 503": 16}, longest held 10.1s
new-hermes legit GETs during slowloris (60 over 15s): {"Some(200)": 21, "Some(503)": 39}, first 200 at Some("10.3s")
new-hermes final /health -> Some(200)
new-memory sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=3.67ms max=7.26ms
new-memory concurrent: 8979 req/s over 0.36s
new-memory concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.43ms p99=12.55ms max=27.74ms
new-memory malformed: 416 requests in 0.09s -> {"400": 15, "closed": 401}
new-memory after malformed: /health -> Some(200)
new-memory oversized 100 KB header: Some(400) in 1.1ms
new-memory oversized 1000 headers: Some(400) in 0.2ms
new-memory oversized Content-Length 10 GB (no body): Some(413) in 2.9ms
new-memory oversized 5 MB body: Some(413) in 0.8ms
new-memory slowloris x80: outcomes {"HTTP/1.1 503": 48, "closed": 32}, longest held 10.1s
new-memory legit GETs during slowloris (60 over 15s): {"Some(200)": 21, "Some(503)": 39}, first 200 at Some("10.3s")
new-memory final /health -> Some(200)
new-desk sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=2.17ms max=6.42ms
new-desk concurrent: 11037 req/s over 0.29s
new-desk concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.35ms p99=11.05ms max=18.04ms
new-desk malformed: 416 requests in 0.04s -> {"400": 411, "501": 2, "505": 1, "closed": 2}
new-desk after malformed: /health -> Some(200)
new-desk oversized 100 KB header: Some(431) in 0.2ms
new-desk oversized 1000 headers: Some(431) in 0.1ms
new-desk oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-desk oversized 5 MB body: Some(413) in 0.7ms
new-desk slowloris x80: outcomes {"HTTP/1.1 408": 32, "HTTP/1.1 503": 48}, longest held 3.0s
new-desk legit GETs during slowloris (60 over 15s): {"Some(200)": 49, "Some(503)": 11}, first 200 at Some("3.3s")
new-desk final /health -> Some(200)
new-room sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.26ms max=1.54ms
new-room concurrent: 36276 req/s over 0.09s
new-room concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.25ms p99=2.12ms max=3.63ms
new-room malformed: 416 requests in 0.03s -> {"400": 411, "501": 2, "505": 1, "closed": 2}
new-room after malformed: /health -> Some(200)
new-room oversized 100 KB header: Some(431) in 0.2ms
new-room oversized 1000 headers: Some(431) in 0.2ms
new-room oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
new-room oversized 5 MB body: Some(413) in 0.5ms
test real_hammer has been running for over 60 seconds
new-room slowloris x80: outcomes {"HTTP/1.1 408": 32, "HTTP/1.1 503": 48}, longest held 8.1s
new-room legit GETs during slowloris (60 over 15s): {"Some(200)": 29, "Some(503)": 31}, first 200 at Some("8.3s")
new-room final /health -> Some(200)
old-hermes sequential GET /health: n=2000 ok=2000 p50=0.07ms p99=0.71ms max=3.53ms
old-hermes concurrent: 25499 req/s over 0.13s
old-hermes concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.23ms p99=4.59ms max=10.38ms
old-hermes malformed: 416 requests in 0.05s -> {"400": 405, "401": 11}
old-hermes after malformed: /health -> Some(200)
old-hermes oversized 100 KB header: Some(413) in 0.4ms
old-hermes oversized 1000 headers: Some(401) in 0.3ms
old-hermes oversized Content-Length 10 GB (no body): Some(413) in 0.2ms
old-hermes oversized 5 MB body: Some(401) in 5.9ms
old-hermes slowloris x80: outcomes {"HTTP/1.1 503": 16, "survived whole drip": 64}, longest held 37.4s
old-hermes legit GETs during slowloris (60 over 15s): {"Some(503)": 60}, first 200 at None
old-hermes final /health -> Some(200)
old-memory sequential GET /health: n=2000 ok=2000 p50=0.07ms p99=0.22ms max=3.60ms
old-memory concurrent: 17227 req/s over 0.19s
old-memory concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.16ms p99=7.44ms max=11.94ms
old-memory malformed: 416 requests in 0.03s -> {"400": 5, "404": 11, "closed": 400}
old-memory after malformed: /health -> Some(200)
old-memory oversized 100 KB header: Some(400) in 3.9ms
old-memory oversized 1000 headers: Some(404) in 0.3ms
old-memory oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
old-memory oversized 5 MB body: Some(413) in 0.2ms
old-memory slowloris x80: outcomes {"HTTP/1.1 503": 48, "survived whole drip": 32}, longest held 37.4s
old-memory legit GETs during slowloris (60 over 15s): {"Some(503)": 60}, first 200 at None
old-memory final /health -> Some(200)
old-desk sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.49ms max=1.16ms
old-desk concurrent: 42788 req/s over 0.07s
old-desk concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.30ms p99=2.02ms max=4.82ms
old-desk malformed: 416 requests in 0.03s -> {"404": 414, "closed": 2}
old-desk after malformed: /health -> Some(200)
old-desk oversized 100 KB header: Some(404) in 0.4ms
old-desk oversized 1000 headers: Some(404) in 0.6ms
old-desk oversized Content-Length 10 GB (no body): Some(200) in 0.3ms
old-desk oversized 5 MB body: Some(200) in 0.4ms
old-desk slowloris x80: outcomes {"HTTP/1.1 404": 80}, longest held 0.0s
old-desk legit GETs during slowloris (60 over 15s): {"Some(200)": 60}, first 200 at Some("0.5s")
old-desk final /health -> Some(200)
old-room sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.41ms max=2.32ms
old-room concurrent: 33063 req/s over 0.10s
old-room concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.28ms p99=4.07ms max=5.36ms
old-room malformed: 416 requests in 0.03s -> {"200": 7, "404": 1, "405": 408}
old-room after malformed: /health -> Some(200)
old-room oversized 100 KB header: Some(200) in 0.2ms
old-room oversized 1000 headers: Some(200) in 0.3ms
old-room oversized Content-Length 10 GB (no body): Some(405) in 0.1ms
old-room oversized 5 MB body: Some(405) in 0.2ms
old-room slowloris x80: outcomes {"HTTP/1.1 405": 80}, longest held 0.0s
old-room legit GETs during slowloris (60 over 15s): {"Some(200)": 60}, first 200 at Some("0.5s")
old-room final /health -> Some(200)
.
== 2026-10-20T21:02:30+02:00 done
after new-hermes: VmHWM:12040 VmRSS:10664 Threads:2
after new-memory: VmHWM:4880 VmRSS:4880 Threads:1
after new-desk: VmHWM:3304 VmRSS:3304 Threads:1
after new-room: VmHWM:3220 VmRSS:3220 Threads:1
after old-hermes: VmHWM:12024 VmRSS:10688 Threads:2
after old-memory: VmHWM:4936 VmRSS:4936 Threads:1
after old-desk: VmHWM:2888 VmRSS:2888 Threads:1
after old-room: VmHWM:2736 VmRSS:2736 Threads:1
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
exit 0
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
