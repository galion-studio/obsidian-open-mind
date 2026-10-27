<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Backlog status (the one table)

This is the single status table for the 89-item backlog (BACKLOG.md of
2026-10-18). Update a row here when an item changes; do not add record-only
docs. The percentage is computed, never typed: run `scripts/status-percent.sh`.

- Status: **D** done (deliverable on GitHub main or the merge branch), **P**
  partial, **N** not started, **O** blocked on the owner.
- Size: 1 = S, 2 = M (QUEUE.md and rust-rewrite-plan.md; un-sized items were
  sized by the 2026-10-20 review, see `PROGRESS.md` of that review).
- Credit: D = 1, P = 0.5, N and O = 0. "Done" does not mean live: only
  WS-HERMES, WS-MEM and B59 run in production.
- Baseline: the 2026-10-20 progress review. Changed by the one-source merge:
  GAIA-1 D -> P, HYG-2 N -> D. The docs-sync swarm branch (2026-10-20) fixed
  evidence text only (GAIA-1, WS-SURV, B38, B42, B46/B47/B62/B65); no status
  changed. swarm/survival-heal: WS-SURV briefly D -> P (guardian/healer were not in
  13a296c), back to D on swarm/integration-1 (2026-10-20, +02:00) where the
  heal port is merged.

Computed on 2026-10-20 (Europe/Zagreb) with `scripts/status-percent.sh`; re-run
on 2026-10-20T18:58:00+02:00 after the docs-sync evidence fixes, and again on
2026-10-21T23:51:59+02:00 on swarm/docs-status-refresh (base integration-5 0d68f46) after
the B42 evidence fix (no status changed), same result:

```
status-percent: items 89 (D 54, P 12, N 2, O 21), weight 122
done: 60.7 % unweighted, 59.0 % weighted
done + half partial: 67.4 % unweighted, 67.2 % weighted
owner-blocked: 23.6 % of items
```

## Live snapshot 2026-10-27 17:40 CET (+01:00), swarm/docs-status-gates-20261027

Docs-only refresh on main-staging `7c95348b`. Sources: `/workspace/swarm/CALL-RECORD.md`
(CR), `PUBLISH-BLOCKERS-20261027.md` (PB), `RUST-MIGRATION-RESUME-20261027.md` (RMR).
The backlog percentages below were NOT re-run (2026-10-21 numbers stand).

- Console cutover **DONE 2026-10-27 17:29 CET**: Vite :8080 replaced by staged
  console-dist served by srvx (Node), backup `/var/backups/console-switch-20261027-162802`
  (CR:884; PB:11; RMR:16). Not to be redone. console.service is still Node, not Rust (CR:929).
- GitHub `main` still `13a296c`; PR #12 (`swarm/main-staging` `7c95348b` -> main) open,
  mergeable_state **unstable** (RMR:20; PB:19,22).
- Live cortex still `gaia-cortex-bd6fbe6`. `swarm/cortex-live-merge-2` `1305dec` and
  `swarm/cortex-sandbox-oom-info` `c68d8cb` are pushed, **not deployed**; c68d8cb does not
  cherry-pick onto bd6fbe6 (CR:882; RMR:14,33,37; PB:12-13).
- Wave-4 owner_ceiling raised 250 -> 400 at 17:30:19 CET, concurrency 18/18/6 (CR:886;
  PB:4-5); slots 250-265 in flight from 17:31:37 CET (CR:888).
- Nerve flood false-alarm / zero-baseline: not coded yet, slot 252 nerve-flood-baseline in
  progress since 17:33:50 CET (CR:918; PB:16; RMR: nerve-flood-zero-baseline MISSING, CR:899).
- Mouth / voice 503 still open (CR:884 "left alone"; PB:26).

