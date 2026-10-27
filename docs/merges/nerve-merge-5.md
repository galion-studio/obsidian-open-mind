<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-merge-5 (wave-4 slot 316, D-292-1 trial), 2026-10-27 CET (+01:00)

`swarm/nerve-merge-5` = `swarm/nerve-merge-4` 5dac9b4a + `--no-ff` merge of
`swarm/nerve-own-services` 9479050c. Box only. **Not deployed**, VPS read-only.

## Merge

- Merge commit 0518a8a3: **0 conflicts**. Tree is byte-identical to slot 292's
  unpushed local trial 24849d30 in `wt/nerve-merge-4` (`git diff` empty).
- Brings `owners.rs` OWN_SERVICES (8672 galion-nerve-shadow AETHER, 8674
  gaia-cortex AETHER, 19120 openmind-queue POSEIDON; `NERVE_UNOWNED_PORTS`
  still wins), inventory hint 8674, `tests/real_own_services.rs` + fixture.
- Cherry-picked slot 292's local cfca4cf: compile-time `REAL_NERVE_PORT_BASE`
  remap for `real_own_services` (default ports 21080-21089 unchanged).
- New (slot 316): `real_replay_matches_live_9d1a4f3_20261027` plus two fixtures
  captured read-only on the VPS: `ss-ltnp-vps-20261027-2226.txt` (`ss -ltnp`,
  22:26:10 +01:00) and `live-exposure-9d1a4f3-20261027-2215.json` (the live nerve's
  `/state .cortex.exposure`, ts 22:15:01 +01:00).

## Relation to the live nerve

Live `galion-nerve.service` = `/opt/galion/rust/bin/openmind-nerve-9d1a4f3 serve --mode event`
(`swarm/nerve-deploy-t273u` 9d1a4f30, which already contains 9479050).
`git diff 9d1a4f3 nerve-merge-5 -- crates/openmind-nerve/src` is now only the
`journal-native` feature from 9a76ff6f (off by default; new `evt/journal_native.rs`,
cfg-gated hook in `evt/journal.rs`) plus the `Msg` clippy allow. One default-build
behaviour change: `NERVE_EVT_JOURNALCTL=native` without the feature now reports a
journal source error instead of trying to exec a binary called `native`; the live
unit does not set it. Without this fold, deploying merge-4 would have **dropped**
the own-service ownership the live nerve has (proved below).

## Gates (box, 2026-10-27 22:25-22:45 CET +01:00)

- `cargo fmt --all --check`: ok
- `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings`: ok
- `cargo clippy -p openmind-nerve --all-targets --features journal-native -- -D warnings`: ok
- `cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  **276 suites, 1333 passed, 0 failed, 6 ignored** (hermes-tools excluded because its
  sandboxed tests must not run on the box, sandbox_vps_only; merge-4's 1341 included it)
- `cargo test -p openmind-nerve --features journal-native`: 19 suites, 171 passed, 0 failed
- `scripts/no-python.sh`: ok; no new dependencies
- openmind-nerve release musl: x86_64 1,463,072 B (sha256 99ef53ccc7610354...),
  aarch64 1,211,624 B (sha256 c5b281239571499e...); `cargo check` aarch64-linux-android
  and armv7-unknown-linux-musleabihf ok.
- Sandbox crates not run (sandbox_vps_only).

## Real tests (GALION_REAL=1, release, box ports 21860-21869, 22:33:27-22:41:10 +01:00)

Env `REAL_NERVE_PORT_BASE=21860 REAL_WIRE_GAPS{,2,3}_PORT_BASE=21860 REAL_WIRING_PORT_BASE=21860`, `--test-threads=1`.

| Test | Result | Time |
|---|---|---|
| real_own_services | 3/0 | 1 s |
| real_flood_baseline | 2/0 | 74 s |
| real_flood_cycle_gap | 2/0 | 74 s |
| real_publish_recoveries | 2/0 (VPS journal replay: first fault 2026-10-21 23:06:25 +02:00) | 28 s |
| real_wire_gaps | 3/0 (live-first 6, gaps-2 6, gaps-3 5 services) | 128 s |
| real_wiring | 1/0 | 108 s |
| journal_native_rss (`--features journal-native`) | 1/0: box journalctl 7528 KiB, nerve replay 4508 KiB, nerve native 4520 KiB; VPS live nerve 8088 KiB vs its journalctl child 50264 KiB | 4 s |

Output excerpts:

```
238 replay: unmanaged [18800]; finding "1 loopback listener(s) no monitored unit owns: :18800"
238 replay override: unmanaged [8674, 18800]
316 live-parity replay: unmanaged [18080]; 17 listeners judged == live 9d1a4f3
238 serve mode_event=false after 0.5s: unmanaged [21861]; stray holder "real_own_servic"
238 serve mode_event=true  after 0.5s: unmanaged [21861]; stray holder "real_own_servic"
```

The live-parity test asserts that replaying today's real VPS listener table gives
exactly the same (port, owner, gaia) ownership list (17 entries) and the same
`unmanaged_ports [18080]` as the running live nerve.

**Negative control** (temp worktree of 5dac9b4a + the same test file/fixtures,
release): `real_replay_live_vps_listeners` FAILS (left [8672, 8674, 18800, 19120]),
`real_replay_matches_live_9d1a4f3_20261027` FAILS (left **[8674, 18080, 19120]** vs
live [18080]), rc 101. So merge-4 alone would raise two false stray-listener findings
(gaia-cortex, openmind-queue) on today's live box. Temp worktree + target removed.

Not re-run (hard-coded ports outside 21860-21869, unchanged by this merge):
real_box, real_consumer_compat, real_cortex_sensor, real_dst_clock.

## Live VPS (read-only)

- 22:26 +01:00: `ss -ltnp` (19 listeners), GET :8670 /state, `/proc/<pid>/cgroup` of the
  :18080 holder. The temporary /tmp copy of /state on the VPS was removed.
- 22:43:25 +01:00: nerve /health 200 (status ok, system degraded, score 64, cycle 25),
  /stats 200; galion-nerve, gaia-cortex, openmind-queue, console-rust-primary active.

## Findings / owner decisions

- **F-316-1 / D-316-1**: :18080 is `console-rust-primary.service`
  (`galion-console-serve-2d846fb8 --bind 127.0.0.1:18080 ...`, up since 18:28 +01:00).
  The live nerve (and this branch) report it as an unmanaged stray listener
  (warn "1 loopback listener(s) no monitored unit owns: :18080") because no nerve
  component or own-service owns 18080. Options: (A) add a `console-rust-primary`
  component (port 18080, HEPHAESTUS) to the nerve config on the VPS; (B) add 18080
  to OWN_SERVICES in code; (C) keep it flagged until the console cutover settles.
  Recommended A (config, not code). Not changed here.
- **D-292-1**: answered as a trial: the fold is clean and green; nerve-merge-5 is the
  deploy candidate instead of merge-4. Deploy remains a separate owner/deploy step.
- D-292-2 (integration-5 lineage) and D-292-3 (allow vs Box) unchanged from merge-4.
