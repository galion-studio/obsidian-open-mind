<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Publish blockers, refreshed: 2026-10-28 05:55 CET (+01:00)

Wave-4 slot 362, docs only. Refreshes `/workspace/swarm/PUBLISH-BLOCKERS-20261027.md` (2026-10-27 ~17:30 CET).
Sources: the slot DONE lines in `/workspace/swarm/CALL-RECORD.md` (slots 300-361, 2026-10-27 18:25 to
2026-10-28 05:40 +01:00), owner decisions digest 2 (`swarm/owner-decisions-digest-2` @ 5ff76fad), and
fresh **read-only** checks made for this file (GitHub API and VPS GET/`systemctl show`, 05:47-05:50 CET).
**Nothing here was applied, deployed or merged.** You make the decisions.

## 0. The one-paragraph answer
Publishing the Rust app is no longer blocked by CI: **PR #12 (main-staging -> main) is now `mergeable_state: clean`**
(head 38a07700; slot 360 confirmed 20/21 checks green, miri skipped by design). What still blocks it is
(1) **landing** the finished work onto main-staging (one rehearsed, conflict-free order exists),
(2) **two security fixes** that must not ship as they are live today (`link.env` 666, restore test that lies),
(3) **one deploy window** for console + nerve + cortex (all staged and tested, none installed), and
(4) **inputs only Slavik holds** (crate ownership rows, admin allowlist, alert channel token).

## 1. Live state now (read-only probes, 2026-10-28 05:47-05:50 CET)

| What | Seen | Change since 10-27 file |
|---|---|---|
| GitHub `main` | 13a296ce (2026-10-20 18:36 CEST) | unchanged |
| PR #12 main-staging -> main | open, head 38a07700, mergeable **clean** | was "unstable / CI red": **fixed** (PR #13 + slot 360) |
| Live nerve | `openmind-nerve-9d1a4f3 --mode event`, :8670 /state 200, cycle 55 | unchanged binary |
| Live cortex | `gaia-cortex-73583ab-llm`, listens **127.0.0.1:8674**, /health 200 (`nerve:true`, 2 incidents, 1 escalation) | unchanged binary. Note: slot 360's "cortex 8672 not listening" was the wrong port, not an outage |
| Console :8080 / :18080 | `/` 302; `/api/v1/admin` 404 both; `/api/arena` and `/api/v1/bank` 404 | unchanged: phone pairing and board still broken live |
| Nerve view of console | `healthy` (HTTP 302 on /, since 2026-10-27 20:30 CET) | unchanged blind spot F-313-2: admin/board 404 not seen until nerve 9baa450c is deployed |
| Mouth :8002 | 503 `loading/chatterbox`, inc-865945dd **11.5 h** | worse (longer); slot 358 proved it is "not deployed", not stuck |
| Edge | inc-06ffb32f: 15 hostnames -> 6 closed origin ports, ~234 failed req/min | still open |
| Alerts | "none configured (no Telegram/Hermes alert channel)" | unchanged |
| `link.env` | `/opt/galion/galion-light/publish/obsidian/link.env` mode **666** root:root | unchanged (sourced as root by play.sh) |
| hermes-gateway.service | inactive (hermes-rs :8642 /health 200 serves chat) | n/a |

## 2. What is done (wave-4 slots 300-361; SHA = branch head; P = on GitHub, L = local only)

### Console (the app surface)
- 300 console-v2-merge-1 2087316a P; 302 console-rust-serve-impl 2d846fb P (the live build); 303 board API parity 9c0f9d1a P.
- 313 admin flags staged 49b31b3f P; **323 F-313-1 lockout fix** dfa120c6 P; 321 merge-2 cd1204a0 (P as ancestor);
  **331 console-v2-admin-fold c7cebb28 P**; 336 arena/bank reader 7888d8fb P; **343 deploy candidate 4f763ade P (code 5aa31927)**.
- 349 console-static artifact test 99cc25bd **L**; 351 bank reader plan 65fa0271 **L** (F-351-1 cookie/auth forwarding).
- 307/319 Android admin plan + pairing client f4b0bfac / 2f324ee1 P.

### Nerve
- 310 planned-stop tag 12956b45 P; 315 console deep health 38750655 P; 316 merge-5 b1a0d683 P; 332 blind-spot test 607e44fe P;
  **333 nerve-merge-6 9baa450c P** (deploy candidate); 344 cycle latency 1fff59e5 P (N-332-1 is by design);
  348 size headroom 463f6a71 P (139,992 B under 2 MiB); 352 VPS shadow compare b8288769 P (supports D-333-1);
  327 wire gaps 3b3d480d P; 358 mouth not-deployed finding cab4a6b0 P.

