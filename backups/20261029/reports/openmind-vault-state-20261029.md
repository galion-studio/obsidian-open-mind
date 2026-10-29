# OpenMind vault: state on 2026-10-29 (area 1 of 4)

Read-only research for owner turn t375u. Written 2026-10-29 around 11:57 CET (+01:00, Europe/Zagreb).
No git command was run on the VPS. No write, push, PR, issue or service change anywhere. link.env was not printed. IPs are redacted.
Rules applied to the checklist: t377u (test everything in the sandbox before live; Hermes makes no live changes) and t379u (the sandbox runs live-style deploys; Hermes may change live only after those pass the security checks; deep reasoning goes to higher OpenRouter models, routine work to cheap ones).

## 0. What "the vault" means here (3 separate things found)
| Name | Where | What it is |
|---|---|---|
| A. OpenMind repo ("Obsidian Open Mind") | github.com/galion-studio/obsidian-open-mind (private); VPS copy /opt/galion/openmind | Rust workspace, docs/goals.md, docs/main.md and the publish gate. README line 35: "not affiliated with the Obsidian note-taking app". |
| B. Galion memory vault (a real Obsidian vault) | VPS /opt/galion/galion-light/docs/build-session (has `.obsidian/`); remote github.com/galion-studio/galion-vault (private) | Plain-markdown notes. 23-vault.md: "This folder is the Galion memory vault. Plain markdown. No extra process… New session notes go in `daily/`". |
| C. Old publish folder | VPS /opt/galion/galion-light/publish/obsidian | Not a git repo. Older "Obsidian" release kit (install.sh, play.sh, skills/, guardian/*.py, console/), plus copies of goals.md and main.md. |

## 1. Commands run and results
### GitHub (cursor-github tools, read-only)
| Call | Result |
|---|---|
| get_repository obsidian-open-mind | private=true, visibility=private, default_branch=main, language Rust, license MIT, created 2026-10-18 20:52 (+02:00), pushed_at 2026-10-29 11:00:53 (+01:00), open_issues_count 7 (all 7 are PRs) |
| get_commit main | 7ba85b603d8b0b87c4d4dede1056e1ee80c767ac, 2026-10-28 18:09:32 (+01:00), "Merge swarm/main-staging into main (PR #12, publish 2026-10-28)", 300 files |
| get_commit_combined_status main | state "pending", 0 statuses. The legacy status API has no entries. The tools have no Actions-runs endpoint, so **this check could not confirm whether self-eval passed on main**. |
| list_branches (5 pages) | 456 branches: swarm/* 429, ws/* 12 (brand, foundation, gaia, hermes, memory, nerve, offsite, one-desk-tuner, side, survival, ui, voice), merge/* 1 (merge/one-source-20261020), vps/* 5 + vps-main-20261020, staged/sandbox-v1, cursor/hermes-offhost-continuity-de9e, dependabot 5, main. Vault-adjacent branches: swarm/openmind-write-guard, swarm/openmind-pin-drift, swarm/memory-*, swarm/publish-*, swarm/post-publish-*, swarm/docs-publish-readme |
| list_pull_requests state=all | 14 PRs. Merged: #8 (2026-10-20), #9/#10/#11/#12 (2026-10-28 ~18:09 +01:00), #13 (into main-staging). **Open: #14** hermes-offhost (→ swarm/main-staging), **#7** ws/brand docs, **#1 #2 #4 #5 #6** dependabot (rusqlite 0.41, toml 1.1.8, checkout 7, scorecard-action 2.5.0, upload-artifact 7). #3 was closed without merging. |
| list_issues state=all | 0 real issues (all 14 entries are PRs) |
| list_releases | [] (no releases and no tags released) |
| get_git_tree main recursive | 2938 entries, not truncated, 2391 blobs. 7 workflows: launch-gates, miri, realtest, release, sbom, scorecard, self-eval. REUSE.toml, LICENSES/{MIT,Apache-2.0,CC0-1.0}.txt, deny.toml and .gitleaks.toml are present. No rust-toolchain.toml. |
| get_file_contents README.md, docs/goals.md, docs/main.md, WORKSTREAMS.md, TEACHER/STAGED.md, docs/STATUS.md, docs/PUBLISHING.md, docs/staged-not-live.md, RELEASE-CHECKLIST.md, docs/launch/GATES-OCT31.md, SECURITY.md | read; facts are quoted below. There is no root goals.md, main.md or STAGED.md: they live at docs/goals.md, docs/main.md and TEACHER/STAGED.md. |
| get_repository galion-vault | private=true, "Galion memory vault. Markdown only.", pushed_at 2026-09-23 16:09:39 (+02:00), size 20 KB |

### VPS (`ssh slavik-vps`, files only)
| Command | Result |
|---|---|
| `ls -la /opt/galion/openmind`, `ls docs` | Tree last written Oct 27 18:42–18:43 UTC (19:42–19:43 +01:00). 37 crates. docs/ has goals.md, main.md, STATUS.md, PUBLISHING.md and *.bak files |
| `cat .git/HEAD` and ref files (cat only, no git) | HEAD → refs/heads/swarm/overnight-loop-20261027 (202887d5). Local main 2f5a730b. One remote configured. FETCH_HEAD mtime 2026-10-27 20:04 (+01:00) |
| git-blob SHA-1 of every VPS file (printf "blob N\0" + cat, sha1sum; .git, target and stage excluded) compared with the GitHub main tree | VPS 826 files vs GitHub 2391. **666 identical, 141 differ, 19 VPS-only (all *.bak plus overnight logs, dist/bin-ws/galion-one, crates/galion-one/src/sha256.rs), 1584 GitHub-only** (crates 844, docs 523, vendor 103, …) |
| diff of key docs, VPS vs GitHub main | README.md and docs/memory-api.md are SAME. docs/goals.md: VPS lacks GitHub lines 119–125 (the 2026-10-18 19:54 to-do block). docs/main.md: VPS lacks §15 status snapshot and the 2026-10-21 swarm notes. WORKSTREAMS.md: VPS lacks the ws/gaia section. docs/PUBLISHING.md: VPS lacks the "do not flip this repository, use public-export.sh" update. **The VPS is a strict subset of, and behind, GitHub main.** |
| `systemctl list-units --all \| grep -Ei 'openmind\|obsidian\|memory\|publish'` | openmind-memory.service active/running; openmind-queue.service active/running; galion-one.service active/running; memory-gateway.service **masked**/dead. No obsidian or publish unit, and no matching timers. |
| `ss -tlnp` (addresses classified) | Every TCP listener is loopback except sshd :22. Related ports: 8420 openmind-memory, 19120 openmind-queue, 8670 nerve, 8674 gaia-cortex, 8642 hermes-gateway, 8080/18080 console, 8091/8002 galion-one, 8646 desk, 8660 health |
| `curl 127.0.0.1:8420/health` | 200. Unit: ExecStart /opt/galion/rust/bin/openmind-memory-aa77864, active since 2026-10-22 23:46:47 (+02:00), NRestarts=0 |
| `ls/find /opt/galion/galion-light/publish/obsidian` (link.env skipped) | 254 files, no .git, **8 .py files** (guardian/*.py, one/models.py). goals.md and main.md dated 2026-10-18 19:54 (+02:00). Its goals.md and main.md **differ** from /opt/galion/openmind/docs (sha1 caf13699… vs 1004a0ad…, 249e4cc5… vs caf18d88…). The publish copy has the 19:54 to-do block and §15, which the openmind copy lacks. skills/self-eval/SKILL.md runs `python3 …/self-eval.py`. |
| `find … -name .obsidian` | Only one Obsidian vault on the VPS: /opt/galion/galion-light/docs/build-session |
| `ls/find build-session` | 327 files, 51 .md. Newest note: bih-h200-feasibility.md, 2026-10-16 09:16 (+02:00). **daily/ is empty (0 files)**. .obsidian: core plugins only (file-explorer, global-search, graph, backlink, outgoing-link, tag-pane, daily-notes, note-composer, page-preview, file-recovery); community-plugins `[]`. .git is mode 777; 23-vault.md is 666. Contains a 10 MB grok-computer-2026-09-23.tgz. |
| `stat /opt/galion/hermes/vault` | Exists, mode 777, 1 file, 0 .md (matches the open SEC-3 item) |

### Box
| Command | Result |
|---|---|
| `ls /workspace/swarm/reports` | 56 entries. None is about the Obsidian or memory vault. Latest: model-routing-20261029.md and the stt-* files (2026-10-29) |
| `rg -i 'obsidian vault\|galion-vault\|build-session\|\.obsidian'` over reports, plans, CALL-RECORD, SLAVIK-TODO, STATUS-BY-SECTOR, PUBLISH-BLOCKERS | No vault work recorded (hits were unrelated) |
| `ls /workspace/swarm/wt \| grep -Ei 'vault\|obsidian\|openmind\|memory\|publish'` | crate-publish-hygiene, docs-publish-readme, memory-schema-mergefix, memory-search-local-time, nerve-publish-recoveries, openmind-pin-drift, publish-blocker-memo. **No vault/obsidian worktree.** |
| `gh auth status` | not logged in (GitHub was read through the connector) |

## 2. Feature table
| Feature | State | Evidence |
|---|---|---|
| Private GitHub repo with main = merged staging | built | main 7ba85b6, PR #12 merged 2026-10-28 18:09 (+01:00) |
| Public repo (Priority #1, deadline 2026-10-31) | **missing** | get_repository private=true. PUBLISHING.md says to build the public repo from `scripts/public-export.sh` with fresh history, not to flip this one |
| Self-eval CI (fmt, clippy, test, no-Python, CJK, gitleaks, license) | built; result on main **unverified** | self-eval.yml on main. Combined status: 0 statuses; no Actions endpoint in the tools |
| Launch gates (REUSE, cargo-deny, SBOM, export, pinned actions) | built (on main) | REUSE.toml, LICENSES/, deny.toml, launch-gates.yml, sbom.yml and scripts/public-export.sh are in the main tree. Last measured REUSE FAIL (7 files) on integration-5 per GATES-OCT31.md; not re-measured on 7ba85b6 here |
| Signed release (cosign) + SBOM on tag | built, never run | release.yml present; list_releases = [] |
| SECURITY contact | **missing** | SECURITY.md:26 still `__CONTACT_EMAIL__` |
| Toolchain pin | **missing** | no rust-toolchain.toml in the tree (RELEASE-CHECKLIST 1.8 open) |
| Owner sign-off (RELEASE-CHECKLIST §5, LAUNCH-2) | **missing** | 5.1–5.4 "blocked"; 7.6–7.8 "open" |
| SEC-1..5 (rotation, history purge, perms incl. hermes vault 777, tunnel) | **missing** (owner) | GATES-OCT31.md "Still needing the owner"; VPS hermes/vault is 777 |
| OpenMind memory service (Rust, 8420) | built + **live** | unit active, /health 200, memory-gateway masked |
| Docs single source (goals/main) | **diverged** | 3 different copies: GitHub main ⊃ VPS /opt/galion/openmind; publish/obsidian has another variant |
| VPS repo synced to GitHub main | **missing** | 141 differ / 1584 GitHub-only; VPS HEAD on swarm/overnight-loop-20261027 |
| Obsidian memory vault (build-session) | built (static), **stale** | newest note 2026-10-16; daily/ empty; no service writes to it; core plugins only |
| galion-vault remote in sync | **unknown/stale** | GitHub pushed_at 2026-09-23. The VPS vault has notes from 2026-10-15/16; whether they were pushed was not checked (no git on the VPS) |
| Vault publish/automation service | **missing** | no obsidian or publish unit or timer; publish/obsidian has no .git and contains 8 .py files |
| Vault permissions hardened | **missing** | build-session/.git 777, 23-vault.md 666, hermes/vault 777 |
| Open PRs closed out | **missing** | #14, #7 and 5 dependabot PRs open |

## 3. Repo state summary
- galion-studio/obsidian-open-mind is **still private** (checked 2026-10-29 ~11:55 +01:00). 2 days remain to the 2026-10-31 deadline. No releases.
- main = 7ba85b6 (2026-10-28 18:09 +01:00). 456 branches. 7 open PRs, 0 issues.
- The VPS checkout is behind main and sits on a swarm branch. The docs (goals/main/PUBLISHING/WORKSTREAMS) are older there.

## 4. What remains to reach 100% (vault scope)
Each item lists the live-style sandbox deploy test it must pass, plus its security check, before anything touches live. Hermes changes live only after that. Tier: **cheap** = routine (cheap OpenRouter model or no model); **high** = deep reasoning (higher OpenRouter model); **owner** = owner action, no model.
- [ ] Verify self-eval and launch-gates on main 7ba85b6 (read Actions runs; needs Actions read access). Sandbox: re-run `scripts/self-check.sh` + `scripts/public-export.sh` on a 7ba85b6 checkout in the sandbox; security: gitleaks + 14-shape scan = 0. Tier: cheap.
- [ ] Fix REUSE gate failures (2 .rs fixtures + vendor/candle-core annotation) if they are still present on main. Sandbox: comply lint 0 failed on the export tree. Tier: cheap.
- [ ] Fill `__CONTACT_EMAIL__` (SECURITY.md, CODE_OF_CONDUCT.md, release/repo copies). Owner supplies the address. Sandbox: export-tree scan has no placeholder. Tier: owner + cheap.
- [ ] Pin the toolchain (rust-toolchain.toml or rust-version). Sandbox: clean-cache build + test with the pinned toolchain. Tier: cheap.
- [ ] Decide export scope (stack/, TEACHER/, admin.preferences, TERMINAL-LOG IP, France IP). Sandbox: public-export.sh gate 10 shows 0 IPv4 literals. Tier: high (judgment) + owner.
- [ ] SEC-1..SEC-5, including chmod of hermes/vault (777) and the build-session vault (.git 777, 23-vault.md 666). Sandbox: apply the same permission changes in a live-style sandbox copy and confirm the services still read and write; security scan of the result. Live only after it passes, by the owner (not Hermes). Tier: owner; plan review high.
- [ ] Create the public repo from the export tree (fresh history), then apply branch protection, private vulnerability reporting, secret scanning and Dependabot. Sandbox: dry-run export into a throwaway private repo in a sandbox org, confirm all workflows go green there. Tier: owner (approval) + cheap.
- [ ] Owner sign-off: RELEASE-CHECKLIST §5 and LAUNCH-2. Sandbox: the green sandbox run above is the evidence attached. Tier: owner.
- [ ] First signed release (v* tag → release.yml, cosign, SBOM). Sandbox: tag a pre-release in the sandbox repo; verify the cosign bundle and SBOM. Tier: cheap.
- [ ] Resolve open PRs #14, #7 and the dependabot PRs #1/#2/#4/#5/#6. Sandbox: each branch through self-eval + a sandbox deploy (rusqlite 0.41 must pass the openmind-memory tests and a live-style memory deploy). Tier: cheap; rusqlite/toml majors high.
- [ ] Bring the VPS /opt/galion/openmind in line with GitHub main (currently 141 differing files, VPS on a swarm branch). Sandbox: live-style deploy of main into a sandbox copy of /opt/galion/openmind, units + health checks green, security checks pass; only then a live change, which Hermes may make only after those pass. Tier: cheap.
- [ ] One source for goals.md/main.md: reconcile publish/obsidian, VPS docs and GitHub main (3 variants). Sandbox: diff-check job in the sandbox showing zero divergence. Tier: cheap; owner decides which dictated text wins.
- [ ] Retire or port publish/obsidian (8 .py files, Python self-eval skill), against the no-Python rule. Sandbox: no-python scan on the replacement + live-style deploy of the Rust replacement. Tier: high (port design), cheap (scan).
- [ ] Obsidian memory vault: decide whether it is part of the new stack. If yes: the daily/ write path (frontmatter per 23-vault.md), a sync with galion-vault, and no secrets in notes. Sandbox: live-style deploy of the writer + sync against a sandbox vault copy and a sandbox remote; gitleaks over the vault = 0. Tier: high (design), cheap (routine note writes).
- [ ] Confirm galion-vault remote vs the VPS build-session (GitHub last push 2026-09-23; notes exist from 2026-10-15/16). Read-only check needs git on the VPS, which this task did not allow. Tier: cheap.

Facts not obtainable in this run: self-eval/Actions results on main (no Actions API in the connector; gh not logged in), and galion-vault commit contents.
