<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/plans/wave4-tasks.md copied from the swarm box on 2026-10-21 06:39 CEST +02:00 by swarm/processes-refresh (slot 142). The box copy is the working copy; this is the durable record. -->

# Wave 4 tasks (2026-10-21 00:55 CEST +02:00) — 50 agents, Slavik's 50-group ceiling (max 250)

Common rules for EVERY slot: read /workspace/swarm/RULES.md and /workspace/swarm/WAVE3-PREAMBLE.md first and obey them
(Rust only, no Python; never print/copy secrets; VPS read-only unless your task says otherwise - no service stop/restart/reconfigure;
never touch /opt/galion/openmind; swarm/<slug> branches only, no main, no force-push, no PRs; Zagreb times with +02:00;
SPDX MIT headers "Copyright (c) 3000 Galion / galion-studio"; identity Grok Bot <grokbot@galion.studio>;
push with `export PATH=/workspace/swarm/bin:$PATH; /workspace/swarm/push-branch.sh <worktree> swarm/<slug>`).
BUILD QUEUE (new, mandatory): run every cargo build/test/clippy as `cargo-slot <args>` (in /workspace/swarm/bin; max 3 concurrent builds,
waits for free RAM) with `-j 2` and `CARGO_TARGET_DIR=/workspace/swarm/target/<slug>`. Disk: 73 GB free; delete your own target dir when done.
aarch64 musl: use CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld. Ports: 19700+10*(N-100) .. +9 where N is your slot number.
Branches not in /workspace/swarm/base.git can be fetched from /workspace/swarm/wt/<name>. Make your worktree at /workspace/swarm/wt/<slug>.
Docs/plan-only tasks: no builds needed. Report: branch + SHA, test numbers, real-test evidence, owner decisions, anything unsafe.