### Cortex
- 309 R5b planned stop bdde4a07 (P as ancestor); 324 replay golden fix 40a17ab1 P; **328 F-328-1 fix 30f2de69 P**;
  **341 cortex-live-merge-4 58a12e61 P** (deploy candidate); 347 `/health nerve` honesty 7fc44a00 **L**;
  357 cortex+nerve pair runbook 4bd62a6b P.

### Hermes
- 306 PR #14 review 19d6658e P; 314 off-host F1/F2 fixes a885d473 P; 340 key rotation 6845c350 P;
  318 non-root stage a1255fc0 P; 320 VPS tools retest b2350d56 P; 334 Landlock option c f68def1d **L**;
  346 `--sandbox=require` 4828997d **L**; 355 crash-loop root cause d76b70df P (19:45 coding drop-in; no-loop drop-in staged).

### Release / build / landing
- 312 release artifacts 7091f76b (now on GitHub); 325 repro unify 6ffb2570 (P as ancestor); 317 cortex land check 186cd61c P;
  322 hygiene check 2966386f P; 329 gateway 2 MiB a0d099c2 P; 335 `cargo xtask test` 132a5e9a P; 337 RELEASE_BINS fc481390 P;
  338 cross C toolchain (zig 0.15.2) 0dfc3712 P; 339 release pair check 7427f96e P; 353 one size table 16bfc28d P;
  354 aarch64 real test ports 8555a8f7 P; **359 land-order rehearsal ebc1a63b P**; **360 PR #12 CI flake fixes b721a689 P**.
- Backup/restore: 304 size_kb fix 6266e1e2 P; 311 drop-in fold 5d72cc98 P (gate opens ~2026-10-30 13:17 CET);
  326 Rust restore test bc741f2a P; **361 restore checksum hard-fail 2b24e88b P** (F-361-1 gzip CRC fixed).
- Python retirement: 308 plan 98601101 P; 330 tool 4a7dbd97 P; 345 rollback note dd74d6c0 P.
- Security staging: 305 DSB pack 3289511d (P as ancestor); **356 link.env perm stage b8520146 P**.
- Decisions: 301 digest 1 f2ca42b2 P; 342 digest 2 5ff76fad P.
- Still running at 05:50: slot 350 room-fd-flake-fix (needed with 360 for a flake-free CI, D-360-1).

## 3. What still blocks publishing (ranked by how hard it blocks)

| # | Blocker | Why it blocks | Fix exists? |
|---|---|---|---|
| B1 | **main-staging is behind the finished work** (38a07700) | PR #12 is clean, but it would publish an app without the console admin/board fixes, size fixes and restore fix | **Yes**: D-359-1 order is rehearsed with 0 manual conflicts (tree 223544ea) |
| B2 | **Gateway size budget not in the land tree** | slot 359's land tree still has hermes-gateway 2,202,384 B under a 2.5 MiB exception; live gateway b7fbbc78 is 2,732,248 B (F-353-1) | **Yes**: 329 a0d099c2 + 353 16bfc28d (1,944,432 B) must land too (D-353-3) |
| B3 | **CI flakes** can turn PR #12 red again | 3 timing flakes found (queue lock, nerve burst, room fd) | 360 b721a689 fixes 2; slot 350 (running) fixes the 3rd (D-360-1) |
| B4 | **link.env mode 666, sourced as root** (D-318-2) | any local user can get root on the host the app is published from | Staged command + install.sh/play.sh patch (356 b8520146). One chmod, no restart |
| B5 | **Restore test reports "ok" after a failed checksum** (F-326-1; 361: .sh lets 10/14 tampered archives through) | backups are not proven restorable | 361 2b24e88b staged swap (D-361-1) |
| B6 | **Live console has no admin/board/arena/bank routes** (404) | phone pairing and the board are the app; remote console pages 503 | 343 candidate 4f763ade (code 5aa31927, carries F-313-1 fix). Never deploy 9c0f9d1a or 7888d8fb as they are |
| B7 | **Nerve + cortex lag** (live 9d1a4f3 + 73583ab) | planned stops can trigger restarts once nerve tags them (F-328-1); /health nerve flag not honest (F-337-1) | 333 9baa450c + 341 58a12e61 in one window, runbook 357 (cortex first) ; 347 7fc44a00 still local |
| B8 | **Mouth/voice 503 for 11.5 h** | voice surface of the app is absent | 358: deploy honest galion-one (D-358-1) or retire the probe (D-358-3) |
| B9 | **No alert channel** | incidents reach nobody | Needs Telegram bot token + chat id from Slavik |
| B10 | **Crate ownership rows** (D-325-4 / D-293-2) | release metadata incomplete | Only Slavik can fill |
| B11 | **5 branches local only** | their fixes can't land | Parent/Mate push (section 5) |
| B12 | Edge: 15 hostnames -> 6 closed ports | public pages error (~234 req/min) | Not coded; needs a tunnel ingress decision (no DNS change by agents) |

