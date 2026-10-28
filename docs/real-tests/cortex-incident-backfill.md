<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex-incident-backfill (wave 4 slot 237)

Branch `swarm/cortex-incident-backfill`, base `swarm/cortex-t51-chaos`
e982210. Box only, no deploy, VPS read-only. Ports 21070-21078. Nerve:
release `openmind-nerve` bb287cc (`/workspace/swarm/artifacts/openmind-nerve-bb287cc`).
All times Europe/Zagreb (+02:00).

## The problem, seen live (VPS read-only, 2026-10-22 00:08 and 00:33)

| | live nerve :8670 `/incidents` open | live cortex :8674 `/incidents` open |
|---|---|---|
| 00:08 | edge `inc-a763c076` (22:56:56), galion-one/mouth `inc-d7320b38` (22:56:56), ddos:edge-cloudflared `inc-eb95d192` (23:56:56), ... | only `nerve-incident:ddos:edge-cloudflared` (opened 23:56:58) |
| 00:33 | ddos:edge-cloudflared `inc-eb95d192` **critical**, galion-one `inc-d7320b38` error | **none**; its ddos incident `resolved` 00:26:58 after `cycles: 1` (2 unrelated batches, F3) while the nerve still has it open |

The live ring (`GET /events/stream?since=0`, read 3 s) shows the shapes the
cortex has to match: `incident.opened` data `{"cause": "edge", "id": "edge", ...}`,
`incident.resolved` data `{"id": "edge"}` only (seq 27/28/34/40). The base
cortex keys nerve incidents by `data.cause`, so every `incident.resolved`
landed on a different subject (`engine:incident.opened`) and never closed
anything; incidents only closed through the F3 batch counter.

## Fixes (crates/gaia-cortex)

1. Backfill on every stream (re)connect: `GET /stats` seq, then
   `GET /incidents`; missing causes -> synthetic `incident.opened`
   (`backfill: true`, R11), causes gone from the nerve -> synthetic
   `incident.resolved` (reconciled). First stream starts at the pre-snapshot seq.
2. Nerve incidents keyed by cause; `incident.resolved {"id"}` gets its cause;
   duplicate opens dropped; a restarted nerve's new `inc-...` id followed silently.
3. Incident lifetime: active subjects are carried into every burst until their
   own signal clears (`cleared`), one-shot subjects expire by time
   (`incident_quiet_s`, default 1800 s, `expired`), optional sticky TTL
   (`incident_sticky_max_s`, default 0 = off, `stale`). Decisions (incl.
   stack-wide R0) still use the burst only.

## Real test 1: `real_t237_backfill_and_incident_lifetime` (box, GALION_REAL=1)

```
GALION_REAL=1 GALION_NERVE_BIN=/workspace/swarm/artifacts/openmind-nerve-bb287cc \
GALION_CORTEX_BASE_BIN=<release gaia-cortex built from e982210> \
cargo-slot test -j 2 --release -p gaia-cortex --test real_t237_backfill -- --nocapture
```

Run 1 (00:25:37-00:26:06), 3/3 runs pass (00:25, 00:26, 00:26-00:27), ~29.6 s each:

```
P0 nerve A seq 4: open inc-27d35f98=t237-a inc-7c7ecdb1=t237-b inc-06a94c86=t237-c
P1a base cortex e982210: nerve incidents seen 0 of 3, open incidents 0          <- the live bug, reproduced
P1 cortex backfilled 3/3 in 556 ms (R11 x3, alerts 3, 0 actions)                (519 / 536 ms in runs 2, 3)
P2 tuner incident inc-fd2f85c0 still open after 8 unrelated batches (last_seen unchanged)
P2 tuner Started -> incident resolved_by cleared in 521 ms
P3 nerve resolved inc-27d35f98 after 2 pokes / 12019 ms; its incident.resolved data keys ["id"];
   cortex resolved inc-3f545448 (resolved_by cleared) +1024 ms
P4 nerve A SIGKILL, nerve B on the same port (c inc-06a94c86 -> inc-8c467918):
   b reconciled+resolved in 1505 ms, c remapped (same cortex incident inc-ccda1c9a, decisions 1 unchanged)
final: actions_executed 0, llm_calls 0, nerve_incidents_duplicate 1, reconciled 2, remapped 1, nerve_restarts 1
```

