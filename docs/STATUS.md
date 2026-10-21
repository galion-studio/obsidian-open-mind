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
| B51 | 2 | O | docs/dist-switch.md is staged. Spare-port runs PASS (REAL-*-2). Cutover approval: "no" |
| OMNODE-2 | 2 | O | VPS edge/openmind-node still on `landlock = "0.4"`; no omnode process |
| PHONE-1 | 2 | O | Needs the HTTPS host |
| B42 | 1 | P | SPDX headers in 526/526 .rs files under crates/ (`scripts/spdx-scan.sh`, integration-5 0d68f46, 2026-10-21T23:48:30+02:00). `REUSE.toml` and `LICENSES/` (MIT.txt, Apache-2.0.txt) are present on swarm/integration-1 8fd26f6 and swarm/integration-5 0d68f46, but not on main 13a296c, so the row stays P until a merge reaches main (rule below). REUSE gate 7 (`scripts/public-export.sh`, comply 0.1.1): integration-1 PASS (1053 total, 1007 passed, 0 failed, 46 skipped; 2026-10-21T23:49:31+02:00); integration-5 FAIL (1502 total, 1431 passed, 7 failed, 64 skipped; 2026-10-21T23:48:30+02:00): two .rs test fixtures that hold an HTML SPDX string (crates/bench-banking/src/axes.rs:1131, crates/bench-scoreboard/src/lib.rs:119) and five vendored candle-core test binaries (vendor/candle-core/tests/*.pt, *.npy, *.npz, *.pth) without `.license` companions. Fix (not docs, not done here): add the two .rs files to REUSE.toml annotation 2, the way crates/gaia-minerva/src/lib.rs is, and annotate vendor/candle-core (upstream MIT OR Apache-2.0) |
| B16 | 1 | D | scripts/no-python.sh |
| B08 | 1 | D | scripts/size-budget.sh, docs/low-end-profile.md |
| B44 | 1 | D | scripts/sbom.sh, .github/workflows/sbom.yml |
| B45 | 1 | D | SECURITY.md, docs/openssf.md (contact address still TODO) |
| FE-1 | 2 | N | Console still runs vite dev; no fix commits |
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

## Swarm branches (2026-10-20)

Owner Slavik approved a parallel swarm on 2026-10-20 (dated exception in
`docs/SOURCE-OF-TRUTH.md`). Each agent works on one `swarm/<slug>` branch based
on `main` 13a296c; none is merged, and the owner merges. These are not backlog
rows and do not count in the percentage. A row above changes only when its
branch is merged into `main`.

| # | branch |
|---|---|
| 1 | `swarm/gaia-land` |
| 2 | `swarm/hades-approval` |
| 3 | `swarm/gaia-live` |
| 4 | `swarm/survival-heal` |
| 5 | `swarm/one-probe` |
| 6 | `swarm/voice-mouth` |
| 7 | `swarm/tuner-cutover` |
| 8 | `swarm/room` |
| 9 | `swarm/memory-real` |
| 10 | `swarm/nerve` |
| 11 | `swarm/lan-discovery` |
| 12 | `swarm/poseidon` |
| 13 | `swarm/ucpu` |
| 14 | `swarm/real-harness` |
| 15 | `swarm/launch-gates` |
| 16 | `swarm/common` |
| 17 | `swarm/hermes-tools` |
| 18 | `swarm/desk-linkwatch` |
| 19 | `swarm/sandbox` |
| 20 | `swarm/edge-boot` |
| 21 | `swarm/docs-sync` |

Also: `ci-fix`, being investigated separately (not one of the 21).
