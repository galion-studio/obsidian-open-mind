<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: cortex-t52-fuzz (slot 185, ports 20550-20559)

All times Europe/Zagreb (+02:00), 2026-10-21. Box = swarm box; VPS =
the VPS (ssh alias `slavik-vps`, override `GALION_VPS_SSH`), **read-only** (ssh `ss -ltn`, `cat /proc/net/tcp{,6}`,
`journalctl -o json`, `cat` of the cloudflared config, `curl` GET of the
live nerve `:8670/api/v1/state`, `cat` of the nerve event log). No live
service, unit, file or permission was changed. Captures went to a private
0700 temp dir and were deleted (journal lines carry client IPs); only
counts are recorded here.

## 1. First run on the base (before fixes), 11:44

`cargo test -p openmind-nerve --test fuzz_parsers` (debug, fixed seed):
11 passed, **4 failed**:

| Target | Result |
|---|---|
| `rfc3339` (6000 execs) | 2 violations: event_feed `Some(1792537200)` vs util `None` for `2026-10-21T01:+0:00+02:00`; same for `2026-10-+5T02:00:00+01:00` |
| `ingress-yaml` (3000) | 7 violations: staged config re-parses with more rules than expected; root cause: items less indented than the first ingress item silently ended the list (one input: 1 rule seen, 36 dropped) |
| `ss-ltn` (4000) | 1 violation: `probe::parse_ss` `("fe80::1]", 22)` vs tunnel-lint `("fe80::1", 22)` |
| `gen-ss` (2000) | 1697 violations: `[::1]%lo:PORT` parsed as `::1]` (and so not loopback) |

After a multi-byte seed (`0000000é…` as a 32-byte tcp6 address) was added:
`proc-net-tcp` **23 panics** in 3000 execs, `end byte index 8 is not a char
boundary; it is inside 'é'` (probe::hex_ipv6).

All five fixed (see docs/cortex/fuzzing.md); 4 regression tests added.

## 2. Box run, debug build (overflow checks on), 11:45:03 - 11:51:18

`CORTEX_FUZZ_SECS=25 CORTEX_FUZZ_SEEDS=<VPS captures from 11:37:43> fuzz_parsers --ignored fuzz_all_summary`

```
ss-ltn             execs    408848  distinct     45  corpus   49  panics 0  violations 0  25.0s
proc-net-tcp       execs     47880  distinct   1021  corpus  512  panics 0  violations 0  25.0s
journald-json      execs     44123  distinct    560  corpus  512  panics 0  violations 0  25.0s
cloudflared-msg    execs    438715  distinct   2764  corpus  512  panics 0  violations 0  25.0s
event-jsonl        execs    102945  distinct    340  corpus  351  panics 0  violations 0  25.0s
rfc3339            execs   1131507  distinct   4096  corpus  512  panics 0  violations 0  25.0s
http-event-req     execs   1444440  distinct     21  corpus   24  panics 0  violations 0  25.0s
ingress-yaml       execs    455024  distinct     31  corpus   38  panics 0  violations 0  25.0s
gen-proc-net       execs     75378  distinct     21                panics 0  violations 0  25.0s
gen-ss             execs    257351  distinct     30                panics 0  violations 0  25.0s
gen-journal        execs    118782  distinct     20                panics 0  violations 0  25.0s
gen-cloudflared    execs   1554220  distinct     64                panics 0  violations 0  25.0s
gen-event          execs    147695  distinct      5                panics 0  violations 0  25.0s
gen-rfc3339        execs   9649641  distinct      2                panics 0  violations 0  25.0s
gen-ingress        execs    539581  distinct     12                panics 0  violations 0  25.0s
TOTAL execs 16416130, targets 15, failing 0
```

## 3. `GALION_REAL=1 CORTEX_FUZZ_VPS=1 real_cortex_fuzz` (release), 11:54:02 - 11:57:14

`CORTEX_FUZZ_REAL_SECS=20 CORTEX_FUZZ_REAL_EVENTS=4000`; 1 passed, 191.8 s.

### 3a. Real VPS data through every parser (captures 11:54:02 - 11:54:23)

- `ss -ltn`: 19 LISTEN lines -> 19 listeners (18 loopback), 17 ports.
  `/proc/net/tcp{,6}` (358 + 4 lines): 19 listeners, 17 ports;
  `proc_owner` 19 rows. ss-only ports: none, /proc-only: none.
- `journalctl -o json` (cloudflared, ssh, galion-nerve, last 2 h): 10204
  JSON lines -> 10204 records. cloudflared 9192 lines, **all 9192 classed
  error**; ssh 946 (8 errors); galion-nerve 66 (10 errors, 28 warnings).
  cloudflared: `Request failed ... dest=` 4596, refused dials 9192, 21
  distinct hostnames; **0 debug request lines** (cloudflared runs without
  debug logging, so the T2.12 per-host edge rate only ever sees failures).
  Dead origins: 3004, 3005, 3010, 3011, 3020, 3333, 4001, 4003, 4004, 4020,
  4042, 4060, 4444, 8000.
