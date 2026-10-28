<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: land-order-gateway-size (wave-4 slot 373), 2026-10-28 CET (+01:00)

Tree 49f214b9 (land + a0d099c2 + 16bfc28d + F-373-1). Release bytes: x86_64 musl 14-bin `-p`
set from real_release_repro (`<repro>/out/x86_64-unknown-linux-musl/full`), tagged copies
`<bin>-49f214b` for the artifact tests. Box ports 22430-22439 only. No secrets, no paid calls.

## 1. real_size_2mib (hermes-gateway, 07:37:15)

`GALION_REAL=1 HG2_PORT_BASE=22430 HG2_BIN=<full>/hermes-gateway cargo test -p hermes-gateway --test real_size_2mib`

```
SIZE 1948240 B (budget 2097152 B, headroom 148912 B)
ELF stripped yes, .relr.dyn yes
gateway :22430 /health 200 after 50 ms
GET /v1/models -> 200 (hermes-agent)
POST /v1/chat/completions -> 200 in 11 ms, body has 'stub answer ok': true
turn 2 same session -> 200; /api/sessions/<sid>/messages -> 200 roles 4
legacy FTS5 db (read-only) /api/sessions/s-py/messages -> 200 roles 2
restart: /health after 50 ms; session messages still 200 roles 4
/v1/models without key -> 401; --mock :22431 chat 200, mock-fail 502; stub upstream hits 2
PASS real_size_2mib
```

## 2. real_release_repro (07:31:42-07:36:36)

`GALION_REAL=1 GALION_REPRO_JOBS=4 cargo test -p galion-feature-pin --test real_release_repro`

| binary | bytes | budget | ok |
|---|---|---|---|
| openmind-nerve | 2,017,768 | 2,097,152 | ok |
| hermes-gateway | 1,948,240 | 2,097,152 | ok |
| openmind-memory | 1,559,120 | 2,097,152 | ok |
| galion-backup | 1,245,384 | 2,097,152 | ok |
| desk-hermes | 1,160,392 | 2,097,152 | ok |
| gaia-cortex (llm-advisor) | 1,152,872 | 2,097,152 | ok |
| galion-console-static / -serve | 1,029,096 | 2,097,152 | ok |
| galion-one | 960,648 | 2,097,152 | ok |
| openmind-queue | 894,472 | 2,097,152 | ok |
| link-watch | 785,336 | 2,097,152 | ok |
| galion-desk | 757,704 | 2,097,152 | ok |
| galion-health | 557,256 | 2,097,152 | ok |
| tuner | 481,096 | 2,097,152 | ok |

`solo -p hermes-gateway 1948240 B 8f8ac92530d22893 = full; pair -p openmind-nerve -p hermes-gateway
IDENTICAL` -> **19 compared, 0 differ, 0 over budget/unstripped**.

## 3. size_budget_gate (07:37:24)

`GALION_REAL=1 SIZE_BUDGET_REAL_DIR=<tagged> SIZE_BUDGET_REAL_COUNT=14 cargo test -p galion-realtest --test size_budget_gate`
4/4: budget_table_policy, script_passes_full_tagged_set_at_budget, script_fails_negative_controls
(2,198,288 B gateway FAIL, 2,097,153 B nerve FAIL, unstripped FAIL, 4000-byte table row FAIL),
real_release_set `hermes-gateway-49f214b 1948240 ok`, `size-budget: ok (14 binaries)`.

## 4. real_release_artifacts (07:54:39, ports 22430, 22431, dead upstream 22439)

First run 07:37:29 used base 22432, whose dead-upstream port base+9 = 22441 is outside the slot range
(nothing listens there; connect-refused probe only). Re-run 07:54:39 with base 22430, same result.
`GALION_REAL=1 GALION_RELEASE_TAG=49f214b GALION_RELEASE_PORT_BASE=22430 ... --test real_release_artifacts`:
**21 checks passed** - 9 `--version`, nerve/tuner/health/queue/memory/gateway/one/desk/console-static
HTTP 200, backup snapshot + verify integrity=ok tables=15, gaia-cortex-49f214b /health 200
(dry-run), galion-console-serve-49f214b / 302 and /api/v1/health 200.

## 5. real_release_smoke (07:37:42)

Hard-coded slot-337 ports 22070-22079 remapped in the working copy only to 22434/22435/22438/22439,
then reverted (`git checkout`), like slot 359 P-359-1.
`GALION_REAL=1 GALION_RELEASE_BINS_DIR=<full>`: **14 checks passed** (gaia-cortex llm-advisor
transport present, /health /stats /incidents /decisions /alerts /escalations 200, /nope 404;
console-serve / 302, /api/v1/health 200, /api/v1/ping 200, /api/v1/session 401,
`/../etc/passwd` 404).

## 6. Live VPS, read-only (07:31 CET)

nerve 127.0.0.1:8670 /state 200; cortex :8674 /health 200; gateway :8642 /health 200;
hermes-rs active, NRestarts 0, ExecStart `/opt/galion/rust/bin/hermes-gateway-b7fbbc78`
= 2,732,248 B, no `.relr.dyn` (F-353-1: the live build is the old one; this branch's
1,948,240 B build is 784,008 B smaller). Open nerve alert inc-865945dd (galion-one mouth stuck
loading 13 h, slot 358). Nothing changed on the VPS.

All started processes exited with the tests; ports 22430-22439 free afterwards.
