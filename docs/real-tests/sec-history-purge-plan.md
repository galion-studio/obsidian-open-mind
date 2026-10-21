<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# Real-life evidence: sec-history-purge-plan (wave 4 slot 126)

2026-10-21, 04:44 to 04:55 CEST (+02:00), box scratch dir
`/workspace/swarm/scratch/purge-plan-126` (deleted after the run). No
history of any real repository was rewritten; nothing was pushed except
this branch. Outputs are counts, paths and object ids only.

## Inputs

- Union scratch mirror: `git clone --mirror /workspace/swarm/base.git`
  (28 refs), plus `/workspace/swarm/all.bundle` heads as `refs/bundle/*`
  (34), plus the swarm/* head of every box worktree (109). Result: 141
  refs, 470 commits, 20 MB, no remote.
- Tool: `galion-purge` built from swarm/sec-runbooks
  c5fc269a1b605fa4e685a9716911981742d6b07c with
  `cargo-slot build --release --locked -p history-purge -j 2` (4.4 s).
- Scanner: gitleaks 8.30.1 (`--redact=100`, JSON report; only RuleID,
  File, line, commit were extracted).

## Results

| Check | Command (shortened) | Result |
|---|---|---|
| tool tests | `cargo-slot test --locked -p history-purge -j 2` | 15 + 2 + 3 passed, 0 failed, 1 ignored |
| tool real test | `GALION_REAL=1 cargo-slot test --release -p history-purge --test real_sec_runbooks -- --ignored` | PASS, 12.6 s (scratch-mirror purge + SEC-3 perms dry run/apply/rollback on a fixture tree) |
| secret scan, before | `gitleaks git union.git --redact=100` | 3 findings, RuleID generic-api-key, all `crates/openmind-nerve/tests/fixtures/hades/ledger.jsonl` lines 3/4/7 (commit f3468ce), field `token_sha256_16` (hash prefix, false positive) |
| path scan | `galion-purge scan --repo union.git --paths list-base.txt` | 470 commits, 463 trees, 6 patterns: 96 paths, 93 blob ids (contrib 45, dist 11, edge 6, stack 34); 1.54 s, 9 MB RSS |
| tips | `ls-tree -r` of every ref | 141 of 141 ref tips contain listed paths; main has 85 |
| secret-shaped names | `log --all --name-only` filtered for .env/.pem/.key/id_*/credentials/cookie/token/secret/password names | only `*.redacted` inventory names, `brand/tokens.css`, `scripts/real/gaia-hades-tokens.sh`, `stack/kms/galion-light/guard/lock-secrets.sh`; no raw key/env files |
| VPS IP / quick-tunnel host | `git grep -l` over all 470 commits | IP in 21 paths, quick-tunnel host in 3 paths (values not written here) |
| root password lines | masked print of the 3 lines | "rotation advised", "value never recorded here"; no value |
| rehearsal rewrite | `cp -a union.git rehearsal.git; galion-purge rewrite --repo rehearsal.git ... --confirm-scratch-copy --strip-remotes` | REWRITE OK, 2.61 s, 26 MB RSS; dropped 184 M lines, 0 commits empty; 0 listed paths after; 90 old blobs gone, 3 shared (MIT LICENSE texts, `.license` sidecars, `release/repo/LICENSE`) |
| verify | `galion-purge verify`; plain-git cross-check; `git fsck --full` | exit 0; 0 lines; clean |
| structure | refs / commits / merges before vs after | 141/141, 470/470, 64/64; commit map 471 lines, every commit id changed (root 9ddf211 had listed paths); size 20 MB -> 8 MB |
| source untouched | sha256 of `for-each-ref` of union.git before/after | identical |
| secret scan, after | gitleaks on rehearsal.git | same 3 fixture findings only |

## VPS (read-only, 2026-10-21 04:52 CEST +02:00)

- nerve `GET 127.0.0.1:8670/state`: 200, 13,139 B (cortex not live yet; nerve used).
- `GET /health` 8642 (hermes-rs) 200, 8650 (tuner) 200, 8091 (galion-one) 200.
- `systemctl is-active tuner-quick cloudflared`: active active (SEC-5 still open).
- Archive `/opt/galion/archive/swarm-20261020-233213+0200/` (0700 root):
  bundle 10,190,817 B mode 0600; `bundle-refs.txt` 95 refs,
  `github-heads-ls-remote.txt` 94 heads. All heads present in the union
  mirror except 2 dependabot heads updated since.
- `/var/backups/galion` 0700 root:galion-offsite (contents not listed).
- `/opt/galion/openmind` not touched.

## Cleanup

Scratch dir and the target dir `/workspace/swarm/target/sec-history-purge-plan`
removed after the run.

## Re-verification by the resume agent (2026-10-21 05:07 to 05:10 CEST +02:00)

The first slot-126 agent was stopped by the request limit after committing
e07f877 (unpushed). The resume agent re-ran every check from scratch in
`/workspace/swarm/scratch/purge-verify-126` (deleted afterwards):

| Check | Result |
|---|---|
| tool tests (sec-runbooks c5fc269) | 15 + 2 + 3 passed, 0 failed, 1 ignored |
| tool real test (`GALION_REAL=1`, release) | 1 passed, 15.1 s |
| fresh union mirror | 146 refs (141 heads incl. 114 worktree swarm/* heads, 5 bundle-only), 477 commits, 64 merges; grew by 7 commits since 04:46 from new wave-4 branches |
| path scan | 96 paths, 93 blob ids (contrib 45, dist 11, edge 6, stack 34); path list **identical** to `docs/security/history-purge-affected-paths.txt` (diff empty); 0.99 s, 9 MB RSS |
| tips | 146 of 146 ref tips contain listed paths; main 85 |
| rehearsal rewrite | REWRITE OK, 2.56 s, 25 MB RSS; 90 old blobs gone, 3 shared (same 3 ids as before); commit map 477 lines |
| verify / cross-check / fsck | VERIFY OK exit 0; plain-git cross-check 0 lines; `git fsck --full` clean |
| structure | refs/commits/merges 146/477/64 before and after; size 20 MB -> 8.1 MB; source mirror `for-each-ref` sha256 unchanged |
| gitleaks 8.30.1 (`--redact=100`) | before: 3 findings, same fixture lines 3/4/7 (f3468ce); after: the same 3 on the rewritten commit (0e9c9fc); no new finding |
| VPS read-only (05:09 CEST) | nerve `/state` 200 13,138 B (cortex still not live); 8642/8650/8091 `/health` 200; `tuner-quick` and `cloudflared` active (SEC-5 open); `/opt/galion/openmind` at 2f5a730 (read only); archive bundle 0600 root 10,190,817 B |

Conclusion: the plan's numbers hold; new branches add commits but no new
purge-list paths. Step 5 of the plan ("expect about 96 paths / 93 blobs,
more if branches were added") is confirmed.
