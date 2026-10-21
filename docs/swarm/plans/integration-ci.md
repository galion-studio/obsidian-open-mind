<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Sector plan: integration-ci

Written 2026-10-20 20:40-20:55 CEST (+02:00) by the integration-ci planner (planning only: no code, no commits,
no pushes, no builds, no VPS contact). Launch deadline 2026-10-31 (CET +01:00 from 2026-10-25 03:00).
Sources: /workspace/swarm/{RULES,WAVE2,CALL-RECORD,CALL-RECAP}.md, /workspace/review-20261020/*.md,
/workspace/swarm/wt/integration-1 at 8fd26f6 (docs/integration/integration-1.md = "INT1", docs/real-tests/integration-1.md = "INT1-RT",
docs/launch/GATES-OCT31.md, docs/launch/SCORECARD-READINESS.md, RELEASE-CHECKLIST.md, .github/workflows/*, .cargo/config.toml),
`git log`/`git status` of every wave-2 worktree (read-only), and a dry `git merge-tree` of the gaia branches in a throwaway
bare clone under /tmp (deleted afterwards; no worktree touched).

## 1. Scope

1. Get `swarm/integration-1` (8fd26f6) onto main (13a296c). The owner merges; the swarm opens no PR (RULES.md rule 6).
2. Fold wave-2 branches into the integration line: gaia-poseidon 498e3d2, gaia-hades-tokens f3f41a2, gaia-land, and the 13
   still-running branches (flake-fix, sec-runbooks, console-static, common-migrate-2, fuzz-sast, one-hardening, nerve-wiring,
   lan-routing, hermes-enroll, deploy-sim, tunnel-audit, memory-schema).
3. CI workflows: realtest.yml `--with-nerve`, pinned actions, ucpu gate in CI, public-export in CI, repo-check 28 `utc` warnings.
4. The `.cargo/config.toml` `[build] rustflags` / RUSTFLAGS-override no-SIMD risk.
5. Release tagging v0.1.x.
Out of scope (other sectors): SEC-1..5 execution (security), cutovers/deploys (vps-ops), nerve/cortex features (nerve, cortex),
Gaia ownership decision POSEIDON vs HEPHAESTUS (gaia), white paper (docs-whitepaper).

## 2. Current state (2026-10-20 20:40 CEST)

### 2.1 integration-1
- `swarm/integration-1` head **8fd26f6** (20:32:58), pushed per CALL-RECORD.md results log 20:34. 20 wave-1 branches merged with
  `--no-ff`, 12 clean, 8 conflicts (R1-R7), plus fixups F1 c4fcae7 and F2 177dd0c (INT1 merge table, "Fixups").
- Evidence (INT1-RT): workspace 206 suites **950 passed / 0 failed / 4 ignored**; omnode 72/0; `ucpu-gate.sh all` ok
  (x86_64 + aarch64 musl static, size budget, armv7 + aarch64-android check); public-export 0 FAIL / 1 WARN (gate 10:
  20 public IPv4 literals + 1 quick-tunnel host); repo-check 1074 files, 0 errors, **28 `utc` warnings**; realtest stack
  **76/77 + 1 skip** (nerve-ingest, needs `--with-nerve`; 81/81 with it); live **41/43** (mouth 8002 503, link-watch.service inactive).
- Never run on GitHub Actions: `self-eval.yml`, `realtest.yml`, `launch-gates.yml` trigger on `pull_request` (and push to main /
  dispatch); swarm branches have no PR, so integration-1's CI is still unproven on a clean runner.
- INT1 "Needs Slavik" 1-8 (INT1:143-160) still open.

### 2.2 Wave-2 branches (head, own work, dirty files at 20:40)
| branch | head | state | files it touches that integration-1 also touched | fold risk |
|---|---|---|---|---|
| gaia-poseidon | 498e3d2 | done (pushed per record) | built on gaia-live + poseidon (both in INT1) | **dry merge-tree: clean** onto 8fd26f6 |
| gaia-hades-tokens | f3f41a2 | done | built on gaia-live + hades-approval | **clean** after gaia-poseidon (dry run); together +27 files, +3315/-551 |
| gaia-land | c15a4b7 + 24 dirty | running (slot 01) | its committed head only re-merges ws/gaia 04778e6, which **is already in INT1** (via gaia-live 5510c96) | committed head: `Cargo.lock` conflict only; dirty work (ownership.rs, view.rs, gaia/src/{lib,main}.rs, gaia-core/src/lib.rs) overlaps gaia-poseidon/hades-tokens edits to crates/gaia/src/{lib,main}.rs |
| flake-fix | d194c19 (=sandbox) + 20 dirty | running | openmind-sandbox/examples/escape.rs (**INT1 F1 Android PTRACE_SEIZE fix**), link-watch/src/main.rs, galion-one/src/cli.rs | high: must keep F1 or ucpu tier-2 android breaks again |
| common-migrate-2 | 26d98b0 (=common) + 12 dirty | running | galion-desk/src/main.rs (R5), room/src/main.rs (R1), link-watch/src/main.rs, openmind-memory/src/http.rs, hermes-rs/src/http.rs, Cargo.lock | high: based on pre-rewrite room/desk files |
| nerve-wiring | 5802d0e (=nerve) + 11 dirty | running | hermes-rs/src/{api,http,spend}.rs, openmind-memory/src/{http,main}.rs, new crates/nerve-client, Cargo.lock | high: 3-way with common-migrate-2 + memory-schema |
| memory-schema | 723b46c (=memory-real) + 4 dirty | running | openmind-memory/src/{config,engine,main,store}.rs | medium; shared schema change is an owner decision (CALL-RECAP §3 #9) |
| one-hardening | b44cd4f (=one-probe) + 5 dirty | running | galion-one/src/main.rs (INT1 R7 dropped `mod sha256`), probe.rs | medium |
| lan-routing | b153af8 (merge only) + 5 dirty | running | openmind-discovery/src/{admit,cmd}.rs (INT1 F1 `repo-check: allow` lines), openmind-provider | low-medium |
| hermes-enroll | 92f7693 (merge only) + 1 dirty dir | running | new hermes-rs/src/enroll/ (+ a `mod` line later) | low |
| fuzz-sast | bb89351 (merge only) + 2 dirty | running | root Cargo.toml `exclude = ["contrib","fuzz"]`, new fuzz/ with **its own Cargo.lock** | low conflict; **RUSTFLAGS risk** (cargo-fuzz sets RUSTFLAGS) |
| sec-runbooks | 13a296c + 5 dirty | running | new crates/history-purge, deploy/staged/sec3-hermes-perms.sh, docs/launch/, Cargo.lock | low (lock only) |
| console-static | 13a296c + 5 dirty | running | new crates/console-static, deploy/, scripts/, Cargo.lock | low (lock only) |
| tunnel-audit | 13a296c + 1 dirty | running | new crates/tunnel-lint | low |
| deploy-sim | 5fd1611 (=tuner-cutover), clean | running, no own commit yet | - | none yet |
Workspace members are the glob `crates/*` (Cargo.toml:4), so new crates add no member-list conflicts; every new crate does
conflict in `Cargo.lock` (handle as INT1 R6: union of (name, version) pairs, `cargo metadata --locked --offline`).

### 2.3 CI workflows on integration-1
- 6 workflows (`.github/workflows`: self-eval 281 lines, realtest 54, launch-gates 76, release 84, sbom 39, scorecard 47).
- **Pinned actions: done.** Every `uses:` is a commit SHA (F1 pinned the last @v4/@stable ones); `.github/dependabot.yml`
  exists (SCORECARD-READINESS.md:22). Remaining: the toolchain itself is "stable" (floating); no `rust-toolchain.toml` and no
  `rust-version` (RELEASE-CHECKLIST.md 1.8 open). Box rustc is 1.99.1.
- **ucpu gate in CI: present, unrun.** self-eval.yml jobs `ucpu-tier1` (x86_64 + aarch64 musl build/static/size/clippy, 40 min),
  `ucpu-tier2` (armv7, aarch64-android check), `ucpu-qemu` (scripts/real/ucpu.sh; installs qemu-user-static = GPL tool, CALL-RECAP §3 #12).
- **realtest.yml**: builds 9 packages, runs `galion-realtest stack` without `--with-nerve`. Turning it on is not one line:
  the build step must also build `-p openmind-nerve` (stack.rs:47 looks for `openmind-nerve`/`galion-nerve` in --bin-dir).
- **public-export in CI**: launch-gates.yml on PRs to main runs `scripts/public-export.sh --online`; gate 10 is WARN only.
- **repo-check**: self-eval job prints warnings, fails only on errors. 28 `utc` warnings came back after docs-sync had 0
  (docs-sync real-tests:41 vs INT1-RT); sources: room, desk-linkwatch, gaia-live, hades-approval, nerve, common, poseidon,
  lan-discovery, memory-real, survival-heal (INT1:157-158). Separate best-effort `zagreb-timestamps` job (Z-suffixed ISO) is
  `continue-on-error`.
- **release.yml**: v* tag only; release job additionally gated on repo public AND `vars.RELEASE_ENABLED == 'true'`. It builds
  `cargo build --release --workspace` for the **default gnu target** and packages only 5 binaries (galion-one, galion-desk, room,
  desk-hermes, link-watch) for x86_64. Not the static musl / aarch64 artefacts the ucpu gate proves.

### 2.4 RUSTFLAGS / no-SIMD
`.cargo/config.toml` `[build] rustflags = ["--cfg", "curve25519_dalek_backend=\"serial\""]` (from hades-approval, kept in R2).
Cargo replaces (does not merge) `build.rustflags` when `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_TARGET_<triple>_RUSTFLAGS`
or a `target.<triple>.rustflags` entry is set; curve25519-dalek then compiles its AVX2 backend on x86_64. Guard today: only the
test `crates/hades-gate/tests/simd_free.rs` (a test failure, not a build failure) and `ucpu-gate.sh:94` (which only rejects
target-cpu/target-feature strings in RUSTFLAGS, not a missing cfg). Known triggers: cargo-fuzz (fuzz-sast), cargo-llvm-cov,
sanitizers, any developer's RUSTFLAGS. fuzz/ is its own workspace but still picks up the root `.cargo/config.toml` only if
RUSTFLAGS is unset, which cargo-fuzz never leaves unset.

### 2.5 Release tags
No tags anywhere (CALL-RECORD Q4); `[workspace.package] version = "0.1.0"`, all 53 crates inherit it; live VPS binaries already
report v0.1.0 (CALL-RECORD Q4). LAUNCH-1 says the public repo is created **from the exported tree with fresh history**
(GATES-OCT31.md LAUNCH-1), so a tag on the private repo does not carry over: the public v0.1.x tag must be created on the new repo.

### 2.6 Box capacity (hard limit independent of the request limit)
8 cores, 15 GB RAM, 68 GB free disk (`df /` 47 %). A full workspace build per own CARGO_TARGET_DIR at `-j 3`: at most
**3 concurrent builders** for this sector before it starves other sectors. Read-only reviewers cost requests, not CPU.

## 3. Sessions

Agent counts: **target** = what to assign if the 250 ceiling held; **useful** = the most that can work it in parallel without
waiting on each other. No task in this sector can use a 50-agent group: merging is serial per file hotspot, and builds are
bounded by 8 cores. The biggest honest fan-out is read-only review (S1-T2: 9).

### S1. Land integration-1 on main (Tue 20 21:00 - Wed 21 12:00 CEST)
Goal: owner can merge 8fd26f6 with confidence; first GitHub Actions run of the merged tree is green.
| task | target | useful | parallel units | inputs / deps | success criteria (real-life) |
|---|---|---|---|---|---|
| T1 clean-clone re-verify of 8fd26f6 | 6 | 3 | (a) fmt+clippy+workspace test, (b) omnode, (c) ucpu tier-1 x86_64+aarch64, (d) tier-2 + qemu real run, (e) public-export + repo-check + no-python, (f) realtest stack x3 with and without `--with-nerve` + sandbox flake 25x | fresh clone of 8fd26f6, own target dirs, ports 19500-19551 reserved for one runner at a time | same numbers as INT1-RT (950/0/4, 72/0, ucpu ok, 0 FAIL/1 WARN, 76/77+1 skip, 81/81) on 3 consecutive realtest runs; sandbox stress 25/25 (sandbox.md:85 had 24/25) |
| T2 read-only review of resolutions | 9 | 9 | one per resolution: R1 room, R2 .cargo/config + journal.rs, R3 STATUS.md, R4 nerve, R5 desk, R6 Cargo.lock union, R7 common x4 files, F1, F2 | INT1 "Resolutions"/"Fixups"; `git show` of each merge | each reviewer signs "nothing from the losing side dropped" with file:line, or files a fix item; R7 tuner golden 22/22 still byte-identical |
| T3 owner merge pack (draft only) | 1 | 1 | - | T1, T2 | text for the PR body/merge commit: 20 branches + F1/F2, Needs-Slavik list, expected CI jobs; handed to Slavik, nothing opened |
| T4 first CI-run triage | 4 | 4 | per job family: self-eval core (fmt/clippy/test/omnode/no-python/cjk/gitleaks/license), ucpu (tier1/tier2/qemu), realtest, launch-gates | **owner** opens the PR (or dispatches self-eval on the branch) | all required jobs green on GitHub; fixes land on a `swarm/int1-ci-fix` branch, never on main |
Approval needed: merge of integration-1 (INT1 Needs #1); opening the PR / workflow_dispatch (owner); WS-SURV D in STATUS.md (#3);
gate 10 (#4).

### S2. integration-2: the finished gaia branches (Wed 21 09:00-20:00)
Goal: `swarm/integration-2` = 8fd26f6 + gaia-poseidon + gaia-hades-tokens (+ gaia-land when committed); does not wait for S1's merge.
| task | target | useful | parallel units | inputs / deps | success criteria |
|---|---|---|---|---|---|
| T1 merge gaia-poseidon 498e3d2 then gaia-hades-tokens f3f41a2 | 1 | 1 | serial (both touch crates/gaia) | 8fd26f6; dry merge-tree said clean | merges clean; `scripts/real/gaia-poseidon.sh` RESULT pass=60 fail=0; `scripts/real/gaia-hades-tokens.sh` pass; workspace tests >= 950 + new gaia tests, 0 failed |
| T2 merge gaia-land | 1 | 1 | serial after T1 | gaia-land agent commits its 24 files and pushes; Cargo.lock conflict (known) + crates/gaia/src/{lib,main}.rs (expected) | gaia CLI real test (crates/gaia/tests/real_cli.rs) + ownership registry tests pass; heal-tick and POSEIDON scripts still pass after the merge |
| T3 verify integration-2 | 4 | 3 | same units as S1-T1 (a,c,e,f) | T1/T2 | all S1-T1 numbers held or improved; realtest 81/81 with nerve |
Approval: POSEIDON vs HEPHAESTUS (gaia-ownership.md vs VPS docs; CALL-RECAP §4 #9) before gaia-land's registry lands on main.

### S3. Wave-2b fold in four lanes (Wed 21 - Fri 23, as each branch finishes)
Goal: every wave-2 branch on one tested branch `swarm/integration-3`. Lanes are grouped by file hotspot so lanes are disjoint
except `Cargo.lock`; each lane branches from integration-2, merges its branches serially, then T5 joins the lanes.
| task | target | useful | parallel units (merge order) | hotspot / dependency | success criteria |
|---|---|---|---|---|---|
| T1 lane A memory/hermes | 2 | 2 | merger + verifier; order common-migrate-2 -> nerve-wiring -> memory-schema | openmind-memory/src/http.rs+main.rs, hermes-rs/src/http.rs; memory-schema needs owner OK | memory real suite (memory-real.md 7 passed) + nerve-client events reach nerve; 8420 GET contract unchanged |
| T2 lane B one/desk/room/sandbox | 2 | 2 | one-hardening -> flake-fix | galion-one/src/main.rs (keep R7), escape.rs (**keep F1**), link-watch/src/main.rs (vs lane A's common-migrate-2: lane A goes first or T5 resolves) | `ucpu-gate.sh check` aarch64-android ok; sandbox stress 25/25; one fail-closed scenarios 7/7 each |
| T3 lane C discovery/provider/enroll | 2 | 2 | lan-routing -> hermes-enroll | admit.rs/cmd.rs keep F1 allow lines | lan-discovery real run + routing real script; repo-check 0 errors |
| T4 lane D new crates | 2 | 2 | sec-runbooks, console-static, tunnel-audit, fuzz-sast, deploy-sim (any order) | Cargo.lock only; fuzz/ Cargo.lock added to deny/ucpu openssl scan | each branch's own scripts/real/<slug>.sh passes on the merged tree; `cargo deny` over both locks |
| T5 join lanes -> integration-3 + full verify | 4 | 3 | joiner + 2-3 verifiers (S1-T1 units) | T1-T4 | 0 failed workspace tests; ucpu all ok; public-export 0 FAIL; realtest 3x green with nerve; live probe 41/43 or better (read-only) |
Note: a branch still dirty when its lane runs is skipped and joins later; never merge uncommitted work, never touch its worktree.

### S4. CI workflows (Wed 21 - Sat 24; branch `swarm/ci-hardening` off integration-1, merged into integration-3 at the end)
| task | target | useful | parallel units | deps | success criteria |
|---|---|---|---|---|---|
| T1 realtest.yml `--with-nerve` | 1 | 1 | - | nerve in INT1 | build step adds `-p openmind-nerve`; run step passes `--with-nerve`; CI shows 81/81 (or more after wave 2); 3 consecutive green runs |
| T2 RUSTFLAGS / no-SIMD guard | 1 | 1 | - | owner picks the approach (below) | proposal: `compile_error!` in hades-gate when `not(curve25519_dalek_backend = "serial")` (build fails, not just a test); CI step asserting RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS unset in every job; fuzz job passes the cfg inside its RUSTFLAGS; ucpu-gate check extended to "cfg missing". Test: build with `RUSTFLAGS=` set must fail loudly |
| T3 ucpu gate in CI | 2 | 2 | tier-1 matrix / tier-2+qemu | S1-T4 first run | all three jobs green under 40 min; zig download checksum-verified (scripts/install-zig.sh); add `ucpu-gate.sh grep` to the fast PR path |
| T4 public-export in CI | 1 | 1 | - | owner decision on gate 10 and export scope | launch-gates job green; gate 10 either FAIL-on-hit after the allow-list is fixed, or documented WARN; export-ignore lines from PUBLIC-EXPORT.md applied if approved |
| T5 repo-check utc 28 -> 0 | 10 | 4 | per crate (10 crates in INT1:157-158), 2-3 crates per agent | **after S3** (same files as lanes) | repo-check 0 warnings; then (owner) make `utc` an error |
| T6 toolchain pin | 1 | 1 | - | - | `rust-toolchain.toml` + `rust-version`; all jobs use it; RELEASE-CHECKLIST 1.8 done |
| T7 release.yml artefacts | 2 | 2 | x86_64 / aarch64 musl packaging | ucpu-gate build outputs | static musl binaries for both tier-1 targets, all release binaries (not 5), SHA256SUMS, SBOM; still locked (public + RELEASE_ENABLED) |
| T8 SAST + fuzz CI | 2 | 2 | CodeQL rust job / fuzz smoke job | fuzz-sast branch, security sector | CodeQL SARIF uploads; fuzz job runs each target 60 s with the serial cfg; Scorecard SAST/Fuzzing move from missing |

### S5. Release v0.1.x (Mon 26 - Tue 27 CET +01:00)
| task | target | useful | parallel units | deps | success criteria |
|---|---|---|---|---|---|
| T1 version + CHANGELOG | 5 | 3 | per area (platform/one, memory/nerve, gaia, edge/ucpu, launch/CI) | integration-3 on main | proposal **v0.1.1** (live VPS binaries already say 0.1.0, so the first tag must be distinguishable); CHANGELOG built from merge records, Zagreb +01:00 times |
| T2 private tag dry run | 1 | 1 | - | **owner** pushes `v0.1.1-rc.1` | self-eval runs via release.yml; release job skipped (private); no Rekor write |
| T3 artefact rehearsal on box | 2 | 2 | x86_64 / aarch64 (qemu) | S4-T7 | binaries start, `--version` = 0.1.1, sizes within budget, SHA256SUMS match |
| T4 public-repo tag plan | 1 | 1 | - | LAUNCH-1 | checklist: export -> new repo -> settings (tag ruleset, branch protection) -> RELEASE_ENABLED -> tag v0.1.1 -> cosign verify |

### S6. Freeze and go/no-go (Wed 28 - Sat 31)
| task | target | useful | parallel units | deps | success criteria |
|---|---|---|---|---|---|
| T1 full re-verify of main HEAD | 4 | 3 | S1-T1 units | S5 | all gates green on GitHub and on the box; live 43 probe read-only |
| T2 go/no-go pack | 1 | 1 | - | security + vps-ops sectors | GATES-OCT31.md and RELEASE-CHECKLIST.md refreshed with evidence |
| T3 post-release verification | 2 | 2 | cosign verify / run binaries | owner publishes | `cosign verify-blob` ok for every asset; downloaded binaries run on box (x86_64) and under qemu (aarch64) |

## 4. Totals
| session | tasks | target agents | useful agents | peak concurrent (sector) |
|---|---|---|---|---|
| S1 land integration-1 | 4 | 20 | 17 | 9 (T2 read-only) |
| S2 integration-2 (gaia) | 3 | 6 | 5 | 3 |
| S3 wave-2b lanes | 5 | 12 | 11 | 8 |
| S4 CI workflows | 8 | 20 | 14 | 6 |
| S5 release v0.1.x | 4 | 9 | 7 | 3 |
| S6 freeze / go-no-go | 3 | 7 | 6 | 3 |
| **total** | **27** | **74** | **60** | build-bound at 3 builders |

## 5. Realistic schedule at 16 concurrent agents (all sectors)
This sector should hold **3-4 slots** (2-3 builders + 1 reviewer/merger), not more: the box cannot run more builds and the
other 6 sectors need slots.
| window (Zagreb) | slots | work |
|---|---|---|
| Tue 20 21:00-24:00 CEST | 4 | S1-T1 (2 builders), S1-T2 in 3 waves of 3 reviewers, S1-T3 |
| Wed 21 08:00-20:00 | 4 | S2-T1 + S2-T3 (1 builder), S1-T4 when owner opens PR, S4-T1/T2/T6 (small, 1 slot) |
| Thu 22 | 4 | S2-T2 (when gaia-land pushes), S3 lanes A+B (2 mergers), 1 verifier |
| Fri 23 | 4 | S3 lanes C+D, S4-T3/T4 |
| Sat 24 | 4 | S3-T5 join + verify, S4-T5 (2 agents), S4-T7/T8 |
| Sun 25 (DST: 03:00 -> +01:00) | 2 | S4 wrap-up; check all new timestamps carry +01:00 |
| Mon 26-Tue 27 CET | 3 | S5 (owner: merge integration-3, rc tag) |
| Wed 28-Sat 31 | 3 | S6 |
Total agent-starts on this schedule: about 35-40 (reviewers and verifiers reused across sessions), versus 74 target.

## 6. Merge order (summary)
main 13a296c <- integration-1 8fd26f6 (owner) <- integration-2 (gaia-poseidon -> gaia-hades-tokens -> gaia-land)
<- lane A (common-migrate-2 -> nerve-wiring -> memory-schema) <- lane B (one-hardening -> flake-fix)
<- lane C (lan-routing -> hermes-enroll) <- lane D (sec-runbooks, console-static, tunnel-audit, fuzz-sast, deploy-sim)
<- ci-hardening <- utc-zero (S4-T5) -> owner merges integration-3 -> v0.1.1 tag.
Normal merges only, no rebase, no force-push (RULES.md 6, WAVE2.md).

## 7. Needs Slavik (this sector)
1. Merge `swarm/integration-1` 8fd26f6 into main, and open the PR (or dispatch self-eval) so GitHub CI runs once (S1).
2. RUSTFLAGS/no-SIMD approach: hard `compile_error!` guard + CI env assertion (recommended), or accept dalek's runtime-detected AVX2 as a SIMD exception.
3. Public-export gate 10 (20 IPv4 literals, 1 quick-tunnel host) and export scope (stack/, TEACHER/, TERMINAL-LOG.md).
4. WS-SURV marked D on the branch while no guardian unit is installed (INT1 Needs #3).
5. POSEIDON vs HEPHAESTUS before gaia-land's ownership registry reaches main.
6. memory-schema shared schema change (memory-real.md:100).
7. GPL tools in CI (qemu-user-static in ucpu-qemu; strace/util-linux in room tests): confirm "tool only" is acceptable.
8. Make repo-check `utc` an error once at 0 warnings; make gate 10 a FAIL.
9. Version for the first tag (v0.1.1 proposed) and who pushes tags (tag ruleset: owner only, SCORECARD-READINESS.md:52).
10. GitHub settings: branch protection, tag ruleset, RELEASE_ENABLED after the public repo exists.

## 8. Top risks
1. **Owner merge bottleneck / unproven CI.** integration-1 has never run on GitHub Actions; every later session stacks on it.
   If the PR is not opened by Wed 21, CI failures surface late (ucpu tier-1 alone is up to 40 min per target).
2. **Hotspot conflicts in wave-2b** silently undoing integration fixes: flake-fix vs F1 (escape.rs Android PTRACE_SEIZE),
   common-migrate-2 on pre-rewrite room/desk main.rs, and a 3-way on openmind-memory http.rs/main.rs (common-migrate-2,
   nerve-wiring, memory-schema).
3. **RUSTFLAGS override** brings back the AVX2 dalek backend with only a test to catch it; cargo-fuzz (fuzz-sast) always sets RUSTFLAGS.
4. **Release path not real yet:** release.yml ships 5 gnu x86_64 binaries, not static musl x86_64+aarch64; private-repo tags do not
   carry to the fresh-history public repo; tag v0.1.0 would collide with what live binaries already report.
5. **Box capacity and agent loss:** 8 cores cap the sector at ~3 builders; wave-2 agents lost to the request limit (11 of 15 at
   ~20:14) can leave branches dirty and unmergeable (gaia-land 24 dirty files, flake-fix 20, common-migrate-2 12, nerve-wiring 11).
