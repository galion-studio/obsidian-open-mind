<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/RULES.md copied from the swarm box on 2026-10-21 06:39 CEST +02:00 by swarm/processes-refresh (slot 142). The box copy is the working copy; this is the durable record. -->

# TOP PRIORITY RULE (Slavik, 2026-10-20 20:56 CEST +02:00), applies to all work: Slavik values good company and productive conversations, the kind where you leave with more than you came in with. Every report, tool and conversation must leave him with more than he came in with: substance, a useful next step, no filler.

# PROJECT 42 / Gaia coding swarm: shared rules (approved by owner Slavik, 2026-10-20)

Read this fully before starting. Breaking a hard rule means stop and report.

## Hard rules
1. Rust only. NEVER run Python, not even `python3 --version` or `which python3` probes (two agents already slipped on this). FIRST command of every shell session: `export PATH=/workspace/swarm/bin:$PATH` (it blocks python and pip). NEVER run Python, and add no Python files. No openssl, asm, SIMD or target-cpu flags. Minimal dependencies. x86_64 and aarch64 musl must build; aarch64-android and armv7 must pass `cargo check`.
2. Never print, log, echo or copy any secret value. Never read files under /opt/galion/galion-light/secrets or /root/galion-secrets, except the push helper below doing it on the VPS. Never change permissions on secrets/persist. Never touch the galion-control key.
3. NEVER touch the VPS working copy /opt/galion/openmind (it stays at 2f5a730; live services run from it). Never commit, checkout, build or write there.
4. Never stop, restart, reconfigure or replace a live service or systemd unit on the VPS. Never build on the VPS (production). On the VPS you may only do READ-ONLY probes of live endpoints (HTTP GET, `systemctl status`, `ss -ltn`, `journalctl` reads). No POSTs that change state.
5. No real money moves and no paid API calls (no OpenRouter, RunPod or GPU spend) unless Slavik has approved that spend. No DNS changes, no new accounts, no publishing, no changes to repo visibility. Revolut: STANDING BAN LIFTED 2026-10-20 ~22:56 CEST (+02:00) by Slavik (typed, relayed by Mate) for the Hermes Rust V2 redesign of the V1 Hermes Python build — Revolut may be designed, mocked, read and wired into that redesign and the admin-phone link. Agents still must NOT place orders, send transfers, top up, withdraw, buy, sell or otherwise move money on Slavik's behalf. Live mutating Revolut calls need a separate written approval.
6. All work goes on your own branch `swarm/<your-slug>`, based on GitHub main 13a296ce. Never push to main, never force-push, and never push someone else's branch. Don't open or merge PRs (the owner merges).
7. Times are Europe/Zagreb with an explicit offset (CEST +02:00 until Oct 25 03:00, then CET +01:00). Never `Z`/UTC in docs.
8. New files get an SPDX MIT header; the copyright line is `Copyright (c) 3000 Galion / galion-studio`.
9. This swarm runs about 21 agents at once on one box: be CPU-polite. Use `nice -n 10 cargo ... -j 4`, your OWN `CARGO_TARGET_DIR=/workspace/swarm/target/<slug>`, and no busy loops.

## Open-source borrowing
- Research first (read-only): GitHub, crates.io, docs.
- Allowed licences only: MIT, Apache-2.0, MIT OR Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, 0BSD, CC0. NOT allowed: GPL, LGPL, AGPL, MPL-only, SSPL, BUSL, "non-commercial", or no licence.
- Document EVERY new dependency or copied code in `docs/third-party/<your-slug>.md` on your branch: name, version, licence (SPDX), source URL, what it's used for, and why it was chosen over writing it ourselves. Copied code keeps its original copyright notice plus a comment with its source URL.
- Check transitive dependency licences too (read the Cargo.lock entries' licences via `cargo metadata`; you can `cargo install cargo-deny` and run `cargo deny check licenses` if that helps).

## Real-life tests (required, not just unit tests)
- Start the real binary (release build) on the BOX on a spare port in 19000-19999 (use ONLY your assigned range: slot NN uses 19NN0-19NN9, e.g. slot 07 uses 19070-19079), and hit its real HTTP/CLI endpoints with real requests. Put these under `tests/real/` or `crates/<x>/tests/real_*.rs`, gated with `#[ignore]` or an env var `GALION_REAL=1`. The Rust test itself builds, starts, probes and tears down (Rust-only rule 20:41: no new scripts/real/*.sh).
- Where it helps, also do READ-ONLY probes of the matching live VPS service (for example GET http://127.0.0.1:8420/... over ssh) and compare the behaviour. Record the outputs (no secrets) in `docs/real-tests/<slug>.md` with Zagreb timestamps.
- Kill your started processes when you're done.

## Setup
```
git clone -q /workspace/swarm/base.git /workspace/swarm/wt/<slug>
cd /workspace/swarm/wt/<slug> && git checkout -q -b swarm/<slug> 13a296ce15b5bbd66adc26f6bfae787b2c1b5190
export CARGO_TARGET_DIR=/workspace/swarm/target/<slug>
```
Other branches (`ws/gaia`, `vps/ws-fort`, `vps/ws-voice`, `vps-main-20261020` and so on) are in base.git too.

## Pushing
`/workspace/swarm/push-branch.sh /workspace/swarm/wt/<slug> swarm/<slug>`
(It pushes through the VPS; the token stays there. Only swarm/* branches. Push only after fmt, clippy and tests pass. If the push is blocked by an automatic safety review, retry the exact same command once with the approval flag and the block reason. If it fails, don't improvise another route; report it.)

## Before each push
`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` (or at least your crates when the full run is too slow), `scripts/no-python.sh` if present. Don't break other crates.

## Report back (final answer)
Branch and head SHA (pushed?), what was done, the real-life test evidence (commands and outputs), the third-party deps with licences and URLs, open issues, and anything that needs the owner's approval. Be honest about what isn't done.

## Standing rule: sandbox runs only on the VPS (Slavik via Mate, 2026-10-21 04:29 CEST +02:00)
The sandbox (openmind-sandbox, cgroup-detect, real_escape / real_sh_sandbox, any sandboxed tool run) is RUN only on the VPS, never on this box. Build binaries here if needed (static x86_64 musl), but run every sandbox test on the VPS as an ordinary non-root user in a throwaway folder /var/tmp/sandbox-test-<ts> (never /opt/galion/openmind), then remove the folder, processes, cgroups and any temp user. Local `cargo test --workspace` must add `--exclude openmind-sandbox --exclude cgroup-detect`; never set GALION_REAL=1 for sandbox tests locally. Recorded in config/swarm/swarm-settings.toml [standing_rules] sandbox_vps_only (swarm/settings-snapshot e0013f3).

## Standing rule: no skipping, extensive real-server testing (Slavik via Mate, 2026-10-21 04:38 CEST +02:00)
Every agent must test its work extensively before finishing. No agent may skip its assigned task or mark it done without running the tests and reporting the real numbers. Verify results against real data on the real server (the VPS), not mocks or paper runs (mocks may supplement, never replace). Observe the new Rust infrastructure in real time through the nerve (VPS 127.0.0.1:8670 /state) and, once deployed, the cortex; until the cortex is live, say so and use the nerve. Stay inside your task's VPS permissions (read-only unless the task says otherwise), never print or copy secrets, sandbox runs only on the VPS (sandbox_vps_only). Recorded in config/swarm/swarm-settings.toml [standing_rules] no_skip_extensive_testing.