First attempts found two more real facts (kept in the test): three hard-down
components fold into one `stack-wide` nerve incident (so b and c are
`"critical": false`), and the nerve's built-in `gaia-cortex` component opens
its own incident on the box (no cortex on :8674 there).

## Real test 2: `real_t237_live_nerve_shadow_read_only` (live VPS nerve, read-only)

`ssh -N -L 127.0.0.1:21077:127.0.0.1:8670 slavik-vps`, then the new release
cortex (dry-run, LLM off) behind a std GET-only proxy (:21076; any non-GET ->
403, never forwarded), 90 s, 00:35:18-00:36:51:

```
live nerve: seq 41 streams 2 open [ddos:edge-cloudflared inc-eb95d192 (23:56:56), galion-one inc-d7320b38 (22:56:56)]
shadow cortex backfilled 2 live nerve incident(s) in 2747 ms
after 90 s: live nerve open [ddos:edge-cloudflared, galion-one]; shadow cortex open [both];
  nerve_incidents {"ddos:edge-cloudflared":"inc-eb95d192","galion-one":"inc-d7320b38"};
  decisions 2, actions_requested 0, actions_executed 0, alerts_to_nerve 0; proxy blocked 3 POST(s)
live cortex :8674 at 00:37: open [], opened_total 1
```

Disclosure: the probes used one nerve stream slot of 4 for ~3 s (curl, 00:33)
and one for ~95 s (shadow); the nerve frees a dead stream at its next write
(keepalive <= 5 min). Nothing was written on the VPS; no service touched.

## Regression: T5.1 chaos still green, S3 now shows the F3 fix

`real_t51_chaos` (ports 20970-20973, 00:32:06-00:33:39): 2/2 pass, 92.4 s.
`S3: S1 incident inc-d2214fda open true resolved false after 6 unrelated bursts`
(base e982210 recorded `resolved true`).

## Gates

- `cargo test -p gaia-cortex`: 9 + 1 + 11 (incident_lifecycle) + 22 (pipeline) + 1 + 2 + contract 8 ok.
- Mutation: carried signals dropped -> 5 of 8 lifecycle tests fail.
- Workspace (excl. openmind-sandbox, cgroup-detect, hermes-tools, `--no-fail-fast`):
  230 suites 1139 passed / 1 failed / 4 ignored; the 1 is
  `openmind-nerve replay_corpus::every_saved_capture_is_in_the_corpus`
  (box state: live captures saved by other slots; same failure recorded by slot 227 on the base).
- fmt ok; clippy `-D warnings` workspace (excl. sandbox crates) 0, gaia-cortex with `llm-advisor` 0.
- x86_64-musl (static-pie) and aarch64-musl (rust-lld, static) release builds ok;
  aarch64-linux-android and armv7-unknown-linux-musleabihf `cargo check` ok.
- `scripts/no-python.sh` ok; repo-check 4 errors / 44 warnings = base (1243 vs 1242 files).
- No new dependencies, `Cargo.lock` unchanged.

## Owner decisions (Slavik)

- **D-237-1 deploy**: this branch is not deployed. Live gaia-cortex runs
  bd6fbe6 (cortex-live-fixes = cortex-hermes-boss + e982210) and shows the bug
  right now (0 open vs 2 open on the nerve). Merge 237 into cortex-live-fixes
  and redeploy? (engine.rs/main.rs will need a small merge with the hermes
  handoff code.)
- **D-237-2 sticky TTL**: the nerve never publishes recoveries for
  postfix/ssh/other non-lifecycle units, so their cortex incidents stay open
  until a TTL. Keep `incident_sticky_max_s = 0` (never; honest but grows), or
  set e.g. 86400 (24 h, `stale`)? Alternative: the nerve publishes
  `unit.started` for every unit that had a failure (nerve change).
- **D-237-3 backfill alerts**: backfilled incidents alert once (critical
  escalates) after every cortex start, subject to the 120 s cooldown only
  within one process. Keep, or make backfill silent (record only)?
- **D-237-4 one-shot quiet**: 1800 s for process exits / tasks / other.
