<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# VPS working copy drift: `/opt/galion/openmind` is at 202887d5, RULES pin says 2f5a730

Slot 391 `openmind-pin-drift` (wave 4), 2026-10-28 14:49–14:56 CET (+01:00).
Read-only: `git rev-parse` / `git log` (plus `log -g`, `diff --stat/--name-status`,
`ls-files`, all with `GIT_OPTIONAL_LOCKS=0`), `systemctl status`, `stat`,
`sha256sum`, loopback GETs. Nothing on the VPS was written, checked out or
restarted. Raw outputs: `docs/real-tests/openmind-pin-drift.md`.

## Short answer

The drift is real and has one cause: on **2026-10-27 at 19:42:42 CET** the
overnight-report push task (t312u, run as part of the overnight loop) used the
live working copy itself as its git clone. It checked out the branch
`swarm/overnight-loop-20261027`, committed, reset that branch to
`origin/swarm/overnight-loop-20261027` and committed again (19:43:11 CET).
Nothing has touched the repo's git state since (last reflog entry 19:43:11 CET).
That broke hard rule 3 ("never commit, checkout ... there"). **No live
service was hurt**: none of them runs from tracked files in that tree.

## What 202887d5 is

| fact | value |
|---|---|
| HEAD | `202887d56ddbede97acfc47bebd262a6eedd2a39`, attached to branch `swarm/overnight-loop-20261027` |
| parent chain | 202887d5 → 9fa897b9 → **13a296ce** (GitHub main, the swarm base in RULES §Setup) |
| 202887d5 | adds `docs/swarm-overnight/overnight-loop-log.md` (31 lines), committer `Hermes (OpenMind) <hermes@localhost>` |
| 9fa897b9 | adds `swarm/reports/overnight-loop-log.md` (31 lines), committer `Galion Swarm` |
| relation to 2f5a730 | 2f5a730 is an **ancestor** of 202887d5 (merge base = 2f5a730). 13a296ce merged VPS main 2f5a730 into GitHub main (commit 1a90b778, "one source of truth"). So 202887d5 = 2f5a730 + 60 commits from GitHub main + 2 report-only commits = 62 commits, 0 commits lost |
| tree diff 2f5a730 → 202887d5 | 507 files: 185 added, 282 modified, 30 deleted, 10 renamed; +23,820 / −8,614 lines. Mostly crates/ (45.5 %), docs/ (17.7 %), brand/ (10.6 %), deploy/ (4.9 %). Deletes are the micro-crates folded by the one-source merge (gaia-core, gaia-registry, galleon-voice, openmind-fallback/route/units/origin/boot, hades-journal) |
| tracked `dist/` | **no change** between the two commits; `dist/bin/SHA256SUMS` still verifies 5/5 OK; files dated 2026-10-18 |
| untracked rollback binary | `dist/bin-ws/galion-one` 828,640 B, sha256 `090c15d0…0191` — same as `docs/cutover/galion-one.md` |
| dirty tracked files | 0 (`git diff --name-only HEAD`) |
| untracked files | 240 (mostly `stage/console-src/`, `dist/bin-ws/`), untouched by the checkout |
| local `main` | still `2f5a730b` (the old pin is intact, one `checkout main` away) |
| orphan | `b3b95e33` (first 19:42:58 attempt, parent 98466ab9 main-staging) is only in the reflog; no value in it |
| remote | `origin/swarm/overnight-loop-20261027` is now 4 commits ahead (`9056c37a`, 19:58 CET, pushed from another clone, last fetch 20:04 CET) |

## Reflog (Zagreb time)

```
19:42:42 +0100  checkout: moving from main to swarm/overnight-loop-20261027   (98466ab9, main-staging line)
19:42:58 +0100  commit:   overnight: loop log bootstrap 19:42 CET (t312u)    -> b3b95e33
19:43:11 +0100  checkout: swarm/overnight-loop-20261027 -> same branch
19:43:11 +0100  reset:    moving to origin/swarm/overnight-loop-20261027     -> 9fa897b9
19:43:11 +0100  commit:   overnight: loop log bootstrap 19:43 CET (t312u)    -> 202887d5
before: 2026-10-20 10:44:56 +0200  commit ONE-PROBE ... -> 2f5a730b  (the RULES pin)
```

