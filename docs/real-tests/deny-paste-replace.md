<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: deny-paste-replace (`paste` -> `pastey` shim, RUSTSEC-2024-0436)

Slot 153, base `swarm/candle-zip-fix` a4f49b5. Box, 8 vCPU, shared with about 20 other agents. All times
Europe/Zagreb, CEST (+02:00), 2026-10-21. Every cargo run went through `cargo-slot` with `-j 2` and
`CARGO_TARGET_DIR=/workspace/swarm/target/deny-paste-replace`.

## cargo-deny (cargo-deny 0.20.2, `--all-features`)

| When | Tree | Result |
|---|---|---|
| 07:08:43 | a4f49b5 unchanged | `check advisories`: **FAILED**, one error: `error[unmaintained]: paste - no longer maintained` (RUSTSEC-2024-0436, "No safe upgrade is available!"), paste 1.0.15 via gemm 0.19.0, gemm-common, gemm-f16/f32/f64/c32/c64 and pulp 0.22.3, all under candle-core 0.9.2 |
| 07:09:14 | with the shim | `check advisories`: **ok** |
| 07:26 | with the shim | `check` (all four) with the RULES.md licence allow-list (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, 0BSD, CC0-1.0; private crates checked too): **advisories ok, bans ok, licenses ok, sources ok** |
| 07:54:47 | with the shim (re-check on resume) | same: **advisories ok, bans ok, licenses ok, sources ok**; `check advisories` with the default config: ok |

The repo has no `deny.toml`; with cargo-deny's built-in default (empty licence allow-list) `licenses` rejects
every crate, before and after this change, so the allow-list above was passed with `--config`.
`bans` only warns (duplicate `hashbrown` 0.14/0.16/0.17 and similar, unchanged by this branch).

`cargo tree -i paste --all-features` now shows `paste v1.0.15+galion-pastey (vendor/paste-pastey)` under gemm,
and `cargo tree -i pastey` shows `pastey v0.2.3 (proc-macro)` only under that shim.

## Shim behaviour (unit test)

`cargo test --manifest-path vendor/paste-pastey/Cargo.toml`: 1 passed, 0 failed
(`concat_and_case_modifiers_match_paste_1_0_15`: identifier concat inside `macro_rules!`, `:upper`, `:lower`,
`:snake`, `:camel`, string-literal pieces, item generation at module level, the forms gemm-common/pulp use).

## Voice real tests (release, `--features whisper,kitten,chatterbox`, `GALION_REAL=1`)

Run 07:38:56 (build) -> 07:46:57, real models, gemm/pulp compiled against pastey. The voice tests use their
fixed ports 19063 and 19371-19375 (as in candle-zip-fix), outside slot 153's range 20230-20239; they were free and
nothing was left listening afterwards.

- `real_chatterbox` (07:42:06 -> 07:43:04): **2 passed** (57.8 s). "Hello, I am Samantha. It is lovely to meet
  you." -> 47 chars, 73 tokens, 2.92 s audio (t3 15825 ms, flow 11852 ms, hift 2192 ms) -> Whisper heard the same
  sentence. Worker frame protocol ok, model loaded in 2569 ms.
- `real_voice_serve` (07:43:04 -> 07:43:08): **1 passed** (3.8 s). `GET /health` 503 (loading) -> 200,
  `POST /v1/audio/transcriptions` 200 (352 078 B WAV in), `POST /synthesize` 200 (140 044 B WAV out).
- `real_voice_rust` (07:43:08 -> 07:46:57): **2 passed** (228.6 s), load average 12.6 at start (busy box).
  Ear 127.0.0.1:19371 and mouth 127.0.0.1:19372 both 503 "loading", healthy after 4010 ms.

| # | voice | audio s | synth ms | X-RTF | ear ms | heard | words |
|---|---|---|---|---|---|---|---|
| 1 | samantha | 3.200 | 49149 | 15.359 | 18506 | Hi, I am right here with you and I am listening. | 100% |
| 2 | samantha | 3.440 | 48245 | 14.025 | 17671 | That sounds like a lot to carry. Do you want to talk about it? | 100% |
| 3 | jarvis | 2.680 | 33741 | 12.590 | 13943 | Good evening. All systems are running normally. | 100% |
| 4 | samantha | 2.000 | 26599 | 13.299 | 15438 | I love the way the light looks this time of day. | 100% |

Mean word match 100.0% (candle-zip-fix: 98.2%, "carry" -> "Carrie" once). Contract: empty text 400
`{"error":"text required"}`, non-JSON 400, ear without a file part 400. Peak RSS ear 4 610 196 kB, mouth
4 161 600 kB (candle-zip-fix: 4.77 / 4.16 GB). Synthesis was about 1.5x slower than in candle-zip-fix (load 12.6 vs
an idle box); `pastey` is a compile-time macro crate and generates the same code, so this is box load, not the
change. Galion One stage skipped (no `ONE_BIN`).

The source tree was not edited after 07:30, so these runs cover exactly the committed tree.

## Live VPS (read-only, 07:55:38)

One `ssh slavik-vps sh -s` batch of GETs: nerve `GET 127.0.0.1:8670/state` 200, 13 091 B, mode `live`;
`GET 127.0.0.1:8420/health` 200, 278 B. No systemd unit matching `*voice*` runs on the VPS, so the candle voice
crates this change touches are not deployed there; nothing live is affected. The cortex is not live yet, so the
nerve was used. Nothing was written, built or restarted on the VPS.

## Other checks

| Check | Result |
|---|---|
| `cargo fmt --check` (07:54) | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` (07:55) | ok |
| `cargo clippy -p galion-voice-cpu -p galion-chatterbox --all-targets --all-features -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` (07:55-07:56) | 500 passed, 0 failed, 0 ignored (119 test binaries) |
| `cargo test -p galion-voice-cpu -p galion-chatterbox -p galion-voice-core --all-features` | 60 passed, 0 failed (real tests self-skip without `GALION_REAL=1`) |
| `scripts/no-python.sh` / `scripts/spdx-scan.sh` | ok / 219 scanned, 0 missing |
| `cargo build --release -p galion-voice-cpu --features whisper,kitten,chatterbox --target aarch64-unknown-linux-musl` (rust-lld, 07:56:49-07:58:26) | ok: `galion-voice-serve` ELF aarch64, statically linked |
| same, `--target x86_64-unknown-linux-musl` (07:58:26-07:59:57) | ok: `galion-voice-serve` ELF x86-64, static-pie |
| `cargo check -p galion-voice-cpu -p galion-chatterbox --features galion-voice-cpu/whisper,kitten,chatterbox`, aarch64-linux-android and armv7-unknown-linux-musleabihf | ok / ok |
| `cargo check --workspace` (excl. sandbox crates) for aarch64-linux-android and armv7-unknown-linux-musleabihf | fails in `libsqlite3-sys` 0.30.1 build script (no C cross-compiler for those targets on the box); not caused by this branch (its Cargo.lock diff is only paste -> shim + pastey), unrelated to `paste` (slot 156 sqlite-aarch64-cc covers it). Every crate that uses `paste` checks fine on both targets (row above) |

Sandbox crates (openmind-sandbox, cgroup-detect) and hermes-tools were not run locally (sandbox_vps_only); this
branch does not touch them.
