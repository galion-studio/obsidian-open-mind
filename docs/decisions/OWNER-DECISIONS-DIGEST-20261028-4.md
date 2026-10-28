<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Owner decisions digest 4 (for Slavik and Mate)

Prepared 2026-10-28 21:45 CET (+01:00) by wave-4 slot 429. Docs only. Follows digest 3
(`swarm/owner-decisions-digest-3` @ a7dfb3d6, 18:05 CET). Sources: DONE lines in `/workspace/swarm/CALL-RECORD.md`
up to 21:40 CET (slots 397-424), each slot's branch docs, and fresh read-only checks at 21:40-21:41 CET
(`docs/real-tests/owner-decisions-digest-4.md`). Nothing was applied, deployed or merged by this slot.

## 0. The short version
- **Published.** PR #12 merged into main at **18:09:32 CET** (Mate t334u), merge commit **7ba85b60**; main is still
  7ba85b60 at 21:41 CET. PUBLISH-PATH step 5 is **done**.
- **Live is steady:** console 5aa31927 (:8080 + :18080), cortex 58a12e6, nerve 9baa450, hermes-rs b7fbbc78, all
  active, NRestarts 0. Cortex escalations_open 0, incidents_open 2 (edge).
- **Nerve 68 D** is not a regression: 15 of the lost points are the mouth (galion-one :8002) and 17 are edge, of which
  **2 are `tuner-quick`**, a transient quick-tunnel unit lost in the VPS shutdown of 2026-10-22 (slot 420). With
  D-420-1 the score goes to 70 C.
- **Only security item left: D-361-1** (live `galion-restore-test.sh` still the 840 B .sh, no `.pre361`, checked 21:40).
- **Next deploy windows are ready to pick** (all candidates built, tested, staged only): cortex 5026d9cf (D-412-1 +
  D-417-1), nerve from nerve-merge-8 674482f1 (D-424-1 + D-420-1/2), console 4e9e9e50 (D-413-1 + D-410-1), hermes
  gateway 989958be (D-373-3 / D-416-2). Post-publish test batch lands via D-421-1.

## 1. Resolved since digest 3

| ID | What | Evidence |
|---|---|---|
| PR #12 merge (step 5) | main-staging 3250d07d -> main, no force-push | merge commit 7ba85b60, merged_at 18:09:32 CET; GitHub API 21:41 CET |
| D-396-3 | Gate 503 wording | **Coded** in 410 115637af; ships with the next console build (D-410-1). |
| D-395 + F-395-3/4 | Console option A routes + bank / label fixes | **Coded** in 413 c2522a99 (code 4e9e9e50); deploy is D-413-1. |
| D-385-2 | Gate the 7 S2 sensors | Coded 404 b5a340c8, carried into 424 674482f1. |
| D-383-5 | nerve t20 RSS test red | Explained by 401 bca13683: debug-profile artefact, release 4.6-5.8 MiB; profile-aware budget (D-401-1). |
| D-383-6 / D-400-1 | GALION_REAL leaking into sandbox runs | Gated in code by 400 a98a75bc (needs GALION_REAL_SANDBOX=1 + VPS host check); unsetting the shared profile var is still nice-to-have. |
| D-415-2 | Keep `tsv_budget()` hard ceiling | Kept in 421 5199d7d5. |

## 2. Still open, one line each (recommendation in bold)

**Security / VPS (owner runs, no restart)**
- **D-361-1** restore test swap: **yes**, symlink to galion-backup-dd89bea4, keep .sh as .pre361 (361 2b24e88b, ~5 min).
- **D-419-1** adopt `galion-report-push` (419 679d404f) for the overnight report push: **yes** (guard refuses any write into /opt/galion/openmind).
- **D-419-2** tripwire on the live tree: **B, index.lock + reference-transaction hook** (hook alone lets checkout rewrite files, F-419-1).
- **D-419-3** read-only git into the tree stays **warn**; **D-419-4** keep allow-marked superseded TEACHER lines: **yes**.
- **D-398-1** apply the 17-row venv o+w plan: **yes** in a VPS window; **D-398-2** retire hidden node_modules trees: **yes, later**; **D-398-3** idea-arch.key / migrate-ram.env: **owner looks** (secrets dir, agents do not read).
- **D-406-1 (=D-388-2)** archive legacy root vectors.db by runbook: **yes** (4 rows, all in live DB); **D-406-2** keep archive 30 d: **yes**; **D-406-3** drop root entry from galion-backup after: **yes, own slot**; **D-406-4** leave manifest: **yes**.
- **D-414-1** console./dashboard.galion.studio (NXDOMAIN, never live): **A retire**, tidy code later, no DNS change; **D-414-2** reword F-396-4: **yes** (box proxy artefact).
- **D-420-3** republish tuner quick tunnel: **no, leave unpublished** (random public URL, nobody uses it since 22 Oct).