| Slot | Slug | Sector | Task |
|---|---|---|---|
| 100 | fuzz-sast-2 | security | Resume swarm/fuzz-sast e78b2e7 (previous agent stalled 4 h): read its docs, finish its remaining fuzz/SAST work, push. |
| 101 | nerve-wiring-2 | nerve | Resume swarm/nerve-wiring 8d7a924 (stalled): finish it and port its .sh real test to a Rust GALION_REAL=1 test. |
| 102 | hermes-enroll-2 | security | Resume swarm/hermes-enroll f2121a8 (stalled): finish and push. |
| 103 | tunnel-audit-2 | integration-ci | Resume swarm/tunnel-audit d35ef73 (stalled): finish and push. |
| 104 | cortex-t20-merge | cortex | T2.0: merge swarm/cortex-s2-a 9557eb3, -b 51100e1, -c dbec5c9, -d f3468ce onto cortex-sensor e536446; resolve sensors/mod.rs conflicts; add ssh.service to journal collection in collect.rs; update docs/cortex/sensors.md table; all tests green. |
| 105 | cortex-state-titles | cortex | On cortex-sensor e536446: remove top.title/top.fix bind-address leak from cortex_events in /api/v1/state (cortex.rs); goldens updated. |
| 106 | cortex-event-ingest | cortex | Plan T2.10 other direction: Gaia POST /event JSONL -> cortex findings sensor, on cortex-sensor e536446. |
| 107 | cortex-t212-edgerate | cortex | T2.12 per-hostname edge rate from cloudflared journal lines, incl. the engine.rs hook, on cortex-sensor e536446. |
| 108 | cortex-score-v2 | cortex | T3.3 score v2 design+impl so ddos-kind findings score on their own (no double counting with Signals), on cortex-sensor e536446. |
| 109 | candle-zip-fix | security | Bump candle in galion-voice-cpu so zip >= 9.0.0 (RUSTSEC-2026-0367/8/9); base swarm/voice-rust 03d21a9 if candle lives there else main; prove voice tests still pass. |
| 110 | integration-3 | gaia | Build swarm/integration-3 = integration-2 df42e0f + hades-redteam e6ceca7 + health-land f4a66ea + rust-ports 26a2a8e + deploy-rust 787a54d + cutover-prep a5ac49b + build-info db9924d + bench-hermes 457ef74 + cortex-sensor e536446. Resolve conflicts, workspace green. Do not include cortex-s2-*. |
| 111 | integration-verify | integration-ci | On integration-2 df42e0f: re-run the 7 older real-life test scripts, REUSE and cargo-deny/licence checks; report pass/fail per item (the main-merge blockers). |
| 112 | realscript-port-1 | integration-ci | Port the first third (alphabetical) of remaining scripts/real/*.sh on integration-2 to Rust GALION_REAL=1 tests; delete ported .sh. |
| 113 | realscript-port-2 | integration-ci | Same, second third. |
| 114 | realscript-port-3 | integration-ci | Same, last third. |
| 115 | ci-workflow-rust | integration-ci | Inventory .github CI YAML on integration-2; make every CI step call Rust tools (cargo xtask style); keep YAML minimal; document. |
| 116 | vps-traffic-hermes | vps-ops | READ-ONLY: find what makes ~1.4 new conn/s to hermes-rs :8642 and ~1.1/s to galion-one :8091 (ss/journalctl/proc, no captures with addresses committed). Write docs/ops/steady-traffic.md. |
| 117 | vps-strays | vps-ops | READ-ONLY: document stray mock gateways 8652/8653 and /root/growth-20261018/bin .new builds (owner, start time, memory trend, leak?). Recommend stop/keep; do not stop them. |
| 118 | vps-tuner-dns | vps-ops | READ-ONLY: why tuner.galion.app doesn't resolve (cloudflared config, DNS records via public resolvers); write fix proposal. |
| 119 | backup-perf | vps-ops | Measure galion-backup run (rust-ports 26a2a8e) on a box fake tree sized like the VPS (46 MB DBs, 1845 files); time, RSS; compare bash. |
| 120 | tuner-quick-unit | vps-ops | Turn transient tuner-quick (public URL) into a proper unit file + runbook step (deploy/ files only, not installed). Base cutover-prep a5ac49b. |
| 121 | galion-deploy-fastfail | vps-ops | On deploy-rust 787a54d: optional fail-fast on crash-loop to cut L1 rollback 20 s, behind a flag; rehearsal numbers. |
| 122 | shared-tar-crate | vps-ops | Shared std-only tar writer+reader crate used by galion-backup and galion-deploy; base on integration of rust-ports 26a2a8e + cutover-prep a5ac49b; parity tests unchanged. |
| 123 | guard-shadow | security | On guard-rust d5f7874: shadow mode (log-only, no nft writes) + side-by-side comparison report vs fail2ban format; rollout doc. |
| 124 | hades-option-b | security | Prototype HADES option B (typed signed plan steps) on hades-redteam e6ceca7 as a branch; redteam 82 attacks must stay 0; doc for Slavik's choice. Does not decide for him. |
| 125 | hades-ledger-fail | security | Make hades-gate detectable when its ledger silently fails to write (finding 4 of cortex-s2-d); base hades-redteam e6ceca7. |
| 126 | sec-history-purge-plan | security | Plan (doc only, no rewrite, no force-push) for SEC-1..5 git history purge of old leaked secrets: tool (Rust git-filter equivalent), affected blobs by path only, rotation list, steps. |
| 127 | hermes-core-docs | gaia | Update docs/hermes-core.md section 7 on hermes-core 45b7980 for the Revolut lift (scope allowed; no money moves); push swarm/hermes-core-docs. |
| 128 | hermes-gateway-bench | gaia | Register hermes-bench tools in hermes-gateway behind HERMES_RS_BENCH=1, base bench-hermes 457ef74; loopback test. |
| 129 | hermes-core-bench-sector | gaia | Add a "bench" sector to hermes-core (45b7980 + bench-hermes 457ef74) so Hermes coordinates benchmark runs; readiness gate entry. |
| 130 | hermes-readiness-recheck | gaia | Re-run hermes-core readiness G1-G15 (box), check G11 OpenRouter on the VPS read-only (status only, never print key). Report what still fails. |
| 131 | bench-banking-adv | benchmarks | Adversarial banking set: prompt-injection in transaction memos, social-engineering payee asks; must still refuse; base bench-hermes 457ef74. |
| 132 | bench-ws-adv | benchmarks | Adversarial Wall Street set: reworded insider tips, pump-and-dump asks, beyond phrase matching; base bench-hermes 457ef74. |
| 133 | bench-vending-adv | benchmarks | Harder vending seeds + pricing axis; keep scripted 100% reachable; base bench-hermes 457ef74. |
| 134 | bench-data-licence | benchmarks | Research (doc only) free public delayed market-data sources whose terms allow committing a small slice to a public repo; cite terms URLs. |
| 135 | console-port-1 | docs-whitepaper | Port the 1st fifth (alphabetical) of the 17 remaining TS console routes to Rust in console-ui (base landing-overview 669dc18). |
| 136 | console-port-2 | docs-whitepaper | Same, 2nd fifth. |
| 137 | console-port-3 | docs-whitepaper | Same, 3rd fifth. |
| 138 | console-port-4 | docs-whitepaper | Same, 4th fifth. |
| 139 | console-port-5 | docs-whitepaper | Same, last fifth. |
| 140 | whitepaper-update | docs-whitepaper | Update the whitepaper evidence (8c0ac37) with tonight's results: HADES 48->0, benchmarks, cortex findings, byte-identical rebuild. Facts with SHAs only. NOTE: vending whitepaper done on swarm/whitepaper-vending 5ddd828; link it, don't duplicate (docs/whitepapers/vending-benchmark.md). |
| 141 | slavik-todo-hr | docs-whitepaper | Rewrite docs/SLAVIK-TODO.md on swarm/slavik-todo 87fdd91 in short Croatian: every open Slavik decision from /workspace/swarm/CALL-RECORD.md (merge main, cutover D1-D4, HADES A/B/C, live bench H1, banking 4, data source, sensors, candle, Galion OS model). Max 2 sentences each. |
| 142 | processes-refresh | docs-whitepaper | Refresh docs/swarm/PROCESSES.md and BIG-PICTURE.md on swarm-record 749ddbd with everything after 23:33 CEST. |
| 143 | galion-os-threat-model | galion-os | Doc: security/privacy/anonymity threat model for Galion OS (plan /workspace/swarm/plans/galion-os.md), device-independent. Stage into /workspace/swarm/galion-os-repo (local commit only; GitHub not connected yet). |
| 144 | galion-os-sandbox-proto | galion-os | Device-independent: Rust aarch64 sandbox launcher prototype (namespaces/proot-style, coexists, no root needed for tests) tested under qemu-aarch64 on the box. Stage in /workspace/swarm/galion-os-repo, local commits only. No phone access. |
| 145 | toolchain-aarch64 | integration-ci | Add repo-level .cargo/config.toml so aarch64 musl links with rust-lld by default (every agent hit this); verify all crates build. Base integration-2. |
| 146 | serde-order-pin | integration-ci | Decide-ready doc + patch: serde_json preserve_order differs -p vs --workspace; propose pinning; base integration-2. |
| 147 | voice-speed | gaia | voice-rust 03d21a9: CPU speed work on the mouth (target < 15 s for 3 s speech) without new asm deps; numbers. |
| 148 | one-no-shellout | security | Research+prototype replacing pdftotext/curl shell-outs in galion-one (one-hardening fed3de4) with pure-Rust no-asm options; report what's possible. |
| 149 | health-fixes | nerve | On health-land f4a66ea, separate branch: fix split-request reads and single-thread hang (issues 1+2) as a NEW version; keep proven binary branch untouched. |

Queue beyond slot 149 (up to 250): filled from the 7 sector plans in /workspace/swarm/plans/*.md as slots finish.
