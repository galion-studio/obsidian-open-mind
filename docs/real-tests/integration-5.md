<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: integration-5 (wave-4 slot 177)

Box, 8 vCPU shared with other agents; every cargo run through `cargo-slot`, `-j 2`,
`CARGO_TARGET_DIR=/workspace/swarm/target/integration-5`. Times Europe/Zagreb, CEST (+02:00), 2026-10-21.
Ports used: 20470-20478 only. VPS: read-only GETs only.

## Box

| When | Command | Result |
|---|---|---|
| 09:23 | `cargo metadata --locked`, `cargo fmt --check` | ok, ok |
| 09:24 | `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | ok |
| 09:24 | `cargo deny --all-features --locked check advisories bans licenses sources` | advisories ok, bans ok, licenses ok, sources ok (warnings only: duplicate syn/hashbrown, wildcard path deps) |
| 09:25 | same on integration-4 `42de672` (throwaway worktree, removed) | advisories FAILED: RUSTSEC-2024-0436, RUSTSEC-2026-0367/0368/0369 |
| 09:24 | `scripts/no-python.sh` | no-python: ok |
| 09:30:01-09:30:55 | `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools --locked --no-fail-fast` | **1241 passed, 0 failed, 6 ignored**, 263 suites |
| 09:29-09:31:55 | `cargo build --release --workspace --target x86_64-unknown-linux-musl` | ok, 37/37 binaries static |
| 09:37-09:38:52 | same for aarch64-unknown-linux-musl (rust-lld; zig cc 0.15.2, `CRATE_CC_NO_DEFAULTS=1`, explicit `-O2` CFLAGS) | ok, 37/37 binaries AArch64 static |
| 09:39:01-09:40:36 | `cargo check --workspace --all-targets` armv7-unknown-linux-musleabihf, aarch64-linux-android | ok, ok |

Self-reported slip: a first test run at 09:24:40-09:25:15 did not exclude hermes-tools, so its `tests/sandboxed.rs`
(5 tests, 0.24 s, throwaway git repo through openmind-sandbox) ran on the box, against sandbox_vps_only. That run was
1253/0/6; the recorded numbers above are the re-run without hermes-tools.

## GALION_REAL=1 real_wiring (release openmind-nerve, hermes-gateway --mock, openmind-memory; `REAL_WIRING_PORT_BASE=20470`)

Run 1 09:27:45-09:28:56, **passed** (107 s). Run 2 09:29:08-09:30:19 (prebuilt binaries), **passed** (72 s).

- Start-up: both sources pushed 5 `started` events, 0 dropped. Ownership (listen scope 20470-20479): 20470 galion-nerve
  (self, AETHER), 20471 hermes-rs (ARTEMIS), 20472 openmind-memory (APOLLO), stray 20475 -> "unknown owner" +
  unmanaged-listener finding ":20475".
- 3 slow chats 1.5 s each = 200; 3 memory `POST /seed` -> 501; incidents with Gaia owners: hermes-rs 30 errors (mock 502s, by design) ARTEMIS,
  openmind-memory 3 errors APOLLO.
- hermes p99 (ms), run 1 / run 2: base 8.80 / 9.75, up 10.54 / 20.50, hung (SIGSTOP) 11.42 / 10.49, down 19.39 / 16.23, up2 11.92 / 13.80.
  Interleaved A/B (fresh gateways, `NERVE_URL` unset vs set with nerve down) p99 B vs A: **-27.2% / -50.2%** (B was faster both runs).
  The sequential "down vs base" (+120% / +67%) compares a fresh process with a long-lived one; slot 155 showed the same
  pattern, and the interleaved A/B is the fair measurement. memory p99 stays 0.5-0.9 ms in every phase.
- After hung + down: hermes-rs client queued 81, sent 49, dropped 21, failed 32; memory 12/12/0/0.
- hermes killed -> critical incident "health probe down ... connection refused" (ARTEMIS). Restart with
  `HERMES_RS_DAILY_CALL_CAP=5` -> 6x 402, `spend_cap` event, warn incident.
- Conn-cap phase: memory :20476 and hermes :20477 answer 503 x3 while one idle connection holds the cap, `conn_cap`
  event each, 200 after release.
- RSS kB: nerve 4388, hermes 4208, memory 4536. All processes killed; `ss -ltn` shows nothing on 20470-20479 afterwards.

## Live VPS, read-only (09:40:45 +02:00)

nerve `127.0.0.1:8670/state`: version 0.1.0, mode live, **cycle 20, started 09:31:08** (the live nerve restarted at 09:31,
not by this slot; earlier today it was at cycle ~7020). Healthy 11 / degrading 2 (galion-one-mouth 503 "model loading";
cloudflared "tunnel origin errors 84/91 requests in 9 min", 862 of 1583 tunnel requests errored) / absent 2 (voice-worker, gaia).
`signals.sources` = [] (no live service pushes to the nerve: this wiring is not deployed). `/healthz` 200;
`/health`, `/api/v1/signals`, `/api/v1/cortex` 404 (cortex not live; observed through the nerve instead).
hermes `:8642/health` 200 in 0.33 ms, memory `:8420/health` 200, galion-one :8091 healthy.
