<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life test: galion-common HTTP (slot 16, ports 19160-19169)

Run: `OLD=1 scripts/real/common.sh` on the box, 2026-10-20 20:02:25 to
20:04:01 +02:00 (Europe/Zagreb). Release builds (`opt-level = "z"`, LTO,
`panic = "abort"`) of the migrated **tuner** (127.0.0.1:19160, cap 64
connections, 5 s head deadline) and **galion-origin** (127.0.0.1:19161, cap 32,
10 s head deadline). For comparison, the pre-migration binaries built from
13a296ce ran as old-tuner (19163) and old-origin (19162). The hammer is
`crates/galion-common/tests/real_hammer.rs` (gated by `GALION_REAL=1`). All
processes were killed at the end (trap in the script).

Scenarios per service: 2000 sequential `GET /health`; 16 clients x 200
concurrent; 416 malformed requests (16 hand-written: bad request lines, bad
versions, `Host :`, obs-fold, NUL, duplicate/negative CL, TE chunked, CL+TE,
a TLS ClientHello, truncated head, empty; plus 400 random byte strings);
oversized (100 KB header, 1000 headers, `Content-Length: 10000000000`, a real
5 MB body); slowloris (80 connections dripping one header byte every ~0.5 s)
with 60 legit GETs over 15 s during the wave; final health check.

## Output (verbatim, trimmed to the new vs old pairs)

```
== 2026-10-20T20:02:25+02:00 start
before tuner: VmHWM:2264 VmRSS:2264 Threads:1
before origin: VmHWM:2200 VmRSS:2200 Threads:1
before old-tuner: VmHWM:2076 VmRSS:2076 Threads:1
before old-origin: VmHWM:2316 VmRSS:2316 Threads:1
tuner sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.37ms max=4.32ms
tuner concurrent: 32281 req/s over 0.10s
tuner concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.23ms p99=4.59ms max=6.43ms
tuner malformed: 416 requests in 0.03s -> {"400": 411, "501": 2, "505": 1, "closed": 2}
tuner after malformed: /health -> Some(200)
tuner oversized 100 KB header: Some(431) in 0.3ms
tuner oversized 1000 headers: Some(431) in 0.1ms
tuner oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
tuner oversized 5 MB body: Some(413) in 0.7ms
tuner slowloris x80: outcomes {"HTTP/1.1 408": 64, "HTTP/1.1 503": 16}, longest held 5.1s
tuner legit GETs during slowloris (60 over 15s): {"Some(200)": 41, "Some(503)": 19}, first 200 at Some("5.3s")
tuner final /health -> Some(200)
origin sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.37ms max=2.52ms
origin concurrent: 52716 req/s over 0.06s
origin concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.21ms p99=1.11ms max=1.67ms
origin malformed: 416 requests in 0.03s -> {"400": 411, "501": 2, "505": 1, "closed": 2}
origin after malformed: /health -> Some(200)
origin oversized 100 KB header: Some(431) in 0.2ms
origin oversized 1000 headers: Some(431) in 0.1ms
origin oversized Content-Length 10 GB (no body): Some(413) in 0.1ms
origin oversized 5 MB body: Some(413) in 0.6ms
origin slowloris x80: outcomes {"HTTP/1.1 408": 32, "HTTP/1.1 503": 48}, longest held 10.1s
origin legit GETs during slowloris (60 over 15s): {"Some(200)": 21, "Some(503)": 39}, first 200 at Some("10.3s")
origin final /health -> Some(200)
old-tuner sequential GET /health: n=2000 ok=2000 p50=0.06ms p99=0.25ms max=0.78ms
old-tuner concurrent: 49845 req/s over 0.06s
old-tuner concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.25ms p99=1.14ms max=2.08ms
old-tuner malformed: 416 requests in 0.03s -> {"200": 7, "404": 1, "405": 406, "closed": 2}
old-tuner after malformed: /health -> Some(200)
old-tuner oversized 100 KB header: Some(200) in 0.6ms
old-tuner oversized 1000 headers: Some(200) in 0.1ms
old-tuner oversized Content-Length 10 GB (no body): Some(405) in 0.1ms
old-tuner oversized 5 MB body: Some(405) in 0.4ms
old-tuner slowloris x80: outcomes {"HTTP/1.1 405": 80}, longest held 0.0s
old-tuner legit GETs during slowloris (60 over 15s): {"Some(200)": 60}, first 200 at Some("0.5s")
old-tuner final /health -> Some(200)
old-origin sequential GET /health: n=2000 ok=2000 p50=0.07ms p99=0.37ms max=2.98ms
old-origin concurrent: 42072 req/s over 0.08s
old-origin concurrent 16x200 GET /health: n=3200 ok=3200 p50=0.35ms p99=1.45ms max=2.87ms
old-origin malformed: 416 requests in 0.03s -> {"200": 7, "400": 96, "405": 313}
old-origin after malformed: /health -> Some(200)
old-origin oversized 100 KB header: Some(200) in 0.3ms
old-origin oversized 1000 headers: Some(200) in 0.1ms
old-origin oversized Content-Length 10 GB (no body): Some(405) in 0.2ms
old-origin oversized 5 MB body: Some(405) in 0.3ms
old-origin slowloris x80: outcomes {"closed": 3, "survived whole drip": 32, "write cut": 45}, longest held 37.4s
old-origin legit GETs during slowloris (60 over 15s): {"None": 60}, first 200 at None
old-origin final /health -> Some(200)
== 2026-10-20T20:04:01+02:00 done
after tuner: VmHWM:3604 VmRSS:3604 Threads:1
after origin: VmHWM:2844 VmRSS:2844 Threads:1
after old-tuner: VmHWM:2504 VmRSS:2504 Threads:1
after old-origin: VmHWM:3032 VmRSS:3032 Threads:1
peak threads seen by the 1 s sampler:
  origin 33
  old-origin 33
  old-tuner 2
  tuner 65
tuner: alive
origin: alive
old-tuner: alive
old-origin: alive
```

