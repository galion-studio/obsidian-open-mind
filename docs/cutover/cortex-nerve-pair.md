<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Runbook: cortex 58a12e61 + nerve 9baa450c deploy pair (D-341-1 + D-333-1)

Wave-4 slot 357, 2026-10-28 CET (+01:00). **Staged only. Nothing here has been
installed, reloaded or restarted.** The deploy is an owner/parent step. Evidence:
`docs/real-tests/cortex-nerve-pair-runbook.md`. Machine-checked source of truth:
`deploy/staged/cortex-nerve-pair/pair.json`, checked by
`crates/gaia-cortex/tests/pair_preflight.rs`.

## What changes

| | live now (rollback target) | candidate |
|---|---|---|
| cortex | `gaia-cortex-73583ab-llm` (cortex-live-merge-3, sha256 `3cdab111…6af5`, 1,136,200 B) via `90-deploy-t273u.conf` | `gaia-cortex-58a12e6-llm` (cortex-live-merge-4 58a12e61, `--features llm-advisor`, sha256 `7ca6512b…49a2`, 1,147,680 B) via staged `91-pair-58a12e6.conf` |
| nerve | `openmind-nerve-9d1a4f3` (sha256 `d0c70b87…1d45`, 1,458,760 B) via `90-deploy-t273u.conf` | `openmind-nerve-9baa450` (nerve-merge-6 9baa450c = merge-5 + console deep health 38750655 + planned-stop tag 12956b45, sha256 `8c9d7a5f…93da`, 1,479,456 B) via staged `91-pair-9baa450.conf` |
| nerve table | `/etc/galion-nerve/components-live.json` 12 entries, sha256 `dc343493…e05c`, 2,669 B, unchanged since 2026-10-21 21:56 CET | staged `components-live.json` 13 entries, sha256 `283db8c3…8df4`: live + `deep` on `console` + alert-only `console-rust-primary` (:18080) |

Both staged drop-ins only reset and set `ExecStart=` (plus `Description=`).
Env, users, sandboxing, live actions, LLM advisor, Hermes handoff, guards and
event mode all stay in the existing 50-90 drop-ins. Because `91-*` sorts after
`90-deploy-t273u.conf`, removing the `91-*` file is the whole binary rollback.

Expected visible change (D-333-2 / D-352-1): nerve `console` turns `degrading`
(deep routes 404 until D-303-1 / D-313-1 land), one new warn incident
`console`, `:18080` leaves the unmanaged list, nerve score about -4. No restarts
follow from it (every `restarts_per_hour` is 0). Cortex: a planned restart of a
unit is `R5b-planned-stop` with no restart request (the live 2026-10-27 19:28:35
console false positive goes away); real crashes stay `R3-known-crash`.

## Order and why

**Cortex first, then nerve.** Rollback in reverse (nerve, then cortex). Every
intermediate state was rehearsed on real binaries (`deploy_order_rehearsal`):

| state | planned restart | crash | note |
|---|---|---|---|
| live 73583ab + 9d1a4f3 | R3 + 1 restart request (the bug) | R3 + 1 | today |
| cortex 58a12e6 + nerve 9d1a4f3 | **R5b, 0 requests** (via job markers / exit 143) | R3 + 1 | safe to stop here |
| cortex 58a12e6 + nerve 9baa450 | **R5b, 0 requests** (nerve `unit.planned_stop` tag) | R3 + 1 | target |
| nerve rolled back | R5b, 0 requests | — | |
| cortex rolled back | R3 + 1 (bug back) | R3 + 1 | = today |

Nerve first would be safe too (tagged nerve + 73583ab gives R5/R99, no
restarts, slot 328/357 `real_planned_stop_pair`), but it loses the R5b label
until the cortex follows, so cortex first is the cleaner order. The cortex
re-subscribed 0.9 s after a nerve restart and 1.4 s after its own restart in
every rehearsal run; open incidents survived each binary swap in both
directions (state dir compatible forwards and backwards).

## Pre-flight (box; before touching the VPS)

1. `GALION_REAL=1 cargo test --release -p gaia-cortex --test pair_preflight -- --nocapture --test-threads 1`
   (artifacts in `/workspace/swarm/artifacts`, or `GALION_PAIR_ARTIFACTS=<dir>`), must be 5/5:
   manifest ↔ drop-ins ↔ components table; size, sha256, static x86_64 ELF,
   `--version`, build markers of all 4 binaries; 5-step order rehearsal.
2. Optional deeper pair check: `REAL_CORTEX_PORT_BASE=22270 GALION_REAL=1
   GALION_NERVE_BIN=…/openmind-nerve-9baa450-x86_64-musl GALION_NERVE_BASE_BIN=…9d1a4f3…
   GALION_CORTEX_BASE_BIN=…73583ab-llm… cargo test --release -p gaia-cortex --features llm-advisor --test real_planned_stop_pair`.
3. `--version` prints only `0.1.0` for both crates (F-357-1): the **sha256 is the
   only build identity**. Never install a binary whose sha256 is not in `pair.json`.
   The non-llm cortex build is rejected by the marker check (`key has unusable characters`
   exists only with `llm-advisor`; checked against the VPS `gaia-cortex-73583ab`, 0 hits).

## Deploy (owner/parent only; VPS as root; times CET)

VPS clock is UTC: `systemctl`/`journalctl` times read 1 h behind CET.

