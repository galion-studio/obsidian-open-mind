<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/WAVE2.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Wave 2 (approved by owner Slavik, 2026-10-20 ~20:11 +02:00)

Read /workspace/swarm/RULES.md fully first; all of it applies (no Python, PATH blocker on EVERY shell command: `export PATH=/workspace/swarm/bin:$PATH;`, no secrets, never touch /opt/galion/openmind or live services, VPS read-only GET probes only and NO files written on the VPS, not even /tmp, no paid calls, own branch swarm/<slug>, no PRs, no force-push, Zagreb times, SPDX MIT headers, licence allowlist + docs/third-party/<slug>.md, real-life tests on your own port range).

## Wave 2 additions
- Base: main 13a296ce, unless your task says to build on top of one or more wave-1 branches (merge them into your branch with normal merges, never rebase someone else's commits). Wave-1 branches are on GitHub; fetch them into your worktree from any existing worktree, e.g. `git fetch /workspace/swarm/wt/<slug> swarm/<slug>:swarm/<slug>`.
- Create your worktree as RULES.md describes, at /workspace/swarm/wt/<slug>.
- Disk is limited (shared box). Build with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`, `nice -n 10 ... -j 3`, own CARGO_TARGET_DIR=/workspace/swarm/target/<slug>. If `df /` shows < 8 GB free, pause 2 minutes and re-check instead of building. Run `cargo clean` on your target dir when done.
- aarch64 musl: `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`; zig wrappers for bundled SQLite exist on the box (see swarm/ucpu docs/ucpu.md).
- Push: /workspace/swarm/push-branch.sh <worktree> swarm/<slug>; on a remote error, retry once after 30 s.
- Never delete files you didn't create.
- Report back: branch + head SHA, what was built, test counts, real-life test summary, deps with licences, open issues, items needing Slavik's approval.

## Wave-1 branches (all based on 13a296c, pushed)
one-probe b44cd4f (galion-one fail-closed pairing, ONE-PROBE, /api/v1/services) · ucpu 3e70ffb (universal-CPU gate, SQLite trim, armv7 seccomp fix) · room 5bb27e1 · hades-approval ccaab42 (Ed25519 single-use tokens) · docs-sync 80e28f3 · tuner-cutover 5fd1611 · desk-linkwatch a54c792 · survival-heal 45a18aa · gaia-live 4fbbb7f (ws/gaia merged + gaia heal-tick + nerve serve) · sandbox d194c19 · memory-real 723b46c · real-harness 5b3c662 (galion-realtest, ports 19500-19551) · voice-mouth 5d776c1 · lan-discovery fe5f0bd · poseidon 7d0c478 · nerve 5802d0e (take its openmind-nerve main.rs over gaia-live's) · edge-boot 753676e (needs ucpu first for armv7) · launch-gates 79ee761 · hermes-tools 8ccdfcc (includes sandbox d194c19) · common 26d98b0 (touches tuner: merge after tuner-cutover).
Still running: gaia-land (slot 01, local merge c15a4b7, not pushed) — do not touch wt/gaia-land.
