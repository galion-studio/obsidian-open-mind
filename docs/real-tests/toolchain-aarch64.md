<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: toolchain-aarch64 (slot 145), 2026-10-21 CEST (+02:00)

Box: 8 vCPU x86_64, rustc stable, base swarm/integration-2 df42e0f. No
`CARGO_TARGET_*_LINKER` or `RUSTFLAGS` in the environment for any run below.
Ports 20150-20159. All builds via `cargo-slot ... -j 2`.

| time (+02:00) | what | result |
|---|---|---|
| 06:36 | baseline (config unchanged): `cargo build --release -p galion-boot --target aarch64-unknown-linux-musl` | FAIL: `/usr/bin/ld: unrecognized option '--fix-cortex-a53-843419'` |
| 06:36 | same with the new `[target.aarch64-unknown-linux-musl] linker = "rust-lld"` | ok 15.1 s; AArch64, no PT_INTERP, no dynamic section, 744,840 B |
| 06:36-06:37 | `--workspace --keep-going`, aarch64, no C compiler set | all Rust-only crates link; only `libsqlite3-sys` fails (`failed to find tool "aarch64-linux-musl-gcc"`), as expected |
| 06:38 | `--workspace`, aarch64, `CC_aarch64_unknown_linux_musl` = zig cc 0.15.2 wrapper | ok 32 s; **27/27 binaries AArch64, PT_INTERP 0, NEEDED 0**; largest hermes-gateway 1,847,472 B (< 2 MiB) |
| 06:38-06:40 | `--workspace`, armv7-unknown-linux-musleabihf, zig cc | ok 1 m 33 s; **27/27 binaries ARM, static** |
| 06:47 | `GALION_REAL=1 cargo test -p galion-realtest --test real_toolchain_aarch64` (with zig CC) | **2 passed**: build tuner+galion-one+openmind-memory 21.4 s; e_machine 183, static; under qemu-aarch64-static: tuner `GET :20150/health` 200 `{"ok":true}`, `GET :20150/` 200 index; galion-one `GET :20151/health` 200 `ok`; openmind-memory `GET :20153/health` 200 `{"status":"ok",...}`, `POST /capture` 200, `POST /search/conversations` 200 with the written row (SQLite C code works on aarch64) |
| 06:47 | negative control: same test with `.cargo/config.toml` reverted | **2 failed** (config assert; build fails with `--fix-cortex-a53-843419`), so the test catches a regression |
| 06:48 | armv7 tuner under qemu-arm-static, `GET :20155/health` and `/` | 200 `{"ok":true}`, index served; process killed, no :2015x listeners left |
| 06:47-06:48 | host `cargo clippy --workspace --all-targets -D warnings` | ok |
| 06:48-06:49 | host `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 205 suites, **964 passed, 0 failed, 3 ignored** |
| 06:49-06:50 | `cargo check --workspace --all-targets --target aarch64-linux-android` (zig CC, musl headers) | ok |
| 06:50-06:51 | `cargo build --release --workspace --target x86_64-unknown-linux-musl` (x86_64 left on host default) | ok, 27 binaries |
| 06:48 | `scripts/ucpu-gate.sh grep`, `scripts/no-python.sh` | ok, ok |

## VPS (read-only, ssh slavik-vps, 06:51-06:52 +02:00)
* `uname -m` = x86_64, kernel 6.8.12-43-pve; no qemu-aarch64 on the VPS, so
  aarch64 binaries cannot run there; nothing was copied, built or run there.
* The published phone artefact `/opt/galion/openmind/release/omnode-0.1.0/omnode-0.1.0-linux-aarch64.tar.gz`
  (read with `tar -xO | head -c 20`): ELF64 LE, e_machine `b7 00` = 183
  (AArch64), the same machine value the new test asserts for our builds.
* Nerve `GET 127.0.0.1:8670/state`: HTTP 200, 13,090 B; 15 components
  (openmind-memory, hermes-rs, link-watch-rs, galion-desk, tuner, galion-one,
  galion-one-mouth, console, galion-health, cloudflared, tuner-quick,
  voice-worker, gaia, lxc-host, galion-nerve). Cortex not deployed yet, so the
  nerve is the observation point. This change touches build config only; no
  VPS service is affected.
