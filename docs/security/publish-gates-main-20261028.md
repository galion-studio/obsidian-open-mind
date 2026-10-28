<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Post-publish launch gates on GitHub main 7ba85b60 (slot 428, 2026-10-28)

Started 2026-10-28 21:39 CET (+01:00). Branch `swarm/publish-gates-main`.
NO main update, NO PR, NO repo-settings change, NO deploy. No secret value is
printed or committed anywhere in this record; hits are named by file:line and
rule (or deny-list entry number) only.

## What was scanned

- GitHub `main` head read-only via the GitHub API at 21:40 CET: `7ba85b60`
  (PR #12 merge, tree `a6f6c163`, parents `13a296ce` + `3250d07d`). The repo
  is **private** (`visibility: private`); "public" = the `scripts/public-export.sh`
  tree (git archive + export-ignore) the owner would publish from.
- `7ba85b60` itself cannot be fetched on the box (HTTPS needs credentials;
  only the VPS holds the token). Used `2ce1a08f` from slot 421
  (swarm/post-publish-batch-main): its tree is **`a6f6c163` = main's tree
  byte-for-byte** (`git rev-parse 2ce1a08f^{tree}`), so every tree gate ran on
  exactly what main holds. 18d68395 (373+374 batch) was not needed.
- History gates ran on the box lineage of that tree (815 commits, 637 non-merge
  scanned by gitleaks). `3250d07d` (main-staging CI follow-ups) is not on the box;
  its 13-file delta is in the tree that was scanned.

## Gate results on main's tree (2ce1a08f = tree a6f6c163)

| gate | result | detail |
|---|---|---|
| 1 git archive | PASS | 2297 of 2391 tracked files exported (94 export-ignored) |
| 2 path policy | PASS | no contrib/, *.redacted, stack/kms/, key/env/cookie/token files |
| 3 no binaries | PASS | no ELF/PE/Mach-O in the export (dist/bin is export-ignored) |
| 4 no Python | PASS | 0 *.py |
| 5 built-in secret patterns | PASS | 0 hits, 14 token shapes |
| 6 gitleaks dir (.gitleaks.toml) | PASS | gitleaks 8.30.1, export tree and full worktree: no leaks |
| 7 REUSE (comply) | PASS | 2273 total, 2205 passed, 0 failed, 68 skipped |
| 8 cargo deny (deny.toml, --online advisories) | PASS | workspace + edge/openmind-node: advisories, bans, licenses, sources ok |
| 9 SBOM CycloneDX 1.5 | PASS | cargo-cyclonedx 0.5.9: 74 files, 235 components, 0 without licence |
| 10 infra disclosure | **WARN** | 71 public IPv4 literals, 1 quick-tunnel hostname -> triaged below |
| gitleaks git history, repo .gitleaks.toml | PASS | 637 commits, no leaks |
| gitleaks git history, default rules only | 4 hits, all known false positives | `generic-api-key` x3 `crates/openmind-nerve/tests/fixtures/hades/ledger.jsonl:3,4,7` (commit f3468cea; 16-hex sha prefixes) and x1 `crates/openmind-nerve/tests/statedir_compat.rs:390` (commit 9e16bad6; the line sets `NERVE_EVT_LLM_KEY_ENV`, an env-var *name*). Both are exactly what `.gitleaks.toml` allowlists. |
| scripts/sbom.sh | PASS | 246 packages, all licences on the allowlist |
| scripts/spdx-scan.sh | PASS | 815 *.rs scanned, 0 missing |
| scripts/no-python.sh | PASS | ok |
| repo-check (release, --copyright) | PASS | 2294 files, 0 errors, 86 warnings (all `utc` review warnings) |

## Gate 10 triage (values not repeated here)

| id | what | where | verdict | action |
|---|---|---|---|---|
| F-428-1 | live production VPS public IPv4 | docs/ops/vps-baseline-20261020.md:6,161; docs/ops/vps-baseline.md:7; docs/real-tests/vps-baseline-2.md:5 | **real** (maps the production host; it is in the export tree) | redacted to `<vps-public-ip>` (74125369) |
| F-428-2 | swarm box egress IPv4 (also the live fail2ban ignoreip) | docs/TERMINAL-LOG.md:36,48,51 | **real** | redacted to `<box-egress-ip>` |
| F-428-3 | retired "France server" IPv4 | TEACHER/QUEUE.md:29; stack/hermes/SOUL.md:9 | **real** (old host) | redacted to `<france-server-ip>`. SOUL.md is persona text; nothing deployed. |
| F-428-4 | concrete quick-tunnel hostname | docs/session-2026-10-18-evening.md:45 | real but dead (transient unit lost 2026-10-22, slot 420) | redacted to `<quick-tunnel-name>` |
| F-428-5 | retired France IPv4 compiled into a committed binary | dist/bin/link-watch (built 2026-10-18, commit 25ffe16c) | real, **not in the export** (`/dist/bin export-ignore`), present in the private repo | not changed: owner decision D-428-2 |
| - | 1.1.1.1 / 8.8.8.8 / Cloudflare anycast (188.114.x, 198.41.x) | docs, nerve log fixtures | public resolvers / CDN | none |
| - | test fixtures marked `repo-check: allow`, RFC/ISO section numbers (7.4.x, 7.6.x), CGNAT/benchmark ranges | crates/galion-guard, galion-one, repo-check tests | false positives | none |

After the fix the export WARN list holds only resolvers, CDN anycast, fixtures
and section numbers (71 -> 65 address lines, quick tunnels 1 -> 0).

## New guard: `repo-check --deny-file` (check `deny`)

Gate 10 is a regex WARN; it cannot tell the production IP from 8.8.8.8, and
`repo-check hosts` only covers crates/, deploy/, .github/ by design. The new
`deny` check takes an operator list (one literal per line) that **must live
outside the scanned tree** (the CLI refuses a file inside `--root`, exit 2),
scans every file including binaries, ignores the allow marker, and reports
`file:line` + entry number only. Proof on main's tree with the operator list
(kept in /tmp on the box, never committed):

- before (main tree, 2ce1a08f): 10 errors (exactly F-428-1..4) -> FAIL
- after (this branch): only `dist/bin/link-watch` (binary, entry #3 = F-428-5)
- public export tree after: 0 errors -> PASS

CI wiring (suggestion, owner decision D-428-3): store the list as a repo/org
secret, write it to `$RUNNER_TEMP/deny.txt`, run
`repo-check --only deny --deny-file "$RUNNER_TEMP/deny.txt" --root "$RUNNER_TEMP/public/tree"`
in launch-gates. Not done here (needs a secret = repo settings change).

## Owner decisions

- **D-428-1 history purge of F-428-1..4.** The values remain in git history of
  the private repo (docs added before tonight). Redaction is forward-only; no
  history rewrite was done. If the repo (or its history) is ever made public,
  purge per docs/security/SEC-HISTORY-PURGE-PLAN.md (galion-purge runbook; Rust
  git-filter, never force-push without the owner). Recommendation: no purge
  while the repo stays private and only the export tree is published (the
  export tree is now clean); purge before any visibility change.
- **D-428-2 dist/bin binaries.** dist/bin/link-watch (2026-10-18) embeds the
  retired France IPv4. Options: A `git rm dist/bin/*` from main and ship signed
  release assets (matches SCORECARD-READINESS Binary-Artifacts), B rebuild
  link-watch from current source (no built-in host since the TEACHER fix),
  C keep (export-ignored). Recommendation: A.
- **D-428-3 deny-list secret in CI** as above. Recommendation: yes.
- **D-428-4 land this branch** (3 forward commits over main's tree; docs
  redactions + repo-check deny check + tests). Recommendation: yes, normal
  test-gated merge.
