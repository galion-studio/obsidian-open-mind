<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-shadow-compare (wave-4 slot 352), 2026-10-28 CET (+01:00)

Pre-deploy evidence for **D-333-1** (install the staged `components-live.json` with the
nerve-merge-6 build). Branch `swarm/nerve-shadow-compare` off nerve-merge-6 `9baa450c`,
docs only. No code change, no deploy, no live change.

## Bottom line

Over 10 shadow cycles, the merge-6 build with the staged table gave the **same verdict
and reasons as the live nerve for 12 of 15 components** in every cycle. All 3
differences are expected or come from the test setup. None is a regression:

1. `console`: shadow **degrading** (deep routes `/api/v1/admin` and `/api/v1/services`
   return HTTP 404), live **healthy**. This is the F-313-2 blind spot being closed. A direct
   GET at 04:17 confirms 404 on both routes on :8080 and :18080, so the shadow is right.
   This is the change D-333-2 predicts.
2. `console-rust-primary`: present in the shadow (degrading, same 2 reasons) and missing
   from the live nerve. Live still lists :18080 as unmanaged, while the shadow's
   unmanaged list is `[]` of 20 judged listeners.
3. `cloudflared`: shadow **healthy** (cycles 1-2), then **degrading** "tunnel origin
   errors 85/100 requests in 9 min" (metrics only). Live shows **failing-soon** "96 errors
   in 5 min" (journald). This comes from the setup: `nobody` cannot read journald
   (`probe_errors: ["journald: journalctl exit 1"]`). The live nerve runs as root,
   and so would a deployed merge-6 build, so it would still see the journal.

During all 10 cycles the shadow took **no action**: reflex dry/live 0/0 on every budget,
0 restart lines in its log, escalation off, `actions=dry-run`, `mode=observe-only`.

## Setup

