<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Gaia S3.1: supervised `gaia heal-tick --loop` and the global restart ledger

Branch `swarm/gaia-tick-loop` (wave-4 slot 160), based on integration-3 625eaf0. Written 2026-10-21 CEST (+02:00).
Scope: **box test services only**. Nothing here is installed on the VPS, and no unit or timer was written or enabled.
Update 2026-10-21 (slot 175, `swarm/gaia-loop-decisions`): owner decisions D-L1..D-L4 were **accepted 2026-10-21** and are implemented; see "Owner decisions D-L1..D-L4: accepted 2026-10-21" below.

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
| hang: accepts, never answers | (slot 160) probe times out (~1 s); each restart failed because the hung process keeps the port; 3 x apply-failed, then capped. **Since slot 175 (D-L4):** confirmed by a second probe, reported, never restarted: outcome `hung` (exit 8) every tick | 160: 3; **175: 0** | 160: 3 x `gaia.heal.failed` + capped; **175: `gaia.heal.hung` per tick (listener pid, both probe details), no failed/capped** |
| flap: each instance exits after 1.2 s | 3 x restarted, then capped every tick | 3 | 3 x `gaia.heal.applied`, ~30 x capped |
| crash: first instance exits once | 1 restart, healthy afterwards (42-44 healthy ticks) | 1 | 1 x applied |
| any of the above with a brand-new `--state` | one-shot: exit 5 capped; a fourth loop: 3/3 capped, 0 restarts | 0 | capped |

## Owner decisions D-L1..D-L4: accepted 2026-10-21

Mate accepted Grok Bot's recommendations for slot 160 on 2026-10-21 (08:15:59 CEST +02:00, logged in CALL-RECORD.md). Slot 175 (`swarm/gaia-loop-decisions`, based on `swarm/gaia-tick-loop` e4631a6) implements them. Evidence: docs/real-tests/gaia-loop-decisions.md.

| # | Question (slot 160) | Accepted 2026-10-21 | Implemented (slot 175) |
|---|---|---|---|
| D-L1 | Install a heal-loop unit or timer on the VPS? | **No, not yet.** Revisit after D-L2/D-L3 are in and the cortex is live. | Nothing installed. No unit, timer or binary written to the VPS. The VPS was read-only probed (nerve `/state`); `gaia*` units/timers: none. |
| D-L2 | Nerve reflex (3/h) + Gaia (3/h) = 6/h per service? | **Gaia counts the nerve's restarts**, so the combined total stays at 3 per service per rolling hour. | Every tick that could restart reads the nerve's `GET /state` (read-only) twice: before the token is consumed and again under the budget lock. See "D-L2" below. |
| D-L3 | Token supply for an always-on loop? | **Small batches of short-lived, single-use, pre-made hades-gate tokens.** No standing grant, no HADES mode change. | `gaia tokens request / mint / status`; the loop moves expired tokens aside. See "D-L3" below. |
| D-L4 | Hung service (holds the port, never answers)? | **Report-only.** Hand it to the nerve/operator with the evidence; never kill or stop it. | Outcome `hung` (exit 8) and a `gaia.heal.hung` nerve event; no restart, no kill, no token. See "D-L4" below. |

### D-L2: combined Gaia + nerve restart cap
- **Source.** The nerve's `/state` (live galion-nerve on the VPS and the Rust `openmind-nerve serve` answer the same shape). For the target, Gaia takes the component whose `name` is the service, or whose `unit` is the service or `<service>.service`. The nerve count is `max(components[].restarts_1h, reflex.budgets[unit].live_1h)`. Both fields describe the same restarts, so they are not added. `restarts_1h` also includes restarts the reflex did not make (for example a deploy or an operator restart, which shows up as `restarts_1h: 1` with `live_1h: 0`). Gaia counts those too. `dry_1h` (dry-run reflex) is not a restart. `n_restarts_systemd` is systemd's own counter since unit start, not per hour, so it is not used.
- **Rule.** `heal::cap_check_combined(gaia_history, nerve_count, now)` allows a restart only if Gaia's restarts in the last hour plus the nerve's count is less than 3. The nerve gives a count, not times, so the retry hint is an upper bound (when the nerve alone is at 3, it is the full window). Every tick re-reads the nerve, so the cap lifts as soon as the nerve's count drops. The real test measured that: nerve 2 gave 1 Gaia restart and then capped; nerve set to 0 gave 2 more within about 3 s, then capped at 3 Gaia + 0.
- **Fail closed.** If the nerve cannot be reached, `/state` is not a JSON object, has no `components` list, or a matched row's count is not a non-negative integer, the tick is a setup error (exit 2; in the loop, back-off). This happens before any token is consumed. The re-read under the lock happens after consumption. If that re-read fails, the token is spent and nothing restarts, which is safe. The loop then marks the token `refused` on its replay. A disabled gate and admin-safety refusals still come first and do not need the nerve.
- **Unwatched services.** A service the nerve does not list counts as 0 (every box test service today).
- **Read-only view.** `gaia nerve-restarts (--nerve 127.0.0.1:N | --state-file F) --service S ...` shows the nerve count and what is left for Gaia.
- **Events.** `gaia.heal.capped` / `applied` / `failed` and the report's `cap` step now carry `gaia_restarts_last_hour`, `nerve_restarts_last_hour` and the matched nerve row (`component`, `unit`, `restarts_1h`, `reflex_live_1h`, `cycle`).

