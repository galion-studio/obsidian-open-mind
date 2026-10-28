<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# SEC-1..5: git history purge plan (plan only)

Written 2026-10-21T04:55:00+02:00 (Europe/Zagreb), wave 4 slot 126,
branch `swarm/sec-history-purge-plan` (base main 13a296c).
**Plan only.** No history was rewritten, nothing was force-pushed, no GitHub
setting was changed, no secret was read, rotated or copied. The VPS was
probed read-only (GETs, `ls`/`find` metadata, two ref lists). Affected
blobs are named **by path only**; no blob contents were read.

This plan builds on `swarm/sec-runbooks` c5fc269 (slot 26): the
`galion-purge` tool (`crates/history-purge`), `docs/launch/SEC-1-ROTATION.md`,
`docs/launch/SEC-1-HISTORY-PURGE.md`, `docs/launch/SEC-RUNBOOKS.md` and
`docs/launch/purge-paths.obsidian-open-mind.txt`. That branch is not on main.
This document refreshes its numbers against **today's** history (141 refs,
470 commits instead of 27 refs, 262 commits) and adds the copy inventory,
the per-SEC mapping and the owner decisions.

## 1. Bottom line (what Slavik needs to know)

1. **No raw secret value is in this repo's history.** gitleaks 8.30.1 over
   all 470 commits of all 141 refs: 3 findings, all in one test fixture
   (`crates/openmind-nerve/tests/fixtures/hades/ledger.jsonl`, branch
   `swarm/cortex-s2-d` f3468ce), all of field `token_sha256_16`: a 16-hex
   prefix of a SHA-256 of a token, not a token. False positive (see D6).
   The root password lines in `docs/session-2026-10-18*.md` say "rotate";
   they never contain the value (checked with the value masked).
2. The **leaked values** (GitHub token, tunnel token in
   `gateway-deaths.log`, root password from a screenshot) live on the VPS
   and in chat, not in git. **Rotation is the real fix; the purge is
   hygiene** that removes the secret *inventory* (names and paths of every
   secret file), operator material and committed binaries.
3. History today: **96 paths, 93 blob ids** match the 6-pattern purge
   list, and **every one of the 141 refs** still carries them at its tip
   (main: 85 such paths). The purge changes **all 470 commit ids** (the
   root commit 9ddf211 already has them).
4. Rehearsal on a throwaway copy: 2.6 s, 0 listed paths after, 90 blob ids
   gone, 3 kept only because identical text lives at kept paths (MIT LICENSE
   texts and `.license` sidecars), fsck clean, refs/commits/merges 141/470/64
   unchanged, size 20 MB -> 8 MB, source mirror byte-identical.
5. **Recommended route (A):** rotate first (Oct 21-26), then publish a
   *new* public repo from the export tree (fresh history) and keep this repo
   private and unrewritten. Rewriting this private repo (route B) is
   optional and costs a full re-clone/rebase of ~110 branches.

## 2. What is in history, per SEC item (paths only)

Full path list: `docs/security/history-purge-affected-paths.txt` (96 lines).

| SEC | Owner item | In git history? | Paths (counts) | History action |
|---|---|---|---|---|
| SEC-1 | rotations + purge (O1) | inventory only: `*.redacted` files carry variable **names**, values are `<REDACTED:NAME>` markers | `stack/kms/` 30 (e.g. `stack/kms/root/galion-secrets/github.token.redacted`, `.../from-grok-build/*.env.redacted`), `stack/hermes/{.env,auth.json,config.yaml}.redacted` 3, `stack/INDEX.md` 1 | purge list rows `stack/kms/`, `*.redacted`, `stack/INDEX.md` |
| SEC-1 | operator material | yes | `contrib/grokbot/` 41 (VPS MCP server source, study notes), `contrib/ui/` 4 | row `contrib/` |
| SEC-1 | committed binaries | yes | `dist/bin/` 11, `edge/dist/` 6 | rows `dist/bin/`, `edge/dist/` |
| SEC-2 | log containing the tunnel token | **no**: no `*.log` and no `gateway-deaths` path in any commit | none | none (VPS-side only, SEC-RUNBOOKS.md) |
| SEC-3 | Hermes 666/777 perms | no (perms are not in git); the *redacted* config is | `stack/hermes/config.yaml.redacted` | covered by `*.redacted` |
| SEC-4 | private key copy in from-grok-build | only the **name**: `stack/kms/root/galion-secrets/github_ed25519.redacted` and the `from-grok-build/*.redacted` names; no key material (gitleaks 0 private-key hits) | covered by `stack/kms/` | covered |
| SEC-5 | tuner-quick trycloudflare | the random quick-tunnel hostname appears in 3 paths: `docs/session-2026-10-18-evening.md` (272 commits), `crates/tuner/src/lib.rs` and `crates/tuner/tests/tuner_http.rs` (38 each) | not on the purge list | dead once the unit is stopped (option A); no purge needed |
| (new) | production VPS IP | 21 paths, e.g. `TEACHER/A-foundation.md` (227 commits), `docs/real-tests/survival-heal.md` (56), `docs/persona-plex-v2-design.md` (50), `stack/hermes/SOUL.md`, `stack/INDEX.md`, `docs/session-2026-10-18*.md`, `docs/rust-rewrite-plan.md`, `deploy/offsite/**` | only 1 of 21 (`stack/INDEX.md`) is on the list | see D3: path purge would delete real docs; needs text replacement (tool gap) or acceptance |

