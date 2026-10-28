<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Owner decisions digest 2: wave 4, slots 303-337 (for Slavik)

Prepared 2026-10-28 02:35 CET (+01:00) by slot 342. Docs only. This refreshes the slot 301 digest
(`swarm/owner-decisions-digest` @ f2ca42b2, items D-245..CF-403 there still stand).
Sources are the slot DONE lines in `swarm/CALL-RECORD.md` (2026-10-27 21:03 to 2026-10-28 01:46 +01:00) and each branch's docs.
"Live" notes are the read-only VPS probes those slots recorded; no new VPS probe was made for this file.
**You make the decisions. Nothing here has been applied, deployed or merged into main.**

## 1. Fastest path (your 5 minutes)
1. **Security first:** say "**D-318-2 yes**" (chmod 600 `link.env`, now mode 666 and sourced as root) and "**F-326-1 fix**" (live restore test lies "ok").
2. **Console:** say "**deploy console c7cebb28 + D-313-1 + D-336-1**". That fixes the 404 board/admin routes, the phone pairing, and the F-313-1 lockout bug in one window.
3. **Nerve:** say "**deploy nerve-merge-6 9baa450c with D-333-1**". The nerve then sees the console blind spot (F-313-2) and the :18080 unit.
4. **Cortex:** say "**deploy cortex 30f2de69 with the nerve**" (F-328-1: planned stops no longer trigger restarts).
5. **Landing:** say "**D-337-1 land order ok**" and push the 2 local-only branches (section 5).

## 2. Security and honesty items (act soon)

