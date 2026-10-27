<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# cortex-oom-onto-bd6fbe6 — thin cherry-pick of R4b-sandbox-oom onto live bd6fbe6

Europe/Zagreb (CET +01:00). Slot 253. **NO live deploy.**

## Goal

Port Mate-approved rule `R4b-sandbox-oom` from `swarm/cortex-sandbox-oom-info`
`c68d8cb791f02e83b6cd9389e3acebcb5f2c5cec` (base `swarm/cortex-live-merge-2`
`1305dec`) onto the live-line source `bd6fbe657837db2d72ade96891b80c73961f7f94`
(`swarm/cortex-live-fixes` / running `gaia-cortex-bd6fbe6`) as a **thin**
cherry-pick that does **not** pull in merge-2 (api-load / flap-dedupe /
incident-backfill).

## Why not a mechanical cherry-pick

`git cherry-pick c68d8cb` / `git apply` of the raw commit onto `bd6fbe6` fails
on hunk 1 of `crates/gaia-cortex/src/rules.rs`: the merge-2 doc table context
includes `R3-cap-total` / `restarts_total_hour`, which do not exist on live
`bd6fbe6`. Hunks 2 (decide() early return) and 3 (helper + unit tests) apply
with line offset. `tests/pipeline.rs` applies cleanly (same blob as
`c68d8cb^`).

`Limits` on live also lacks `restarts_total_hour`, so the unit-test helper
from `c68d8cb` would not compile without a one-line drop of that field.

This is **not** an irreducible conflict: the behavioural rule body and the
pipeline test are identical to `c68d8cb`; only the module doc line and the
test `Limits { ... }` initializer were thin-adapted to live's shape.

## Changes (this branch)

- `crates/gaia-cortex/src/rules.rs`: doc row + early `R4b-sandbox-oom` return
  for `sbxtest*` / `sbxtest.slice` + `EventKind::Oom`; `is_sbxtest_unit`;
  unit tests (Limits without `restarts_total_hour`).
- `crates/gaia-cortex/tests/pipeline.rs`: `sbxtest_oom_is_info_no_hermes_handoff`
  (live seq 51 shape; no alert, no Hermes handoff).

## Ports

Assigned 21220–21229. Used only if a release binary real probe is run; unit +
pipeline tests do not bind.

## Deploy

**Forbidden for this slot.** Artifact (if built) is under
`/workspace/swarm/artifacts/` for a later owner-approved cutover. Live remains
`gaia-cortex-bd6fbe6` until then.

## Real release probe (box, Europe/Zagreb CET +01:00)

Binary: `/workspace/swarm/target/cortex-oom-onto-bd6fbe6/release/gaia-cortex` (glibc release; musl artifact also built).
Listen: `127.0.0.1:21220`. Hermes handoff ON, no nerve URL, coalesce 200 ms.

```
health: {"busy":false,"escalations_open":0,"events_in":0,"handoffs":0,"hermes_handoff":true,"incidents_open":0,"llm_enabled":false,"mode":"live","nerve":false,"ok":true,"service":"gaia-cortex","ts":"2026-10-27T17:37:29+01:00","version":"0.1.0"}
ingest sbxtest: {"accepted":1,"invalid":0,"ok":true}
decisions after sbxtest: [{"action":null,"alert":false,"decided_by":"rules","dry_run":false,"escalate":false,"events":1,"id":"dec-1","incident":"info:unit:sbxtest.slice","kinds":["oom.kill"],"llm":null,"outcome":"none","reason":"sbxtest.slice: sandbox OOM kill (sbxtest* memory cap); info only, no Hermes escalate","rule":"R4b-sandbox-oom","severity":"info","subject":"unit:sbxtest.slice","ts":"2026-10-27T17:37:29+01:00","unit":"sbxtest.slice"}]
stats: {"actions_circuit_open":0,"actions_executed":0,"actions_failed":0,"actions_rate_limited":0,"actions_refused":0,"actions_requested":0,"actions_simulated":0,"actions_unknown":0,"alerts":0,"alerts_to_nerve":0,"batches":1,"decisions":1,"decisions_suppressed":0,"escalations":0,"escalations_open":0,"events_duplicate":0,"events_housekeeping":0,"events_ignored":0,"events_in":1,"events_invalid":0,"handoffs":0,"handoffs_budget_stopped":0,"handoffs_duplicate":0,"handoffs_failed":0,"handoffs_rate_limited":0,"hermes_handoff":{"budget_stopped":false,"enabled":true,"inbox":"/workspace/swarm/tmp/slot253-probe-data/hermes-inbox","incidents_handed":0,"per_hour":6,"sent_last_hour":0},"incidents_open":0,"incidents_opened_total":0,"last_batch":"2026-10-27T17:37:29+01:00","last_event":"2026-10-27T17:37:29+01:00","llm":{"budget_usd_total":15.0,"calls_per_hour":6,"calls_this_process":0,"enabled":false,"endpoint":"https://api.river.ai/v1","endpoint_guard":"ok","key_env":"RIVER_API_KEY","ledger":{"calls_total":0,"input_tokens":0,"output_tokens":0,"refused_total":0,"remaining_usd":15.0,"spent_usd":0.0},"max_tokens":400,"model":"deepseek-ai/DeepSeek-V4.1-Flash","refusals":{}},"llm_calls":0,"llm_refused":0,"llm_skipped_disabled":0,"llm_spend_usd":0.0,"mode":"live","nerve_restarts":0,"started":"2026-10-27T17:37:29+01:00"}
alerts: []
escalations: {"open":[],"total":0}
hermes-inbox files: 1
ingest mystery: {"accepted":1,"invalid":0,"ok":true}
decisions after mystery: [{"action":null,"alert":true,"decided_by":"rules","dry_run":false,"escalate":true,"events":1,"id":"dec-2","incident":"info:unit:mystery.service","kinds":["oom.kill"],"llm":null,"outcome":"none","reason":"mystery.service failed; not on the restart allowlist, no rule can act","rule":"R4-unknown-unit","severity":"warn","subject":"unit:mystery.service","ts":"2026-10-27T17:37:31+01:00","unit":"mystery.service"},{"action":null,"alert":false,"decided_by":"rules","dry_run":false,"escalate":false,"events":1,"id":"dec-1","incident":"info:unit:sbxtest.slice","kinds":["oom.kill"],"llm":null,"outcome":"none","reason":"sbxtest.slice: sandbox OOM kill (sbxtest* memory cap); info only, no Hermes escalate","rule":"R4b-sandbox-oom","severity":"info","subject":"unit:sbxtest.slice","ts":"2026-10-27T17:37:29+01:00","unit":"sbxtest.slice"}]
```

## Artifact

- Path: `/workspace/swarm/artifacts/gaia-cortex-oom-onto-bd6fbe6-x86_64-musl`
- sha256: `9f6679953c5ac035aa8e8d390d3872e38f54eeb4e24d87da4c269115cd2dd312`
- Built: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld cargo build -p gaia-cortex --release --target x86_64-unknown-linux-musl`
- Size: 980136 bytes
- NO live deploy (slot forbids).
