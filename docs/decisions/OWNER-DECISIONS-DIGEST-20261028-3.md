<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Owner decisions digest 3 (for Slavik and Mate)

Prepared 2026-10-28 18:05 CET (+01:00) by wave-4 slot 418. Docs only. It follows digest 2
(`swarm/owner-decisions-digest-2` @ 5ff76fad, 02:35 CET) and the publish path file
(`docs/release/PUBLISH-PATH-20261028.md`, slot 392, 15:40 CET).
Sources: the DONE and decision lines in `/workspace/swarm/CALL-RECORD.md` up to 18:02 CET, each slot's branch docs,
the cutover report `/workspace/swarm/reports/core-stack-live-20261028.md`, and fresh read-only checks made for
this file at 18:03 CET (`docs/real-tests/owner-decisions-digest-3.md`). Nothing was applied, deployed or merged by this slot.

## 0. The short version
- **Since 16:05 (Mate t332u) and 17:27 (Mate t334u) a lot is closed:** link.env is now 600, the 7 local
  branches are pushed (403 too, 17:53), the core stack is live on the new binaries (console 5aa31927,
  cortex 58a12e6, nerve 9baa450, 17:30-17:41 CET, no rollback), and 373 + 374 are landed on
  `swarm/main-staging` (head 3250d07d, 17:58 CET).
- **Left for the publish:** PR #12 CI on 3250d07d (pending at 18:03), then the PR #12 merge (step 5), and the
  restore test swap D-361-1 (the last open security item).
- **The rest below is polish or after-publish.** One line each, with my recommendation.

## 1. Resolved (no action needed)

| ID | Decision | When / by | Evidence now (18:03 CET) |
|---|---|---|---|
| D-391-1 | Accept 202887d5 as the live `/opt/galion/openmind` HEAD | 16:05, Mate t332u | Recorded in CALL-RECORD. |
| D-391-2 | Ban git writes in `/opt/galion/openmind` | 16:05, Mate t332u | Guard being staged by slot 419 (openmind-write-guard). |
| D-393-1 | Stamp the nerve at the next deploy, keep the staged pair unstamped | 16:05, Mate t332u | Next nerve deploy after 9baa450 carries the stamp. |
| D-318-2 | chmod 600 `link.env` | applied 16:05 | `stat`: **600 root:root** (was 666). Top security risk closed. |
| D-396-1 / D-343-1 | Deploy console candidate 5aa31927 (:18080 first, then :8080) | 17:30 / 17:34, t334u | Both units on `galion-console-serve-5aa31927`, NRestarts 0; `/api/v1/admin` 200, `/console` 200 (were 404 / 503). |
| D-396-2 | Nerve deep console health in this nerve deploy | 17:40, t334u | Nerve /state: console + console-rust-primary healthy. |
| D-341-1 / D-333-1 | Cortex 58a12e61 then nerve 9baa450c (pair runbook) | 17:37 / 17:40, t334u | `gaia-cortex-58a12e6-llm` + `openmind-nerve-9baa450`, NRestarts 0, components 283db8c3. |
| D-373-1 / D-373-2 | Land order 373 then 374, carry F-373-1 | 17:58, t334u | main-staging: ef99947c (merge 83e573e2), b4fe2383 (merge d50478db), then 3 CI fixes 811797a9, 09fb719c, 3250d07d. |
| D-374-1 | Keep heal_loop slack 5d03a5ce | 17:58, t334u | Inside d50478db, landed. |
| D-386-1 | Commit the x86_64 musl `rust-lld` line | via 373 land | The line is in the 373 tree now on main-staging; drop 386's duplicate comment when its test batch lands (slot 415). |
| D-388-4 | Correct slot 367 "last write Oct 13" | slot 406, 17:39 | Fixed in `memory-search-local-time.md` (7aba14ec). |
| B11 push | 7 local-only branches + 403 | 15:49, 16:05, 17:53 | All on GitHub (385 8f40f18e, 334/346/347/349/351/369, 403 8887a0cb). |

## 2. Still open (one line each, recommendation in bold)