### D-L3: small batches of short-lived single-use tokens
```
gaia tokens request --token-dir D [--count 3] [--ttl-s 900] --service S --port P --allow-ports A-B -- CMD   # gate host, no key
gaia tokens mint --request D/req-....request.json --seed ADMIN_SEED                                         # ADMIN side only
gaia tokens status --token-dir D [--hades-config C] [--prune]                                               # gate host
```
- **Batch** 1-3 (default 3). 3 is the combined hourly cap, so more tokens could never be used inside one window. **Lifetime** 1-3600 s (default 900 = hades-gate's default `max_ttl_secs`; 3600 = its hard ceiling).
- **No stockpiling.** `request` and `mint` both refuse when the dir's live tokens (unexpired `*.tok`; unreadable ones count) plus the batch would be more than 3. Spent, refused and expired files do not count.
- **Request file** (`req-<unix>-<random>.request.json` + `.plan`): action `heal-apply`, target, exact plan bytes and SHA-256, count, TTL, Zagreb `created`, `standing_grant: false`, `hades_mode_change: false`. `mint` refuses a request that is edited (plan hash mismatch, batch/TTL out of range, another action, a grant or mode-change flag, a plan path with `/`), older than 1 h, or already minted (`.minted` marker).
- **Minting** is the same Ed25519 code as `hades-gate mint`. Files are `<id>-<i>.tok`, mode 0600, never overwritten. The admin seed is only read on the admin side. The gate host keeps only the public key, as before. No token string is printed by any command, or sent to the nerve.
- **Loop.** Before each tick, `tokens::next_live` renames tokens whose signed `exp` has passed to `*.tok.expired`, so hades-gate never sees them and the replay ledger is untouched. The summary has `tokens_expired`. `status` shows each token's state (ready/expired/spent/refused/malformed), expiry and remaining seconds (Zagreb). With `--hades-config` it also shows whether the signature verifies and whether the nonce is already in the replay ledger (read-only; it never creates the ledger).
- HADES is unchanged: same approver, same disable switch, same admin-safety allow-list, same per-plan single-use rule.

### D-L4: hung services are report-only
- **Detection.** The health probe connected and sent the GET, but the read (or write) timed out (`read: operation would block` / `timed out` / deadline). Nothing listening (`connect: connection refused`) is "down", not hung. A second probe 300 ms later must agree: if it answers, the tick is `healthy`; if the port is now closed, the normal heal path runs.
- **Action.** Post `gaia.heal.hung` (severity `error`) with the service, port, probe id, both probe details, the read timeout, `listener_pids` (from `/proc/net/tcp{,6}` + `/proc/<pid>/fd`, read-only), `gaia_started_pids` (which of those Gaia restarted itself, from its ledgers), `action: report-only`, `restart/kill/stop: false`, `token_consumed: false` and `handed_to: nerve/operator`. Then stop: exit 8, outcome `hung`. No token is read. The restart budget is not touched.
- The loop reports it every tick (one event per interval). It never escalates. Freeing the port is an operator act.

### Remaining owner points (not decided here)
1. **D-L2b, per-unit nerve limits.** The nerve's own limit is lower for some units: `cloudflared_per_hour: 1`, and `reflex_restarts_per_hour: 0` for units it never restarts (tuner-quick, galion-one-mouth). The combined cap here is a flat 3. The 08:44 CEST VPS read shows `cloudflared` with `restarts_1h: 1`, so Gaia would see 2 left. Today this cannot matter, because Gaia's apply path refuses everything outside 127.0.0.1:19000-19999. Before Gaia ever heals live units, decide whether the cap per unit should be `min(3, reflex_restarts_per_hour)` (and what 0 means).
2. **D-L1 revisit trigger.** D-L2 and D-L3 are now implemented, but the cortex is still not live (`/api/v1/cortex` 404 on the VPS at 08:41 CEST), so D-L1 stays "no install".
3. **Token custody.** `gaia tokens mint` must run where the admin seed lives (not the VPS). Who mints, from which machine, and how the batch reaches the token dir are still open, and still admin acts.
