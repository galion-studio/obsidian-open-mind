<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: ci-toolchain-pin (slot 196, 2026-10-21 CEST +02:00)

Box: 8 vCPU / 15 GB, rustup 1.29.2, toolchains stable (1.99.1 58cf8f9c3), nightly, and
`1.99.1-x86_64-unknown-linux-gnu` (installed 15:49 for this pin, same commit as stable).
All builds through `cargo-slot` / a build-queue lock, `-j 2`, own target dir. Ports 20660-20662.

## 1. Guard and real pin test (`crates/galion-realtest/tests/toolchain_pin.rs`)

`GALION_REAL=1 cargo-slot test -j 2 -p galion-realtest --test toolchain_pin -- --nocapture`,
15:53:50-15:53:57, 8 passed / 0 failed (re-run 16:07 after the CARGO-path check: 8/0):

```
channel 1.99.1; 12 cargo jobs all pinned
CARGO is the pinned 1.99.1 cargo
repo: rustup show active-toolchain -> 1.99.1-x86_64-unknown-linux-gnu (overridden by '.../rust-toolchain.toml')
repo: rustc -vV release line: Some("release: 1.99.1")
repo: cargo -V -> cargo 1.99.1 (5f94df478 2026-08-27)
neg: channel 1.0.0, RUSTUP_AUTO_INSTALL=0 -> ok=false "error: toolchain '1.0.0-x86_64-unknown-linux-gnu' is not installed"
build tuner x3 targets: 6.1s
x86_64-unknown-linux-musl: ["rustc version 1.99.1 (58cf8f9c3 2026-10-13)"]
x86_64-unknown-linux-musl: GET 127.0.0.1:20660/health -> 200 {"ok":true}
x86_64-unknown-linux-musl: GET 127.0.0.1:20660/ -> 200 <!doctype html><title>slot196 pin</title>
aarch64-unknown-linux-musl: ["rustc version 1.99.1 (58cf8f9c3 2026-10-13)"]   (qemu-aarch64-static)
aarch64-unknown-linux-musl: GET 127.0.0.1:20661/health -> 200 {"ok":true}
armv7-unknown-linux-musleabihf: ["rustc version 1.99.1 (58cf8f9c3 2026-10-13)"] (qemu-arm-static)
armv7-unknown-linux-musleabihf: GET 127.0.0.1:20662/health -> 200 {"ok":true}
```

Negative controls (always run): floating channel, workflow `toolchain: stable`, job without a
toolchain step, crate without `rust-version`, mismatched `rust-version`, `cargo +nightly` in a
script, `RUSTUP_TOOLCHAIN` in xtask: each makes the checker fail. Ports closed after the test.

Limit, stated honestly: box `stable` is the same rustc commit as 1.99.1, so the rustc stamp
cannot tell "built by the pin" from "built by stable" today. What proves the file is in charge
is rustup's own answer (`overridden by rust-toolchain.toml`) and the negative control (a
pinned channel that is not installed is a hard error, never a fallback).

## 2. Full gates on the pinned compiler (15:54-16:21)

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` (+ omnode) | ok |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | ok (110 crates checked) |
| `cargo test --workspace --locked --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` | 262 suites, 1223 passed / 0 failed / 6 ignored |
| omnode clippy -D / test | ok / 73 passed, 0 failed |
| `scripts/ucpu-gate.sh grep build static size check` (zig 0.15.2) | first run: build/static/size/check ok, grep 1 hit = the cherry-picked slot-145 test's own `"target-cpu"` literal; fixed (56ddb31, `concat!`); re-run 16:21 RESULT ok |
| x86_64 + aarch64 musl release, workspace + omnode | 38 + 38 binaries, all static, size-budget ok (37+1 per target), every binary stamped `rustc version 1.99.1 (58cf8f9c3 2026-10-13)` |
| armv7-unknown-linux-musleabihf, aarch64-linux-android `cargo check` (workspace + omnode) | ok |
| `scripts/no-python.sh` / `scripts/spdx-scan.sh` | ok / 507 scanned, 0 missing |
| repo-check (musl release, `--copyright "3000 Galion / galion-studio"`) | 1390 files, 0 errors, 47 warnings (utc, pre-existing) -> PASS |

hermes-tools is excluded locally as other wave-4 slots do (it can start sandboxed tool runs;
sandbox_vps_only). `real_toolchain_aarch64` was run without GALION_REAL (2/2): its real part
uses slot 145's ports 20150-20153, outside this slot's range; my real test covers the same
ground (aarch64 build + qemu run) on 20660-20662.

## 3. xtask compatibility (ci-workflow-rust 38a26cb, scratch copy, not committed)

Same pin applied to a scratch checkout of 38a26cb: before the workflow sed the guard flagged
17 `toolchain: stable` lines and 17 unpinned cargo jobs; after it, all checks passed except
the expected cross-linker assertion (38a26cb predates 4d0c036). `cargo test -p xtask` 11/11
(incl. "every `run:` is `cargo xtask`"). xtask spawns `$CARGO` (run.rs:60) = the pinned cargo.

## 4. VPS (read-only) and nerve

15:54 +02:00 over ssh, GET only, no writes: nerve `127.0.0.1:8670/state` 200 (49 135 B,
cycle 742, mode live, system degraded). Every Rust binary in `/opt/galion/rust/bin`
(galion-backup, -deploy, -health, -nerve, -nerve-shadow, -one, hermes-gateway(+aa77864),
link-watch, openmind-memory(+aa77864), openmind-nerve-aa77864) carries
`rustc version 1.99.1 (58cf8f9c3 2026-10-13)`: the pin is exactly the compiler production runs.
No Rust toolchain on the VPS PATH (nothing is built there, as required).
16:31 +02:00: nerve cycle 816, healthy 11 / degrading 1 / failing-soon 1 / absent 2, score 61 D
(pre-existing, unrelated to this change). Cortex is not deployed (`:8680` connection refused),
so the nerve is the observation point.