The checkout at 19:42:42 rewrote the 507 tracked paths (mtimes on
`docs/ws-hermes.md`, `scripts/no-python.sh` = 19:42:42 CET, `crates/hermes-rs/Cargo.toml`
= 19:43:11 CET).

## Live impact (checked 14:51–14:54 CET)

- `/etc/systemd/system` has exactly one unit naming the tree: `galion-one.service`,
  `ExecStart=/opt/galion/openmind/dist/bin-ws/galion-one`, but its drop-in
  `50-rust-bin.conf` clears it and runs `/opt/galion/rust/bin/galion-one`.
  Active since 2026-10-22 22:46:47 CET (5 days). So the old binary is a rollback
  path only, and it is intact.
- No running process has its exe or cwd under `/opt/galion/openmind`.
- No unit or cron references `openmind/{dist/bin,target,scripts,deploy}`.
- Nerve `:8670/state` 200, cortex `:8674/health` 200. The nerve shows the open
  incident `galion-one-mouth ... stuck loading for 20.2 h` (14:30 CET), which comes
  from the mouth model loading, not from this tree (galion-one binary is outside it).

Verdict: **cosmetic for live**, but real for the rules: every doc, RULES.md line
3, `config/swarm/swarm-settings.toml` (`vps = "... at 2f5a730 untouched"`) and the
console-ui overview text (`crates/console-ui/src/overview.rs`, "stays at
2f5a730") now state something false. The live console on `:8080` does not show
that string in its HTML/API (checked `/`, `/overview`, `/api/v1/overview`).

## Findings

- **F-391-1** Hard rule 3 was broken by the 2026-10-27 19:42 CET overnight push
  (t312u). Cause: the push step used `/opt/galion/openmind` as its clone instead
  of a separate one. A git process in that tree also wrote a new commit
  identity (`hermes@localhost`).
- **F-391-2** Pin text is stale in RULES.md, swarm-settings.toml and
  console-ui overview.rs (also several docs: cutover/desk.md, cutover/galion-one.md,
  cutover/README.md, staged/hermes-tools-unit.md, security/SEC-HISTORY-PURGE-PLAN.md).
- **F-391-3** The working copy now tracks a `swarm/*` branch that other clones
  keep pushing to. Any future `git pull` there would pull report commits into
  the live tree; the rollback binary in untracked `dist/bin-ws/` survives that,
  but it is the wrong home for report work.
- **F-391-4** No content was lost: 2f5a730 is an ancestor of HEAD and local
  `main` still points at it.

## Owner decision proposal (doc only; nothing applied)

**D-391-1 — which pin.** Options:

| | action | VPS write | live risk | note |
|---|---|---|---|---|
| A (recommended) | **accept 202887d5 as the documented state now**, update the pin text to "HEAD 202887d5 = GitHub main 13a296ce + 2 report-only commits, branch swarm/overnight-loop-20261027" | none | none | tree now matches the swarm base everyone builds from; nothing to roll back |
| B | restore: `git -C /opt/galion/openmind checkout main` (back to 2f5a730) in a maintenance window | yes (507 paths) | very low (no service runs tracked files) | brings back the exact old wording; touches the tree a second time |
| C | detach at `13a296ce` (`checkout --detach 13a296ce`) | yes (2 files) | none | cleanest content (pure GitHub main, off any swarm branch); still a write |

Recommendation: **A now**, and **C** in the next owner maintenance window if
you want the tree off a swarm branch (F-391-3). B gains nothing: the code it
brings back is already contained in 13a296ce.

**D-391-2 — stop it happening again.** Add to RULES hard rule 3: "no git
command of any kind that writes (checkout, commit, reset, pull, fetch) in
`/opt/galion/openmind`; report pushes use the box `push-branch.sh` or a
separate clone such as `/opt/galion/swarm-reports-git`". Recommend yes.

**D-391-3 — text fix.** After D-391-1, one follow-up doc/config branch updates
RULES.md, swarm-settings.toml, console-ui overview.rs and the docs listed in
F-391-2 to the chosen pin. Not done here (task is doc-only, and the text
depends on your pick).

**D-391-4 — orphan b3b95e33.** Leave it; it expires with the reflog. No action.