**Land onto main (post-publish)**
- **D-421-1** post-publish test batch (415 18d68395 onto main): **B, replay** the diff 2ce1a08f..96e24ce2 (421 5199d7d5, tree 19d5c983, gates green); **D-415-1** = same decision.
- **D-407-1** land order on a staging branch off 7ba85b60: **yes**, 388 -> 389(+F-407-4) -> 386 -> 383 -> 387, then 411 nerve line (407 3d9dff48).
- **D-407-2 / D-411-2** who lands the live nerve lineage 9baa450c (11 conflicts, same as live): **one dedicated slot after D-407-1**.
- **D-407-3** re-pin 386's live nerve test to 9baa450c after that: **yes**; **D-409-3** bump default live pin to 9baa450c: **yes, same time**.
- **D-407-4** 387's handoff control asserts `>= 1`: **yes**.
- **D-407-5** nerve 79-81 KB under the 2 MiB cap: **trim, do not raise the cap**; use the lean recipe (see D-424-1).
- **D-422-1** fold hub /event cause into `gaia:<src>` (one cause = 15 not 30): **yes** in the next nerve merge; **D-422-2** redact addresses in hub incident text: **yes**.
- **D-423-1** GAIA_TEST_PORTS override (GALION_REAL only, 19000-22999): **accept**; **D-423-2** stray env: **keep refuse**.
- **D-402-1** DST replay test-only fix: **accept** (cause-only ids not recommended).
- **D-401-1** profile-aware RSS budgets: **accept**; **D-401-2** live nerve RSS self-check: **later**.
- **D-400-2** hermes-tools sandboxed.rs 8 plain tests still run sandbox on box: **gate them like 400 did**.
- **399 (no decision ID)** fmt/android baseline 861fc659: **land with the batch** (no product change, byte-identical binaries).
- **D-409-1** android stays check-only (no NDK): **yes**; **D-409-2** build the next nerve from the rust-lld tree: **yes** (live 9baa450 was linked by host cc, F-409-1).
- **D-416-1** hermes-gateway C compiler: **zig as recorded by 373** (reproducible 3/3); **D-416-3** record exact build cmd per deploy: **yes**.

**Cortex window (one restart, rollback 58a12e6)**
- **D-412-1** deploy cortex 5026d9cf: **yes** (adds real nerve reachability, build stamp, exclude off by default).
- **D-412-2** `/health` gains nerve_contact/commit/build and `nerve:false` on stale nerve: **yes** (live-fix watch will see real outages).
- **D-417-1** include escalation autoclose + drop-in 93: **yes** (needs a cortex built with 7e73796b on top of 5026d9cf); **D-417-2** max age **6 h**; **D-417-3** persist + unique ids: **yes, follow-up slot**; **D-417-4** also install 92 (=D-387-1): **yes, same deploy**.
- **D-387-2** accept per-subject exclusion; **D-387-3** `swarmtest-*` naming: **yes** (unchanged).

**Nerve window (rollback 9baa450)**
- **D-424-1** recipe for 674482f1 (default 2,016,064 B, 81 KB headroom / lean+edgerate / 404's (d)): **(d) lean plus only the features whose env is set** (=D-404-2; lean 1,631,040 B; default headroom is now too thin, D-385-1 revisited).
- **D-424-2 / D-405-3** memory binary >= 1d7164b4 side by side for the freshness sensor: **yes**; **D-405-1 (=D-388-3)** install sensor: **yes** (fires until D-388-1 is live); **D-405-2** 24 h warn: **yes**.
- **D-424-3** INTERIM console-static line until nerve-client is wired in: **yes**.
- **D-420-1** install components-live-d420.json (tuner-quick expected:false, 68 -> 70): **yes**; **D-420-2** ship the state.rs/replay.rs fix: **yes**, merge 420 b3d71822 into the 424 line before building (not in 674482f1 yet).
- **D-411-1** stamped nerve candidate from 0d2d964f: **superseded by 424** 674482f1, which includes it.
- **D-382-1** alert channel: **A, local ntfy webhook, crit 6/h** (now in 424 build).
- **D-404-1** accept S2 gates: **yes**; **D-404-3** keep edge-rate unset warning: **yes**.

**Console window (rollback 5aa31927)**
- **D-413-1** deploy candidate-2 4e9e9e50 with staged drop-ins: **yes**, includes D-410-1 wording (D-410-3 pair 503 included); **D-410-2** third text: **later**.
- **D-384-2** bank stays retired (410): **yes**; **D-384-3** /hermes thread list stays gone: **yes** for now.
- **D-389-1/2** survival refresh in-console, interval 900 s: **yes** (slot 425 folding it now); **D-389-3** HOLD write fails under ProtectSystem: **accept**.
- **D-408-1** ship the survival card with the V2 cutover: **yes**; **D-408-2** desk card: **later**.

**Hermes gateway window (D-373-3, separate, rollback rm drop-in -> b7fbbc78)**
- **D-373-3 / D-416-2** install 95-gateway-candidate.conf (hermes-gateway-989958be, 1,952,592 B, parity P1-P5 ok): **yes, own window**.
- **D-403-1** enable `HERMES_RS_MEMORY` (=D-388-1 a): **yes, after slot 426 scopes recall** (**D-403-2** cross-session recall: **opt-in only**, **D-403-4** key = session_key else run id: **yes**); **D-403-3** port query-mmd/offload/session-end: **later**.
- **D-397-1** install hermes-coding tmpfiles: **yes**; **D-397-2** toolchain: **c, keep coding off** until a toolchain is chosen; **D-397-3** clone source: **owner** (local base.git mirror suggested).

## 3. Not in this digest
Older open items from digests 1-3 not named above still stand (edge closed ports D-380-*, mouth B8 D-358-*, Hermes
off-host D-306, bank reader D-351-1/D-365-1). Slots 425-428 were running at 21:40 and are not covered.
