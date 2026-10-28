<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: post-publish-batch-main (wave-4 slot 421), 2026-10-28 CET (+01:00)

Tree: `swarm/post-publish-batch-main` 96e24ce2 (main tree a6f6c163 + slot 415 batch 18d68395).
Slot ports 22910-22919. Release builds, loopback only. Every started process was stopped by the
tests; `ss -ltn` showed 0 listeners on 22910-22919 at 20:51.

## Box, GALION_REAL=1, `--release`

| time | test | env | result |
|---|---|---|---|
| 20:48:26-20:48:37 | `console-static --test real_landing` | `LANDING_REAL_PORT_BASE=22910` | 1/1 ok: `/console` 200 31,678 B, 0 `<script>`, 9 cards, 2 svg; base css 20,324 B, overview css 6,040 B; /status, /services 200 (same bytes as slot 415) |
| 20:48:37-20:48:43 | `room --test real_room` | `ROOM_REAL_PORT=22912` | 2/2 ok: host session on :22912 (24 objects, 32 concurrent clients 200, port closed after teardown); netns session on :22913 answers inside the namespace, host connect refused |
| 20:48:43-20:50:46 | `openmind-nerve --test real_cross_target --test-threads=1` | `NERVE_XT_PORT_BASE=22915`, pin mode budget | **8/8 ok, 97.3 s**: x86_64 musl 2,017,768 B sha256 ee9f33e9... (WARN above 1,887,435 B, headroom 79,384 B to 2,097,152 B); aarch64 1,666,696 B sha256 f207de74...; live recipe == solo (ee9f33e9); smoke :22915 / :22916 (qemu aarch64) / :22917 first cycle 0.10 / 0.21 / 0.10 s, /health wire order equal; android aarch64 check ok 7.7 s, armv7 check ok 5.4 s; `size_budget_table` (D-415-2 `BUDGET <= tsv_budget()`) ok |

The nerve release binary from this tree has the same sha256 (ee9f33e9...) and size as slot 373 / 415:
main's extra 5 files and the batch do not change it.

Not re-run: `hermes-gateway real_enroll` (no hermes-gateway file differs from slot 415's tree;
71/72 there = known F-383-5).

## VPS, read-only (ssh, GET / systemctl is-active only)

| time | probe | result |
|---|---|---|
| 20:45:25 | `systemctl is-active galion-nerve gaia-cortex` | active / active |
| 20:45:25 | `curl http://127.0.0.1:8674/health` (live cortex) | ok true, mode live, nerve true, llm_enabled true, events_in 37, incidents_open 2, escalations_open 0, handoffs 0 |
| 20:46 | `curl http://127.0.0.1:8670/state` (live nerve) | mode live, started 17:40:13, cycle 14, cycle_ms 1054, self_eval 68 D, 15 components, 1 failing (tuner-quick), 3 incidents |

Note: the first probe at 20:45:25 saved the /state body to `/tmp/.s421` on the VPS for a byte count
(32,844 B); that temp file was removed at 20:46 (checked gone). No other VPS write.
