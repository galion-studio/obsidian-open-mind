<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Gaia S3.1: supervised `gaia heal-tick --loop` and the global restart ledger

Branch `swarm/gaia-tick-loop` (wave-4 slot 160), based on integration-3 625eaf0. Written 2026-10-21 CEST (+02:00).
Scope: **box test services only**. Nothing here is installed on the VPS, and no unit or timer was written or enabled.

## What changed
| Before (625eaf0) | After (this branch) |
|---|---|
| One `gaia heal-tick` = one tick; "no daemon or timer loop yet" (docs/real-tests/gaia-live.md) | `gaia heal-tick --loop` runs ticks forever or until a budget is spent, under supervision |
| Restart cap ledger = `<--state>/restarts.jsonl` only; a new `--state` dir got a fresh 3/h budget | Cap = `--state` ledger **plus** global `gaia-restarts.jsonl` next to hades-gate's replay ledger (from `--hades-config`); a new `--state` cannot reset it |
| Check-then-record unlocked | OS file lock (`<global>.lock`, `File::lock`) from the final cap re-read to the ledger write; released before the health wait |
| Restarted children never reaped (fine for a one-shot that exits) | The loop reaps exited children (`waitpid` WNOHANG via `Child::try_wait`; never a signal), so a crash loop leaves no zombies |
| `gaia-dummy` = healthy only | `--mode ok|hang|flap|crash` fault modes for tests |

Code: `crates/gaia/src/heal_loop.rs` (loop), `crates/gaia-live/src/heal.rs` (`Budget`, `global_ledger_path`, `Ledger::{at,entries,record_synced}`, `reap`), `crates/gaia/src/live_heal.rs` (uses `Budget`; `HealTick` gains `approval_code` and `spawned`), `crates/gaia/src/bin/gaia-dummy.rs`.

## Usage
```
gaia heal-tick --loop [--interval-ms 30000] [--jitter-ms 0] [--max-ticks N] [--max-runtime-s S]
               [--max-restarts R] [--max-errors 5] [--stop-file F] [--token-dir D] [--seed N]
               --service NAME --port P --allow-ports A-B --nerve 127.0.0.1:N --state DIR
               --hades-config C [--wait-ms MS] [--health /health] -- /abs/start/cmd ARGS...
```
- **Interval + jitter.** Sleep after each tick = `interval + U(0, jitter)` ms on the monotonic clock (`Instant`), so the 2026-10-25 03:00 CEST -> CET switch cannot shorten or stretch a sleep. Jitter is from `/dev/urandom` via `hades_gate::keys::os_random`, or from `--seed` (splitmix64) for a reproducible run. The default of 30 s matches the live nerve's 30 s cycle (`interval_s: 30` on the VPS, 07:14 CEST).
- **Budget.** The loop stops at the first of `--max-ticks`, `--max-runtime-s`, `--max-restarts` (restarts this loop spawned), `--max-errors` consecutive errors (exit 7), or the `--stop-file` existing. The stop file is checked between ticks and every 100 ms while asleep. The real test measured 100-101 ms from writing the file to all three loops exiting.
- **Scope first.** A target that fails the scope guard (live port, range outside 19000-19999, forbidden program) stops the loop before the first tick with exit 4. Every tick also re-checks it.
- **Supervision.** A tick that returns a setup error (nerve down, ledger unreadable or corrupt) or panics (`catch_unwind`) does not end the loop. The loop backs off for `min(interval * 2^(errors-1), interval + 60 s)` plus jitter and tries again. An ok tick resets the error count.
- **Tokens.** HADES tokens are single use, so a loop needs one per restart. `--token-dir D` holds pre-minted `*.tok` files. A tick that needs a token takes the first one by name. After hades-gate consumes it (`approved`), the file becomes `*.tok.spent`. If hades-gate refuses it for good (`replayed`, `expired`, `forged`, `malformed`, `wrong-scope`, `lifetime`, unreadable), it becomes `*.tok.refused`. A token that only met `no-token`, the cap, a healthy service or a disabled gate stays. Token contents are never printed: the real test checks stdout and the nerve log for every token string and for the `hades1` prefix.
- **Output.** One compact JSON line per tick on stdout (tick, Zagreb `at`, `ms`, exit, outcome, detail, approval code, spawned, token file name, token moved). The final summary is the command result: stop reason, ticks, restarts, outcomes, tokens spent/refused, every sleep in ms, children reaped and still running.
- **Nerve.** `gaia.loop.started` and `gaia.loop.stopped` are posted (best effort). Each tick posts its usual `service.probe` and `gaia.heal.*` events, unchanged.

## Global ledger: where and why there
`gaia_live::heal::global_ledger_path(cfg.ledger)` = `<dir of hades-gate replay ledger>/gaia-restarts.jsonl`.
- The path comes from the same file that holds the admin public key. Changing it means pointing Gaia at a different HADES config, which is an admin act, not an accident of `--state`.
- Gaia already writes the replay ledger in that dir, so no new writable location is needed.
- Each line is written with a single `write` and then `fdatasync`. A corrupt or torn line is an error, so the tick fails closed before any token is consumed (test `a_corrupt_global_ledger_fails_closed`). Repairing it is manual. S3.2 "ledger corrupted/torn" can decide whether to skip a torn last line.
- A restart that sits in both ledgers counts once (same epoch and pid). Old `--state`-only records still count.

## Measured fault behaviour (box, real binaries; docs/real-tests/gaia-tick-loop.md)
| Fault (gaia-dummy mode) | Loop outcome | Tokens consumed | Nerve |
|---|---|---|---|
| hang: accepts, never answers | probe times out (~1 s); each restart fails, because the hung process keeps the port and Gaia never signals it; 3 x apply-failed, then capped | 3 | 3 x `gaia.heal.failed`, then `gaia.heal.capped` |
| flap: each instance exits after 1.2 s | 3 x restarted, then capped every tick | 3 | 3 x `gaia.heal.applied`, ~30 x capped |
| crash: first instance exits once | 1 restart, healthy afterwards (42-44 healthy ticks) | 1 | 1 x applied |
| any of the above with a brand-new `--state` | one-shot: exit 5 capped; a fourth loop: 3/3 capped, 0 restarts | 0 | capped |

## Open points and owner decisions (for Slavik; nothing decided here)
1. **D-L1 Install.** Should a `gaia-heal-loop` unit or timer exist at all, and for which services? Today the apply path refuses everything outside 127.0.0.1:19000-19999, so on the VPS it could only heal test services. A staged unit text is S3.3 (not done here). Any install needs approval (docs/cutover-approval.md).
2. **D-L2 Double budget.** The live nerve on the VPS already has its own restart reflex (`restarts_per_hour_default 3`, cloudflared 1, `max_actions_per_cycle 2`; `/state` 07:14 CEST). If Gaia ever heals live units, a service could get 3 nerve restarts plus 3 Gaia restarts per hour. Options: (a) Gaia reads the nerve's `reflex.budgets` and counts them, (b) the nerve hands live restarts to Gaia, (c) keep them separate and document it.
3. **D-L3 Token supply.** A loop needs one admin-minted token per restart (`--token-dir`), and each expires (max TTL 900 s by default). For an always-on loop, the admin would have to mint ahead or a standing scoped grant would be needed. That changes the HADES model, so it is Slavik's HADES A/B/C call. This branch does not change HADES mode.
4. **D-L4 Hang.** Gaia never kills, so a hung test service is reported (`failed`, then `capped`) but never healed. Healing it would need a stop/kill step in the signed plan, which is outside today's allow-list verbs.