## Summary

| | tuner new | tuner old | origin new | origin old |
|---|---|---|---|---|
| survived everything | yes | yes | yes | yes |
| sequential p50 / p99 | 0.06 / 0.37 ms | 0.06 / 0.25 ms | 0.06 / 0.37 ms | 0.07 / 0.37 ms |
| concurrent p50 / p99 | 0.23 / 4.59 ms | 0.25 / 1.14 ms | 0.21 / 1.11 ms | 0.35 / 1.45 ms |
| malformed handling | 400/501/505, 2 silent closes (empty and truncated) | answered 200/404/405 to garbage | same as new tuner | 200/400/405 to garbage |
| 100 KB header / 1000 headers | 431 / 431 | 200 / 200 (read 4 KiB, ignored the rest) | 431 / 431 | 200 / 200 |
| CL 10 GB / 5 MB body | 413 / 413 in < 1 ms | 405 / 405 | 413 / 413 | 405 / 405 |
| slowloris x80 | all 80 cut: 64 got 408 at 5.1 s, 16 over cap got 503 | not affected (answers after first byte) | all 80 cut: 32 got 408 at 10.1 s, 48 got 503 | **32 slots held for 37 s (entire drip)**, 45 dropped |
| legit GETs during slowloris | 41/60 ok (503 until 5.3 s) | 60/60 | 21/60 ok (503 until 10.3 s) | **0/60** (silently dropped) |
| RSS before -> after (VmHWM) | 2264 -> 3604 KiB | 2076 -> 2504 KiB | 2200 -> 2844 KiB | 2316 -> 3032 KiB |
| peak threads (1 s sampler) | 65 (cap 64 + accept) | 2 | 33 (cap 32 + accept) | 33 |
| threads after | 1 | 1 | 1 | 1 |
| x86_64 binary size | 344,912 B | 331,936 B | 366,208 B | 356,832 B |

Notes:
- The old tuner looked "immune" to slowloris only because it answered after a
  single read of whatever bytes had arrived (it returned 405 to a one-byte
  request); it also had no thread cap at all (`thread::spawn` per accept).
- The old origin read with a 10 s timeout *per read*, so a client sending a
  byte every 0.5 s held a thread indefinitely; with its 32-connection cap the
  service was fully unavailable for the whole wave. The shared server uses a
  total deadline for the head, so the same wave is cut at the deadline and
  the service recovers by itself.
- Trade-off that remains: while a wave fills the cap, new connections get a
  fast 503 until the head deadline frees slots (5 s tuner, 10 s origin). A
  per-peer cap would not help on loopback, where everything is 127.0.0.1.
- Peak RSS stays under 4 MiB with 64 live connection threads (128 KiB stacks
  for tuner, 64 KiB for origin).

## Read-only probe of the live VPS tuner (2026-10-20 20:04:20 +02:00)

`ssh ... root@VPS` then `GET /health` on 127.0.0.1:8650 over `/dev/tcp`
(read only; nothing changed). The live reply is:

```
HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: 12
Cache-Control: no-store
X-Content-Type-Options: nosniff
Referrer-Policy: no-referrer
Permissions-Policy: microphone=(self)
Content-Security-Policy: default-src 'self'; style-src 'self'; script-src 'self'; media-src 'self' blob:
Connection: close

{"ok":true}
```

The migrated tuner on the box (127.0.0.1:19164, same request) returned the
same bytes (`cmp` identical; sha256 of the reply
`6625bd800f1c40e462dd7cf7f429a2c2bb95a26884057dc2e5e5a63749d7cc1e`).
The origin port 8655 was not listening on the VPS at that time (`connect:
refused`), so there was no live origin to compare.

## Ported to Rust (2026-10-21, +02:00)
`scripts/real/common.sh` was replaced on swarm/realscript-port-1 by the gated Rust test `crates/galion-realtest/tests/real_sh_common.rs` (same checks; run with `GALION_REAL=1`). The evidence above is from the shell version; the Rust run is in `docs/real-tests/realscript-port-1.md`.