Commit counts are "commits whose tree contains that path with a match".

## 3. Tool

**`galion-purge`** (crate `history-purge`, std-only Rust, zero dependencies,
MIT, on `swarm/sec-runbooks` c5fc269). Rebuilt and re-tested today: 20 unit
and integration tests pass, `GALION_REAL=1` real test passes (12.6 s).
It drives `git fast-export --all --no-data | byte-exact path filter | git
fast-import --force`, then `reflog expire` + `gc --prune=now`, re-scans
every tree of every ref, checks old blob ids with `cat-file --batch-check`
and writes `galion-purge-commit-map.tsv` (old TAB new). It refuses
non-bare repos, shallow clones, any path under `/opt/galion`, and repos with
a remote unless `--strip-remotes` (so the result cannot be pushed by
accident). It never reads blob contents and never pushes.

Excluded alternatives (unchanged from SEC-1-HISTORY-PURGE.md): git-filter-repo
(Python), BFG (GPL-3.0, JVM), `git filter-branch` (deprecated, no
verification), gitkat / nornir-git (young, large or library-only).

**Gap:** it drops whole paths; it has no `--replace-text`. If D3 = "scrub
the VPS IP from history", a `replace` subcommand (fast-export **with** data,
literal byte replacement inside blobs, same verification) must be added,
about 150 lines plus tests. Not built here (plan only); it would be a
follow-up slot.

External scanner used for verification: gitleaks 8.30.1 (MIT, Go binary,
run as a program, never vendored or linked).

## 4. Rotation list (must come first)

Full table with rotation method and health check per row:
`docs/launch/SEC-1-ROTATION.md` (swarm/sec-runbooks). Summary:

| Prio | Rows | When (Zagreb) |
|---|---|---|
| P0 known-leaked | 13 VPS root password; 1 GitHub token (L); 2 GitHub SSH key + SEC-4 copy; 3 Cloudflare tunnel credential + tunnel token (L) | Oct 21-22 (CEST +02:00) |
| P1 live | 14 Hermes config.yaml fields (after SEC-3 perms); 5 OpenRouter; 7 River; 6 API_SERVER_KEY; 8 memory keys; 4 Cloudflare API/R2; 10 console password/PIN/device; 11 app token/Replicate; 12 browser cookies; 9 sealing keys (last) | Oct 22-26 |
| P2 dormant | 15 grok-build backend env (revoke); 16 Stripe roll with overlap | Oct 26-29 |
| P3 public | 17 publishable front-end keys (confirm only) | Oct 26-29 |
| after swarm | 18 box SSH key | Oct 31 (CET +01:00) |

