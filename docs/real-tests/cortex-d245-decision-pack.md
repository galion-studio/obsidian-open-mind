<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 272 cortex-d245-decision-pack: D-245-1 / D-245-2

Branch `swarm/cortex-d245-pack`, base `swarm/cortex-live-merge-3` `73583abf`.
All times Europe/Zagreb (CET +01:00), 2026-10-27. **Docs only. NO deploy.**

## Decisions

| ID | Question | Recommendation |
|---|---|---|
| **D-245-1** | Live config pins `restarts_per_unit_hour: 3`; merge-2/3 default is **2** (nerve executor contract, D-235-1). Which? | **Adopt 2.** Remove the pin or set `"restarts_per_unit_hour": 2` in `/etc/gaia-cortex/config.json` before cutover. Keep total cap 6/h. |
| **D-245-2** | First-connect backfill of open nerve incidents fires R11 alerts (critical → escalate + Hermes handoff, capped 6/h). Accept the burst, or make backfill silent? | **Keep alerting backfill** (current merge-2/3 behaviour). Expect a one-off burst; **0 actions**. See numbers below. |

Owner still decides; this pack is evidence + a clear recommend.

## Method (box, ports 21420-21429)

1. VPS read-only GET (17:46-17:47 CET): nerve `/incidents`, cortex `:8674` `/health`+`/stats`, config pin, running binary `gaia-cortex-bd6fbe6`.
2. Saved capture: `docs/real-tests/captures/vps-incidents-20261027-1747.json` (5 open: 1 critical, 2 error, 2 warn).
3. Historical upper-bound capture (slot 245 shape, 9 distinct causes): `captures/vps-incidents-historical-9.json`.
4. Std-only stub nerve on the box served each capture (`GET /stats`, `GET /incidents`, long-poll `/events/stream`).
5. Replayed against:
   - **merge-2** binary `/workspace/swarm/artifacts/gaia-cortex-1305dec-x86_64-musl` (merge-3 `73583abf` keeps the same backfill + R11 path; io-unlock/R4b do not change these counts).
   - **live** binary `gaia-cortex-bd6fbe6` (copied read-only from the VPS).
6. Cortex: dry-run actions, LLM off, Hermes handoff on (`per_hour=6`), `repeat_poll_s=0`. Measured `/stats`, `alerts.jsonl` / `escalations.jsonl`, Hermes inbox files.
7. Raw numbers: `docs/real-tests/captures/d245-measurements.json`.

## Live VPS gap (read-only, 17:47 CET)

| | Live cortex `bd6fbe6` :8674 | Nerve :8670 |
|---|---|---|
| Open incidents | **1–2** (process view) | **5** (capture) |
| Backfill on connect | **none** (no `GET /incidents`) | n/a |
| Config `restarts_per_unit_hour` | **3** (pinned) | executor cap **2** |
| Hermes handoff | on (18 handoffs lifetime) | — |
| Actions executed (lifetime) | 1 | — |

This is the slot-237 gap: the live cortex never learns nerve incidents opened before it subscribed.

## D-245-2 measurements (first-connect backfill)

| Run | Binary | Capture | Backfilled | R11 alerts | Escalations | Hermes handoffs (inbox) | Actions |
|---|---|---|---|---|---|---|---|
| merge2_live5_cap2 | 1305dec | live 5 open | **5** | **5** | **1** | **1** | **0** |
| bd6_live5_cap3 | bd6fbe6 | live 5 open | **0** (no GET /incidents) | **0** | **0** | **0** | **0** |
| merge2_hist9_cap2 | 1305dec | historical 9 | **9** | **9** | **3** | **3** | **0** |
| bd6_hist9_cap3 | bd6fbe6 | historical 9 | **0** | **0** | **0** | **0** | **0** |
| merge2_live5_cap3 | 1305dec | live 5, cap=3 | **5** | **5** | **1** | **1** | **0** |

Per-decision (live 5, merge-2): every open cause → `R11-nerve-incident`, `action: null`, reason contains `backfill: opened before the cortex was listening`. Critical `ddos:edge-cloudflared` also `escalate: true` → one Hermes inbox file. Warn/error → alert only.

Notes:

