<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: hades-rustflags-guard (slot 209), 2026-10-21 +02:00

Box: 8 vCPU x86_64, rustc 1.99.1. Ports 20790-20799. Logs in
/workspace/swarm/logs/hades-rustflags-guard/ (box only). No secrets used; flag
values are never printed, only env var names.

## Control on base 0d68f46 (no guard), 20:24 +02:00

| Build (x86_64 musl, release) | exit | dalek backend (build script) | size | `ymm` insns | `vpermd`/`vpmuludq ymm` |
|---|---|---|---|---|---|
| `RUSTFLAGS=""` | 0 (silent) | `simd` | 647,888 B | 1,704 | 2 |
| normal | 0 | `serial` | 619,216 B | 42 | 0 |

## `GALION_REAL=1 cargo test -p hades-gate --test real_rustflags_guard -- --test-threads 1`

Run 2026-10-21 20:27:51 → 20:29:50 +02:00, **3/3 tests, 19/19 cases**. Each case is a
real `cargo check -vv --offline --locked -p hades-gate --lib --target T` in a fresh target dir.

| Case | x86_64-musl | aarch64-musl |
|---|---|---|
| normal | PASS, serial | PASS, serial |
| `RUSTFLAGS=""` | FAIL guard (dalek would be simd) | FAIL guard (dalek serial) |
| `RUSTFLAGS="-C debuginfo=0"` | FAIL guard (simd) | FAIL guard (serial) |
| `RUSTFLAGS='--cfg ..="serial"'` | PASS, serial | PASS, serial |
| `RUSTFLAGS='-C debuginfo=0 --cfg ..="serial"'` | PASS, serial | PASS, serial |
| `CARGO_ENCODED_RUSTFLAGS=""` | FAIL guard (simd) | FAIL guard (serial) |
| `CARGO_TARGET_<T>_RUSTFLAGS="-C debuginfo=0"` | FAIL guard, names the var (simd) | FAIL guard, names the var (serial) |
| `--config target.<T>.rustflags=["-Cdebuginfo=0"]` | FAIL guard (simd) | FAIL guard (serial) |
| `--manifest-path` from a dir outside the repo | FAIL guard (simd) | FAIL guard (serial) |
| `RUSTFLAGS='--cfg ..="simd"'` | FAIL guard (simd) | n/a (dalek itself panics on non-x86_64) |

All failing cases exit 101 with
`hades-gate: SIMD guard (project rule 1, no SIMD): ...`.

* `compile_error_backstop_plain_rustc`: plain `rustc` on src/lib.rs without the
  cfg prints the guard error (exit 1); with the cfg the guard error is absent
  (exit 1 only from unresolved crates, expected without cargo).
* `normal_release_binaries_run`: normal release builds; x86_64 native and aarch64
  under qemu-aarch64: `keygen` ok (throwaway key, temp dir), `plan-hash` both
  `c408b5b88ce5cb3769f15e1fa15c94cd117e725624b14e5696ab7e6064bbae25`
  (x86_64 619,216 B = same as base, aarch64 532,576 B).
* Earlier run 1 (20:24:05 → 20:25:41): 17/17 cases (before the outside-cwd case was added).

## Existing hades-gate real tests on this branch (release)

* `GALION_REAL=1 HADES_REAL_CLI_PORT=20790 real_cli` 1/1 (20:30:00-20:30:22 +02:00):
  CLI + HTTP round trip on 127.0.0.1:20790 (port override added in this branch; default stays 19021).
* `GALION_REAL=1 HADES_REDTEAM_PORT=20791 real_redteam` 1/1 (20:30:27): 82 attack plans → 82 × 403 admin-safety.
  (A first run at 20:30:22 forgot the port override and used the test's default
  19380 (slot 38 band) for ~0.4 s; its bind succeeded, so no other process held it.)

## Built x86_64 musl release binary on this branch

619,216 B, 42 `ymm` (std), 0 `vpermd`/`vpmuludq ymm`: identical size to base → guard adds no code.

## VPS read-only (ssh, 2026-10-21 20:30:39 +02:00)

* CPU x86_64, 8 cores, **`avx2` present** → a RUSTFLAGS-built hades-gate would run dalek's AVX2 path there.
* nerve 127.0.0.1:8670 `/state` HTTP 200 (open incident: galion-one-mouth stuck loading, 503).
* cortex 127.0.0.1:8674 `/health` 200: mode dry-run, nerve true, incidents_open 0, `llm_enabled: true`.
* No hades unit (`systemctl list-units --all '*hades*'` = 0), no hades-gate binary in
  /usr/local/bin or /opt/galion/bin: hades-gate is not deployed, so nothing live changes.
* 21 listening sockets. Nothing written, started or stopped.

## Gates at HEAD

`cargo fmt --all --check` ok; clippy `--workspace --all-targets` (excl
openmind-sandbox, cgroup-detect) `-D warnings` ok; workspace tests (excl
openmind-sandbox, cgroup-detect, hermes-tools) 265 suites 1250 passed / 0 failed / 6 ignored
(20:32-20:34); hades-gate unit+integration 54 incl. new rustflags_guard 6/6;
`scripts/no-python.sh` ok; hades-gate release x86_64 + aarch64 musl ok;
aarch64-linux-android + armv7-unknown-linux-musleabihf `cargo check` ok; Cargo.lock unchanged.
