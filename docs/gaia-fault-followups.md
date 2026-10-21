<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Gaia fault follow-ups (wave-4 slot 199, 2026-10-21)

Base: `swarm/gaia-fault-inject` e970622 (slot 189). Box only; VPS read-only.
Ports used by this slot: 20690-20692 (inside 20690-20699).
This slot fixes two of slot 189's findings in code (the test-port part of
D-FI4, and the bug part of D-FI3). The other findings are written up below
as proposals for the owner, with no behaviour change.

## 1. D-FI4: test ports never include live ports (done)

| Before (e970622) | After (this branch) |
|---|---|
| `TEST_PORTS = 19000..=19999`. POSEIDON's live `127.0.0.1:19120` sat inside the band, so on the VPS the scope guard would have treated a live port as a test port | `TEST_PORTS = 19000..=20999` **minus** `LIVE_PORTS`. `check_target` refuses a live target, and also any `--allow-ports` range that *contains* a live port. So `19000-19999` (which holds 19120) is now refused, and so is `20240-20249` (which holds cloudflared 20241/20242) |
| `gaia-dummy` accepted any `19000..=19999` | `gaia-dummy` uses `gaia_live::heal::is_test_port` (same band, same live list) |
| Wave-4 Gaia tests could not use their own slot bands (20000+), so they borrowed 19600/19750/19890 ports. Slot 175 used 20750-20759, which is **slot 205's** band | Every Gaia test is now inside its own slot band (table below) |
| No check | `crates/gaia-live/tests/port_bands.rs`, 5 tests (see below) |

`LIVE_PORTS` (in `crates/gaia-live/src/heal.rs`, sorted, one comment per port)
holds every VPS listener and every tunnel-ingress origin port from
`docs/ops/vps-baseline.md` (refresh 2026-10-21 11:23 +02:00). It was checked
against a read-only `ss -ltn` at 17:57 and 18:30 +02:00, and against the nerve's
edge finding at 18:30 +02:00, which added `:3333`. A further read-only `ss -ltn` at 18:51 +02:00 added 8673 and 8675 (openmind-nerve instances) and 8674 (gaia-cortex, dry-run). The full list is 22, 25, 53,
3004, 3005, 3010, 3011, 3020, 3333, 4001, 4003, 4004, 4020, 4042, 4060, 4444,
8002, 8003, 8080, 8091, 8420, 8431, 8642, 8646, 8650, 8652, 8653, 8660, 8670,
8671, 8672, 8673, 8674, 8675, 18800 (a transient sshd forward), 19120, 20241 and 20242.

### Where each Gaia test's ports now live

Band rule (from `plans/wave4-tasks.md`): wave 1-3 slot NN uses 19NN0-19NN9, and wave-4
slot N uses 19700+10*(N-100)..+9.

| File | Slot | Band | Ports (old -> new) |
|---|---|---|---|
| `gaia/tests/heal_tick.rs` | 23 | 19230-19239 | allowed 19000-19999 -> 19230-19239; free port 19236-19239 (dropped the 19900-19999 fallback); 19030-19039 parse values -> 19230-19239; wrong-scope 19999 -> 19235 |
| `gaia-live/tests/heal.rs` | 23 | 19230-19239 | 19030-19039 -> 19230-19239 (19240 = "just outside"); new live-port test also uses 20690 (slot 199) |
| `gaia/tests/heal_loop.rs` | 160 | 20300-20309 | 19999 -> 20309, 19600-19609/19601 -> 20300-20309/20301 |
| `gaia/tests/real_gaia_loop.rs` | 160 | 20300-20309 | nerve 20752 -> 20300, dummies 19755-19757 -> 20301-20303 |
| `gaia/tests/heal_decisions.rs` | 175 | 20450-20459 | 19960-19979 -> 20450-20457, 19980-19989 -> 20458-20459, allowed 19000-19999 -> 20450-20459 |
| `gaia/tests/tokens.rs` | 175 | 20450-20459 | 19750-19759/19758 -> 20450-20459/20458 |
| `gaia/tests/real_gaia_decisions.rs` | 175 | 20450-20459 | nerves 20750/20751 -> 20450/20451, dummies 19750-19754 -> 20452-20456 |
| `gaia/tests/real_gaia_faults.rs` | 189 (+199) | 20590-20599, 20690-20699 | nerves 20590/20591 kept, 10 fault services 19890-19899 -> 20592-20599 + 20690-20691, new 20692. Each tick now allows only its own port |
| `real_cli.rs` (01), `real_gaia_live.rs` (23), `real_gaia_poseidon.rs` + `poseidon.rs` (24) | | | unchanged, already in band |

### The guard test (`gaia-live/tests/port_bands.rs`)

1. `LIVE_PORTS` is sorted and has no duplicates, and it holds every baseline port.
2. No port that `is_test_port` accepts is on the live list. The live ports
   inside the band are exactly 19120, 20241 and 20242, and `check_target`
   refuses all 38 live ports.
3. No registered slot band holds a live port. Known collisions: slot 12's
   band holds 19120 and slot 154's holds 20241/20242, so those bands must
   never be given to a Gaia test that binds ports.