0. Record the "before": `systemctl show gaia-cortex galion-nerve -p MainPID,NRestarts,ExecMainStartTimestamp`,
   `curl -s 127.0.0.1:8674/health`, `curl -s 127.0.0.1:8670/health`,
   `curl -s '127.0.0.1:8674/decisions?n=500'` rule histogram.
1. Backups (one timestamp `TS`):
   `cp -a /etc/systemd/system/gaia-cortex.service.d /var/backups/gaia-cortex.service-$TS.d`,
   `cp -a /etc/systemd/system/galion-nerve.service.d /var/backups/galion-nerve.service-$TS.d`,
   `cp -a /etc/galion-nerve/components-live.json /var/backups/components-live.json-$TS`.
2. Binaries: copy both artifacts to the VPS (e.g. `/var/tmp`), then
   `install -m 0755 -o root -g root <file> /opt/galion/rust/bin/gaia-cortex-58a12e6-llm` and
   `… /opt/galion/rust/bin/openmind-nerve-9baa450`; `sha256sum` both = `pair.json`.
   Keep `gaia-cortex-73583ab-llm` and `openmind-nerve-9d1a4f3` (rollback).
3. Unit verify, before installing any drop-in (no reload, no live change):
   copy the live fragment + live drop-ins of each unit plus the staged `91-*` into a
   throwaway `/var/tmp/pair-verify-$TS/<unit>.service{,.d/}` and run
   `SYSTEMD_UNIT_PATH=/var/tmp/pair-verify-$TS: systemd-analyze verify --man=no /var/tmp/pair-verify-$TS/<unit>.service`.
   Must be rc 0 for both. (Before step 2 it fails with
   `Command /opt/galion/rust/bin/… is not executable`, which also proves the 91
   `ExecStart` is the effective one.) Remove the throwaway folder.
4. **Cortex**: `install -m 0644 91-pair-58a12e6.conf /etc/systemd/system/gaia-cortex.service.d/`,
   `systemctl daemon-reload`, `systemctl restart gaia-cortex`.
   Gates (all within 60 s): `is-active` active; `systemctl show -p ExecStart` names
   `gaia-cortex-58a12e6-llm`; NRestarts 0; `/health` `ok:true mode:live llm_enabled:true
   hermes_handoff:true busy:false nerve:true`; `journalctl -u gaia-cortex --since -2min`
   shows no panic/`error`. Fail → rollback cortex (below). Wait 2 min.
5. **Nerve**: `install -m 0644 -o root -g root components-live.json /etc/galion-nerve/components-live.json`
   (staged sha256 `283db8c3…`), `install -m 0644 91-pair-9baa450.conf
   /etc/systemd/system/galion-nerve.service.d/`, `systemctl daemon-reload`,
   `systemctl restart galion-nerve`.
   Gates: active, ExecStart names `openmind-nerve-9baa450`, NRestarts 0; `/health`
   `status:ok`; cortex `/health` `nerve:true` again within 30 s (box: 0.9 s) and
   nerve `/stats` `streams` 1; `/state`: `unmanaged_ports` `[]`, `console` and
   `console-rust-primary` `degrading` with the two `deep route … HTTP 404` reasons and
   `consecutive_down` 0; `cloudflared` judged from journald as before (the nerve runs as root).
   Fail → rollback nerve.
6. Watch one heartbeat (15 min): `/decisions` has no new `R3` on a unit with a stop
   job; nerve `/actions` has no new restart; incident ids and history in
   `/var/lib/galion-nerve` kept, plus at most one new warn `console` incident.
   The parent's live-fix watch should note the deploy time.

## Rollback (each half on its own; reverse order)

- Nerve: `rm /etc/systemd/system/galion-nerve.service.d/91-pair-9baa450.conf`;
  `cp -a /var/backups/components-live.json-$TS /etc/galion-nerve/components-live.json`
  (sha256 back to `dc343493…`); `systemctl daemon-reload`; `systemctl restart galion-nerve`
  → `openmind-nerve-9d1a4f3` via `90-deploy-t273u.conf`.
- Cortex: `rm /etc/systemd/system/gaia-cortex.service.d/91-pair-58a12e6.conf`;
  `systemctl daemon-reload`; `systemctl restart gaia-cortex` → `gaia-cortex-73583ab-llm`.
- After rollback the console planned-restart false positive (R3 + restart request)
  is back by design (rehearsal step 4). Rollback gates = the step 4/5 gates with the
  old names.

Rollback triggers: unit not active within 60 s or NRestarts > 0; cortex `/health`
not ok or `nerve:false` for > 60 s after the nerve step; any restart request for a
unit that had a stop job; any panic in the journal.

## Not covered / owner decisions

- D-341-1 (cortex) and D-333-1 / D-352-1 (nerve table, option a/b/c) stay owner
  decisions. With D-352-1 option (b) drop the two `deep` arrays from the staged table
  first (and update `pair.json` sha; the preflight test then needs its `deep`
  assertions changed) — not prepared here.
- Still open from merge-3: D-245-1, D-245-2, D-247-1..5 (unchanged by this pair).
- F-357-1: no commit id in `--version` of either binary (sha256 is the identity).
  Cortex side fixed on `swarm/cortex-version-commit` (slot 377): `--version` and
  `/health` carry commit + Zagreb commit date; NOT in the staged 58a12e61 pair binary.
- F-357-2: `docs/merges/nerve-merge-6.md` says the live table has 13 entries; the
  live file has **12** (read 2026-10-28 05:2x CET), staged has 13.