| item | value |
|---|---|
| binary | `openmind-nerve` x86_64 musl release built on the box at `9baa450c`, 1,479,456 B, sha256 prefix `8c9d7a5fa4f84d6d` (byte-identical to slot 333's build) |
| table | `deploy/staged/nerve/components-live.json` (live file plus `deep` on `console` plus the `console-rust-primary` entry; diff checked against live `/etc/galion-nerve/components-live.json` at 04:06, no other drift) |
| where | VPS throwaway `/var/tmp/nerve-352-20261028040630`, data dir owned by `nobody`, run with `setpriv --reuid=nobody --regid=nogroup --clear-groups` under `timeout 1800` |
| listen | 127.0.0.1:22220 |
| env | `NERVE_MODE=event NERVE_INTERVAL=30` (same as live) + `NERVE_REFLEX=0 NERVE_DRY_RUN=1 NERVE_ROLLBACK=0 NERVE_EVT_ESCALATE=off NERVE_EVT_HEARTBEAT=60` (live 900, so 6+ cycles fit in about 10 min) `NERVE_KNOWN_PORTS=25,53,8670,8671,20241,20242,22220`; no Telegram, no Gaia bin, no event log |
| run | started 04:06:53, cycles 1-10 from 04:06:57 to 04:15:57, killed 04:17:20 |
| capture | from the box, every ~20 s: read-only `GET /state` of the shadow :22220 and the live nerve :8670 in the same ssh call; one pair saved per new shadow cycle |

## Per-cycle results

Shadow columns: cycle, ts, cycle_ms, console, console-rust-primary, cloudflared,
unmanaged, score, reflex dry/live. Live columns: live cycle (ts), console, cloudflared,
unmanaged, score.

| # | ts | ms | console | primary | cloudflared | unmanaged | score | dry/live | live cycle | live console | live cloudflared | live unmanaged | live score |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 04:06:57 | 527 | degrading | degrading | healthy | [] | 70 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 2 | 04:07:57 | 545 | degrading | degrading | healthy | [] | 70 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 3 | 04:08:57 | 541 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 4 | 04:09:57 | 538 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 5 | 04:10:57 | 552 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 6 | 04:11:57 | 531 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 7 | 04:12:57 | 483 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 8 | 04:13:57 | 509 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 9 | 04:14:57 | 575 | degrading | degrading | degrading | [] | 68 | 0/0 | 48 (04:00:01) | healthy | failing-soon | [18080] | 64 |
| 10 | 04:15:57 | 601 | degrading | degrading | degrading | [] | 68 | 0/0 | 49 (04:15:01) | healthy | failing-soon | [18080,22220] | 64 |

Same in all 10 cycles, status and reasons (12): openmind-memory, hermes-rs,
link-watch-rs, galion-desk, tuner, galion-one, galion-one-mouth (degrading, HTTP 503),
galion-health, tuner-quick (failing, unit/endpoint not found), voice-worker (absent),
lxc-host, galion-nerve. For both consoles, every cycle had consecutive_down 0 and
restarts_1h 0. They stay degrading and never turn failing, so the restart reflex is never armed.

System: both sides `degraded`, worst `failing`. Shadow score 70 then 68 (after
cloudflared turned degrading), live 64. Shadow counts at cycle 10: healthy 9, degrading 4,
failing 1, absent 1. Live counts: healthy 10, degrading 1, failing-soon 1, failing 1, absent 1.

## Incidents (cycle 10 against live cycle 49)

| shadow | live |
|---|---|
| inc error `edge`: tuner-quick failing (no journal, so no dead-origin evidence) | inc-06ffb32f error `edge`: cloudflared + tuner-quick, tunnel routes 17 hostnames to 10 closed origin ports (journald) |
| inc warn `console`: console-rust-primary degrading (deep 404 x2), **new after deploy** | — |
| inc warn `galion-one`: mouth degrading HTTP 503 | inc-865945dd error `galion-one`: mouth "stuck loading for 10.0 h" (needs the history that the shadow's fresh data dir does not have) |

A merge-6 deploy should keep the live incident IDs and history in
`/var/lib/galion-nerve` and add **one new warn incident** for the console. Telegram is
not configured on the host ("none configured" in live `/state`), so it pages nobody.

## Side effects and cleanup

- **Live nerve saw the shadow.** Live cycle 49 (04:15:01) listed `unmanaged [18080,22220]`.
  :22220 is the shadow process itself, a read-only observation that should clear at the
  next live cycle after 04:17. Slot 333's throwaway did not overlap a live
  cycle. Future shadows will hit this too unless they avoid live heartbeat times
  (:00/:15/:30/:45) or the owner adds 22220-22229 to the live `NERVE_KNOWN_PORTS` (not done).
- The shadow only made read-only HTTP GETs to live health and deep routes, plus `systemctl show`
  and `list-units` reads. `journalctl` was refused for `nobody`.
- Cleanup 04:17:20-04:17:40: process killed, no `nobody` nerve or `timeout` process left,
  folder `/var/tmp/nerve-352-*` removed, ports 22220-22229 closed.
- Live units afterwards: galion-nerve, console, console-rust-primary all `active`.
  Live nerve `9d1a4f3` was not touched.

## Other observation

- Live edge incident grew from 6 closed origin ports (03:30) to **10** at 04:15
  (:3010 :3011 :3015 :3020 :4001 :4003 :4004 :4020 :4060 :18800). This is out of scope
  here (cloudflared config/DNS is an owner decision), but `:18800` is new on that list.

## Owner decisions

- **D-352-1 (feeds D-333-1)**: the evidence supports installing the staged
  `components-live.json` with the merge-6 build. Expected visible change: console turns
  `degrading`, one new warn incident, :18080 no longer unmanaged, score drops by about 4
  (0 restarts and no actions follow from this). Options: (a) install as staged;
  (b) install without `deep` (quiet, keeps the blind spot); (c) wait for the board
  API/admin store (D-303-1/D-313-1) first.
- **D-352-2**: whether to add the shadow port range to live `NERVE_KNOWN_PORTS` so
  future pre-deploy shadows do not show up as stray listeners (rec: no; time shadows
  between heartbeats instead).
- **D-352-3**: a full-fidelity shadow (journald too) would need a temp user in
  `systemd-journal`, which is a VPS permission change. Not done. Rec: not needed for D-333-1, since the staged table changes only console entries; the cloudflared journal path was not re-verified here
  (it runs as root live, as before).

## Gates

Docs only: `scripts/no-python.sh` and `scripts/spdx-scan.sh` (see commit). No Rust changed.
Hermes not used. Raw captures (20 JSON pairs) are kept on the box in
`/workspace/swarm/logs/nerve-shadow-compare/` and not committed (they contain public
hostnames and incident text).