Not a blocker any more: PR #12 red/unstable (now clean), hermes-rs crash loop (root cause found, config only),
N-332-1 nerve "slow cycles" (event mode by design), aarch64 cross C compiler (338 zig toolchain).

## 4. Owner decisions needed (short form; details in digest 2 and each branch doc)

Security, do first:
- **D-318-2 yes** - chmod 600 link.env (+ D-356-1 install.sh/play.sh patch, D-356-2 EnvironmentFile drop-in,
  D-356-3 remove o+w on 18 dirs and 53 console-dist files, D-356-4 migrate-ram.env 644).
- **D-361-1 yes** (supersedes D-326-1) - swap the restore .sh for `galion-backup-dd89bea4 restore-test`, keep `.sh.pre361`.

Landing (agents may merge to main-staging only with your/Mate's go; main merge is the publish step):
- **D-359-1** land order `fc481390 -> 7091f76b -> 5aa31927` (+ **D-359-2** e158400a), then
  **D-353-3** size budget branch 16bfc28d (with 329 a0d099c2), then **D-360-1** 9dceed0a + cde9c5e1 + slot 350.
- D-353-1 keep "no row above 2 MiB" (recommend yes).

Deploy window (one each, all staged):
- **Console**: 5aa31927 via 343 drop-ins (D-303-1/D-313-1, D-323-2 refuse RO store, D-336-1 arena root).
- **Cortex then nerve**: 58a12e61 then 9baa450c (D-333-1, runbook 357; D-352-1 expects +1 warn console incident, 0 restarts);
  add D-344-2 (cycle latency) and D-358-2 (mouth finding) to the next nerve build.
- **Hermes**: D-355-1 no-loop drop-in, D-355-3 coding worktree in /srv; D-318-1 non-root; D-320-1 option c.
- **Gateway**: D-353-2 ship a <= 2 MiB build at the next gateway deploy.
- **Mouth**: D-358-1 deploy honest galion-one, or D-358-3 retire the probe.

Inputs only you hold: D-325-4 crate ownership rows; D-283-3 admin allowlist; Telegram bot token + chat id;
HADES H1-C5 custody keys; D-306 D3 off-host key provisioning; D-351-1 bank data source (A/B/D/E).

## 5. Branches still local only (checked on GitHub 05:49 CET: "No commit found")

| Branch | SHA | Slot |
|---|---|---|
| `swarm/coding-tools-landlock-stage` | f68def1d | 334 |
| `swarm/hermes-sandboxed-require` | 4828997d | 346 |
| `swarm/cortex-health-nerve-live` | 7fc44a00 | 347 |
| `swarm/console-static-artifact-test` | 99cc25bd | 349 |
| `swarm/console-bank-reader-plan` | 65fa0271 | 351 |

Push command (from the parent side): `push-branch.sh /workspace/swarm/wt/<slug> swarm/<slug>`.
`swarm/release-artifacts-refresh` 7091f76b is now on GitHub (no longer local only).

## 6. Shortest path to publishing (ranked)

1. **Security, 2 commands, no restart** (B4, B5): say "D-318-2 yes" and "D-361-1 yes". ~5 min in a VPS window.
2. **Push the 5 local branches** (B11). ~2 min from the parent; unblocks 347 for the cortex window.
3. **Land onto main-staging** (B1-B3): D-359-1 order, then 353/329 size, then 360 + 350 flakes. Re-run CI on PR #12;
   all three are rehearsed or tested, so expect green. ~1 h of CI time.
4. **One deploy window** (B6, B7): console 5aa31927, then cortex 58a12e61, then nerve 9baa450c, per runbook 357
   (rollback = previous binaries 2d846fb8 / 73583ab / 9d1a4f3). ~30 min.
5. **Merge PR #12 into main** = the publish step (owner/Mate go; never force-push).
6. **Fill the inputs only you hold** (B9, B10) and pick the mouth option (B8). These can follow the publish,
   but the alert channel should come before real users.

Edge closed ports (B12) and the bank data source (D-351-1) are polish after the publish, not blockers to it.