4. Every `.rs` file under `crates/gaia/{src,tests}` and
   `crates/gaia-live/{src,tests}` (29 files) must be in its registry. In each
   file's code (comments skipped, strings included), every 5-digit number
   from 18000 to 21999 must be inside that file's slot band(s), or be listed
   as a deliberate value (for example a port that a test proves is refused).
   A live port is only ever allowed as a listed refusal value. A new file
   fails the test until someone registers it.
5. A self-test of the scanner (comments, dates, hex values, suffixes).

Negative controls (18:28 +02:00): adding `"127.0.0.1:19890"` to
`gaia/tests/live.rs` and `19120` to `real_gaia_loop.rs` failed the test with
2 problems. Both edits were reverted.

## 2. D-FI3 bug part: the loop counts an unrecorded restart (done)

Before: if `POST gaia.heal.applied` (or `gaia.heal.failed`) to the nerve
failed after the spawn, `heal_tick` returned `Err` through `?`. The restart
had happened and was in both ledgers, but `--loop` counted the tick only as an
error. So `restarts` stayed 0, `--max-restarts` did not stop the loop, and the
token was not retired as `.spent` (the next tick then got `replayed` and moved it
to `.refused`).

After: the tick returns `Ok(HealTick { exit: 2 (exit::UNRECORDED), spawned:
true, approval_code: "approved", report.nerve_error: "..." })`. The outcome
(`restarted` / `apply-failed`) is unchanged, and the detail ends with
"; result NOT recorded on the nerve". The loop:

* counts the restart (`restarts`, `--max-restarts`);
* moves the token to `.spent` (`tokens_spent`);
* still counts the tick as an error (`errors_total`, consecutive errors,
  back-off, `--max-errors`, `last_error`), and the tick line carries
  `nerve_error` and `consecutive_errors`.

A one-shot `gaia heal-tick` still exits 2, as it did before. It now prints the
JSON report on stdout and the line `gaia heal-tick: restart result not
recorded on the nerve: ...` on stderr. The 3/h cap was never affected: the
global ledger already had the restart.

Tests: hermetic `heal_loop.rs` `a_restart_whose_applied_post_failed_still_counts` and
`unrecorded_restarts_count_toward_max_errors` both fail on the old loop code
(checked 18:28 +02:00) and pass on the new code. The real test
`real_gaia_faults` F7(f) is updated, and the new F7(g) runs the real `--loop`
with `--max-restarts 1` while the applied POST fails. It stops at
`max-restarts` after 1 tick, with restarts 1, errors_total 1, tokens_spent 1,
global ledger 1 and consumed 1.

## 3. Owner decisions (code on this branch)

* **D-199-1: band widened to 19000-20999 minus live ports.** This is needed
  so that wave-4 slot bands (20000+) can host Gaia tests. Options: (a)
  **recommended:** accept; (b) keep 19000-19999 minus live ports, which puts the
  wave-4 tests back on borrowed ports; (c) move the whole band to, for example,
  29000-29999 and keep slot ranges out of it (D-FI4 option b). No money or
  compute cost.
* **D-199-2: `LIVE_PORTS` is a hand-kept constant.** It must be refreshed with
  every VPS baseline (the `:3333` origin appeared between 11:23 and 18:30). Options:
  (a) keep it and add a step to the baseline slot; (b) **recommended before
  any VPS install (D-L1):** also refuse at heal time any port that a systemd
  unit or a non-Gaia process listens on (D-FI4 option a, runtime check).
  (b) is a code change, and this slot did not make it.
* **D-199-3: an unrecorded restart is a restart *and* an error** (section 2).
  Options: accept (recommended), or count it only as a restart (no back-off).
* **Planner note:** slot 12's band (19120-19129) and slot 154's band
  (20240-20249) contain live ports. Never give them to a box test that binds.

## 4. Proposals only (no behaviour change on this branch)

* **D-FI3 token burn.** Today the token is consumed at the approval step. If
  the `/state` re-read under the budget lock fails afterwards, or
  `POST gaia.heal.approved` fails, nothing restarts and the token is gone (F7d,
  F7e). Options: (a) keep it and document it (fail closed, it only costs a token);
  (b) **recommended:** verify the token without consuming it at the approval
  step, then take the budget lock, re-read `/state`, check the cap and only then
  consume it (hades-gate would need a `verify` / `consume` split, and the
  replay ledger stays the single source); (c) (b), plus a
  `gaia.heal.token-burnt` event when consumption happened but the spawn did not.
  Under (b), `POST approved` would move after the consume and before the spawn,
  so a failure there still burns a token (smaller window). No money or compute cost.
* **D-FI2 restarting a service whose port is held.** **Recommended:** before
  spawning, if `listener_pids(port)` is not empty, report `gaia.heal.held`
  with the pids. That spends no token and no budget, exits 8 like hung, and
  restarts nothing. Alternatively, report only for foreign pids.
* **D-FI6 two loops on one service.** **Recommended:** a per-service loop
  lock file `<global-ledger-dir>/loop-<service>.lock` (flock, held for the
  loop's life), so a second loop refuses to start with exit 4. Alternatively, re-probe
  under the budget lock before each spawn (this also covers one-shot ticks).

Token-handling and D-FI2/D-FI6 changes wait for the owner. Nothing here
changes HADES mode, the nerve or any VPS service.