- Backfill is keyed by **cause** (max one synthetic open per cause per connect). Duplicate same-cause rows in a snapshot collapse.
- Caps (`restarts_per_unit_hour` 2 vs 3) do **not** change the backfill burst (R11 never requests a restart).
- Hermes handoffs for the burst stay inside `CORTEX_HERMES_PER_HOUR=6` (3 criticals in the hist-9 run → 3 handoffs).
- Stub returned 404 on cortex `POST /event` (alert courier path); local `alerts.jsonl` still recorded every alert. On the real nerve those posts land as today.
- merge-3 `73583abf` = merge-2 + io-unlock + R4b-sandbox-oom; backfill/R11 counts identical to 1305dec for this decision.

### D-245-2 options

| Option | Effect | When to pick |
|---|---|---|
| **A. Keep R11 on backfill (recommend)** | One-off N alerts + up to min(criticals, 6) Hermes handoffs on first connect after deploy; closes the live gap immediately | Default; operators see what the nerve already knows |
| B. Silent backfill (record only, no alert/escalate) | Cortex incidents catch up with 0 alert noise; Hermes does not learn criticals until a new stream event | Only if Mate wants a quiet cutover and will watch `/stats.nerve_incidents_backfilled` by hand |
| C. Defer deploy until nerve open count is low | Smaller burst | Optional timing; not required for safety (0 actions either way) |

**Recommend A.** The burst is bounded, action-free, and is the fix for the gap live shows today (cortex 1–2 open vs nerve 5).

## D-245-1 measurements (caps 3 vs 2)

Backfill runs above with `per_unit_hour` 2 and 3 produced the **same** alert/handoff counts (expected: R11 is not cap-gated).

Restart-cap behaviour (from merge-3 `real_flap_dedupe`, 17:43 CET, ports 21270-21279, default cap **2**):

- Dry-run and live: tuner ring count 3 → cortex rules `[R3-known-crash, R3-known-crash, R3-cap]`; nerve saw **2** restart requests.
- Total hour: 6 restarts then `R3-cap-total`.
- With live pin **3**, the third per-unit restart would still be asked before `R3-cap` (one extra restart vs the nerve executor's own 2/unit/h).

Live VPS config still has `"restarts_per_unit_hour": 3` (read 17:46 CET). Drop-in keeps `CORTEX_ACTIONS=live`. Merge-2/3 default comes from `nerve_cortex_contract::NERVE_RESTARTS_PER_UNIT_HOUR` (= 2).

### D-245-1 options

| Option | Effect | When to pick |
|---|---|---|
| **A. Cap 2 (recommend)** | Cortex matches nerve executor; dry-run loops cannot ask a 3rd restart the nerve would refuse anyway | Align with D-235-1 / flap-dedupe contract |
| B. Keep pin 3 | Cortex may request one more restart per unit per hour than the nerve will execute; live pin wins over the binary default | Only if Mate wants a looser cortex-side allowance on purpose |

**Recommend A.** Set config to 2 (or delete the key so the binary default applies). Total 6/h unchanged.

## Deploy checklist delta (for whoever deploys merge-2/3 later; this slot does not)

1. Before cutover: set `restarts_per_unit_hour` to **2** (D-245-1 A) in `/etc/gaia-cortex/config.json`.
2. Expect first-connect backfill ≈ current nerve open count (today **5** R11 alerts, **1** Hermes handoff if one critical is open). Hist-9 shape: 9 / 3.
3. Watch after restart: `/stats.nerve_incidents_backfilled` ≈ nerve open causes; `actions_executed` not spiked by the burst; `hermes_handoff.sent_last_hour` ≤ 6; `caps.per_unit_hour` = 2.
4. Rollback unchanged: `ExecStart=` → `gaia-cortex-bd6fbe6`.

## Artifacts

- `docs/real-tests/captures/vps-incidents-20261027-1747.json`
- `docs/real-tests/captures/vps-incidents-historical-9.json`
- `docs/real-tests/captures/d245-measurements.json`
- `docs/decisions/D-245.md` (short owner form)

## Not done / out of scope

- No live cortex/console/nerve deploy, no CF DNS/HADES, no force-push.
- merge-3 musl binary not rebuilt (backfill path identical to 1305dec for these decisions).
- Silent-backfill code path (option B) not implemented; would be a follow-up if the owner picks B.
