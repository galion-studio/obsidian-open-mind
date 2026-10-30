<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Cortex heal verify (the VERIFY step of the heal loop)

Branch `swarm/gaia-heal-verify` (base: live cortex 3a8f6ff1, merge-6). Code:
`crates/gaia-cortex/src/verify.rs`, wired in `engine.rs`.

## Why

The live loop is detect (nerve) -> decide (cortex rules) -> act (nerve `POST /actions`,
`systemctl restart --no-block`). Before this change the cortex wrote `executed` and
stopped: nobody checked whether the restart **healed** the unit. A restart that did not
help looked the same as one that did. The circuit opened only if the unit crashed again
3 times in 15 min.

## What it does

For every restart the nerve **executed** (or whose answer was lost), the cortex arms one
check per unit with a deadline (`heal_verify_s`, default 180 s):

| evidence | verdict | effect |
|---|---|---|
| nerve `unit.state` `recovered:true` / `incident.resolved` (state active) for the unit, after the request | `verified` | nerve event `gaia.heal.verified` (info) |
| a new crash / OOM / start-limit / `failed` state of the unit before the deadline | `failed_again` | nerve event `gaia.heal.failed_again` (warn); the rules decide the crash as usual (R2 flap / R3 cap) |
| deadline: one `GET /state` from the nerve, unit `active`/`running` | `verified` (by nerve-state) | `gaia.heal.verified` |
| deadline: unit listed but not active (`failed`, `inactive`, still `activating`, ...) | `failed` | **R12-heal-failed**: open the unit's circuit (1 h, no more restarts), alert, escalate once; `gaia.heal.failed` (error) |
| deadline: nerve unreachable, unit not listed, or no `unit_state` (nerve without systemd) | `unverified` | **R12-heal-unverified**: alert only, never opens a circuit |

Never adds an action, never touches a unit; the only rule effect is *more* conservative
(an earlier circuit). Event-driven: no timer when nothing is pending; the decider sleeps
until the earlier of the escalation sweep and the next heal deadline. The `/state` read
happens outside the cortex lock (5 s timeout, 8 MiB cap).

Not armed: dry-run requests, simulated / refused / rate-limited / failed answers.
A newer restart for the same unit replaces the older check.

## Surfaces

- `GET /heals`: `{enabled, window_s, pending, armed, rearmed, verified, failed_again, failed, unverified, success_rate, pending_checks[], recent[]}` (newest first, last 100). Each finished check has `outcome`, `by`, `detail`, `heal_s`.
- `GET /health`: `heal_verify` (bool), `heals_pending`.
- `GET /stats`: `heals` (summary).
- Decision record: `action.verify {check, due}` on an armed restart.
- Persistence: `heals.jsonl` in the data dir (`armed` / `finished` lines); unfinished checks are re-armed on start (tail 500, 30 s grace if already overdue).

## Config

`heal_verify_s` in the JSON config or `CORTEX_HEAL_VERIFY_S`: `0` = off, else 30..=3600.
Anything else (or not a number) refuses to start (exit 2).

## Tests

- `src/verify.rs` unit tests (9): matching, replacement, ordering, persistence.
- `tests/heal_verify.rs` (9): engine wiring with a mock nerve (verified, failed + circuit +
  escalation, deadline state reads, failed-again keeps the rules, dry-run/off/lost reply,
  restart survival, lock-free read, wait choice, config bounds).
- `tests/real_heal_verify.rs` (GALION_REAL=1, ports 23200-23209): release cortex + REAL
  release event-mode nerve with live actions and a fake `systemctl` first on `PATH`
  (journal from a FIFO). Proves: executed restart -> verified by nerve recovery in ~3 s;
  restart of a unit that stays `failed` -> R12-heal-failed at the 30 s deadline, circuit
  open, the next crash is not restarted; `gaia.heal.*` events reach the nerve ring;
  window 5 / 4000 / "soon" refuse to start.

```
GALION_REAL=1 GALION_NERVE_BIN=<openmind-nerve 18a5a72 release> \
  cargo test --release -p gaia-cortex --test real_heal_verify -- --nocapture --test-threads 1
```