| ID | Size | Status | Evidence |
|---|---|---|---|
| GAIA-1 | 2 | P | Partial, pending `swarm/gaia-land`: canonical ws/gaia (11 crates, 52 systems, approval-token HADES path) is on a GitHub branch, not on main; the VPS dry-run gaia-core left the workspace in the 2026-10-20 merge |
| GAIA-2 | 2 | P | VPS `gaia-registry` is a static table mapping 37 crates (GAIA-2/3/4 PASS). Nothing routes live services or ... |
| UCPU-1 | 2 | P | docs/universal-cpu.md and scripts/target-check.sh ("four target names KNOWN; no cross build"). The gate is ... |
| HADES-1 | 2 | P | crates/hades-gate: dry-run by default, kill and rotate always refused. HADES-WIRE: not wired or installed. ... |
| WS-SURV | 2 | D | (not live) - galion-boot and galion-origin are on main 13a296c; galion-guardian and galion-healer were DROPPED by the PR #8 merge (not merged, contrary to the old row). Their probe/classify/dry-run repair path and incident store are re-ported onto openmind-survival (`heal`) + guardian-tick (`heal`, `classify`) from swarm/survival-heal, merged on swarm/integration-1 (not yet on main). No guardian/healer/guardian-tick unit installed on the VPS |
| WS-HERMES | 2 | D | live - hermes-rs.service active since Oct 18 22:00 CEST; Python hermes.service inactive |
| WS-MEM | 2 | D | live - openmind-memory.service on 8420 since Oct 18 22:14 CEST; memory-gateway inactive |
| WS-ODT | 2 | D | (partly live) - Merged on GitHub main. Live galion-one is an Oct 18 ws/ui build; desk and tuner are older binaries |
| LAUNCH-1 | 2 | O | Private GitHub main 317d1d4 exists (LIVE.md); the public URL returns 404. VPS main has no remote and has di... |
| SEC-1 | 2 | O | Owner rotation and history purge; no evidence it was done |
| SEC-2 | 1 | O | Owner said not to change secrets/persist permissions yet (BACKLOG, Oct 18 22:35) |
| WS-VOICE | 2 | P | M1 merged; M2 is on ws/voice and vps/ws-voice (202bde2, pushed 2026-10-20), not merged |
| SEC-3 | 1 | O | Checked today: hermes config.yaml and SOUL.md are 666, vault/ is 777 |
| B43 | 1 | D | scripts/self-check.sh PASS 8 |
| LAUNCH-2 | 1 | P | The checklist exists; the AWAITING-APPROVAL boxes are all unchecked |
| B21 | 1 | O | Galion One now has a unit (galion-one.service). The Restart=always drop-ins and guardian timer are staged, ... |
| B51 | 2 | O | docs/dist-switch.md staged. Console dist cutover DONE 2026-10-27 17:29 CET (CALL-RECORD.md:884); other dist cutovers (cortex 1305dec/c68d8cb) still not deployed (RUST-MIGRATION-RESUME-20261027.md:33,37). Status not re-scored |
| OMNODE-2 | 2 | O | VPS edge/openmind-node still on `landlock = "0.4"`; no omnode process |
| PHONE-1 | 2 | O | Needs the HTTPS host |
| B42 | 1 | P | SPDX headers in 526/526 .rs files under crates/ (`scripts/spdx-scan.sh`, integration-5 0d68f46, 2026-10-21T23:48:30+02:00). `REUSE.toml` and `LICENSES/` (MIT.txt, Apache-2.0.txt) are present on swarm/integration-1 8fd26f6 and swarm/integration-5 0d68f46, but not on main 13a296c, so the row stays P until a merge reaches main (rule below). REUSE gate 7 (`scripts/public-export.sh`, comply 0.1.1): integration-1 PASS (1053 total, 1007 passed, 0 failed, 46 skipped; 2026-10-21T23:49:31+02:00); integration-5 FAIL (1502 total, 1431 passed, 7 failed, 64 skipped; 2026-10-21T23:48:30+02:00): two .rs test fixtures that hold an HTML SPDX string (crates/bench-banking/src/axes.rs:1131, crates/bench-scoreboard/src/lib.rs:119) and five vendored candle-core test binaries (vendor/candle-core/tests/*.pt, *.npy, *.npz, *.pth) without `.license` companions. Fix (not docs, not done here): add the two .rs files to REUSE.toml annotation 2, the way crates/gaia-minerva/src/lib.rs is, and annotate vendor/candle-core (upstream MIT OR Apache-2.0) |
| B16 | 1 | D | scripts/no-python.sh |
| B08 | 1 | D | scripts/size-budget.sh, docs/low-end-profile.md |
| B44 | 1 | D | scripts/sbom.sh, .github/workflows/sbom.yml |
| B45 | 1 | D | SECURITY.md, docs/openssf.md (contact address still TODO) |
| FE-1 | 2 | P | Vite dev replaced on live :8080 by console-dist/srvx 2026-10-27 17:29 CET (CALL-RECORD.md:884); still Node, Rust console not live (CALL-RECORD.md:929). Status not re-scored |
| B61 | 1 | O | docs/console-prod.md checklist; 8080/health 404 |
| B33 | 1 | D | NerveEvent in openmind-schema |
| B40 | 2 | D | (not wired) - crates/openmind-nerve; NERVE-ALIGN says it is not wired |
| ART-1 | 2 | P | hermes-rs delegate_task caps (live); spawn and retire exist only in the ws/gaia sim |
| ELEU-1 | 2 | P | gaia-eleuthia seed parser (ws/gaia, sim only, not merged) |
| B34 | 2 | D | crates/openmind-provider |
| B38 | 1 | D | crates/openmind-provider `fallback` module (the openmind-fallback crate until the 2026-10-20 merge) |
| POS-1 | 1 | O | BOSS.md cap went $5 → $10 (committed f0c69e7, per LIVE.md); no record that the owner approved it |
| POS-2 | 1 | O | RunPod/GPU; owner target ≥1500 GB VRAM, spend not approved |
| HYG-1 | 1 | D | Branches merged on GitHub main; LOCKFILE-NOW: lock = HEAD (Oct 20) |
| HYG-2 | 1 | D | One CARGO_TARGET_DIR per worktree is now a rule in docs/SOURCE-OF-TRUTH.md (2026-10-20 merge) |
| HYG-3 | 1 | N | STAGED.md OMNODE-2 text still disagrees with the VPS tree (landlock 0.4) |
| B60 | 2 | D | MEM-SHADOW-RUN and MEM-SEARCH (Oct 20) recorded parity and key diffs |
| B20 | 1 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B23 | 1 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B24 | 1 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B25 | 1 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B54 | 1 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B55 | 2 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B56 | 1 | D | (not live) - PASS lines on Oct 19 plus ws/survival on GitHub main; units rendered, not installed |
| B58 | 1 | D | (B59 live) - docs/memory-api.md; openmind-memory serves 8420 |
| B59 | 2 | D | (B59 live) - docs/memory-api.md; openmind-memory serves 8420 |
| B12 | 2 | D | (not live) - one-defence, one-talk, galion-one, one-set-model. Live One is older: 8002 still returns 503 |
| B13 | 2 | D | (not live) - one-defence, one-talk, galion-one, one-set-model. Live One is older: 8002 still returns 503 |
| B14 | 1 | D | (not live) - one-defence, one-talk, galion-one, one-set-model. Live One is older: 8002 still returns 503 |
| B15 | 1 | D | (not live) - one-defence, one-talk, galion-one, one-set-model. Live One is older: 8002 still returns 503 |
| B05 | 2 | D | (not live) - Config-driven desk and One, tuner imported. Spare runs only |
| B06 | 2 | D | (not live) - Config-driven desk and One, tuner imported. Spare runs only |
| B53 | 1 | D | (not live) - Config-driven desk and One, tuner imported. Spare runs only |
| B52 | 1 | P | docs/source-dedupe.md lists the roots; nothing removed |
| B47 | 1 | D | Persona TOML and galion-voice-core (galleon-voice dropped in the 2026-10-20 merge), relay tracked, openmind-speech (fixtures), voice-to-voice doc |
| B65 | 1 | D | Persona TOML and galion-voice-core (galleon-voice dropped in the 2026-10-20 merge), relay tracked, openmind-speech (fixtures), voice-to-voice doc |
| B62 | 2 | D | Persona TOML and galion-voice-core (galleon-voice dropped in the 2026-10-20 merge), relay tracked, openmind-speech (fixtures), voice-to-voice doc |
| B46 | 1 | D | Persona TOML and galion-voice-core (galleon-voice dropped in the 2026-10-20 merge), relay tracked, openmind-speech (fixtures), voice-to-voice doc |
| WS-SPEECH | 2 | P | openmind-speech crate exists; no ws/speech stream; vision missing |
| B04 | 1 | D | Docs and crates with PASS lines |
| B03 | 1 | D | Docs and crates with PASS lines |
| B07 | 1 | D | Docs and crates with PASS lines |
| B09 | 1 | D | Docs and crates with PASS lines |
| B22 | 1 | D | Docs and crates with PASS lines |
| B28 | 2 | O | Held behind OMNODE-2 |
| B29 | 1 | D | crates/cgroup-detect |
| B30 | 1 | P | Held ("sandbox dirty"); omnode `sandbox run` exists |
| B31 | 1 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B32 | 1 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B35 | 2 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B36 | 1 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B37 | 1 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B39 | 1 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B41 | 1 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B57 | 2 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B63 | 2 | D | Presets, LAN doc, discovery, llama, exo, ts audit, cortex, mcp-stack, tulu |
| B64 | 1 | O | Archive list done; deletion needs approval |
| B66 | 1 | O | Host decision table done; ingress change needs approval |
| STG-B01 | 1 | O | Staged |
| STG-B11 | 1 | O | Staged |
| STG-OMBIN | 1 | O | Staged |
| STG-OMCLEAN | 1 | O | Staged |
| SEC-4 | 1 | O | tuner-quick trycloudflare is still running (SEC-5) |
| SEC-5 | 1 | O | tuner-quick trycloudflare is still running (SEC-5) |
| OPS-1 | 1 | O | tuner-quick trycloudflare is still running (SEC-5) |
| B48 | 1 | D | docs/arena.md, crates/galleon-arena |
| B49 | 2 | D | docs/arena.md, crates/galleon-arena |
| B50 | 1 | D | docs/arena.md, crates/galleon-arena |

## Swarm branches (updated 2026-10-21T23:50:33+02:00)

Owner Slavik approved a parallel swarm on 2026-10-20 (dated exception in
`docs/SOURCE-OF-TRUTH.md`). Each agent works on one `swarm/<slug>` branch.
Wave 1 is based on `main` 13a296c; later waves start from the base their task
names (main, an integration branch or another swarm branch). These are not
backlog rows and do not count in the percentage. A row above changes only when
its branch is merged into `main`. On 2026-10-21 (23:06 +02:00) Slavik allowed
agents to merge branches that pass the tests and the sandbox into `main`; at
23:50 GitHub `main` was still 13a296c, so none of these is merged yet.

How the tables were made (2026-10-21T23:50:33+02:00): "GitHub head" is the
read-only GitHub branch listing of `galion-studio/obsidian-open-mind` (239
branches, 212 `swarm/*`). "Record" is the last line of the box file
`/workspace/swarm/CALL-RECORD.md` that names `swarm/<slug> <sha>` (line
numbers at 23:50; the file is append-only). Wave-4 slot numbers come from
`/workspace/swarm/plans/wave4-tasks.md`. Short SHAs are 7 characters.

### Wave 1 (2026-10-20, base main 13a296c)

| branch | GitHub head | record | state |
|---|---|---|---|
| `swarm/common` | `26d98b0` | CALL-RECORD.md:96 `26d98b0` | pushed (= record) |
| `swarm/desk-linkwatch` | `a54c792` | - | pushed (no SHA in record) |
| `swarm/docs-sync` | `80e28f3` | - | pushed (no SHA in record) |
| `swarm/edge-boot` | `753676e` | - | pushed (no SHA in record) |
| `swarm/gaia-land` | `9d3ec18` | CALL-RECORD.md:83 `9d3ec18` | pushed (= record) |
| `swarm/gaia-live` | `4fbbb7f` | - | pushed (no SHA in record) |
| `swarm/hades-approval` | `ccaab42` | - | pushed (no SHA in record) |
| `swarm/hermes-tools` | `8ccdfcc` | - | pushed (no SHA in record) |
| `swarm/lan-discovery` | `fe5f0bd` | - | pushed (no SHA in record) |
| `swarm/launch-gates` | `79ee761` | - | pushed (no SHA in record) |
| `swarm/memory-real` | `723b46c` | - | pushed (no SHA in record) |
| `swarm/nerve` | `5802d0e` | CALL-RECORD.md:11 `5802d0e` | pushed (= record) |
| `swarm/one-probe` | `b44cd4f` | - | pushed (no SHA in record) |
| `swarm/poseidon` | `7d0c478` | - | pushed (no SHA in record) |
| `swarm/real-harness` | `5b3c662` | - | pushed (no SHA in record) |
| `swarm/room` | `5bb27e1` | - | pushed (no SHA in record) |
| `swarm/sandbox` | `d194c19` | - | pushed (no SHA in record) |
| `swarm/survival-heal` | `45a18aa` | - | pushed (no SHA in record) |
| `swarm/tuner-cutover` | `5fd1611` | - | pushed (no SHA in record) |
| `swarm/ucpu` | `3e70ffb` | - | pushed (no SHA in record) |
| `swarm/voice-mouth` | `5d776c1` | - | pushed (no SHA in record) |

Also: `ci-fix`, investigated separately (not one of the 21; no GitHub branch at 23:50).

### Wave 2-3 and parent branches (not a wave-4 slot row)

Integration branches, wave-2/3 tasks (WAVE2.md, WAVE3-PREAMBLE.md on the box)
and branches the coordinating agent made itself (main-staging, backups,
deploy and live-fix branches).

| branch | GitHub head | record | state |
|---|---|---|---|
| `swarm/bench-banking` | `adda699` | CALL-RECORD.md:150 `adda699` | pushed (= record) |
| `swarm/bench-harness` | `ea8315c` | CALL-RECORD.md:149 `ea8315c` | pushed (= record) |
| `swarm/bench-hermes` | `457ef74` | - | pushed (no SHA in record) |
| `swarm/bench-scoreboard` | `f2ff169` | CALL-RECORD.md:153 `f2ff169` | pushed (= record) |
| `swarm/bench-vending` | `a9daaf1` | CALL-RECORD.md:152 `a9daaf1` | pushed (= record) |
| `swarm/bench-wallstreet` | `4b1e127` | CALL-RECORD.md:151 `4b1e127` | pushed (= record) |
| `swarm/build-info` | `db9924d` | CALL-RECORD.md:138 `db9924d` | pushed (= record) |
| `swarm/ci-miri-schedule` | `658cbb1` | CALL-RECORD.md:690 `658cbb1` | pushed (= record) |
| `swarm/common-migrate-2` | `123eab2` | CALL-RECORD.md:108 `123eab2` | pushed (= record) |
| `swarm/console-static` | `6bd3580` | CALL-RECORD.md:81 `6bd3580` | pushed (= record) |
| `swarm/console-ui-rust` | `c242fed` | CALL-RECORD.md:106 `c242fed` | pushed (= record) |
| `swarm/context-compact` | `3ea7367` | CALL-RECORD.md:681 `3ea7367` | pushed (= record) |
| `swarm/cortex-event-driven` | `c7f56b8` | CALL-RECORD.md:773 `c7f56b8` | pushed (= record) |
| `swarm/cortex-hermes-boss` | `1c5dec4` | - | pushed (no SHA in record) |
| `swarm/cortex-live-fixes` | `bd6fbe6` | CALL-RECORD.md:795 `bd6fbe6` | pushed (= record) |
| `swarm/cortex-protected-hosts` | `60312ac` | CALL-RECORD.md:680 `60312ac` | pushed (= record) |
| `swarm/cortex-s2-a` | `9557eb3` | - | pushed (no SHA in record) |
| `swarm/cortex-s2-b` | `51100e1` | - | pushed (no SHA in record) |
| `swarm/cortex-s2-c` | `dbec5c9` | - | pushed (no SHA in record) |
| `swarm/cortex-s2-d` | `f3468ce` | - | pushed (no SHA in record) |
| `swarm/cortex-sensor` | `e536446` | CALL-RECORD.md:128 `e536446` | pushed (= record) |
| `swarm/cortex-sources` | `d692b9e` | CALL-RECORD.md:156 `d692b9e` | pushed (= record) |
| `swarm/cutover-prep` | `a5ac49b` | CALL-RECORD.md:123 `a5ac49b` | pushed (= record) |
| `swarm/decision-memos` | `1a0b851` | CALL-RECORD.md:136 `c554d92` | pushed; GitHub head differs from the record SHA `c554d92` (later push) |
| `swarm/deploy-rust` | `787a54d` | - | pushed (no SHA in record) |
| `swarm/deploy-sim` | `c5ab8f9` | CALL-RECORD.md:99 `c5ab8f9` | pushed (= record) |
| `swarm/flake-fix` | `b2e69b9` | CALL-RECORD.md:71 `b2e69b9` | pushed (= record) |
| `swarm/fuzz-sast` | `e78b2e7` | - | pushed (no SHA in record) |
| `swarm/gaia-hades-tokens` | `f3f41a2` | - | pushed (no SHA in record) |
| `swarm/gaia-poseidon` | `498e3d2` | - | pushed (no SHA in record) |
| `swarm/galion-one-deploy` | `a93218d` | CALL-RECORD.md:490 `a93218d` | pushed (= record) |
| `swarm/guard-rust` | `d5f7874` | CALL-RECORD.md:101 `d5f7874` | pushed (= record) |
| `swarm/hades-redteam` | `e6ceca7` | CALL-RECORD.md:159 `e6ceca7` | pushed (= record) |
| `swarm/health-land` | `f4a66ea` | CALL-RECORD.md:134 `f4a66ea` | pushed (= record) |
| `swarm/hermes-core` | `45b7980` | CALL-RECORD.md:104 `45b7980` | pushed (= record) |
| `swarm/hermes-enroll` | `f2121a8` | - | pushed (no SHA in record) |
| `swarm/hermes-tools-quota` | `2b9944e` | CALL-RECORD.md:492 `2b9944e` | pushed (= record) |
| `swarm/integration-1` | `8fd26f6` | CALL-RECORD.md:31 `8fd26f6` | pushed (= record) |
| `swarm/integration-2` | `df42e0f` | CALL-RECORD.md:332 `df42e0f` | pushed (= record) |
| `swarm/integration-4` | `42de672` | CALL-RECORD.md:253 `42de672` | pushed (= record) |
| `swarm/integration-4-squash` | `9b5f6ad` | CALL-RECORD.md:253 `9b5f6ad` | pushed (= record) |
| `swarm/lan-routing` | `e8169c9` | CALL-RECORD.md:82 `e8169c9` | pushed (= record) |
| `swarm/landing-overview` | `669dc18` | CALL-RECORD.md:158 `669dc18` | pushed (= record) |
| `swarm/live-ai-plan` | `644c4f7` | CALL-RECORD.md:271 `644c4f7` | pushed (= record) |
| `swarm/main-backup-13a296c-20261021` | `13a296c` | - | pushed (no SHA in record) |
| `swarm/main-staging` | `cfd7686` | - | pushed (no SHA in record) |
| `swarm/memory-schema` | `7952626` | CALL-RECORD.md:87 `7952626` | pushed (= record) |
| `swarm/nerve-dryrun-live` | `80974b9` | CALL-RECORD.md:665 `80974b9` | pushed (= record) |
| `swarm/nerve-event-driven` | `bb287cc` | CALL-RECORD.md:757 `bb287cc` | pushed (= record) |
| `swarm/nerve-eventlog-deploy` | `0ddce9b` | CALL-RECORD.md:544 `0ddce9b` | pushed (= record) |
| `swarm/nerve-shadow-20261021` | `4800770` | CALL-RECORD.md:339 `4800770` | pushed (= record) |
| `swarm/nerve-wiring` | `8d7a924` | - | pushed (no SHA in record) |
| `swarm/no-openrouter-nerve-cortex` | `49163e0` | CALL-RECORD.md:684 `49163e0` | pushed (= record) |
| `swarm/omnode-enroll` | `26b48a4` | CALL-RECORD.md:682 `26b48a4` | pushed (= record) |
| `swarm/one-hardening` | `fed3de4` | CALL-RECORD.md:85 `fed3de4` | pushed (= record) |
| `swarm/plumbing-check` | `13a296c` | - | pushed (no SHA in record) |
| `swarm/poseidon-deploy` | `d2c5cf1` | CALL-RECORD.md:474 `d2c5cf1` | pushed (= record) |
| `swarm/rust-ports` | `26a2a8e` | CALL-RECORD.md:132 `26a2a8e` | pushed (= record) |
| `swarm/sandbox-real-problems` | `334c633` | CALL-RECORD.md:339 `334c633` | pushed (= record) |
| `swarm/sec-runbooks` | `c5fc269` | CALL-RECORD.md:95 `c5fc269` | pushed (= record) |
| `swarm/settings-snapshot` | `241aecf` | CALL-RECORD.md:258 `309c17b` | pushed; GitHub head differs from the record SHA `309c17b` (later push) |
| `swarm/slavik-todo` | `87fdd91` | CALL-RECORD.md:335 `87fdd91` | pushed (= record) |
| `swarm/sqlite-pure-spike` | `98686ef` | CALL-RECORD.md:108 `98686ef` | pushed (= record) |
| `swarm/swarm-record` | `749ddbd` | CALL-RECORD.md:124 `749ddbd` | pushed (= record) |
| `swarm/tunnel-audit` | `d35ef73` | - | pushed (no SHA in record) |
| `swarm/voice-rust` | `03d21a9` | CALL-RECORD.md:345 `03d21a9` | pushed (= record) |
| `swarm/vps-baseline` | `f403eeb` | CALL-RECORD.md:154 `f403eeb` | pushed (= record) |
| `swarm/vps-deploy-20261021` | `d9bd829` | CALL-RECORD.md:278 `d9bd829` | pushed (= record) |
| `swarm/whitepaper-evidence` | `8c0ac37` | CALL-RECORD.md:283 `8c0ac37` | pushed (= record) |
| `swarm/whitepaper-vending` | `5ddd828` | CALL-RECORD.md:262 `5ddd828` | pushed (= record) |

### Wave 4 (slots 100-244)

At 23:50: 121 pushed, 5 local only (all five in the separate galion-os box
repo), 6 in progress, 13 not started (rows 232-244 were added by the 23:36
refill).

| slot | branch | GitHub head | record | state |
|---|---|---|---|---|
| 100 | `swarm/fuzz-sast-2` | `f3a10af` | CALL-RECORD.md:188 `f3a10af` | pushed (= record) |
| 101 | `swarm/nerve-wiring-2` | `d97dab9` | CALL-RECORD.md:190 `d97dab9` | pushed (= record) |
| 102 | `swarm/hermes-enroll-2` | `c28374e` | CALL-RECORD.md:639 `c28374e` | pushed (= record) |
| 103 | `swarm/tunnel-audit-2` | `988e47f` | CALL-RECORD.md:578 `988e47f` | pushed (= record) |
| 104 | `swarm/cortex-t20-merge` | `ee75f75` | CALL-RECORD.md:191 `ee75f75` | pushed (= record) |
| 105 | `swarm/cortex-state-titles` | `967459d` | CALL-RECORD.md:194 `967459d` | pushed (= record) |
| 106 | `swarm/cortex-event-ingest` | `7a55e6c` | CALL-RECORD.md:195 `7a55e6c` | pushed (= record) |
| 107 | `swarm/cortex-t212-edgerate` | `9a48217` | CALL-RECORD.md:196 `9a48217` | pushed (= record) |
| 108 | `swarm/cortex-score-v2` | `7863e3d` | CALL-RECORD.md:200 `7863e3d` | pushed (= record) |
| 109 | `swarm/candle-zip-fix` | `a4f49b5` | CALL-RECORD.md:199 `a4f49b5` | pushed (= record) |
| 110 | `swarm/integration-3` | `625eaf0` | CALL-RECORD.md:201 `625eaf0` | pushed (= record) |
| 111 | `swarm/integration-verify` | `abd4dea` | CALL-RECORD.md:204 `abd4dea` | pushed (= record) |
| 112 | `swarm/realscript-port-1` | `9d04111` | CALL-RECORD.md:205 `9d04111` | pushed (= record) |
| 113 | `swarm/realscript-port-2` | `cd4bf26` | CALL-RECORD.md:206 `cd4bf26` | pushed (= record) |
| 114 | `swarm/realscript-port-3` | `03967fc` | CALL-RECORD.md:217 `03967fc` | pushed (= record) |
| 115 | `swarm/ci-workflow-rust` | `38a26cb` | CALL-RECORD.md:220 `38a26cb` | pushed (= record) |
| 116 | `swarm/vps-traffic-hermes` | `1c40c5e` | CALL-RECORD.md:219 `1c40c5e` | pushed (= record) |
| 117 | `swarm/vps-strays` | `99e7c58` | CALL-RECORD.md:210 `99e7c58` | pushed (= record) |
| 118 | `swarm/vps-tuner-dns` | `f67f37f` | CALL-RECORD.md:209 `f67f37f` | pushed (= record) |
| 119 | `swarm/backup-perf` | `5c3a8ed` | CALL-RECORD.md:221 `5c3a8ed` | pushed (= record) |
| 120 | `swarm/tuner-quick-unit` | `bff2d94` | CALL-RECORD.md:227 `bff2d94` | pushed (= record) |
| 121 | `swarm/galion-deploy-fastfail` | `7a1a615` | CALL-RECORD.md:228 `7a1a615` | pushed (= record) |
| 122 | `swarm/shared-tar-crate` | `820740e` | CALL-RECORD.md:229 `820740e` | pushed (= record) |
| 123 | `swarm/guard-shadow` | `7b7b1fc` | CALL-RECORD.md:236 `7b7b1fc` | pushed (= record) |
| 124 | `swarm/hades-option-b` | `0f41f29` | CALL-RECORD.md:240 `0f41f29` | pushed (= record) |
| 125 | `swarm/hades-ledger-fail` | `84fef87` | CALL-RECORD.md:241 `84fef87` | pushed (= record) |
| 126 | `swarm/sec-history-purge-plan` | `d45b248` | CALL-RECORD.md:297 `d45b248` | pushed (= record) |
| 127 | `swarm/hermes-core-docs` | `3611fc6` | CALL-RECORD.md:339 `3611fc6` | pushed (= record) |
| 128 | `swarm/hermes-gateway-bench` | `20d9a2f` | CALL-RECORD.md:238 `20d9a2f` | pushed (= record) |
| 129 | `swarm/hermes-core-bench-sector` | `12be83b` | CALL-RECORD.md:315 `12be83b` | pushed (= record) |
| 130 | `swarm/hermes-readiness-recheck` | `7c7a613` | CALL-RECORD.md:301 `7c7a613` | pushed (= record) |
| 131 | `swarm/bench-banking-adv` | `44e5c61` | CALL-RECORD.md:302 `44e5c61` | pushed (= record) |
| 132 | `swarm/bench-ws-adv` | `2ecfc19` | CALL-RECORD.md:300 `2ecfc19` | pushed (= record) |
| 133 | `swarm/bench-vending-adv` | `1b0a30c` | CALL-RECORD.md:313 `1b0a30c` | pushed (= record) |
| 134 | `swarm/bench-data-licence` | `493f175` | CALL-RECORD.md:298 `493f175` | pushed (= record) |
| 135 | `swarm/console-port-1` | `394f065` | CALL-RECORD.md:314 `394f065` | pushed (= record) |
| 136 | `swarm/console-port-2` | `0916684` | CALL-RECORD.md:303 `0916684` | pushed (= record) |
| 137 | `swarm/console-port-3` | `9f32aa9` | CALL-RECORD.md:317 `9f32aa9` | pushed (= record) |
| 138 | `swarm/console-port-4` | `f14dbdc` | CALL-RECORD.md:318 `f14dbdc` | pushed (= record) |
| 139 | `swarm/console-port-5` | `38b92a7` | CALL-RECORD.md:316 `38b92a7` | pushed (= record) |
| 140 | `swarm/whitepaper-update` | `cc00dc4` | CALL-RECORD.md:632 `cc00dc4` | pushed (= record) |
| 141 | `swarm/slavik-todo-hr` | `f72999e` | CALL-RECORD.md:335 `f72999e` | pushed (= record) |
| 142 | `swarm/processes-refresh` | `73e1641` | CALL-RECORD.md:338 `73e1641` | pushed (= record) |
| 143 | `swarm/galion-os-threat-model` | - | CALL-RECORD.md:336 `f76eb96` | **local only**, in the separate box repo /workspace/swarm/galion-os-repo (not obsidian-open-mind) |
| 144 | `swarm/galion-os-sandbox-proto` | - | CALL-RECORD.md:446 `4374605` | **local only**, in the separate box repo /workspace/swarm/galion-os-repo (not obsidian-open-mind) |
| 145 | `swarm/toolchain-aarch64` | `4d0c036` | CALL-RECORD.md:344 `4d0c036` | pushed (= record) |
| 146 | `swarm/serde-order-pin` | `1d222ad` | CALL-RECORD.md:378 `1d222ad` | pushed (= record) |
| 147 | `swarm/voice-speed` | `ff6c34f` | CALL-RECORD.md:407 `ff6c34f` | pushed (= record) |
| 148 | `swarm/one-no-shellout` | `40454f9` | CALL-RECORD.md:488 `40454f9` | pushed (= record) |
| 149 | `swarm/health-fixes` | `62ddad8` | CALL-RECORD.md:381 `62ddad8` | pushed (= record) |
| 150 | `swarm/sandbox-hide-galion` | `ad0ed9b` | CALL-RECORD.md:390 `ad0ed9b` | pushed (= record) |
| 151 | `swarm/sandbox-workdir-quota` | `28f6c1d` | CALL-RECORD.md:456 `28f6c1d` | pushed (= record) |
| 152 | `swarm/gaia-hades-typed` | `c223109` | CALL-RECORD.md:410 `c223109` | pushed (= record) |
| 153 | `swarm/deny-paste-replace` | `4bfffe8` | CALL-RECORD.md:408 `4bfffe8` | pushed (= record) |
| 154 | `swarm/candle-zip-rebase` | `b246f66` | CALL-RECORD.md:416 `b246f66` | pushed (= record) |
| 155 | `swarm/nerve-wiring-rebase` | `8046c41` | CALL-RECORD.md:413 `8046c41` | pushed (= record) |
| 156 | `swarm/sqlite-aarch64-cc` | `af6b55c` | CALL-RECORD.md:455 `af6b55c` | pushed (= record) |
| 157 | `swarm/nerve-dst-clock` | `c229a87` | CALL-RECORD.md:412 `c229a87` | pushed (= record) |
| 158 | `swarm/cortex-merge-all` | `6bcb882` | CALL-RECORD.md:472 `6bcb882` | pushed (= record) |
| 159 | `swarm/cortex-t26-ingresslint` | `036f891` | CALL-RECORD.md:448 `036f891` | pushed (= record) |
| 160 | `swarm/gaia-tick-loop` | `e4631a6` | CALL-RECORD.md:493 `e4631a6` | pushed (= record) |
| 161 | `swarm/poseidon-refusal-persist` | `eec2d99` | CALL-RECORD.md:714 `eec2d99` | pushed (= record) |
| 162 | `swarm/hermes-bench-e2e` | `2b11a5b` | CALL-RECORD.md:473 `2b11a5b` | pushed (= record) |
| 163 | `swarm/hermes-legacy-retire` | `7622b8b` | CALL-RECORD.md:445 `7622b8b` | pushed (= record) |
| 164 | `swarm/galion-os-box-escape` | - | CALL-RECORD.md:469 `7c7644e` | **local only**, in the separate box repo /workspace/swarm/galion-os-repo (not obsidian-open-mind) |
| 165 | `swarm/galion-os-admin-cli` | - | CALL-RECORD.md:485 `bf33cba` | **local only**, in the separate box repo /workspace/swarm/galion-os-repo (not obsidian-open-mind) |
| 166 | `swarm/vps-linkenv-doc` | `8c257b9` | CALL-RECORD.md:437 `8c257b9` | pushed (= record) |
| 167 | `swarm/vps-dst-check` | `56c8a7f` | CALL-RECORD.md:436 `56c8a7f` | pushed (= record) |
| 168 | `swarm/vps-anomalies` | `4e5f814` | CALL-RECORD.md:483 `4e5f814` | pushed (= record) |
| 169 | `swarm/console-port-merge` | `508401d` | CALL-RECORD.md:517 `508401d` | pushed (= record) |
| 170 | `swarm/console-route-31` | `1776d16` | CALL-RECORD.md:554 `1776d16` | pushed (= record) |
| 171 | `swarm/docs-realtests-index` | `dc16955` | CALL-RECORD.md:555 `dc16955` | pushed (= record) |
| 172 | `swarm/docs-thirdparty-register` | `7cd7d0c` | CALL-RECORD.md:564 `7cd7d0c` | pushed (= record) |
| 173 | `swarm/bench-scoreboard-adv` | `1f0556d` | CALL-RECORD.md:565 `1f0556d` | pushed (= record) |
| 174 | `swarm/bench-ws-ecb-data` | `03714aa` | CALL-RECORD.md:549 `03714aa` | pushed (= record) |
| 175 | `swarm/gaia-loop-decisions` | `b40208a` | CALL-RECORD.md:760 `b40208a` | pushed (= record) |
| 176 | `swarm/sandbox-quota-hide-merge` | `78c5c49` | CALL-RECORD.md:548 `78c5c49` | pushed (= record) |
| 177 | `swarm/integration-5` | `0d68f46` | CALL-RECORD.md:540 `0d68f46` | pushed (= record) |
| 178 | `swarm/cortex-connrate-fix` | `c1a15f7` | CALL-RECORD.md:534 `c1a15f7` | pushed (= record) |
| 179 | `swarm/cortex-t32-correlate` | `ae604dc` | CALL-RECORD.md:606 `ae604dc` | pushed (= record) |
| 180 | `swarm/nerve-t36-consumer` | `334beaa` | CALL-RECORD.md:521 `334beaa` | pushed (= record) |
| 181 | `swarm/mouth-loading-diag` | `a68ad3e` | CALL-RECORD.md:520 `a68ad3e` | pushed (= record) |
| 182 | `swarm/vps-baseline-2` | `f761f49` | CALL-RECORD.md:581 `f761f49` | pushed (= record) |
| 183 | `swarm/vps-unit-hygiene` | `b985e43` | CALL-RECORD.md:572 `b985e43` | pushed (= record) |
| 184 | `swarm/docs-cutover-index` | `ed5f740` | CALL-RECORD.md:577 `ed5f740` | pushed (= record) |
| 185 | `swarm/cortex-t52-fuzz` | `c2f7cda` | CALL-RECORD.md:581 `c2f7cda` | pushed (= record) |
| 186 | `swarm/nerve-t32-reflex-safety` | `c1a9be8` | CALL-RECORD.md:595 `c1a9be8` | pushed (= record) |
| 187 | `swarm/nerve-t33-backpressure` | `dfa93a1` | CALL-RECORD.md:595 `dfa93a1` | pushed (= record) |
| 188 | `swarm/gaia-heal-tick-unit` | `9468bb4` | CALL-RECORD.md:600 `9468bb4` | pushed (= record) |
| 189 | `swarm/gaia-fault-inject` | `e970622` | CALL-RECORD.md:597 `e970622` | pushed (= record) |
| 190 | `swarm/sec-miri-pure` | `91353ba` | CALL-RECORD.md:617 `91353ba` | pushed (= record) |
| 191 | `swarm/sandbox-seebystander` | `b47f53d` | CALL-RECORD.md:609 `b47f53d` | pushed (= record) |
| 192 | `swarm/cortex-t42-hades-plans` | `1ce9737` | CALL-RECORD.md:613 `1ce9737` | pushed (= record) |
| 193 | `swarm/cortex-t53-replay-corpus` | `55f07ff` | CALL-RECORD.md:624 `55f07ff` | pushed (= record) |
| 194 | `swarm/vps-version-flags` | `81991e2` | CALL-RECORD.md:624 `81991e2` | pushed (= record) |
| 195 | `swarm/vps-preflight-scripts` | `7733aa9` | CALL-RECORD.md:635 `7733aa9` | pushed (= record) |
| 196 | `swarm/ci-toolchain-pin` | `e871395` | CALL-RECORD.md:629 `e871395` | pushed (= record) |
| 197 | `swarm/whitepaper-factcheck` | `58d0d73` | CALL-RECORD.md:636 `58d0d73` | pushed (= record) |
| 198 | `swarm/hermes-mem-growth` | `c2457cd` | CALL-RECORD.md:664 `c2457cd` | pushed (= record) |
| 199 | `swarm/gaia-fault-followups` | `e806777` | CALL-RECORD.md:696 `e806777` | pushed (= record) |
| 200 | `swarm/nerve-t23-specs` | `7c32ec3` | CALL-RECORD.md:711 `7c32ec3` | pushed (= record) |
| 201 | `swarm/sec-deny-wildcards` | `2482909` | CALL-RECORD.md:699 `2482909` | pushed (= record) |
| 202 | `swarm/ci-fuzz-workflow` | `3a188b9` | CALL-RECORD.md:700 `3a188b9` | pushed (= record) |
| 203 | `swarm/docs-cutover-missing` | `878485d` | CALL-RECORD.md:703 `878485d` | pushed (= record) |
| 204 | `swarm/galion-os-image-format` | - | CALL-RECORD.md:683 `f35f735` | **local only**, in the separate box repo /workspace/swarm/galion-os-repo (not obsidian-open-mind) |
| 205 | `swarm/poseidon-auth` | `d10cb09` | CALL-RECORD.md:712 `d10cb09` | pushed (= record) |
| 206 | `swarm/poseidon-crash-soak` | `b71a909` | CALL-RECORD.md:756 `b71a909` | pushed (= record) |
| 207 | `swarm/cortex-t31-calibrate` | `b1c3efd` | CALL-RECORD.md:713 `b1c3efd` | pushed (= record) |
| 208 | `swarm/hades-safety-corpus` | `3c30586` | CALL-RECORD.md:729 `3c30586` | pushed (= record) |
| 209 | `swarm/hades-rustflags-guard` | `b187398` | CALL-RECORD.md:722 `b187398` | pushed (= record) |
| 210 | `swarm/vps-rollback-table` | `7a1990e` | CALL-RECORD.md:721 `7a1990e` | pushed (= record) |
| 211 | `swarm/int5-candle-repocheck` | `58a89bc` | CALL-RECORD.md:750 `58a89bc` | pushed (= record) |
| 212 | `swarm/nerve-parser-fuzz` | `5ad5d99` | CALL-RECORD.md:762 `5ad5d99` | pushed (= record) |
| 213 | `swarm/ci-realtest-with-nerve` | `7a95e06` | CALL-RECORD.md:751 `7a95e06` | pushed (= record) |
| 214 | `swarm/gaia-lan-fallback-test` | `b8358fb` | CALL-RECORD.md:736 `b8358fb` | pushed (= record) |
| 215 | `swarm/hades-key-custody` | `84e988f` | CALL-RECORD.md:735 `84e988f` | pushed (= record) |
| 216 | `swarm/ci-release-artefacts` | `04ce874` | CALL-RECORD.md:742 `04ce874` | pushed (= record) |
| 217 | `swarm/nerve-cross-target` | `71f895b` | CALL-RECORD.md:737 `71f895b` | pushed (= record) |
| 218 | `swarm/gaia-poseidon-handle` | `dd39191` | CALL-RECORD.md:752 `dd39191` | pushed (= record) |
| 219 | `swarm/sec-ledger-race` | `8234150` | CALL-RECORD.md:774 `8234150` | pushed (= record) |
| 220 | `swarm/sec-denylist-parity` | `688d883` | CALL-RECORD.md:775 `688d883` | pushed (= record) |
| 221 | `swarm/gaia-queue-fuzz` | `451db69` | CALL-RECORD.md:792 `451db69` | pushed (= record) |
| 222 | `swarm/cortex-t44-e2e` | `7893698` | CALL-RECORD.md:779 `7893698` | pushed (= record) |
| 223 | `swarm/nerve-t41-statedir` | `cd359d6` | CALL-RECORD.md:776 `cd359d6` | pushed (= record) |
| 224 | `swarm/gaia-live-classify` | `055fe91` | CALL-RECORD.md:781 `055fe91` | pushed (= record) |
| 225 | `swarm/sec-testnet-fixtures` | - | - | in progress at 23:50 (resumed 23:46-23:47, no DONE yet; wt head `c898f6c`) |
| 226 | `swarm/nerve-t24-coverage-gate` | - | - | in progress at 23:50 (resumed 23:46-23:47, no DONE yet; wt head `0d68f46`) |
| 227 | `swarm/cortex-t51-chaos` | `e982210` | CALL-RECORD.md:782 `e982210` | pushed (= record) |
| 228 | `swarm/ci-artefact-rehearsal` | - | - | in progress at 23:50 (resumed 23:46-23:47, no DONE yet; wt head `7a2afb2`) |
| 229 | `swarm/sec-unsafe-safety` | - | - | in progress at 23:50 (resumed 23:46-23:47, no DONE yet; wt head `0d68f46`) |
| 230 | `swarm/gaia-realtest-scenarios` | - | - | in progress at 23:50 (resumed 23:46-23:47, no DONE yet; wt head `7a95e06`) |
| 231 | `swarm/docs-status-refresh` | - | - | this branch (in progress) |
| 232 | `swarm/vps-live-table` | - | - | not started |
| 233 | `swarm/nerve-t61-incidents` | - | - | not started |
| 234 | `swarm/sec-tooling-licence-memo` | - | - | not started |
| 235 | `swarm/cortex-flap-dedupe` | - | - | not started |
| 236 | `swarm/cortex-api-load` | - | - | not started |
| 237 | `swarm/cortex-incident-backfill` | - | - | not started |
| 238 | `swarm/nerve-own-services` | - | - | not started |
| 239 | `swarm/memory-schema-mergefix` | - | - | not started |
| 240 | `swarm/serde-pin-mergefix` | `4137301` | CALL-RECORD.md:870 | done; push pending in record |
| 241 | `swarm/no-openrouter-mergefix` | `4b835a6` | CALL-RECORD.md:856 | LOCAL on purpose (D-241-1/2) |
| 242 | `swarm/shared-tar-mergefix` | `4eca4b7` | CALL-RECORD.md:856 | LOCAL, push blocked (D-242-1) |
| 243 | `swarm/console-v2-audit` | `2a01231` | CALL-RECORD.md:899 | pushed (on GH per record) |
| 244 | `swarm/hermes-gateway-rss` | `7117d54` | CALL-RECORD.md:927 | pushed 2026-10-27 17:34:50 CET |
