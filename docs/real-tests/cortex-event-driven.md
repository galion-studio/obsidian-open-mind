<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex-event-driven (gaia-cortex)

All times Europe/Zagreb (+02:00). The VPS address is not recorded here.

## Local (box)

- `cargo test -p gaia-cortex -p nerve-cortex-contract`: lib unit tests, `tests/pipeline.rs`
  (crash→restart, flapping→circuit→escalate, protected→alert only, budget exhausted→LLM refused,
  openrouter/hermes endpoint refused, idle→0 work/0 calls, escalation.batch is housekeeping,
  nerve incidents → R11) and `tests/binary_e2e.rs` (real `gaia-cortex` binary against a loopback
  mock nerve: stream → decision → `POST /actions` → reply parsed).
- Release builds: x86_64-unknown-linux-musl static 0.9 MB; aarch64-unknown-linux-musl 772968 bytes
  (links with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`).
- `--features llm-advisor` builds and passes clippy; the default build has no LLM transport
  (`NotCompiled`), so nothing can call a model even if the config says `enabled: true`
  (config validation rejects that combination).

## VPS sandbox (2026-10-21, 18:39 – 18:53)

Non-root throwaway user `sbxcx1839` (member of `systemd-journal` only) in
`/var/tmp/sandbox-test-20261021-183907`; spare loopback ports 8674 (cortex) and 8675 (a copy of the
nerve-event-driven worker's binary, `NERVE_MODE=event`, `NERVE_DRY_RUN=1`). No live unit, drop-in or
config was touched; `/opt/galion/openmind` was not touched.

- cortex binary sha256 `be056d95c023eb86…` (build dd1869b, musl static).
- Trigger: `systemd-run --unit cortex-sbx-crash-20261021-183907 --property Restart=no sh -c 'exit 3'`
  three times (18:42:05, 18:42:25, 18:42:45; `reset-failed` between runs).

| time | nerve event | cortex decision | action / reply |
|---|---|---|---|
| 18:42:07 | unit.failed cortex-sbx-crash… | R3-known-crash (failure 1/900 s) | `POST /actions` restart dry_run → nerve `refused` (not on reflex allowlist) → escalated |
| 18:42:26 | unit.failed cortex-sbx-crash… | R3-known-crash (failure 2) | refused → escalated |
| 18:42:26 | unit.failed nerve-evt-fail (other worker's test unit) | R4-unknown-unit | alert + escalate, no action |
| 18:42:47 | unit.failed cortex-sbx-crash… | R2-circuit-open (3 failures in 900 s) | no action; circuit open until 19:42:47; escalated |
| 18:43:07 | synthetic tuner.service unit.failed (sent to the sandbox nerve `POST /event` only) | R3-known-crash | nerve `dry-run`: `would_run: systemctl restart --no-block tuner.service`; why: NERVE_EVT_ACTIONS not live, reflex dry-run, request dry_run |

Our own `remediation` echoes were classified R10 (info, no alert). Stats after the scenario:
events_in 24, batches 7, decisions 8, actions_requested 3 (simulated 1, refused 2, executed 0),
alerts 5, escalations 4, llm_calls 0, spend $0.00. LLM block: enabled false, endpoint guard ok,
ledger spent 0, remaining $15.00.

Findings fixed during the run (build dd1869b): the nerve's `escalation.batch` contains the cortex's
own alerts, so it is now housekeeping (was an R99 alert feedback loop); nerve
`incident.opened/resolved` go to R11.

### Idle (~10 min)

| | 18:43:25 | 18:52:58 |
|---|---|---|
| cpu ticks (utime+stime) | 2 | 2 |
| VmRSS / threads | 772 kB / 3 | 772 kB / 3 |
| events_in / batches / decisions | 24 / 7 / 8 | 24 / 7 / 8 |
| housekeeping (stream keepalive) | 4 | 5 |
| llm_calls / spend | 0 / $0 | 0 / $0 |

Live units unchanged before, during and after (MainPID / NRestarts): galion-nerve 721130/0,
hermes-rs 718450/0, tuner 4004553/0, cloudflared 611846/0, all active.

### Cleanup (18:53)

Killed the user's processes (cortex, nerve copy, its journalctl child), `systemctl reset-failed` of
the transient unit (0 `cortex-sbx-*` units left), removed the sandbox dir, `userdel sbxcx1839`;
ports 8674/8675 free; live units unchanged.