Push freeze (security plan S7.7): rows 1 and 18 break
`/workspace/swarm/push-branch.sh`; schedule a 30-minute no-push window and
re-test one swarm/* push afterwards. Not rotated by anyone here: the
galion-control key.

## 5. Every copy of this history (all must be handled for route B)

| Copy | Where | State checked 2026-10-21 | Route A | Route B |
|---|---|---|---|---|
| GitHub private repo | `galion-studio/obsidian-open-mind` | 94 heads at 23:33 (+02:00, archived ls-remote); now ~110 swarm/* + 6 dependabot; plus read-only `refs/pull/*` | stays private | force-push explicit refspecs, delete dead branches, GitHub Support for cached views and PR refs |
| box mirror | `/workspace/swarm/base.git` | 28 refs, 263 commits | keep | replace with the rewritten mirror |
| box bundle | `/workspace/swarm/all.bundle` | 34 heads (Oct 20 18:44) | keep | delete |
| box worktrees | `/workspace/swarm/wt/*` | 109 swarm/* worktrees | keep | rebase via commit map, then delete old |
| box clones | `/workspace/om*`, `/workspace/oms-*` (at least `oms-repo` is a clone) | not opened | keep | delete or re-clone |
| VPS archive | `/opt/galion/archive/swarm-20261020-233213+0200/` (root 0700), bundle 10,190,817 B mode 0600, 95 refs | all 95 heads present in the union mirror except 2 dependabot heads that were since updated | keep | re-create from rewritten mirror, delete old bundle |
| VPS live working copy | `/opt/galion/openmind` at 2f5a730 | **not touched** (hard rule) | keep | owner, maintenance window only; live services run from its `dist/` |
| VPS backups | `/var/backups/galion` (0700 root:galion-offsite) | contents not listed | check if repo copies are inside | same |
| offsite laptop pulls | `deploy/offsite` targets | unknown | n/a | owner checks |
| owner/dev laptops, CI caches, forks | unknown | unknown | n/a | owner checks; forks of a private repo are deleted with access |

## 6. Steps

Owner executes; the swarm verifies. Nothing below has been run against a
real repository.

1. **Rotate** (section 4). Exit check per row: consumer health GET 200, old
   credential refused where checkable without printing it.
2. **Decide** D1-D6 (section 7).
3. **Freeze**: announce "no pushes" for the window; record `git ls-remote`
   of GitHub (refs only) as the before-state.
4. **Union mirror** on the box or laptop, never on the VPS:
   `git clone --mirror <github url> purge.git`, then fetch any branch that
   exists only locally (box worktrees) into `refs/heads/swarm/*`.
   Backup: `tar -czf purge-mirror-backup-<date>.tgz purge.git` (0600).
5. **Scan** (read-only): `galion-purge scan --repo purge.git --paths
   purge-paths.txt` -> expect about 96 paths / 93 blobs (more if branches
   were added).
6. **Rewrite** the scratch copy: `galion-purge rewrite --repo purge.git
   --paths purge-paths.txt --confirm-scratch-copy --strip-remotes`.
7. **Verify** (all must pass): `galion-purge verify` exit 0; plain-git
   cross-check `git log --all -m --no-renames --format= --name-only | grep
   -E '<purge patterns>'` empty; `git fsck --full` clean; gitleaks 0 new
   findings (the 3 fixture fingerprints remain until D6); ref, commit and
   merge counts unchanged; `cargo check --workspace --locked` on the
   rewritten main; commit map present.
8. **Route A** (recommended): stop here for this repo. Create the public
   repo from the export tree (LAUNCH-1, `docs/launch/PUBLIC-EXPORT.md` on
   swarm/launch-gates), run gitleaks + path policy on it, archive the
   rewritten mirror as evidence. **Route B**: allow force-push on protected
   branches, push explicit refspecs (`refs/heads/*`, `refs/tags/*`, never
   `--mirror`), delete branches that no longer exist, re-enable protection,
   GitHub Support request (cached views, PR refs), re-clone or rebase every
   copy in section 5 using the commit map (old base 13a296c becomes a new
   id; rehearsal value 13c780328b02 is only valid for the rehearsal input).
9. **Destroy old copies** (route B only), after the new state is verified:
   section 5 rows marked delete.
10. **Post-check** (security plan S5.5): gitleaks 0 and no purge-list path in
    any tree of any ref, on the public tree and (route B) on GitHub.

Rollback: up to step 8 nothing outside scratch has changed (delete the
scratch). After a route-B force-push, the backup tarball from step 4 is
the only way back (force-push it again with the same refspecs).

## 7. Owner decisions (Slavik)

| # | Decision | Options | Recommendation and evidence |
|---|---|---|---|
| D1 | Route for obsidian-open-mind | A: new public repo from export, private repo kept unrewritten; B: rewrite private repo too; C: both | A: no raw secrets in history (section 1), B forces ~110 branch rebases and a GitHub Support ticket |
| D2 | Purge list scope | as is (6 patterns); plus the optional lines `TEACHER/`, `admin.preferences`, `docs/TERMINAL-LOG.md` | as is, unless the export-scope decision drops those dirs |
| D3 | Production VPS IP in 21 paths | (a) accept: the IP is reachable anyway; (b) build `galion-purge replace` and scrub it; (c) path-purge the 21 files (loses real docs) | (a) for the private repo; for route A the export tree must be IP-free (integration-verify finding 1, `docs/real-tests/survival-heal.md` line 21, commit 45a18aa) |
| D4 | Timing | after all P0+P1 rotations (Oct 27-30, CET +01:00 after Oct 25) vs earlier | after rotations, so old clones only hold dead values |
| D5 | galion-src repos (O1) | run the read-only O1 path scan on the VPS clones (owner) and plan per repo | owner runs it; the swarm may not run git in those clones |
| D6 | HADES ledger fixture fingerprints | leave and add a `.gitleaksignore` entry; or replace the 16-hex values with obviously fake ones on cortex-s2-d | replace with fake values on the branch before it merges (cheap, removes scanner noise); confirm they were not copied from the live ledger |

## 8. Risks and limits

- Every commit id changes; docs that cite SHAs (CALL-RECORD, swarm docs,
  this file) need the commit map. Route A avoids this.
- Signed tags lose signatures (the 141 refs contain no tags at all).
- Dependabot branches on GitHub move independently (2 heads changed since
  the 23:33 archive); delete and let Dependabot recreate them after a
  route-B push.
- The union mirror used here is a superset of GitHub (local worktree
  heads may include unpushed commits); GitHub `refs/pull/*` were not
  visible from the box.
- Tool gap: no text replacement (D3).