- Cloudflared ingress config: parses, **38 rules** (38 list items, so the
  new shallow-item error does not trip on the real file). Linted against the
  same `ss`: 19 closed ports 3004, 3005, 3010, 3011, 3015, 3020, 3080, 3333,
  4001, 4003, 4004, 4020, 4042, 4050, 4060, 4444, 5530, 8000, 8889.
- Live nerve `/api/v1/state`: cycle 261, score 61, grade D, 0 incidents
  (cortex runs inside the nerve on the VPS: the aa77864 build).
- Live `/var/lib/galion-nerve/events.jsonl`: 1 line, 0 bad.
- Seeded fuzz round, 20 s per target over these captures (release):

```
ss-ltn             execs   1555534  distinct     45  corpus   49  panics 0  violations 0
proc-net-tcp       execs    260682  distinct   1304  corpus  512  panics 0  violations 0
journald-json      execs    201964  distinct    651  corpus  512  panics 0  violations 0
cloudflared-msg    execs   2334790  distinct   3650  corpus  512  panics 0  violations 0
event-jsonl        execs    371547  distinct    422  corpus  433  panics 0  violations 0
rfc3339            execs   4675173  distinct   4096  corpus  512  panics 0  violations 0
http-event-req     execs  11313876  distinct     21  corpus   24  panics 0  violations 0
ingress-yaml       execs   1530785  distinct     31  corpus   38  panics 0  violations 0
```

  plus the 7 generators x 2000 on release: 0 panics, 0 violations.

### 3b. Real release `openmind-nerve serve` on 127.0.0.1:20550 (11:57:04 - 11:57:14)

`--log <tmp>/events.jsonl`, `NERVE_EVENT_FEED_FILE` = the same log,
systemd/journal/reflex off, listen scope 20550-20559.

- Up at cycle 1, RSS 3424 KiB.
- 4000 `POST /event` with fuzzed JSON bodies: 200 x1233, 400 x2765, 413 x2.
- 2000 raw fuzzed HTTP requests (bad heads, huge `Content-Length`, binary
  tails): 200 x215, 400 x1194, 404 x543, 405 x46, 413 x1, closed without a
  status line x1 (a request with a large unread binary tail: the server
  answers and closes, the kernel resets the socket over the unread bytes,
  so the client sees no reply; not a crash). 0 connect failures; 0.8 s.
- After: cycle 1 -> 3, `/api/v1/state` 200, RSS 6780 KiB, cycle_ms 30. The
  log has 1335 lines, 0 invalid JSON (1233 + accepted raw POSTs); the
  event-feed parser reads it back with `lines 1335, bad 0`. The cortex
  shows 105 event findings (<= MAX_KEYS 128), ids cleaned (e.g.
  `heal-capped-_t185` from a fuzzed service name).
- Process killed at the end; temp dirs removed.

## 4. Final re-run on the final commit (ssh alias, 10 s per target), 12:03:04 - 12:04:56

`GALION_REAL=1 CORTEX_FUZZ_VPS=1 CORTEX_FUZZ_REAL_SECS=10 CORTEX_FUZZ_REAL_EVENTS=4000`:
1 passed, 112.2 s. Same picture: ss 19 / /proc 19 listeners, 17 ports, no
difference; journal 10245 -> 10245 records (cloudflared 9278, all errors,
4639 `dest=` failures, 22 hosts, 0 debug request lines); ingress 38 rules,
19 closed ports; live nerve cycle 279, 61/D. 8 seeded targets + 7
generators: 0 panics, 0 violations. Real server :20550: POST /event 200
x1233 / 400 x2765 / 413 x2; raw 200 x215 / 400 x1195 / 404 x542 / 405 x46 /
413 x1 / no-status x1; 0 connect failures; cycle 1 -> 3; log 1335 lines, 0
invalid, feed bad 0; RSS 3448 -> 6720 KiB.

## 5. Unit / workspace (final commit)

- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`:
  **1054 passed, 0 failed, 4 ignored** (incl. 19 `fuzz_parsers` tests +
  1 tunnel-lint regression test).
- `cargo test -p openmind-nerve -p tunnel-lint`: 208 passed, 0 failed, 1
  ignored (`fuzz_all_summary`, the box-run entry point).
- `cargo fmt --all --check` ok; `cargo clippy -p openmind-nerve -p
  tunnel-lint --all-targets -- -D warnings` ok; `scripts/no-python.sh` ok.
- x86_64 + aarch64 musl release builds (static), aarch64-linux-android and
  armv7-unknown-linux-musleabihf `cargo check` ok.
- repo-check: 3 errors, all pre-existing on the base (`surface.rs` 198.18.0.0
  probe address, 3 lines); the 2 this branch first added (a VPS address
  and a sample IP in a seed) were removed.
