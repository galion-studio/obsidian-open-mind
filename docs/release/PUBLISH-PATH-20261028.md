<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Shortest path to publishing, refreshed: 2026-10-28 15:40 CET (+01:00)

Wave-4 slot 392, docs only. Updates section 6 of `docs/release/PUBLISH-BLOCKERS-20261028.md`
(slot 362, 05:55 CET) with what the swarm finished since then (slots 363-391, 05:58-15:18 CET, from the
DONE lines in `/workspace/swarm/CALL-RECORD.md`) and fresh **read-only** checks made for this file
(GitHub API, VPS loopback GETs / `systemctl`, local `git merge-tree`, 15:36-15:40 CET; evidence in
`docs/real-tests/publish-path-refresh.md`). **Nothing was applied, deployed or merged.** You decide.

> **Update 2026-10-28 21:45 CET (slot 429):** step 5 is **DONE**: PR #12 merged into main 18:09:32 CET (Mate t334u),
> merge commit **7ba85b60** (main head unchanged at 21:41). The app is published. Only D-361-1 remains from step 1.
> Post-publish decisions: `docs/decisions/OWNER-DECISIONS-DIGEST-20261028-4.md`.
>
> **Update 2026-10-28 18:05 CET (slot 418):** steps 1 (half: D-318-2 done, D-361-1 open), 2, 3 and 4 below are done;
> step 5 (PR #12 merge) is next once CI is green. Live: console 5aa31927, cortex 58a12e6, nerve 9baa450. The
> sections 0 and 2 below are the 15:40 snapshot. Open decisions: `docs/decisions/OWNER-DECISIONS-DIGEST-20261028-3.md`.

## 0. The answer in four lines
1. **Landing is now two branches, not five steps.** Slot 373 `swarm/land-order-gateway-size` **83e573e2** already
   contains the whole D-359-1 order *plus* the gateway size fix; slot 374 `swarm/pr12-flake-bundle` **d50478db**
   contains all CI flake fixes. They merge with **0 conflicts** (checked now: tree 76ece348).
2. **Security first is unchanged**: `link.env` is still **666 root:root** live (D-318-2), restore test swap still pending (D-361-1).
3. **One deploy window** is still needed (console, cortex then nerve). Nothing changed live today: nerve 9d1a4f3, cortex 73583ab.
4. **7 branches are still local only** (was 5): the parent has to push them; 367 is no longer one of them.

## 1. What changed since 05:55 (only the items that move the path)

| Slot | Branch @ SHA (all on GitHub) | What it changes for publishing |
|---|---|---|
| 373 | land-order-gateway-size **83e573e2** | B1 + B2 in one tree: ebc1a63b (D-359-1 order) + a0d099c2 + 16bfc28d + **49f214b9 F-373-1 fix** (feature-pin test read a stale 2.5 MiB gateway limit). hermes-gateway **1,948,240 B** (148,912 under 2 MiB); nerve 2,017,768 B (79,384 headroom). ws 2215/0 after fix; repro 19/0 differ; release artifacts 21/21. Also commits the x86_64 musl `rust-lld` line. |
| 374 | pr12-flake-bundle **d50478db** | B3 closed: b721a689 (360) + room fd 3011c042 (slot 350 fix; the F-390-1 flake seen in slot 390 runs 14:51-15:14 is this one, on a tree without it) + boot ETXTBSY 278706fd + **new F-374-1** gaia heal_loop timing slack 5d03a5ce. **10 consecutive clean** full ws runs (2005/0/18 each). |
| 375 | real-ports-batch-2 **7c3de266** | Test-only. 15 real tests movable by env. F-375-1: 7 console real tests target Node-era routes that 404 on the live Rust console (D-375-1). Not a publish blocker (GALION_REAL-gated, not in CI). |
| 376 | nerve-xt-pin-refresh **52aedbc7** | Test-only. Nerve cross-target pin now checks the size budget by default (D-376-1) and can rebuild live 9d1a4f3 bit-for-bit. F-376-1: tree nerve is +471,288 B (+32 %) vs live. D-376-2 (commit rust-lld line) is **already satisfied inside 373** and again by 386 9ebbed3a. |
| 384 | console-realtests-rust-routes ab0b7d04 | Answers D-375-1: route map, option A (Rust routes) 7/7 green on live (D-384-1). |
| 385 | nerve-dep-growth 8f40f18e **(local only)** | Explains F-376-1 (rustsec snapshot 98 KB, 7 S2 sensors 184 KB, ...); optional lean features give 351,424 B headroom (D-385-1). |
| 377 | cortex-version-commit b36dc358 | Cortex `--version` + `/health` carry the commit; deploy recipe must pass GALION_BUILD_COMMIT + SOURCE_DATE_EPOCH (D-377-2). |
| 382 | nerve-alerts-stage b074ca20 | Gives B9 a **no-token** option: local ntfy webhook, crit 6/h (D-382-1 A). Telegram token no longer the only way. |
| 364/365/371/389 | 8db841dd / 66e1bb98 / 99ac55b5 / d25f8479 | Console follow-ups for after the publish: board-parity deploy delta (D-364-1), **credential stripping before any bank reader wiring** (D-365-1), bank as-of display, survival refresh writer (Python cron died 2026-10-18). |
| 388 | memory-db-stale-diag 1d7164b4 | Memory DB is really stale since 2026-10-18 23:57 +02:00 (only writer was retired Python Hermes). Not a publish blocker; product gap (D-388-1). |
| 391 | openmind-pin-drift 1fe15a63 | `/opt/galion/openmind` HEAD is 202887d5, not 2f5a730 (report-only commits, no live impact). D-391-1 accept. |

Also done, smaller: 363 0d545118, 366 b859ffb3, 368 bfb102c8, 370 f82c95f2, 372 f0650107, 378 e143b24e, 380 185d12fc,
381 657a9988, 383 26a526c1, 386 9ebbed3a, 387 6e96ee54, 390 403f7179 (fixes the F-373-2 evt_mode test hang, test-only).

## 2. Live state now (read-only, 2026-10-28 15:37 CET)

| What | 05:47 | 15:37 |
|---|---|---|
| GitHub main / PR #12 | 13a296ce / open, head 38a07700, clean | **same** (PR #12 still on the old head: none of 373/374 landed on main-staging yet) |
| Live nerve / cortex | 9d1a4f3 / 73583ab-llm | same binaries; nerve cycle 97, score 64; cortex /health ok, incidents_open 2, **escalations_open 11 (was 1)**, handoffs 7 |
| Console :8080 / :18080 | `/` 302, `/api/v1/admin` 404, `/api/v1/bank` 404 | same |
| Mouth :8002 | 503, 11.5 h | 503, **21.2 h** (inc-865945dd) |
| Edge inc-06ffb32f | 15 hosts -> 6 ports | 16 hosts -> 8 ports; nerve rate is inflated ~37x (F-380-1), slot 380 map: 31 closed rules |
| Alerts | none configured | same |
| `link.env` | 666 root:root | **same (still the top security risk)** |

## 3. Shortest path to publishing (ranked, replaces section 6 of the 05:55 file)

1. **Security, no restart (B4, B5)** — say "D-318-2 yes" (chmod 600 `link.env`, staged 356 b8520146) and
   "D-361-1 yes" (restore test swap, 361 2b24e88b). ~5 min in a VPS window. Unchanged since this morning.
   **Status 18:03 CET: half done.** D-318-2 applied 16:05 (Mate t332u): `link.env` is **600 root:root** (checked 18:03).
   **D-361-1 is still open**: live `galion-restore-test.sh` is still the plain .sh (no `.pre361`). Say "D-361-1 yes".
2. **Push the 7 local branches (B11)** from the parent: 334 f68def1d, 346 4828997d, 347 7fc44a00, 349 99cc25bd,
   351 65fa0271, 369 720548e0, 385 8f40f18e. Only **347** sits on the publish path (cortex `/health nerve` honesty
   for the cortex window); the rest can follow.
   **Status 18:03 CET: DONE.** 385 pushed 15:49, the other six 16:05 (Mate t332u), 403 8887a0cb 17:53. No local-only branches left on this path.
3. **Land onto main-staging (B1-B3) = two merges, already rehearsed** — merge 373 **83e573e2**, then 374 **d50478db**
   (0 conflicts, merged tree 76ece348, base 3f015986 = main-staging 38a07700 tree). Decisions: D-373-1 (this order),
   D-373-2 (carry F-373-1), D-374-1 (keep 5d03a5ce). Then re-run CI on PR #12. ~1 h of CI time.
   Leave the test-only branches (375, 376, 386, 390) for a **separate test batch after the publish**: against the
   373+374 tree they conflict in test files and a comment in `.cargo/config.toml` (386: 2 files, 390: 2 files);
   none of them is needed to publish.
   **Status 18:03 CET: DONE** 17:58 CET (t334u): main-staging ef99947c (373) + b4fe2383 (374) + 3 CI fixes, head 3250d07d.
   Test-only batch is slot 415 (post-publish-test-batch).
4. **One deploy window (B6, B7)** — console 5aa31927 (343 drop-ins; consider D-364-1 board parity), then cortex
   58a12e61, then nerve 9baa450c, per runbook 357; rollback 2d846fb8 / 73583ab / 9d1a4f3. ~30 min.
   Build with GALION_BUILD_COMMIT + SOURCE_DATE_EPOCH if you take the 377 stamp (D-377-2). Watch the 11 open
   cortex escalations before and after.
   **Status 18:03 CET: DONE** 17:30-17:41 CET (Mate t334u; report `/workspace/swarm/reports/core-stack-live-20261028.md`):
   console 5aa31927 on :18080 + :8080, cortex 58a12e6, nerve 9baa450 + components 283db8c3, all NRestarts 0, no rollback.
   Cortex escalations_open 0 at 18:03 (was 11). D-396-3 (503 wording) deferred.
5. **Merge PR #12 into main** = the publish step (your or Mate's go; never force-push).
   **Status 18:03 CET: NEXT.** PR #12 is on main-staging 3250d07d; CI not green yet (combined status pending,
   mergeable_state unstable). Merge when green.
   **Status 21:41 CET: DONE.** Merged 18:09:32 CET (Mate t334u), merge commit 7ba85b60, no force-push; checked via GitHub API 21:41.
6. **After the publish, before real users**: alert channel (B9: now pick D-382-1 A ntfy without a token, or give the
   Telegram token), crate ownership rows (B10), mouth option (B8: D-358-1 / D-358-3).

Not on the path any more: B2 gateway size (inside 373), B3 CI flakes (inside 374), D-376-2 rust-lld line (inside 373).
Still polish after the publish: edge closed ports (B12, D-380-1..5, owner DNS), bank data source (D-351-1 then D-365-1),
memory writer (D-388-1), nerve size headroom (D-385-1).

## 4. Open decisions from the slots named in this task
- **D-375-1** console real tests on Node-era routes: recommend D-384-1 option A (Rust routes, 7/7 green live).
- **D-376-1** nerve cross-target pin default = size budget, live pin as post-deploy gate: recommend yes.
- **D-376-2** commit the x86_64 rust-lld linker line: **done in the 373 tree**; 386 adds the same line (D-386-1);
  take 373's copy, drop 386's comment on merge.
- **D-373-1/2/3**, **D-374-1**: see step 3 above (D-373-3: gateway deploy stays a separate window).
