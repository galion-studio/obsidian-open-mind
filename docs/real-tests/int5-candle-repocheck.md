<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: int5-candle-repocheck (slot 211)

Branch `swarm/int5-candle-repocheck`, base integration-5 `0d68f46` (main-merge blocker 0, slot 177). Box only,
ports 20810-20811 (range 20810-20819). All times CEST (+02:00), 2026-10-21. All started processes exited;
`ss -ltn` afterwards shows nothing on 20810-20819, no `galion-voice-serve` left running.

## What changed

1. `vendor/candle-core`: AVX2 / NEON / simd128 kernels removed (details in `vendor/candle-core/GALION-PATCH.md`
   item 6). The upstream portable kernels are kept unchanged. **No gate rule was weakened and nothing under
   `vendor/` is excluded**: repo-check and `ucpu-gate.sh grep` pass because the code is now portable.
2. `crates/galion-voice-cpu`: the release package override repeats `strip = true` (an override without it fell
   back to cargo's "debuginfo" strip, so `galion-voice-serve` kept `.symtab`). Guard test
   `tests/release_strip.rs`; real test `tests/real_int5_gates.rs`.

## Gates, before (base 0d68f46) and after (this branch)

| gate | base 0d68f46 (21:55) | this branch (21:53) |
|---|---|---|
| `repo-check` | **FAIL**, 1520 files, 10 errors (all `portable`, `core::arch::` in `vendor/candle-core/src/{cpu,quantized}/{avx,neon,simd128}.rs`), 48 warnings | **PASS**, 1515 files, 0 errors, 48 warnings (same warnings as base) |
| `scripts/ucpu-gate.sh grep` | **FAIL** (398 hits, e.g. `vendor/candle-core/src/utils.rs:40 cfg!(target_feature = "simd128")`) | grep ok |
| `scripts/ucpu-gate.sh` (all: grep, build x86_64+aarch64 musl, static, size, check armv7+android) | FAIL (grep; size: voice-serve unstripped, seen by slot omnode-enroll) | **RESULT ok**; `galion-voice-serve` x86_64 musl 572936 B stripped, static on both targets |
| `cargo fmt --all --check` | | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | | ok (vendored candle-core still prints its 4 pre-existing `deprecated f64::INFINITY` warnings in `cpu/erf.rs`, untouched upstream code; not a workspace member) |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools --locked --no-fail-fast` | 263 suites, 1241 passed, 0 failed, 6 ignored (slot 177 record) | 264 suites, **1243 passed, 0 failed, 6 ignored** (+1 suite / +2 tests = `release_strip`) |
| same incl. hermes-tools | | 270 suites, 1255 passed, 0 failed, 6 ignored |
| `scripts/no-python.sh`, `scripts/spdx-scan.sh` | | ok; spdx scanned 528, missing 0 |

## Vendored candle-core's own tests after the SIMD removal

`cargo test --release --manifest-path vendor/candle-core/Cargo.toml --test quantized_tests --test tensor_tests
--test matmul_tests --test conv_tests` (own target dir, removed afterwards; the generated
`vendor/candle-core/Cargo.lock` was deleted, not committed):

| target | run | result |
|---|---|---|
| x86_64 (native) | 21:56:10-21:57:38 | 99 passed, 0 failed (quantized 37, tensor 48, matmul 7, conv 7) |
| aarch64-unknown-linux-musl under `qemu-aarch64` (the target where NEON used to be compiled in) | 21:57:38-21:59:37 | 99 passed, 0 failed (quantized 37 in 22.56 s) |

## Real binary: voice service hears and speaks with SIMD-free candle

`GALION_REAL=1 cargo test --release -p galion-voice-cpu --features "whisper kitten" --test real_int5_gates -- --nocapture`
(the test starts `galion-voice-serve`, waits for `/health` 200, POSTs the JFK sample to
`/v1/audio/transcriptions` (Whisper tiny.en), POSTs a sentence to `/synthesize` (KittenTTS nano), checks the
ELF has no `.symtab`, then kills the process). Models from `/workspace/oom-models`.

| run | binary | result |
|---|---|---|
| x86_64 release, :20810, 21:54:29-21:54:38 | `target/.../release/galion-voice-serve` 3005384 B, sha256 3988d608…4b3c, 33 sections, `.symtab` false | health 503 while loading, 200 after 701 ms; ear 200 in 1449 ms: "And so my fellow Americans ask not what your country can do for you, ask what you can do for your country." (11.00 s audio); mouth 200, 140044 B WAV in 1096 ms; 2/2 passed |
| aarch64 musl release under `qemu-aarch64`, :20811, 21:55:04-21:57:17 (`GALION_VOICE_SERVE_BIN`, `GALION_VOICE_SERVE_RUNNER=qemu-aarch64`) | 2531352 B, sha256 fc5b4727…a5e1, 19 sections, `.symtab` false | health 200 after 1213 ms; ear 200 in 86967 ms (emulated, scalar), identical transcript; mouth 200, 140044 B in 44306 ms; 2/2 passed |

## VPS (read-only, 21:54-21:55)

No voice service runs on the VPS (slot 181), so there is no live endpoint to compare the voice binary with; this
change is box-only and nothing was deployed.

- Nerve `GET 127.0.0.1:8670/state` 200 (37840 B), cycle 13, mode dry-run, healthy 11, failing 0; it reports
  the known `galion-one-mouth` 503 "stuck loading" (chatterbox stub on :8002, slot 181) and a cloudflared
  request-flood incident, both unrelated to this change. `galion-nerve` active.
- Cortex is live: `gaia-cortex.service` active since 19:18 (+02:00), 127.0.0.1:8674 `GET /health` 200
  (`mode` dry-run, `incidents_open` 0, `escalations_open` 0, `events_in` 22); `GET /incidents` 200,
  0 open, 2 opened total (resolved).
- Nothing was changed on the VPS.

## Owner decisions (Slavik)

- D-211-1: accept removing the vendored SIMD kernels (no-SIMD rule) instead of a vendor exclusion. Cost: on
  aarch64 the quantized/vec kernels are scalar (same maths, slower: under qemu Whisper tiny took 87 s for 11 s
  of audio; native aarch64 will be much faster but still below NEON). x86_64 musl is unchanged (AVX2 was never
  in the baseline target).
- D-211-2: merge order: this branch on top of integration-5 clears blocker 0 (repo-check) and the two ucpu-gate
  failures; only Slavik merges into main.