**Publish path**
- **PR #12 merge** (step 5): main-staging 3250d07d -> main once CI is green (combined status *pending*, 0 statuses at 18:03). **Merge when green, never force-push.**
- **D-361-1** restore test swap: live `/opt/galion/rust/bin/galion-restore-test.sh` is still the 840 B .sh that says "ok" after a failed checksum (no `.pre361`, no symlink). **Yes: symlink to galion-backup-dd89bea4, keep the .sh as .pre361** (361 2b24e88b; ~5 min, no restart).
- **D-373-3** hermes-gateway deploy stays its own window (live hermes-rs still `hermes-gateway-b7fbbc78`). **Yes, separate window** (slot 416 is building that candidate now).

**Console**
- **D-396-3** reword the gate 503 text (deferred at cutover). **Yes, low priority** (slot 410 console-gate-wording is on it).
- **D-395** cut the console over to option A (820dfaea `--owner-routes --pulse --hermes`). **Not yet:** wait for slot 413 console-deploy-candidate-2, which folds 820dfaea into the live 5aa31927 line.
- F-395-1 `SPA_FIRST` empty after merge: **no decision**, test already updated.
- F-395-2 `/api/arena` needs `--arena-root`: **resolved live** (the 17:30 drop-ins pass `--arena-root`, arena 200 locally); keep it in 413's drop-ins.
- F-395-3 `/api/v1/bank` still in `DATA_DEFAULT_PATHS`: **keep bank retired (D-384-2) and drop it from the list** in 413.
- F-395-4 forward log says `->8080`: **cosmetic, fix in 413.**
- **D-389-1** survival refresh writer: **yes, in-console refresh** (staged drop-in on console-rust-primary, interval 900 s, `ReadWritePaths=-/opt/galion/galion-light/state`), needs a console build with d25f8479.
- **D-389-2** on-request vs interval: **interval**, so Hermes readers of survival.json are fresh in quiet hours.
- **D-389-3** `lane()` HOLD write to `/opt/galion/hermes/modes.json` fails silently under ProtectSystem: **accept** (intended; do not open `/opt/galion/hermes`).

**Memory**
- **D-388-1** restore a memory writer: **yes, option (a)**: hermes-rs client 8887a0cb (slot 403, flag `HERMES_RS_MEMORY` default off); enable at the hermes-gateway window (D-403-1..4).
- **D-388-2** legacy root `vectors.db`: **archive and remove with runbook `docs/ops/legacy-vectors-archive.md`** (7aba14ec; 4 rows, all also in the live DB; owner runs it).
- **D-388-3** freshness alert: **yes, at the next nerve deploy** with sensor 1010eb7c (slot 405, D-405-1); expect it to fire until D-388-1 is live.

**Cortex**
- **D-387-1** install the `CORTEX_HERMES_EXCLUDE_UNITS` drop-in: **yes, with the next cortex build** (6e96ee54 is not in live 58a12e6; slot 412 cortex-live-merge-5 merges it). Env var only, never the config key.
- **D-387-2** exclusion is per subject: **accept** as built.
- **D-387-3** name throwaway units `swarmtest-*`: **yes**, one pattern covers all.

**Nerve**
- **D-385-1** nerve deploy recipe (default / no-snapshot + `NERVE_RUSTSEC_DB` / lean): **default build for now** (live 9baa450 has room); revisit if the 2 MiB budget gets tight.
- **D-385-2** gate the 7 S2 sensors: **done as code** in b5a340c8 (slot 404, default on, lean saves 376,832 B); **accept**, ships with the next nerve build.
- **D-382-1** alert channel: **A, local ntfy webhook, crit 6/h** (b074ca20; no token needed). Needs a nerve build with it.

**Tests / box hygiene**
- **D-383-5** nerve t20 RSS test red (11.7 MiB > 8 MiB): **(b) build the test's nerve with pinned default features**, then re-measure; do not just raise the limit.
- **D-383-6** `GALION_REAL` set in the shared box shell: **(b) make cargo-slot strip it unless asked**, it already made a sandbox test run on the box once (F-383-10).

## 3. Not in this digest
Items from digests 1 and 2 that are not listed above and not resolved still stand (for example the edge closed ports D-380-*,
mouth B8 D-358-*, Hermes off-host D-306, bank reader D-351-1/D-365-1). New decisions from slots 403-408 (D-403-*, D-405-*,
D-406-*, D-408-*) are in those slots' docs; the ones that touch the items above are named inline.
