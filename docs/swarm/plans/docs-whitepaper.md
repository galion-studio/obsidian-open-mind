<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/plans/docs-whitepaper.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Sector plan: docs-whitepaper
Written 2026-10-20 ~20:45 CEST (+02:00) by the docs-whitepaper planner. Planning only. No code, commits, pushes, builds or VPS contact.
The only commands run were reads: `git log/diff/status`, `rg`, `ls`, `df`, `uptime`, `free`, `nproc`.
Sources are cited as file:line. "wt/X" means /workspace/swarm/wt/X. Repo paths are on wt/integration-1 (head 8fd26f6) unless a branch is named.

## 1. Sector scope
- **Project docs**: docs/STATUS.md, docs/main.md, docs/launch/*, docs/third-party/*, docs/cutover/* (and the *-cutover docs), docs/real-tests/*, docs/integration/*, docs/gaia-ownership.md, docs/SOURCE-OF-TRUTH.md. Also the box records /workspace/swarm/CALL-RECORD.md and CALL-RECAP.md, which are not in the repo.
- **White paper**: Slavik's write-up of tonight's full-capacity swarm test. It started 2026-10-20 20:34 CEST (CALL-RECORD.md:25-29). Rule: **only measured facts**. Every number in it needs a source file and line or a log path. Anything unmeasured is marked TO MEASURE (§5).
- Out of scope: code changes, merges and deploys. Those are other sectors and the owner.

## 2. Current state (measured 20:39-20:45 CEST)
### 2.1 Doc-bearing branches
| branch / worktree | head | time (+02:00) | dirty files | doc relevance |
|---|---|---|---|---|
| main | 13a296c | - | - | base; STATUS.md/SOURCE-OF-TRUTH.md from 227cf2f |
| swarm/integration-1 | 8fd26f6 | 20:32:58 | 0 | all 20 wave-1 docs merged (byte-identical, CALL-RECAP.md:4); 8fd26f6 is docs-only (`git diff --stat 881de9d 8fd26f6` = docs/real-tests/integration-1.md only) |
| swarm/docs-sync | 80e28f3 | 19:11 | 0 | main.md/goals.md/STATUS evidence fixes |
| swarm/launch-gates | 79ee761 | 20:03 | 0 | docs/launch/GATES-OCT31.md, SCORECARD-READINESS.md, PUBLIC-EXPORT.md |
| swarm/gaia-land (slot 01) | c15a4b7 | 18:52 | **24** | corrected docs/gaia-ownership.md (52 systems, 9 functions), not in integration-1. Target dir last written 18:57, so it **looks stalled** |
| swarm/gaia-poseidon | 498e3d2 | 20:25 | 0 | docs/real-tests/gaia-poseidon.md (RESULT pass=60 fail=0 at 20:24:39, line 204) |
| swarm/gaia-hades-tokens | f3f41a2 | 20:25 | 0 | docs/real-tests/gaia-hades-tokens.md (1 real + 10 hermetic, lines 14,17) |
| flake-fix | d194c19 | 19:18 | 20 (was 14 at 20:31 per CALL-RECORD.md:9) | sandbox flake (docs/real-tests/sandbox.md:85 shows 24/25) |

### 2.2 Doc inventory on integration-1
- docs/real-tests: 21 records plus captures/ (3 JSON files). docs/third-party: 21 per-branch files, **with no consolidated register**. docs/cutover: desk.md, room.md, tuner.md. The One cutover is in docs/one-probe-cutover.md. There is **no memory or nerve cutover doc**; deploy-sim (slot 33) is rehearsing the nerve cutover (CALL-RECORD.md:28).
- docs/launch: GATES-OCT31.md (55 lines), PUBLIC-EXPORT.md (107), SCORECARD-READINESS.md (92).
- Evidence logs, box only: /workspace/swarm/evidence/integration-1/*.log (test, clippy, ucpu, public-export, realtest) and evidence/tunnel-audit/.

### 2.3 Measured facts already usable in the white paper
| fact | value | source |
|---|---|---|
| Box hardware at 20:40 | 8 vCPU, 15 GB RAM, 125 GB disk, 58 GB used (46 %), load 3.29/2.56/2.02 | `nproc`, `free -g`, `df -h /`, `uptime` (this planner, 20:40) |
| Swarm target dirs | 11 GB | `du -sh /workspace/swarm/target` (20:39) |
| Burst loss | 15 started, 11 lost to the request limit at ~20:14 | CALL-RECORD.md:7. **Recorded claim only: no raw error log** (see M2) |
| Concurrency | 16 target at 20:34 (8 already running + 8 restarted). At 20:37-20:38 the record says **22 running** (15 coding + 7 planners): "limit not yet re-tested above 16" | CALL-RECORD.md:27-29, :41 |
| Earlier cap | "18:10 wave of 20 agents (16-agent cap hit)" on 2026-10-19, VPS Grok Build tooling | review-20261020/SESSION.md:74 |
| integration-1 merges | 20/20 merged, 12 clean, 8 conflicts, built 20:15-20:35 | docs/integration/integration-1.md:5-30 |
| integration-1 tests | 206 suites, 950 passed, 0 failed, 4 ignored; omnode 72/0 | docs/real-tests/integration-1.md:19-20 |
| integration-1 gates | ucpu-gate ok (27+1 binaries per target); public-export 0 FAIL 1 WARN (20 IPv4, 1 quick-tunnel host); repo-check 1074 files, 0 errors, 28 utc warnings; no-python ok | docs/real-tests/integration-1.md:21-24 |
| Cross-branch bugs found only by integration | Android `PTRACE_SEIZE`, ucpu grep hits, 7 repo-check `hosts` errors, REUSE gate 7 (5 problems) | docs/real-tests/integration-1.md:26-31; integration/integration-1.md F1/F2 |
| realtest stack | 77 checks, 76 pass, 1 skip; with nerve 81/81 | docs/real-tests/integration-1.md:36-60 |
| realtest live (read-only) | 43 checks, 41 pass, 2 known-fail (mouth 8002 503; link-watch.service inactive) | docs/real-tests/integration-1.md:83-92 |
| gaia heal-tick → nerve | healthy/refused/restarted; 5 nerve events; token not leaked | docs/real-tests/integration-1.md:64-81 |
| Diff size | 13a296c..8fd26f6: 462 files, +69 058/−3 760; 69 commits (47 non-merge). **Includes pre-existing ws/gaia code merged via gaia-live, so it is not all swarm-written** | `git diff --shortstat`, `git rev-list --count` |
| gaia systems | 52 (enum in crates/gaia-core/src/system.rs:143ff; the test asserts the count at :237); 9 FunctionId (:26) | code |
| Crate dirs on integration-1 | 53 | `ls crates` |

**Timing caveat (must go in the paper):** gaia-poseidon (20:24:39), gaia-hades-tokens (20:25) and most of integration-1 (20:15-20:35) finished **before** the 20:34 start, and the 11 kills (~20:14) also happened before it. They are wave-2 results that lead into the full-capacity run, not results of it. The paper must separate "pre-run" from "during-run" results.

## 3. Doc defects and fixes (D-list)
| # | defect | evidence | fix | type |
|---|---|---|---|---|
| D1 | CALL-RECAP §4.1: 639/627 test counts are not in any repo file | CALL-RECAP §4.1; CALL-RECORD.md:33 (taken from agents' final reports) | re-measure `cargo test --workspace --locked` totals on 498e3d2 and f3f41a2 and add a "Workspace totals" line with timestamp to each branch's real-tests doc. If the re-run differs, the record changes, not the doc | box re-run (integration-ci) + doc |
| D2 | integration-1 static gates say "final head 881de9d" but the branch head is 8fd26f6 | docs/real-tests/integration-1.md:17 | add one line: "8fd26f6 adds only this file; code = 881de9d" | doc |
| D3 | "23 pushed" cannot be verified from the box (base.git has no swarm refs) | CALL-RECAP.md:3, §4.3; CALL-RECORD.md:34 | store the 20:31 ls-remote output (refs + time) as evidence/fullcap/ls-remote-2031.txt; count unique SHAs separately from branch names (duplicates: nerve-wiring=nerve, one-hardening=one-probe, common-migrate-2=common, flake-fix=sandbox) | record |
| D4 | dirty-file counts change over time (flake-fix 14 → 15 → 20) | CALL-RECORD.md:9; CALL-RECAP §4.4; this plan §2.1 | always stamp counts with a time; leave volatile counts out of the paper | record |
| D5 | "13 units" vs 12: different counting rules | CALL-RECORD.md:38 vs LIVE.md:39 | state the rule (all active units incl. console-getty, tanga-proto) next to the number | record |
| D6 | CALL-RECORD "Needs Slavik" still says galion-health/galion-nerve have "no source in repo" | CALL-RECORD.md:19 vs correction :37 | edit line 19 to "source on branch ws/nerve a63b730, not on main" | box record |
| D7 | Two nine-function lists. integration-1 docs/gaia-ownership.md is the stale VPS GAIA-4 table (GAIA + ARES, no POSEIDON/AETHER, gaia-registry, "thirty-seven directories"), but the code on the same branch has FunctionId HADES..ELEUTHIA | wt/integration-1/docs/gaia-ownership.md:9-44; crates/gaia-core/src/system.rs:11-26; wt/gaia-land/docs/gaia-ownership.md:15-23 | take gaia-land's ownership doc when gaia-land lands. Until then add a banner to integration-1's copy: "superseded; the canonical nine are HADES, MINERVA, APOLLO, HEPHAESTUS, ARTEMIS, POSEIDON, AETHER, DEMETER, ELEUTHIA". Also fix review-20261020/SESSION.md:93 (lists ARES) with an erratum line | doc (after the owner's gaia-land decision) |
| D8 | 51 vs 52 systems | CALL-RECORD.md:39; PROGRESS.md:8 | **52 is measured** (System enum + count test; gaia-design §4.2/§6). `rg` finds no file that says "51 systems", so the 51 comes only from the call. Use 52 everywhere; mark it resolved in CALL-RECORD | record |
| D9 | POSEIDON vs HEPHAESTUS compute ownership is undecided, yet gaia-poseidon already implements the POSEIDON side | CALL-RECAP §4.9; gaia-poseidon.md:32 | doc writes "pending owner decision" until Slavik rules; no doc picks a side | **Slavik** |
| D10 | STATUS.md B42 says "REUSE.toml and LICENSES/ absent", but integration-1 has REUSE.toml, LICENSES/MIT.txt, Apache-2.0.txt, and REUSE gate 7 passes | docs/STATUS.md:54; `ls`; GATES-OCT31.md:13 | B42 → D on integration-1; re-run scripts/status-percent.sh and replace the computed block (STATUS.md:24-30) | doc |
| D11 | STATUS.md "Swarm branches" lists only the 21 wave-1/gaia-land branches; wave-2 branches (integration-1, gaia-poseidon, gaia-hades-tokens, fuzz-sast, hermes-enroll, lan-routing, console-static, sec-runbooks, deploy-sim, tunnel-audit, memory-schema, ...) are missing | docs/STATUS.md (Swarm branches table) | add a wave-2 table with head SHA + pushed/local, stamped with a time | doc |
| D12 | REUSE counts 723 vs 812 flagged "uncertain" | CALL-RECAP §4.13 | resolved: GATES-OCT31.md:13 says repo tree 812 (771 passed, 41 licence texts skipped) vs export tree 723. Mark resolved | record |
| D13 | omnode 84 vs 72; branch totals 497/506/593/950 | CALL-RECAP §4.14-15 | real-tests index (T2.4) with a "measure" column: cargo-test total / real-script checks / realtest checks | doc |
| D14 | main.md is out of date in three places: §7 "use only a couple of agents" (the dated swarm exception is in SOURCE-OF-TRUTH.md:38-46); §15 status snapshot of Oct 18 19:54; repo name `galion-studio/open-mind-stack` vs the real `obsidian-open-mind` (README.md:14,134) | docs/main.md §7, §15; `rg open-mind-stack` hits main.md, goals.md, openssf.md | main.md is Slavik's dictated text ("the base of everything"), so do **not** rewrite it. Add a dated "Notes (not dictation)" block pointing to SOURCE-OF-TRUTH and STATUS. Fixing the repo name in goals.md/openssf.md is a plain doc fix | **Slavik** for main.md |
| D15 | GPL/LGPL test-only tools (strace, util-linux, qemu) and GPL-2.0 WITH syscall-note constants vs the RULES.md licence ban | third-party/room.md:22-27, ucpu.md:13-20; RULES.md open-source section | consolidated register (T2.5) with a "tool only, not linked" column and a status of "pending owner ruling" | **Slavik** |
| D16 | VPS public IP in plain text in docs/real-tests/survival-heal.md (within lines 15-25), although 859ddd0 redacted it in stack/ | CALL-RECAP §4.18 | redact it to `<vps>` or add it to export-ignore. This counts toward gate 10 | **Slavik** (gate 10) |
| D17 | GATES-OCT31.md quotes launch-gates numbers (7 IPv4) that integration-1 superseded (20 IPv4, 1 quick-tunnel host) | GATES-OCT31.md; integration-1.md Needs #4 | add an "as of integration-1 8fd26f6" row | doc |
| D18 | Timezone change: CEST ends 2026-10-25 03:00. Any doc or white-paper text written after that must use +01:00 | RULES.md rule 7; GATES-OCT31.md:6 | a checklist item in S5 | doc |
| D19 | No consolidated cutover index; memory and nerve cutover docs are missing | §2.2 | add docs/cutover/README.md listing each service's doc, approval state ("not approved") and rehearsal evidence. The nerve doc comes from deploy-sim when it finishes | doc |

## 4. White paper outline (title placeholder: "Running a 16+ agent Rust coding swarm on one 8-vCPU box: the 2026-10-20 full-capacity test")
Each section lists **measured evidence available now** (with source) and **TO MEASURE** items (M-ids, §5). No section may state a number that lacks a source.
1. **Abstract.** Written last. Uses only §2.3 numbers and the M-results.
2. **Context and goals.** What Gaia is: a GAIA core plus 9 functions and 52 systems (system.rs; gaia-design §4.2). The Oct 31 launch goal (main.md priority #1, §9). The owner's ask: a ceiling of 250 with auto-accept, ~50 agents per task (PLANNER-BRIEF.md:11). Evidence: present. Flag: the target vs the observed reality.
3. **Setup and rules.** Box specs (§2.3). One worktree, target dir and port range per agent; branch-only; owner merges; push through the VPS helper; no Python; licence allowlist; no deploys; no paid calls (RULES.md 1-9; WAVE2.md). Evidence: present. TO MEASURE: M10, the agent platform and plan tier (Slavik states it; we do not guess).
4. **Timeline.** Wave 1 (20 branches; head commit times 19:03-20:07, from §2.1/`git log`); wave-2 approval ~20:11 (WAVE2.md:1); burst and kills ~20:14; integration 20:15-20:35; full-capacity start 20:34; 7 planners at 20:38; end/cut-off = M9. Needs M1.
5. **Concurrency and the request limit (headline).** Facts: 11 of 15 lost at ~20:14; 16 running from 20:34; 22 recorded at 20:38; box load 3.29 on 8 vCPU at 20:40 → **the request limit, not CPU, was the binding constraint at that moment** (one sample only). Earlier 16-cap observation on Oct 19 (SESSION.md:74, different tooling). TO MEASURE: M1, M2, M3 (it must not say "the ceiling is 16" unless M2 shows it).
6. **Throughput and output.** Branches pushed, commits, lines, tests added, tasks done per agent-hour. Present: integration-1 diff/commit counts (with the ws/gaia caveat), 950 tests on integration-1 vs 497 at docs-sync/launch-gates (CALL-RECAP §1). TO MEASURE: M4, M7.
7. **Quality gates and integration.** fmt/clippy/test, ucpu gate, cargo deny, REUSE, public-export, repo-check. Key finding: 20 green-at-push branches still had 4 cross-branch defect classes that only the integration build caught (F1/F2). Merge cost: 8/20 conflicts, ~20 min, one agent. Evidence: present.
8. **Real-life tests.** realtest stack 76/77 and 81/81; live read-only 41/43; heal-tick → nerve; gaia-poseidon 60/60 and hades tokens (marked pre-run). TO MEASURE: M8 (counts during the run).
9. **Failures and corrections.** 11 lost agents and what work was lost (M5); gaia-land apparently stalled (24 dirty, last commit 18:52); sandbox flake 24/25; two earlier Python slips (RULES.md:6); nerve live "PASS" vs replay FAIL 61/D (nerve.md:47,61); mouth 503; record errors corrected during the call (CALL-RECORD.md:32-39). Evidence: partly present; M5.
10. **Cost.** TO MEASURE: M6 (requests/tokens/$ per agent from the provider dashboard; box cost per hour). State that no OpenRouter/RunPod/GPU spend was allowed by rule (RULES.md rule 5). Note this is a rule, not a measurement, unless billing confirms it.
11. **What did not happen.** Nothing deployed; VPS at 2f5a730 (integration-1.md Needs #8); no PRs; owner merges pending. Evidence: present.
12. **Lessons and sizing.** How many agents a task can really use (from the sector plans in /workspace/swarm/plans/*.md), with the 16-concurrent schedule vs the 250/50 target. Evidence: the plans themselves plus M2.
13. **Limitations and reproducibility.** Single box, single night, one sample of box load, counts taken from agent reports. Appendix: evidence index (file:line, log paths) and the commands used.

## 5. What must still be measured (read-only, no Python, CPU-polite)
| id | metric | method | owner of data |
|---|---|---|---|
| M1 | concurrency time series (running agents per minute, 20:34 → end) | agent-platform session list export. Box proxy: a `ps` count of cargo/rustc processes and worktree mtimes every 60 s, logged to evidence/fullcap/box.log by one `while sleep 60` shell (no busy loop) | Slavik (platform) + box |
| M2 | request-limit ceiling and failure mode | raw error text and time of each kill (the 11 at ~20:14: **which slugs** is not recorded, CALL-RECAP §4.16). A stepped test 16 → 20 → 24 → 32, ≥20 min each, 0 kills = pass. 22 is already running since 20:38, so log whether it holds | Slavik approval (kills lose work) |
| M3 | box load/RAM/disk series | `uptime; free -m; df -h /; du -sh target` every 60 s into the same log | box |
| M4 | throughput | per worktree: `git log --since=20:34 --format='%h %ci'`, `git diff --shortstat 13a296c..HEAD`, push times from push-branch.sh output; tasks done/hour; agent-hours | box |
| M5 | lost work from kills | for each killed slug: dirty files at kill time, last commit, whether it restarted (e.g. fuzz-sast, hermes-enroll and lan-routing have merge-only heads, 2/1/5 dirty) | box |
| M6 | cost | requests, tokens and USD per agent and in total for 20:34 → end, from the provider/billing dashboard; box hosting cost/hour | **Slavik only**. Not visible from the box |
| M7 | integration cost | wall time and conflicts per integration run (integration-1: 20 min, 8 conflicts) for any integration-2 tonight | box |
| M8 | test counts recorded in-repo | per pushed branch: `cargo test --workspace --locked` summary line + time, into its real-tests doc (fixes D1) | integration-ci sector |
| M9 | run end and cut-off | Slavik names the cut-off time; the paper freezes numbers then | Slavik |
| M10 | agent platform, plan and limit settings ("ceiling 250, auto-accept") as configured | screenshot or settings text, no secrets | Slavik |

## 6. Sessions
Parallelism, honestly: doc work splits by **file** (one writer per file, or conflicts follow) and white-paper work by **section**. No task here can use 50 agents. Most extra agents would just repeat each other's reading and add request-limit pressure, which is the very thing the paper measures. Target column = Slavik's ~50/task model applied where it makes sense; Useful = the real maximum.

### S1: Measurement freeze (now → cut-off; runs during the test)
Goal: collect M1-M10 without adding load.
| task | target | useful | parallel units | inputs/deps | success criterion (real test) |
|---|---|---|---|---|---|
| T1.1 Agent ledger evidence/fullcap/ledger.md (slot, slug, start, end, outcome, head, dirty count) | 1 | 1 | none (single file) | CALL-RECORD.md, `git -C wt/* log/status` | every slot in CALL-RECORD.md:27-28 and :41 has a row with times; spot-check 3 rows against `git log` |
| T1.2 Box sampler log (M1 proxy, M3) | 0 (one shell loop run by the orchestrator) | 0 | - | none | ≥1 line/min from 20:45 to the cut-off; `wc -l` matches the minutes elapsed ±2 |
| T1.3 Kill evidence for 20:14 + stepped ceiling test (M2) | 0 extra (uses other sectors' agents) | 0 | step levels | Slavik approval | a table of level → kills → error text; ≥20 min per level |
| T1.4 Throughput snapshot (M4, M5, M7) at the cut-off | 2 | 2 | wave-1 worktrees / wave-2 worktrees | T1.1 | the totals reproduce when the commands are re-run |
| T1.5 Cost + platform settings (M6, M10) | 0 | 0 | - | Slavik exports | numbers carry a source file |
Approval: Slavik for T1.3 (deliberate concurrency steps), T1.5 data, M9 cut-off.

### S2: Doc fixes (branch(es) off integration-1 8fd26f6, after Slavik's merge-order decision)
Goal: fix D1-D19. One writer per file; separate small branches `swarm/docs-<x>` with disjoint files, merged in the order below.
| task | target | useful | parallel units | deps | success criterion |
|---|---|---|---|---|---|
| T2.1 STATUS.md: B42 (D10), wave-2 table (D11), re-run status-percent | 1 | 1 | single file | T1.4 heads | `scripts/status-percent.sh` output equals the block in the file; repo-check 0 errors |
| T2.2 gaia-ownership.md + nine/52 sweep (D7, D8) | 2 | 2 | ownership doc / rg sweep of other docs | **gaia-land decision** | `rg -n "\bARES\b|51 systems|gaia-registry"` returns only historical/erratum lines; `cargo test -p gaia-core` count test still passes (CI) |
| T2.3 Real-tests index docs/real-tests/README.md (D1, D2, D13) | 2 | 2 | wave-1 rows / wave-2 rows | M8 counts | each row: branch, head, measure type, number, file:line, time; 5 random rows re-checked against the files |
| T2.4 Third-party register docs/third-party/README.md (D15) | 2 | 2 | linked deps (cross-check with SBOM: 130 components, GATES-OCT31.md:15) / tools-only | cargo deny output already in evidence | every crate in SBOM that appears in a third-party doc is listed; `cargo deny check licenses` result quoted; GPL tools listed as "pending ruling" |
| T2.5 Cutover index docs/cutover/README.md (D19) | 1 | 1 | single file | deploy-sim result | every live unit from LIVE.md:39 has a row (doc or "none") |
| T2.6 Launch docs refresh (D17, D18) | 1 | 1 | single file | integration-1 numbers | numbers match docs/real-tests/integration-1.md:22-23 |
| T2.7 Box records: CALL-RECORD line 19 (D6), D3/D4/D5/D8/D12 resolutions as an errata block in CALL-RECAP §4 | 1 | 1 | single writer (live file) | none | each §4 item marked resolved/open with a source |
| T2.8 main.md notes block + repo-name fix in goals.md/openssf.md (D14) | 1 | 1 | single file | **Slavik approval** for main.md | Slavik's text unchanged (`git diff` shows additions only) |
| T2.9 Redaction of the VPS IP (D16) | 1 | 1 | single file | **Slavik** (gate 10) | `scripts/public-export.sh` gate 10 count goes down by the redacted lines |
S2 totals: 9 tasks, target 12, useful 12. Merge order: T2.1 → T2.3 → T2.4 → T2.5 → T2.6 → T2.2 (after gaia-land) → T2.8/T2.9 (after approval). All branches pass fmt, clippy, test, no-python and repo-check before push, as RULES.md requires. Docs-only branches still run the CI gates.

### S3: White paper draft (after the cut-off M9)
Goal: the draft in docs/whitepaper/2026-10-20-full-capacity.md (SPDX MIT header, Zagreb times).
| task | target | useful | parallel units | deps | success |
|---|---|---|---|---|---|
| T3.1 Section drafts §4.2-§4.13 | 12 | 6 | sections (pairs: 2+3, 4+5, 6+7, 8+9, 10+11, 12+13) | S1 complete | each number has an inline source; no TO MEASURE left open without a "not measured" label |
| T3.2 Evidence appendix (file:line index, commands) | 1 | 1 | single file | T3.1 | every citation in the draft resolves (scripted `rg` of each cited file:line) |
| T3.3 Abstract + editor merge | 1 | 1 | single | T3.1-T3.2 | consistent numbers across sections (one number table) |
S3 totals: 3 tasks, target 14, useful 8.

### S4: Fact-check (adversarial, separate agents from the writers)
| task | target | useful | parallel units | deps | success |
|---|---|---|---|---|---|
| T4.1 Claim-by-claim check (each number vs its source, re-running read-only commands where possible) | 50 (claims) | 8 | one checker per 1-2 sections | S3 | 0 unsourced numbers; a disagreement list resolved |
| T4.2 Pre-run vs during-run labelling check (the timing caveat in §2.3) | 1 | 1 | single | S3 | every result has a timestamp and the right label |
S4 totals: 2 tasks, target 51, useful 9.

### S5: Publication readiness (agents prepare; Slavik publishes)
| task | target | useful | parallel units | deps | success |
|---|---|---|---|---|---|
| T5.1 Redaction + secret/IP scan of the paper (gitleaks, public-export gate 10 logic, no hostnames of private tunnels) | 1 | 1 | single | S4 | 0 secrets; IPv4/hostnames only with Slavik's OK |
| T5.2 Format gates: SPDX header, Zagreb offsets (+02:00 before / +01:00 after 2026-10-25 03:00), no `Z`/UTC, repo-check | 1 | 1 | single | S4 | repo-check 0 errors on the file |
| T5.3 Slavik review + publish | 0 | 0 | - | T5.1-T5.2 | **Slavik approves and publishes** (agents never publish, RULES.md rule 5) |
S5 totals: 3 tasks, target 2, useful 2.

## 7. Realistic schedule at 16 concurrent (shared with the code sectors)
The docs sector should hold **at most 3 of the 16 slots** while code sectors run. Docs work is light on CPU but costs requests, the same as any agent.
- Tonight (now → cut-off): S1 only, 1-2 slots (T1.1, T1.4 at the cut-off). Meanwhile the orchestrator's sampler shell runs.
- After the cut-off: S2 tasks T2.1, T2.3, T2.4, T2.5, T2.6, T2.7 in 3 slots, two rounds (~6 agent-runs). T2.2/T2.8/T2.9 wait for decisions.
- Next: S3 with 6 slots if the code sectors have freed them (otherwise 3 slots, two rounds), then S4 with 8 slots (or 4 × 2 rounds), then S5 with 2.
- Critical path: M9 cut-off → M6 cost data from Slavik → S3 → S4 → Slavik review. Everything is doable well before Oct 31. The cost section is blocked only on Slavik's export.

## 8. Totals
| session | tasks | target agents | useful agents |
|---|---|---|---|
| S1 measurement | 5 | 3 | 3 |
| S2 doc fixes | 9 | 12 | 12 |
| S3 draft | 3 | 14 | 8 |
| S4 fact-check | 2 | 51 | 9 |
| S5 publication prep | 3 | 2 | 2 |
| **total** | **22** | **82** | **34** |
Peak useful concurrency for this sector: 8 (S4). In the 16-slot reality: 3 slots.

## 9. Items needing Slavik's approval or data
1. Cut-off time for the white paper (M9) and agreement to separate pre-run results (gaia-poseidon, gaia-hades-tokens, integration-1, the 20:14 kills) from during-run results.
2. A stepped concurrency test above 16 (M2). Kills lose work.
3. Cost/usage export and platform settings (M6, M10). Not available from the box.
4. POSEIDON vs HEPHAESTUS (D9); gaia-land merge decision (D7, which unblocks T2.2).
5. GPL/LGPL test-only tools and kernel syscall constants ruling (D15).
6. Gate 10: the VPS IP in survival-heal.md and IPv4/hostname disclosure in the paper (D16, T5.1).
7. Adding a dated notes block to main.md (D14).
8. Publishing the white paper (T5.3). Agents prepare it, Slavik publishes.

## 10. Top risks
1. **The headline is not yet backed by raw evidence.** "11 of 15 lost to the request limit" and "16 survived" exist only as call-record lines. There is no error text, no list of which slugs died, and no time series. Without M1/M2 the paper cannot state a ceiling. The record also shows 22 running at 20:38, so "16 is the limit" may already be wrong.
2. **Mixing pre-run and during-run results.** The best numbers (60/60, 950 tests, 20/20 merges) came before 20:34. Presenting them as results of the full-capacity run would be false.
3. **Cost cannot be measured from the box.** The cost section depends entirely on Slavik's billing export.
4. **Test counts are inconsistent and partly undocumented** (639/627 only in agent reports; different measures: 72 vs 84, 497/506/593/950). Re-measuring needs builds (other sectors' slots and CPU).
5. **Disclosure and governance gaps in a public paper.** The VPS IP is in docs, 20 IPv4 literals and a quick-tunnel host sit at gate 10, live-vs-branch claims are open (nerve PASS vs FAIL, mouth 503, nothing deployed), and the stale nine-function/ARES doc is on the integration branch. Also the CEST→CET switch on Oct 25.
