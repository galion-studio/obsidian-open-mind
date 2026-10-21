<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: candle-zip-fix (candle-core with zip 9.0.5)

Box, 8 vCPU, all times CEST (+02:00), 2026-10-21. All test processes exited; `ss -ltn` afterwards shows nothing
on 19063 / 19371-19375. (These tests use the fixed ports of the voice-rust tests, outside slot 109's range
19790-19799; they were checked free before each run.)

## zip code paths in candle itself (vendored candle-core's upstream tests)

`cargo test --manifest-path vendor/candle-core/Cargo.toml --test pth_tests --test serialization_tests`
(read `.pt`/`.pth` through `ZipArchive::file_names`/`by_name`; write and read `.npz` through `ZipWriter` and
`ZipFile::name`): `test_pth`, `test_pth_with_key`, `test_pth_fortran_contiguous`, `npy`, `npz`, `safetensors`
- 6 passed, 0 failed.

## Voice real tests (release, `--features whisper,kitten,chatterbox`, `GALION_REAL=1`)

Run 1 01:35:04 -> 01:37:43:
- `real_chatterbox`: 2 passed (36.6 s). "Hello, I am Samantha. It is lovely to meet you." -> 2.92 s audio ->
  Whisper heard the same sentence; worker frame protocol ok, model loaded in 4389 ms.
- `real_voice_serve`: FAILED on the first `/health` probe (503 "loading"), the test predates the honest loading
  health of 1b9fd06. Fixed in the test (wait up to 120 s for 200).

Run 2 01:38:05 -> 01:40:46:
- `real_voice_serve`: 1 passed (2.7 s): health 503 -> 200, JFK sample transcribed (200), `/synthesize` 200.
- `real_voice_rust`: 2 passed (150.9 s). Both services 503 while loading, healthy after 4507 ms.

| # | voice | audio s | synth ms | X-RTF | ear ms | heard | words |
|---|---|---|---|---|---|---|---|
| 1 | samantha | 2.680 | 29181 | 10.888 | 10682 | Hi, I am right here with you and I am listening. | 100% |
| 2 | samantha | 2.880 | 27503 | 9.550 | 12457 | That sounds like a lot to Carrie. Do you want to talk about it? | 93% |
| 3 | jarvis | 2.680 | 23190 | 8.653 | 10455 | Good evening, all systems are running normally. | 100% |
| 4 | samantha | 2.200 | 19935 | 9.061 | 11865 | I love the way the light looks this time of day. | 100% |

Mean word match 98.2%. Contract: empty text 400, non-JSON 400, ear without file part 400. Peak RSS ear 4.77 GB,
mouth 4.16 GB. Same behaviour as voice-rust 03d21a9 (its record: mouth 25-34 s, carry->Carrie, 4.2/4.8 GB).
Galion One stage skipped (no `ONE_BIN`).

## Other checks

| Check | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo clippy -p galion-voice-cpu -p galion-chatterbox --all-targets --all-features -- -D warnings` | ok |
| `cargo test --workspace` | 534 passed, 0 failed |
| `cargo test -p galion-voice-cpu -p galion-chatterbox -p galion-voice-core --all-features` | 60 passed, 0 failed (real tests self-skip without `GALION_REAL=1`) |
| `cargo build --release -p galion-voice-cpu --features whisper,kitten,chatterbox --target aarch64-unknown-linux-musl` (rust-lld) | ok |
| `cargo check` same features: x86_64-unknown-linux-musl, aarch64-linux-android, armv7-unknown-linux-musleabihf | ok |
| `scripts/no-python.sh` / `scripts/spdx-scan.sh` | ok / 0 missing |
| rustsec audit of `Cargo.lock` (cortex sensor code) | zip gone; only `paste` 1.0.15 (info) left |
