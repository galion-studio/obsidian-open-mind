<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: nerve-parser-fuzz (wave-4 slot 212, T3.1)

All times Europe/Zagreb (CEST +02:00). Box ports 20820 (nerve) and 20821
(dummy component). VPS: read-only only (HTTP GET, `journalctl` reads,
`systemctl show/list-units`, `ss -ltn`). No POST to any live service.

## 60 s smoke per target (box, release, fixed seed)

`CORTEX_FUZZ_SECS=60 cargo test --release -p openmind-nerve --test nerve_fuzz -- --ignored --nocapture --test-threads 1 nerve_fuzz_all_summary`

Run 1, 21:59:55-22:12:27, at 5e5d547 (after the first three fixes):
3 targets failed -> finds 4 (access JSON ip uncapped) fixed; event-body
1-ULP drift and journald future stamps judged not-bugs (see
docs/nerve-fuzzing.md). Run 2, 22:15:15-22:27:28, at 730a467:

| target | execs | distinct | corpus | panics | violations |
|---|---|---|---|---|---|
| signal-body | 36,637,954 | 38 | 42 | 0 | 0 |
| event-body | 52,903,813 | 36 | 39 | 0 | 0 |
| http-read-req | 7,087,530 | 21 | 27 | 0 | 0 |
| journald-json | 2,491,149 | 289 | 293 | 0 | 0 |
| cf-metrics | 6,576,946 | 28 | 30 | 0 | 0 |
| cf-log-lines | 1,880,403 | 11,040 | 512 | 0 | 0 |
| access-line | 14,424,789 | 9,532 | 512 | 0 | 0 |
| signal-roundtrip | 3,419,238 | 4 | - | 0 | 0 |
| metrics-roundtrip | 28,687,020 | 8 | - | 0 | 0 |
| http-roundtrip | 1,170,192 | 8 | - | 0 | 0 |
| stamp-roundtrip | 52,716,354 | 3 | - | 0 | 0 |
| deep-json-stack | 623,139 | 2 | - | 0 | 0 |
| **total** | **208,618,527** | | | **0** | **0** |

Run 3, 22:35:47-22:38:00, at 6d23120 (after the stack fix, release with
`--features serde_json/preserve_order`): cf-log-lines 1,801,881 execs,
deep-json-stack 520,474 execs, 0 panics, 0 violations.

## Stack headroom (box, `probe_deep_json_stack_kb`, one process per size)

Depth 126-129 and 4096, arrays and objects, `/signal` + `/event` parsers:
debug default ok from 256 KiB; debug + preserve_order ok from 384 KiB
(256 KiB = stack overflow abort); release default ok from 64 KiB;
release + preserve_order ok from 96 KiB. -> `CONN_STACK` 512 KiB.

## Real binary under fuzz (box)

`GALION_REAL=1 NERVE_REAL_FUZZ_N=2000 cargo test --release -p openmind-nerve --test real_parser_fuzz box_ -- --nocapture`
22:28:06, pid 4053261, 127.0.0.1:20820, dry-run, no systemd/journal:

```
signal-body    2000 POST /signal: codes {"202": 169, "400": 1831}
event-body     2000 POST /event: codes {"200": 171, "400": 1829}
http-raw       2000 mutated requests: codes {"200": 299, "400": 1123, "404": 351, "405": 30, "431": 197}
deep depth 127..4000 (arrays + objects): /signal 400, /event 400 (all answered)
head 8192 B: one write 400, 1-byte segments 400, 700-byte segments 400
head 8193 B: one write 431, 1-byte segments 431, 700-byte segments 431
head 9000 B: one write 431, 1-byte segments 431, 700-byte segments 431
after: pid alive, cycle 0 -> 1, /signals 16653 B, /events 219 records, stderr panics 0
```

Same test, debug build with `--features serde_json/preserve_order` (the
build that aborted at 256 KiB) at e88811a: deep depth 127 answered 400/400,
process alive, 0 panics. Process killed and temp dir removed by the test;
ports 20820/20821 free afterwards (`ss -ltn`).

## VPS read-only (live parsers on real data)

`GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p openmind-nerve --test real_parser_fuzz vps_ -- --nocapture`

22:01:23 (debug) and 22:38:25 (release, 6d23120):

```
live nerve /healthz: 200
live nerve /state: cycle 16 mode "dry-run" system "degraded" score 66     (22:01: cycle 13, failing, 41)
cf-metrics     live input 168 lines -> ok   (ha_connections 4, total_requests 10075, request_errors 6284)
cf-log-lines   live input 400 lines -> ok
journald-json  live input 1500 records (8 chunks of <= 200) -> ok
live journal: dead origins [3010, 3011, 3020, 4001, 4003, 4020, 4060], edge failures 346, scan hits 0
live /state tunnel cloudflared {"ha_connections":4,"request_errors":6224,"requests":10010}
```

Cortex (deployed, read-only GET 22:38:49): `gaia-cortex.service` active
(`gaia-cortex-c7f56b8`, 127.0.0.1:8674), `/health` ok, mode dry-run,
events_in 26, incidents_open 0, escalations_open 0. Also active:
`galion-nerve.service`, `galion-nerve-shadow.service`. Nothing was written
on the VPS; only counts were printed, no log text was stored.

## Gates

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` 0 findings.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 273 suites, 1282 passed, 0 failed, 8 ignored (the first run at 33ba160 aborted in nerve_fuzz: find 5).
- `cargo test -p openmind-nerve --test nerve_fuzz` (default and `--features serde_json/preserve_order`): 18 passed, 2 ignored.
- x86_64-unknown-linux-musl release: static-pie, 1,205,024 B; aarch64-unknown-linux-musl release (rust-lld): 1,004,392 B; aarch64-linux-android and armv7-unknown-linux-musleabihf `cargo check` ok.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` missing 0; repo-check 0 errors, 48 warnings (PASS).