| ID | What | Evidence (branch @ SHA) | Live now | Recommend |
|---|---|---|---|---|
| **D-318-2** (link.env 666) | `link.env` is world-writable (mode 666) and `play.sh` sources it as root: any local user can run code as root. | `swarm/sandbox-nonroot-stage` @ a1255fc0 (fix staged as a command only) | Still 666 at 22:39 | **chmod 600 + umask in install.sh**, in the next VPS window. One command, no restart. |
| **F-313-1** | Console admin pairing: with a read-only store, wrong codes never lock out (6x 401) and the right code answers 503, so it's a code oracle. | Found on `console-admin-flags-stage` @ 49b31b3f; **fixed** on `console-admin-lockout-fix` @ dfa120c6, folded into `console-v2-admin-fold` @ c7cebb28 (RO 503x4 then 429) | Not exposed yet: live `/api/v1/admin` is 404 | **Deploy only c7cebb28 or later** (D-323-1). Never deploy 9c0f9d1a or 7888d8fb as they are. |
| **F-326-1** | Live `galion-restore-test.sh` prints "restore test ok" and exits 0 after the checksum FAILED (`&&` defeats `set -e`). | `swarm/restore-test-rust` @ bc741f2a | Live .sh still in use | **D-326-1 yes**: install the Rust galion-backup restore check, run it once, then retire the .sh. |
| **F-313-2 / N-307-1** | Nerve says the console is "healthy" while its admin/board routes 404. | Test `swarm/nerve-console-blindspot-test` @ 607e44fe; fix in `nerve-merge-6` @ 9baa450c | Live nerve 9d1a4f3 still says healthy (01:18) | Deploy the nerve (D-315-2 / D-333-1). |
| **F-328-1** | Tagged nerve sends `unit.planned_stop`, but cortex ignored it, so rule R5b was dead. | **fixed** `swarm/planned-stop-pair-check` @ 30f2de69 (carries 309 bdde4a07) | Live cortex 73583ab, nerve 9d1a4f3: no wrong restarts yet, because the nerve doesn't tag | Deploy the cortex fix **with or before** the tagged nerve. |
| **F-337-1** | Cortex `/health` reports `nerve: true` when the nerve is only configured, not connected. | `swarm/release-bins-add` @ fc481390 | live | Small follow-up fix; no decision needed beyond "yes, fix". |
| **F-335-1** | `hermes-tools` sandboxed tests still SKIP when `CI` is set. | `swarm/ws-test-recipe` @ 132a5e9a | CI | Accept for now (box/VPS runs are real); fix with D-320-1. |
| **D-306 F1-F4** | Hermes off-host backup (PR #14) was not production-ready: unsigned snapshots, secret leak (`github_pat_`), lost sessions, http bearer. | Review `swarm/hermes-offhost-review` @ 19d6658e; fixes `swarm/hermes-offhost-fixes` @ a885d473 (HMAC seal, strict scanner, loopback-only http) | Not deployed | Pick D1 store/token, D2 send session text off-host or not, D3 key rotation (slot 340 is working on this), D4 over-cap. Keep PR #14 unmerged until then. |
| **N-332-1** | Live nerve cycles about every 8 min, though `interval_s` is 30. | Seen by slot 332 (cycle 36 at 01:15, 37 at 01:22) | live | Slot 344 (nerve-cycle-latency) is investigating. No decision yet. |
| MOUTH | `:8002` still 503 "loading/chatterbox" (inc-865945dd, about 6 h). | slot 301 MOUTH item; D-327-2 | live | As in digest 1: **option 1** (honest galion-one). |

## 3. Deploy decisions (one window each, all staged, none installed)

| ID | Question | Evidence | Recommend |
|---|---|---|---|
| **D-303-1 / D-313-1** | Add `--admin-store/--admin-mirror` + `ReadWritePaths` to the console units | `console-admin-flags-stage` @ 49b31b3f (drop-in `60-admin-store.conf`) | **Yes**, but point ExecStart at a binary built from **c7cebb28+** (D-323-1). |
| D-313-2 | RW scope for the store | same | A **dedicated store dir**, not all of `/root/galion-secrets`. |
| D-313-3 | Live store 616 B vs mirror 533 B drift | same | Re-sync the mirror from the live store at deploy time. |
| D-313-4 | One store shared by 2 processes (:8080 + :18080) | same | Allow it only with D-323-2 locking; otherwise give admin to one process only. |
| D-323-2 | RO store: per-process memory lock or refuse to start | `console-admin-lockout-fix` @ dfa120c6 | **Refuse RO store at startup** (fail closed and obvious). |
| D-323-3 | Keep the paired-device bypass of the lockout | same | Keep it. |
| **D-336-1** | Add `--arena-root /workspace/arena` at the console cutover | `console-dist-arena-bank` @ 7888d8fb (needs F-313-1 fold onto c7cebb28 first) | **Yes.** Live `/api/arena` and `/api/v1/bank` are 404 on :8080 and :18080 (01:14). |
| D-336-2 / D-321-1 | Who serves the bank data | same; `console-v2-merge-2` @ cd1204a0 | hermes-rs on loopback via `--data-upstream`. The console stays key-free. |
| D-336-3 | Arena actions (POST) | same | Hold; read-only for now. |
| **D-315-1** | Admin route failure = degrading or failing | `nerve-console-deep-health` @ 38750655 | **Degrading** (never triggers restarts). |
| D-315-2 / **D-333-1** | Deploy the nerve with the staged `components-live.json` (+:18080, deep checks) | `nerve-merge-6` @ 9baa450c | **Yes**, one window with the cortex fix. Note C-333-1: the live nerve reads the file, so built-in defaults alone change nothing. |
| D-315-3 | Check more routes / Host override | same | Later. |
| D-316-1 / D-333-3 | :18080 console-rust-primary: add as a nerve component, non-critical | `nerve-merge-5` @ b1a0d683 → merge-6 | **Yes, non-critical.** |
| D-333-2 | Accept "degrading" for the console until the board API is deployed | merge-6 | Yes. It's honest. |
| **D-318-1** | Run hermes-rs as non-root (`80-nonroot.conf`, sysusers, LoadCredential) | `sandbox-nonroot-stage` @ a1255fc0 | **Yes, in a window.** Plain `User=` alone crash-loops (config dir 0700 root), so use the staged shape only. One live restart. |
| **D-320-1** | VPS has no Landlock: coding tools a allow / b refuse / c layered | `hermes-tools-vps-retest` @ b2350d56; `coding-tools-landlock-stage` @ f68def1d (LOCAL) | **c layered** (proven on the VPS: 14 layers enforced, fails closed). Install after D-318-1 (D-334-1). |
| D-326-1 | Install Rust restore test | `restore-test-rust` @ bc741f2a | Yes (see F-326-1). |
| D-311 gate | Fold the backup drop-ins into the base unit | `backup-dropin-fold` @ 5d72cc98 | Wait for the gate: 12 clean timer runs, earliest about **2026-10-30 13:17 CET**. |
| D-327-1 | Add `NERVE_URL` drop-ins for galion-one, desk, backup, guardian-tick | `nerve-wire-gaps-2` @ 3b3d480d | Yes, with the nerve window. |
| D-327-2 | Mouth 503 push noise | same | Fix the mouth (MOUTH option 1) and don't mute it. |
| D-327-3 | Wiring-depth check: warn or error | same | Warn now; switch to error after D-327-1 is live. |
| D-330 | py-archive Phase A | `py-archive-tool` @ 4a7dbd97 (VPS RO PASS, 5 files) | Go, **but** fix the rollback note first: removing the galion-desk drop-in would revive the old ELF (slot 345 is on it). |

## 4. Build, release and landing decisions

| ID | Question | Evidence | Recommend |
|---|---|---|---|
| D-312-1 | Use the default cc-rs flags matrix as the publish recipe | `release-artifacts-refresh` @ 7091f76b (LOCAL) | Yes. |
| D-312-2 | Merge the `size-budget.sh` tagged-name fix | same | Yes. |
| D-312-3 | Add `--version` to console-static/serve and galion-backup | same | Yes (one line each). |
| D-312-4 | Release bytes depend on the `-p` set | **fixed** `release-repro-unify` @ 6ffb2570 (solo == set, 14/14) | Ratify the fix (D-325-1 land). |
| D-325-2 | Add gaia-cortex + console-serve to RELEASE_BINS | **done** `release-bins-add` @ fc481390 (19 compared, 0 differ) | Ratify. |
| D-325-3 / D-337-4 | aarch64 byte proof not run (no cross C compiler on box) | same | Accept for now; slot 338 (cross-cc-restore) is working on it. |
| D-325-4 / D-293-2 | Crate ownership rows | same | **Only you can fill these.** |
| **D-337-1** | Land order onto main-staging | `release-bins-add` @ fc481390 | **cortex-staging-land-check 186cd61c → release-repro-unify 6ffb2570 → console-v2-admin-fold c7cebb28 → release-bins-add fc481390.** D-293-4 is READY (186cd61c, 2135/0). |
| D-337-2 | Build gaia-cortex with `llm-advisor` in the recipe | same | Yes (matches live 73583ab-llm). |
| D-337-3 | nerve is 2,020,224 B, only 76.9 KB under 2 MiB | same | Watch only (slot 348 nerve-size-headroom). |
| D-329-1..3 | hermes-gateway under 2 MiB: workspace SQLite OMIT flags, RELR/rust-lld config, drop the 2.5 MiB exception | `hermes-gateway-2mib` @ a0d099c2 (2,080,128 B) | **Yes to all three.** This replaces D-275-1/D-294-1 "coding opt-in" as the way to hit budget. |
| D-322-1 | Land hygiene 29a717f1 + safetensors 97141c1c onto real main-staging 38a07700 | `hygiene-staging-check` @ 2966386f | Yes, after AP-1 (digest 1) and CD-3. |
| D-320-2 / D-335-1..3 | Default workspace test recipe | **done** `ws-test-recipe` @ 132a5e9a (`cargo xtask test`) | D-335-1 default **auto + box guard**; D-335-2 confirm on next self-eval; D-335-3 update RULES.md to `cargo-slot xtask test`. |
| D-324 | Cortex replay golden: box-only failure | **fixed** `cortex-replay-golden-fix` @ 40a17ab1 | Ratify. Not a regression. |
| D-321-3 | Base admin-fold on merge-2 tip | **done** (331 c7cebb28 carries cd1204a0) | Closed. |

## 5. Branches not on GitHub (need a push from Mate's side)
Blocked by Auto-review in the agent; never retried. Push command: `push-branch.sh wt/<slug> swarm/<slug>`.

| Branch | SHA | Status |
|---|---|---|
| `swarm/release-artifacts-refresh` | 7091f76b | **LOCAL ONLY** (slot 312) |
| `swarm/coding-tools-landlock-stage` | f68def1d | **LOCAL ONLY** (slot 334) |
| `swarm/cortex-r3-planned-stop` | bdde4a07 | On GitHub as an ancestor of 30f2de69 (slot 328); branch name not pushed |
| `swarm/release-repro-unify` | 6ffb2570 | On GitHub as an ancestor of fc481390 (slot 337); branch name not pushed |
| `swarm/console-v2-merge-2` | cd1204a0 | On GitHub as an ancestor of c7cebb28 (slot 331); branch name not pushed |
| `swarm/sandbox-dsb-pack` | 3289511d | On GitHub as an ancestor of a1255fc0 (slot 318); branch name not pushed |
| `swarm/android-admin-plan` | f4b0bfac | On GitHub as an ancestor of 2f324ee1 (slot 319); branch name not pushed |

Only the first two have commits that aren't on GitHub at all.

## 6. Still waiting on inputs only you hold (unchanged from digest 1)
D-283-3 admin allowlist; HADES H1-C5 custody keys; Hermes Telegram bot token and chat id; crate ownership rows (D-325-4); Hermes off-host key provisioning (D-306 D3).
