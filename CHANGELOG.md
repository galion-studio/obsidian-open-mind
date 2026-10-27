<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Changelog

All times Europe/Zagreb with explicit offset. DRAFT (wave-4 slot 263, integration-ci S5-T1, 2026-10-27 CET +01:00).
Not tagged, not released. Built only from the merge record `MERGE-20261021.md` (staging run 2026-10-21 23:07 +02:00 to 2026-10-22 01:27 +02:00).

## [0.1.1] - UNRELEASED (proposed)

Status: content = `swarm/main-staging` 7c95348b (PR #12, open, CI not green). main is still 13a296c.
This section is valid only once PR #12 (or an integration-3 successor) is merged by the owner; re-generate if the merged set changes.

Gate evidence on staging 7e2a12e1: 367 suites, 1951 passed / 0 failed / 18 ignored (box, excl. openmind-sandbox, cgroup-detect, hermes-tools); VPS sandbox suites passed (see MERGE-20261021.md). 7c95348b adds CI fixes only (clippy, omnode seccomp SYS_OPEN_TREE/SYS_MOVE_MOUNT, miri socket test, REUSE).

### Platform / galion-one / VPS ops
- `galion-deploy-fastfail` (7a1a615)
- `galion-one-deploy` (a93218d)
- `health-fixes` (62ddad8)
- `live-ai-plan` (644c4f7)
- `mouth-loading-diag` (a68ad3e)
- `omnode-enroll` (26b48a4)
- `one-hardening` (fed3de4)
- `one-no-shellout` (40454f9)
- `tuner-quick-unit` (bff2d94)
- `vps-anomalies` (4e5f814)
- `vps-baseline` (f403eeb)
- `vps-baseline-2` (f761f49)
- `vps-deploy-20261021` (d9bd829)
- `vps-dst-check` (56c8a7f)
- `vps-linkenv-doc` (8c257b9)
- `vps-rollback-table` (7a1990e)
- `vps-traffic-hermes` (1c40c5e)
- `vps-tuner-dns` (f67f37f)
- `vps-unit-hygiene` (b985e43)

### Nerve
- `nerve-cross-target` (71f895b)
- `nerve-dryrun-live` (80974b9)
- `nerve-dst-clock` (c229a87)
- `nerve-event-driven` (bb287cc)
- `nerve-eventlog-deploy` (0ddce9b)
- `nerve-parser-fuzz` (5ad5d99)
- `nerve-shadow-20261021` (4800770)
- `nerve-t23-specs` (7c32ec3)
- `nerve-t32-reflex-safety` (c1a9be8)
- `nerve-t33-backpressure` (dfa93a1)
- `nerve-t36-consumer` (334beaa)
- `nerve-t41-statedir` (cd359d6)

### Cortex
- `cortex-connrate-fix` (c1a15f7)
- `cortex-event-ingest` (7a55e6c)
- `cortex-merge-all` (6bcb882)
- `cortex-s2-a` (9557eb3)
- `cortex-s2-c` (dbec5c9)
- `cortex-s2-d` (f3468ce)
- `cortex-state-titles` (967459d)
- `cortex-t20-merge` (ee75f75)
- `cortex-t212-edgerate` (9a48217)
- `cortex-t32-correlate` (ae604dc)
- `cortex-t52-fuzz` (c2f7cda)

### Gaia / Poseidon / Hades
- `gaia-lan-fallback-test` (b8358fb)
- `gaia-poseidon-handle` (dd39191)
- `gaia-tick-loop` (e4631a6)
- `hades-key-custody` (84e988f)
- `hades-option-b` (0f41f29)
- `poseidon-auth` (d10cb09)
- `poseidon-deploy` (d2c5cf1)
- `poseidon-refusal-persist` (eec2d99)

### Hermes and benchmarks
- `backup-perf` (5c3a8ed)
- `bench-banking-adv` (44e5c61)
- `bench-data-licence` (493f175)
- `bench-scoreboard-adv` (1f0556d)
- `bench-vending-adv` (1b0a30c), already contained via a later merge
- `bench-ws-adv` (2ecfc19)
- `hermes-enroll` (f2121a8), already contained via a later merge
- `hermes-enroll-2` (c28374e), already contained via a later merge
- `hermes-legacy-retire` (7622b8b)
- `hermes-mem-growth` (c2457cd)
- `hermes-tools-quota` (2b9944e)

### Sandbox and security
- `guard-shadow` (7b7b1fc)
- `sandbox-hide-galion` (ad0ed9b), already contained via a later merge
- `sandbox-quota-hide-merge` (78c5c49)
- `sandbox-real-problems` (334c633)
- `sandbox-seebystander` (b47f53d)
- `sandbox-workdir-quota` (28f6c1d)
- `sec-denylist-parity` (688d883)
- `sec-history-purge-plan` (d45b248)
- `sec-miri-pure` (91353ba)

### Console
- `console-port-merge` (508401d)
- `console-route-31` (1776d16)

### CI, toolchain and integration
- `ci-miri-schedule` (658cbb1)
- `int5-candle-repocheck` (58a89bc)
- `integration-5` (0d68f46)
- `integration-verify` (abd4dea)
- `realscript-port-3` (03967fc)
- `sqlite-pure-spike` (98686ef)
- `toolchain-aarch64` (4d0c036)

### Docs and records
- `decision-memos` (1a0b851)
- `docs-cutover-index` (ed5f740)
- `docs-cutover-missing` (878485d)
- `docs-realtests-index` (dc16955)
- `docs-status-refresh` (0d9c4bb)
- `docs-thirdparty-register` (7cd7d0c)
- `processes-refresh` (73e1641)
- `settings-snapshot` (241aecf)
- `slavik-todo` (87fdd91), already contained via a later merge
- `slavik-todo-hr` (f72999e)
- `swarm-record` (749ddbd)
- `whitepaper-evidence` (8c0ac37)
- `whitepaper-factcheck` (58d0d73)
- `whitepaper-update` (cc00dc4)
- `whitepaper-vending` (5ddd828)

### Not in this release (skipped at merge time, 45 branches)
- `bench-ws-ecb-data` (03714aa): merge conflict: crates/galion-wallstreet/src/http.rs crates/galion-wallstreet/src/main.rs
- `ci-fuzz-workflow` (3a188b9): gate fail: GATE b10-ci-1 REPOCHECK FAIL (log logs/b10-ci-1)
- `ci-realtest-with-nerve` (7a95e06): merge conflict: crates/galion-realtest/tests/real_stack.rs
- `ci-release-artefacts` (04ce874): merge conflict: Cargo.toml crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs crates/galion-health/src/main.rs
- `ci-toolchain-pin` (e871395): merge conflict: crates/galion-realtest/tests/real_toolchain_aarch64.rs
- `ci-workflow-rust` (38a26cb): merge conflict: .cargo/config.toml .github/workflows/self-eval.yml docs/third-party/ucpu.md
- `context-compact` (3ea7367): merge conflict: crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs
- `cortex-event-driven` (c7f56b8): contains t42 + protected-hosts: 28 conflict hunks resolved (WIP tag wip-event-driven-resolution, local only) but cortex_protected_hosts fails vs hades-gate S3.1; NOTE this is the live gaia-cortex build
- `cortex-hermes-boss` (1c5dec4): cortex-event-driven lineage (contains t42/protected-hosts refused by hades-gate S3.1); textual conflicts in nerve engine/lib/main/replay; NEXT: dedicated cortex-lineage integration
- `cortex-live-fixes` (bd6fbe6): cortex-event-driven lineage (contains t42/protected-hosts refused by hades-gate S3.1); textual conflicts in nerve engine/lib/main/replay; NEXT: dedicated cortex-lineage integration
- `cortex-protected-hosts` (60312ac): contains cortex-t42-hades-plans; cortex_protected_hosts 3/6 fail vs hades-gate S3.1 (route -> network change)
- `cortex-t31-calibrate` (b1c3efd): contains cortex-t53-replay-corpus: same replay_corpus drift (2 tests); text conflicts resolved (WIP tag wip-t31-resolution, local only)
- `cortex-t42-hades-plans` (1ce9737): real conflict: text conflicts resolvable, but cortex_t42_plans 4/5 fail: plan templates refused by staging hades-gate S3.1 safety (allow-listed plan verbs; words route/source -> AdminAccount network change, Opaque source). Needs owner decision: reword templates or extend allow-list
- `cortex-t44-e2e` (7893698): cortex-event-driven lineage (contains t42/protected-hosts refused by hades-gate S3.1); textual conflicts in nerve engine/lib/main/replay; NEXT: dedicated cortex-lineage integration
- `cortex-t51-chaos` (e982210): cortex-event-driven lineage (contains t42/protected-hosts refused by hades-gate S3.1); textual conflicts in nerve engine/lib/main/replay; NEXT: dedicated cortex-lineage integration
- `cortex-t53-replay-corpus` (55f07ff): replay_corpus golden drift after merge with the nerve lineage (19 verdict changes on fixture-cortex-replay; new capture fixture:consumer not in corpus.json); regenerating goldens = owner call
- `flake-fix` (b2e69b9): merge conflict: crates/galion-desk/tests/desk_http.rs crates/openmind-sandbox/examples/escape.rs crates/openmind-sandbox/tests/sandbox.rs
- `fuzz-sast` (e78b2e7): merge conflict: .github/workflows/fuzz.yml Cargo.toml
- `fuzz-sast-2` (f3a10af): merge conflict: .github/workflows/fuzz.yml Cargo.toml
- `gaia-fault-followups` (e806777): contains gaia-loop-decisions (same repo-check hosts error)
- `gaia-fault-inject` (e970622): contains gaia-loop-decisions (repo-check hosts error 8.8.8.8 in crates/gaia/tests/heal_decisions.rs:420)
- `gaia-hades-typed` (c223109): merge conflict: crates/gaia/src/lib.rs crates/gaia/src/main.rs
- `gaia-heal-tick-unit` (9468bb4): gate fail: GATE b06-gaia-3 REPOCHECK FAIL (log logs/b06-gaia-3)
- `gaia-live-classify` (055fe91): gate fail: GATE b07-gaia-6 REPOCHECK FAIL (log logs/b07-gaia-6)
- `gaia-loop-decisions` (b40208a): gate fail: GATE b06-gaia-2 REPOCHECK FAIL (log logs/b06-gaia-2)
- `gaia-queue-fuzz` (451db69): merge conflict: crates/openmind-queue/src/http.rs docs/poseidon.md
- `hades-ledger-fail` (84fef87): merge conflict: crates/hades-gate/src/main.rs crates/hades-gate/tests/real_cli.rs
- `hades-rustflags-guard` (b187398): merge conflict: crates/hades-gate/tests/real_cli.rs
- `hades-safety-corpus` (3c30586): gate fail: GATE b09-sec-2 TEST FAIL (log logs/b09-sec-2)
- `hermes-bench-e2e` (2b11a5b): merge conflict: .gitignore Cargo.lock crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs crates/hermes-gateway/Cargo.toml crates/hermes-gateway/src/main.rs docs/gaia-ownership.md
- `hermes-core` (45b7980): merge conflict: .gitignore Cargo.lock crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs
- `hermes-core-bench-sector` (12be83b): merge conflict: .gitignore Cargo.lock crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs docs/gaia-ownership.md
- `hermes-core-docs` (3611fc6): merge conflict: .gitignore Cargo.lock crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs
- `hermes-gateway-bench` (20d9a2f): merge conflict: crates/hermes-gateway/Cargo.toml crates/hermes-gateway/src/main.rs
- `hermes-readiness-recheck` (7c7a613): merge conflict: .gitignore Cargo.lock crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs
- `integration-4-squash` (9b5f6ad): superseded squash of integration-4 (its content arrived via the individual branches); merge conflicts with many files
- `memory-schema` (7952626): gate fail: GATE b01-ws-14 REPOCHECK FAIL (log logs/b01-ws-14)
- `nerve-t24-coverage-gate` (457bc87): gate fail: GATE b11-vps-11 REPOCHECK FAIL (log logs/b11-vps-11)
- `no-openrouter-nerve-cortex` (49163e0): real conflict: its test no_openrouter (no LLM path in nerve source) fails against nerve-event-driven evt/escalate.rs (openrouter.ai, chat/completions); lib.rs text conflict resolvable but semantics need owner decision
- `poseidon-crash-soak` (b71a909): merge conflict: docs/poseidon.md
- `realscript-port-1` (9d04111): merge conflict: crates/galion-common/tests/real_hammer.rs docs/gaia-poseidon.md docs/real-tests/gaia-live.md
- `realscript-port-2` (cd4bf26): merge conflict: Cargo.lock crates/openmind-nerve/Cargo.toml scripts/real/nerve-capture-vps.sh
- `sec-deny-wildcards` (2482909): merge conflict: crates/galion-health/Cargo.toml
- `sec-ledger-race` (8234150): merge conflict: crates/hades-gate/src/main.rs crates/hades-gate/tests/real_cli.rs docs/hades-gate.md
- `sec-runbooks` (c5fc269): gate fail: GATE b09-sec-11 TEST FAIL (log logs/b09-sec-11)
- `serde-order-pin` (1d222ad): gate fail: GATE b01-ws-2 TEST FAIL GATE b01-ws-2 REPOCHECK FAIL (log logs/b01-ws-2)
- `shared-tar-crate` (820740e): gate fail: GATE b01-ws-5 TEST FAIL (log logs/b01-ws-5)
- `sqlite-aarch64-cc` (af6b55c): merge conflict: .gitignore
- `voice-speed` (ff6c34f): merge conflict: crates/galion-voice-cpu/Cargo.toml
- `vps-preflight-scripts` (7733aa9): merge conflict: crates/gaia-core/src/ownership.rs crates/gaia/tests/commands.rs
- `vps-strays` (99e7c58): merge conflict: crates/hermes-gateway/src/main.rs crates/hermes-rs/src/provider.rs
- `vps-version-flags` (81991e2): merge conflict: Cargo.lock crates/galion-health/src/main.rs crates/galion-voice-cpu/src/bin/galion-voice-serve.rs crates/hermes-bench/src/main.rs

### Known issues at draft time (from PR #12 CI)
- Box stack scenarios 11/12 fail (galion-boot supervise exits; one/memory/tuner/desk never healthy): runtime regression, blocks release.
- hermes-tools `sandboxed` test fails on GitHub runners (userns uid/gid map refused).
- ucpu armv7: galion-backup tests/real_perf.rs ru_maxrss i32 vs i64.
- ucpu grep gate: 3 `-mcpu` hits in galion-realtest.
- miri gate-schema: hades-gate tests/typed.rs REALTIME clock under isolation.
- gitleaks: 4 false positives (reported, not allowlisted).

## [0.1.0] - 2026-10-20 (main 13a296c, PR #8 merge/one-source-20261020)
- Baseline. Live VPS binaries report 0.1.0.
