<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Source of truth

- Written: 2026-10-20 (Europe/Zagreb, +02:00), with the one-source merge
  (`merge/one-source-20261020`). Approved by the owner (Slavik).
- This file replaces the "private only, no remote, no push" rule in
  `docs/next.md` and the VPS-working-tree habit of the 2026-10-19/20 waves.

## The rules

1. **GitHub is the only source of truth:**
   `https://github.com/galion-studio/obsidian-open-mind` (private), branch `main`.
   Nothing else is a line of development: not the VPS repo, not a box clone,
   not a laptop copy.
2. **Agents work on branches and pull requests, never on the VPS working
   tree.** One branch per task (`ws/<name>`, `fix/<name>`, ...), one PR per
   branch. `/opt/galion/openmind` on the VPS is a deploy checkout only. It has
   a plain https `origin` remote (no credentials in git config) so it can
   fetch. Nobody commits there.
3. **At most 3 agents at once**, across Grok Bot, Grok Build and Hermes
   together.
4. **Builds run on the box or in CI, never on production.** Each worktree has
   its own `CARGO_TARGET_DIR` (for example `/workspace/<worktree>-target`);
   no per-desk target dirs in the VPS `/tmp`.
5. **Nothing is pushed straight to `main`.** Every change lands through a PR
   with the self-eval gate green (`.github/workflows/self-eval.yml`: fmt,
   clippy `-D warnings`, test, tier-1 musl builds, repo-check, no-Python,
   licence, gitleaks). The owner merges.
6. **Deploys come from a merged `main` commit**, built on the box or in CI and
   copied as a binary. A live cutover still needs the owner's approval
   (`docs/cutover-approval.md`).
7. **Status lives in one table:** `docs/STATUS.md`, percentage computed by
   `scripts/status-percent.sh`. No record-only commits (port lists, pid
   snapshots, `ALREADY` notes); put observations in the PR or in STATUS.md.

## The 2026-10-20 merge (what became one)

GitHub `main` 317d1d4 and VPS `main` 2f5a730 forked at 74d52ec (2026-10-18).
The merge branch starts at 317d1d4 and merges 2f5a730 with a real merge commit
(through c7aaf64, which only normalises the copyright headers), so both
histories are kept. The VPS line is also on GitHub as `vps-main-20261020`.

| crate / area | winner | reason |
|---|---|---|
| copyright headers, LICENSE | GitHub `3000 Galion / galion-studio` | owner's licence rule; LICENSE stays MIT |
| galion-one | GitHub | portable, pure-Rust read-only SQLite; VPS build.rs pinned x86_64 glibc libsqlite3 |
| openmind-memory | GitHub | the build live on 8420; the VPS rewrite's routes are a subset |
| openmind-sandbox, openmind-schema, guardian-tick, tuner, link-watch, zagreb-time | VPS | better hardened / more complete and better tested (review) |
| openmind-survival | VPS + GitHub additive modules | VPS `decide` is canonical; GitHub `degrade`, `nerve`, `patterns`, `http_get`/`verdict` kept (galion-boot, guardian-tick use them) |
| galion-desk, desk-hermes, room | VPS | config-driven desk (B05) and DESK-3; GitHub side changed only headers/env paths |
| persona-plex-relay | GitHub lib + VPS main.rs | keeps the galion-voice-core backend and RELAY-2 behaviour |
| openmind-config, edge/openmind-node | auto-merged | no conflicts |
| boot: galion-boot vs openmind-boot + openmind-units | **galion-boot** | superset (probe, renderer, secrets, supervisor) and drives `deploy/staged` |
| origin: galion-origin vs openmind-origin | **galion-origin** | a real loopback server with a staged unit; the other is a 302 builder with no listener |
| healer/guardian: galion-healer + galion-guardian vs openmind-survival + guardian-tick | **openmind-survival + guardian-tick** | owner rule (take VPS survival and guardian-tick); the GitHub daemons used the GitHub `decide` API that no longer exists; neither had an installed unit |
| voice: galion-voice-core vs galleon-voice | **galion-voice-core** | real VAD/jitter/duplex engine that persona-plex-relay uses; persona loading stays in openmind-persona |
| gaia-core: ws/gaia vs VPS gaia-core (+ gaia-registry) | **ws/gaia** | full orchestrator (52 systems, approval-token HADES path); lands through its own PR |
| openmind-route, openmind-fallback | folded into openmind-provider (`route`, `fallback`) | micro-crates |
| hades-journal | folded into hades-gate (`journal`) | micro-crate |

Removed crates stay in history: the VPS ones at 2f5a730, the GitHub ones at
317d1d4. No live unit or script refers to a removed crate path (the live
binaries are prebuilt; the only live path inside the VPS repo is the
untracked `dist/bin-ws/galion-one`).

Follow-ups and what was dropped are listed in the merge pull request.
