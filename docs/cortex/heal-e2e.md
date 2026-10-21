<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex T4.4: the heal path end to end (slot 222)

Branch `swarm/cortex-t44-e2e`, base `swarm/cortex-t42-hades-plans` 1ce9737.
Test: `crates/openmind-nerve/tests/real_cortex_t44.rs` (gated `GALION_REAL=1`,
box only, ports 20920-20929).

```
cortex finding -> incident -> HADES plan template -> hades-gate token -> heal-apply -> nerve record
stuck-loading-  inc-xxxx     plan-unit-restart-      keygen/mint/serve    restart the    POST /api/v1/event
t44-dummy                    t44-dummy.service       (test key, box)      dummy once     (source hades-heal)
```

Every link is a real process with a real socket: the release `openmind-nerve
serve`, the release `hades-gate serve` + `keygen`/`mint`/`disable`/`enable`
CLI, and a dummy service (the test binary re-executed) that hangs in
`503 loading` until it is restarted.

## The executor is test code

The nerve and the cortex only *write* plans (T4.2). The heal-apply executor
in this test is the "later, separately approved executor" the plan text
names, kept inside the test so nothing on the VPS gains the ability to run
plans. Its rules, in order, each failing closed:

1. action `heal-apply`, family `unit-restart` only (`executor:action`, `executor:family`);
2. target not protected by the nerve's `is_protected` nor hades-gate's
   `review_target` (`executor:protected-target`);
3. `POST /v1/approve` to the gate; unreachable or odd reply refuses
   (`executor:gate-unreachable`, `executor:gate-bad-reply`); `DryRun` runs
   nothing; any 403 is `gate:<code>`;
4. the approval must name the same action, target and SHA-256 of the exact
   plan text (`executor:approval-mismatch`);
5. the unit must be one the executor owns (`executor:unknown-unit`);
6. plan precondition: the component still fails its check (`executor:precondition`);
7. restart once, verify `200` with a new pid within 30 s (`executor:verify-failed`);
8. every attempt, applied or not, is recorded in the nerve.

## Owner decisions

- **D-T44-1** A real executor. This branch proves the chain but ships no
  executor. If one is wanted: a separate binary, allow-list of units, gate
  over loopback HTTP, the rules above, records to the nerve; deploying it is
  a separate approval. Recommended: keep `unit-restart` the only family it
  runs at first.
- **D-T44-2** Live stray plans for our own services. On the live state of
  2026-10-21 23:02 +02:00 the T4.2 templates give `stray-stop` plans for
  :8674 (the live gaia-cortex), :8672 (galion-nerve-shadow), :19120
  (openmind-queue) and :18800. The plan's precondition (a unit owns it ->
  stop) aborts them and this executor refuses the family, but the nerve's
  component table / `NERVE_KNOWN_PORTS` on the VPS should list them
  (a VPS config change: owner).
- **D-T44-3** Heal records. Now an event-log line plus a hub signal from
  source `hades-heal`. Should the incident itself carry the heal (e.g.
  `incident.heals[]`) and should the cortex treat a refused heal as an
  escalation? Not done here.
- **D-T44-4** The live nerve (bb287cc) has no `/api/v1/cortex/plans` (404
  "not found" at 23:02): T4.2 is not deployed, so this chain cannot run
  against live yet; the live plans were made on the box from a live state
  capture.
