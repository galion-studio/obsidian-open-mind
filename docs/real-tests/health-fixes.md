<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: swarm/health-fixes (galion-health v0.2.0)

Wave 4 slot 149, box ports 20190-20199. All times Europe/Zagreb CEST +02:00. The VPS was only read: HTTP GETs on loopback,
`systemctl is-active/show`, `sha256sum`, and SSH local port forwards (`ssh -L`) so the box binary could GET the live
services. Nothing on the VPS was written, restarted, built or deployed.

## 1. Live v0.1.0 on the VPS shows finding 1 (2026-10-21 06:54:12-06:54:22 +02:00)
Raw bash `/dev/tcp` client on the VPS, GET only, 200-300 ms between writes:
| request writes | live 8660 answer | second write |
|---|---|---|
| `GET /healthz HTTP/1.0\r\n` + `Host: x\r\n\r\n` (x3) | 200 `ok` | failed (rc=1, socket already closed) |
| `GET /stat` + `us.json HTTP/1.0\r\n...` | **404 not found** | failed |
| `GE` + `T /healthz HTTP/1.0\r\n...` | **200 text/html (the / page)** | failed |
| one write | 200 `ok`, client saw `Connection reset by peer` once | - |

## 2. Box real test, v0.2.0 vs v0.1.0 (`tests/real_fixes.rs`)
`GALION_REAL=1 cargo test -p galion-health --test real_fixes -- --ignored --nocapture`
(`GALION_HEALTH_BIN=<path>` to test another binary). Fake upstream 20191, black-hole upstream 20192 (accepts, never answers),
raw TCP 20193, closed 20194, server 20190. Runs: v0.2.0 06:59:26, v0.1.0 06:59:42-07:14:10, v0.2.0 again 07:19:17
(glibc) and 07:19:33 (static x86_64 musl, sha256 `9daec765489610f59eb609f624816720c7e0813568f6943549af3a1acac7937e`).

| scenario | v0.2.0 | v0.1.0 (f4a66ea build) |
|---|---|---|
| `--version` | PASS `galion-health 0.2.0` | FAIL (no flag; it starts serving) |
| parity /status.json, /json, /healthz, /, /status 404 (5 checks) | PASS | PASS |
| split mid-path -> /status.json | PASS 200 | FAIL **404** |
| split in method -> /healthz | PASS `ok` | FAIL (serves / HTML) |
| split headers, 2nd/3rd write | PASS | FAIL `Broken pipe` |
| byte-by-byte head (34 writes, 15 ms gaps) | PASS in 0.50 s | FAIL `Broken pipe` |
| 6000 B unread body -> clean close | PASS | FAIL `Connection reset by peer` |
| garbage request | PASS 400 | FAIL (200 /) |
| 9 KB head | PASS 431 | FAIL RST |
| 2 idle clients, then a 3rd GET | PASS 0.3-0.6 ms | FAIL **6.15 s** |
| idle client | PASS 408 after 2.9-3.06 s | FAIL (200 / page) |
| cold cache, 12 hung upstreams, 10 concurrent clients | PASS worst **2.51 s** | FAIL worst **30.62 s** |
| 404 during that probe round | PASS 1.51 s (all 3 workers busy waiting; bound = one round) | FAIL 29.62 s |
| threads (TasksMax=8 budget) | PASS 5 threads, VmRSS 2.0-2.3 MB glibc / 532 kB musl | 1 thread |
| hung upstream -> `no-reply` x12 | PASS | PASS |
| expired cache, single-flight refresh | PASS 2.51 s | FAIL 30.89 s |
| 1 worker + 32 queued idle -> 503 busy | PASS in 0.2-0.7 ms | FAIL (no answer in 15 s) |
| closed port -> `down`, /healthz 503 `failing: gone` (2 checks) | PASS | PASS |
| **total** | **23 pass, 0 fail** (3 runs) | **9 pass, 14 fail** |

No listeners left on 20190-20199 after each run; no galion-health processes left. (While first running v0.1.0 for comparison,
its missing `--version` made it start a server on the box's default 127.0.0.1:8660 until killed at about 07:12; box only.
The test now pins the address and kills it after 2 s.)

## 3. Real data: v0.2.0 against the live VPS services (2026-10-21 07:14:28-07:14:55 +02:00)
`ssh -L` forwards box 20191-20198 to VPS loopback 8420, 8642, 8646, 8650, 8091, 8080, 20241, 22 and 20199 to the live 8660.
Unit columns are `-` (systemd units cannot be read through a port forward).

`galion-health --once`, 3 rounds each, alternating binaries, same 8 live services:
| round | v0.1.0 (serial) | v0.2.0 (side by side) | codes (both binaries, all rounds) |
|---|---|---|---|
| 1 | 3742 ms | 753 ms | 200 200 200 200 200 302 200 tcp-open |
| 2 | 3957 ms | 753 ms | same |
| 3 | 4383 ms | 560 ms | same |
The live 8660 `/status.json` at 07:14:29 gave the same codes for these 8 services (openmind-memory 200, hermes-rs 200,
galion-desk 200, tuner 200, one 200, console 302, cloudflared 200, ssh tcp-open), all 12 ok. The round times include the
SSH forward's latency (about 0.4-0.6 s per probe); on the VPS loopback both would be near 0 ms, as the live `ms` values show.

Serve mode (v0.2.0 on 20190, live upstreams): `curl /healthz` 200 in 0.568 s (cold), then 0.39 ms and 0.45 ms (cache);
split `GET /stat` + `us.json` -> 200 application/json (828 B); split `GE` + `T /healthz` -> 200 `ok`; 5 threads, VmRSS 2216 kB.
Startup log: `galion-health 0.2.0 listening on 127.0.0.1:20190 checks=8 workers=3`.

## 4. VPS state after the tests (07:15:04 +02:00, read-only)
galion-health `active`, NRestarts=0, active since 2026-10-18 22:14:38 +02:00, binary sha256 still `6afe668c...` (unchanged).
Nerve 127.0.0.1:8670 `/state`: cycle 6800, `healthy` 11, `galion_health_ok` true for 9 components and null for 6 (no 8660 entry).
The cortex is not live yet (nothing answers on 8680), so the nerve was used, per no_skip_extensive_testing.

## 5. Gates (2026-10-21 07:15-07:19 +02:00)
- `cargo fmt --check` OK; `cargo clippy --all-targets -- -D warnings` (workspace) OK.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 116 result lines, **477 passed, 0 failed**, 2 ignored.
- galion-health unit tests: 14 passed (6 old + 8 new: split writes, EOF without blank line, empty/timeout/bad/too-large,
  total deadline vs trickling client, status line parsing, slow upstreams side by side, split upstream status line, no systemctl without units).
- Targets: x86_64 musl release 557856 B (static), aarch64 musl release 496888 B (rust-lld), `cargo check` OK on
  armv7-unknown-linux-musleabihf and aarch64-linux-android.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` scanned 178, missing 0.
