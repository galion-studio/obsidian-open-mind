<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: sqlite-aarch64-cc (slot 156), 2026-10-21 CEST (+02:00)

Box: 8 vCPU x86_64, cargo 1.99.1, zig 0.15.2 (found by xtask at
`/workspace/tools/zig-x86_64-linux-0.15.2/zig`, nothing on PATH). Base
swarm/toolchain-aarch64 4d0c036. Ports 20260-20269. All builds via
`cargo-slot ... -j 2`.

"Clean shell" = `env -i HOME=... PATH=<cargo bin dir>:/usr/bin:/bin
CARGO_TARGET_DIR=...` (plus RUSTUP_* if set): no `CC_*`, `AR_*`,
`CARGO_TARGET_*_LINKER`, `RUSTFLAGS`, `ZIG`, `GALION_ZIG`, no wrapper script.
The Rust test `crates/galion-realtest/tests/real_xtask_cross.rs` builds that
environment itself.

| time (+02:00) | what | result |
|---|---|---|
| 06:59 | clean shell `cargo xtask cross aarch64-unknown-linux-musl build --release -p openmind-memory` (first try) | ok 30.0 s; build-script log shows `CC_aarch64_unknown_linux_musl = Some(.../debug/galion-cross/galion-cc)`; ELF aarch64, statically linked |
| 07:04 | `GALION_REAL=1 ... --test real_xtask_cross` (first run) | aarch64 ok; **armv7 FAILED**: cc-rs 1.8 passes the clang triple `--target=arm-unknown-linux-musleabihf`, which the wrapper (preferring `--target=`) did not know. Fix: `$TARGET` (cargo's rust triple) wins, clang spellings added to the table; regression asserts added |
| 07:05 | same | hermes-gateway did not listen: it refuses to start without `API_SERVER_KEY` (>= 16 chars). Test now passes a fixed loopback-only dummy value (not a real key) |
| 07:07 | same | **2 passed** (see output below) |
| 07:07-07:10 | `GALION_REAL=1 GALION_REAL_FULL=1 ... --test real_xtask_cross` | **2 passed, 163 s**: `--workspace` aarch64 80.3 s, armv7 82.3 s; **27/27 bins per target** (list from `cargo metadata`, xtask excluded), e_machine 183 / 40, no PT_INTERP, no PT_DYNAMIC; qemu run as below |
| 07:14 | clean shell `cargo xtask cross aarch64-linux-android check -j 2 --workspace --all-targets` | ok 30.3 s |
| 07:14-07:16 | clean shell `cargo xtask cross x86_64-unknown-linux-musl build --release -j 2 --workspace` | ok 1 m 36 s, 27 bins, static-pie |
| 07:16 | `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings` | ok; ok |
| 07:17-07:18 | `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 210 suites, **978 passed, 0 failed, 3 ignored** (slot 145 base: 964; +7 xtask unit, +5 xtask wrapper_modes, +2 real_xtask_cross non-real paths) |
| 07:18 | `scripts/no-python.sh`; `cargo run -p repo-check` | ok; PASS (0 errors) |
| 07:19 | `scripts/ucpu-gate.sh grep` | ok, after `ucpu-gate: allow` markers on the `-mcpu=generic+v7a` rewrite (generic ARMv7-A = the armv7 target's own baseline, not CPU pinning) and on slot 145's guard line in real_toolchain_aarch64.rs (was a pre-existing 1-hit FAIL on 4d0c036) |
| 07:19 | `cargo check -p xtask --target` aarch64-linux-android / armv7 / aarch64 musl | ok x3 |
| 07:19-07:20 | final `GALION_REAL=1` re-run on the committed code | **2 passed**; afterwards no listener on :2026x |

Output of the 07:20 run (trimmed):

```
negative control: exit=Some(101) 0.1s, libsqlite3-sys: no C cross compiler=true
cargo xtask cross aarch64-unknown-linux-musl: ok 10.8s
cargo xtask cross armv7-unknown-linux-musleabihf: ok 0.1s   (cached)
aarch64-unknown-linux-musl: 2 binaries, e_machine=183, static (no PT_INTERP/PT_DYNAMIC)
armv7-unknown-linux-musleabihf: 1 binaries, e_machine=40, static (no PT_INTERP/PT_DYNAMIC)
GET 127.0.0.1:20260/health -> 200 {"status":"ok","version":"0.1.0",...}     (openmind-memory, qemu-aarch64)
POST 127.0.0.1:20260/capture -> 200
POST 127.0.0.1:20260/search/conversations -> 200 hit=true                   (SQLite write+read on aarch64)
GET 127.0.0.1:20261/health -> 200 {"status":"ok","version":"0.1.0",...}     (openmind-memory, qemu-arm)
POST 127.0.0.1:20261/capture -> 200
POST 127.0.0.1:20261/search/conversations -> 200 hit=true                   (SQLite write+read on armv7)
GET 127.0.0.1:20262/health -> 200 {"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs",...}
hermes-gateway state db: 40960 B, SQLite header ok                          (hermes-gateway --mock, qemu-aarch64)
```

Sizes (release, from the full run): hermes-gateway aarch64 1,847,472 B (same
as slot 145's per-shell zig wrapper build), armv7 1,817,228 B, x86_64 musl
2,058,016 B; openmind-memory 1,621,320 / 1,585,364 / 1,767,200 B. All < 2 MiB.

## VPS (read-only, ssh slavik-vps, 07:19:42-07:19:45 +02:00)
* `uname -m` = x86_64; no qemu-aarch64 on the VPS, so the aarch64/armv7
  binaries cannot run there. Nothing was built, copied, installed or run there.
* Nerve `GET 127.0.0.1:8670/state`: HTTP 200, 13,091 B, 15 components
  (openmind-memory, hermes-rs, link-watch-rs, galion-desk, tuner, galion-one,
  galion-one-mouth, console, galion-health, cloudflared, tuner-quick,
  voice-worker, gaia, lxc-host, galion-nerve). The cortex is not deployed yet,
  so the nerve is the observation point.
* Live hermes-rs `GET 127.0.0.1:8642/health`:
  `{"status":"ok","platform":"hermes-agent","version":"0.21.4","implementation":"hermes-rs","implementation_version":"0.1.0"}`;
  `systemctl is-active hermes-rs` = active. Our aarch64 hermes-gateway under
  qemu answered the same status/platform/version/implementation fields.
* The probe wrote the nerve response to a VPS temp file `/tmp/.s156n` to count
  it and deleted it in the same command (no secrets in it). No service touched.

## Re-verification after restart (2026-10-21, +02:00)
The first agent was interrupted at 07:31 before pushing; a new executor
re-checked commit 2c444ff unchanged:

| time (+02:00) | what | result |
|---|---|---|
| 07:57 | `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools -- -D warnings` | ok; ok |
| 07:58-08:08 | `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` | 1 failure in `room` `default_and_fixture_do_not_open_a_socket` (counts the process's open sockets while the parallel `http::tests::server_over_a_real_socket` holds 2; crate untouched by this branch); `-p room --lib` alone: 30/30 ok |
| 08:05-08:15 | same with `--no-fail-fast` | 204 suites, **966 passed, 0 failed, 3 ignored** (hermes-tools now excluded locally per the 07:54 swarm rule, as it once ran the sandbox on the box; the 07:17 run with it was 978) |
| 08:15 | `GALION_REAL=1 GALION_REAL_FULL=1 ... --test real_xtask_cross` | **2 passed**; negative control exit 101 (no C cross compiler); 27/27 bins aarch64 (e_machine 183) and armv7 (40), static; qemu openmind-memory capture+search hit=true on 20260/20261, hermes-gateway --mock /health 200 + 40,960 B SQLite state DB on 20262; no listener left on :2026x |
| 08:15 | `scripts/no-python.sh`; `scripts/ucpu-gate.sh grep`; `cargo run -p repo-check` | ok; ok; PASS (0 errors, 29 warnings) |
| 08:16 | clean shell `cargo xtask cross aarch64-linux-android check -j 2 --workspace --all-targets` | ok 30.8 s |
| 08:16:28-08:16:30 | VPS read-only: `uname -m`; nerve `GET :8670/state`; hermes-rs `GET :8642/health`; `systemctl is-active hermes-rs galion-nerve` | x86_64, no qemu; nerve 200 13,091 B (cortex still not deployed); hermes-rs health same fields as our aarch64 build; active, active. Nothing written or changed on the VPS |
