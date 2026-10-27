<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests — safetensors-mit-alt (slot 295, ports 21650-21659)

Box, 2026-10-27 CET (+01:00). Branch `swarm/safetensors-mit-alt` (base 400e93a5). No secrets in any output.

## 1. Parity vs upstream safetensors 0.7.0 on all 7 real model files (19:24 +01:00)
Scratch harness (not committed) links both crates, mmaps each file, parses with both, compares every tensor
(name, dtype, shape, exact data pointer + length), the `__metadata__` map, two slices per 2-D+ tensor
(`slice(1..n/2+1)` and `slice((.., 1..m/2+1))`, the two forms candle-nn ShardedSafeTensors uses), and for
files < 200 MB re-serializes every tensor with both writers and compares the bytes.

| File | Size B | Tensors | Meta keys | Slice checks | Diffs | Re-serialize |
|---|---|---|---|---|---|---|
| chatterbox/s3gen | 1 056 484 620 | 2489 | 0 | 1690 | 0 | skipped (>200 MB) |
| chatterbox/t3_cfg | 2 129 653 744 | 292 | 0 | 444 | 0 | skipped |
| chatterbox/ve | 5 695 784 | 16 | 0 | 14 | 0 | byte-identical 5 695 784 B |
| kitten-tts-nano/kitten-nano | 56 068 376 | 458 | 3 | 322 | 0 | byte-identical 56 068 216 B |
| kitten-tts-nano/kitten-voices | 3 277 776 | 8 | 2 | 16 | 0 | byte-identical 3 277 400 B |
| whisper-large-v3-turbo/model | 1 617 824 864 | 587 | 1 | 474 | 0 | skipped |
| whisper-tiny.en/model | 151 060 136 | 167 | 1 | 138 | 0 | byte-identical 151 060 104 B |

Total 4017 tensors, 3098 slice checks, 0 differences. (Re-serialized files are a little smaller than the originals
because the originals carry `__metadata__`, which this run did not pass back in.) Header parse times
(MIT 22 µs-1.5 ms vs upstream 3-6 ms) are single cold runs, indicative only.

## 2. Differential header fuzz (19:25 +01:00)
200 000 copies of kitten-voices.safetensors with 1-3 random bytes changed in the length prefix + JSON header:
`both_ok=9341 both_err=190659 hf_only_ok=0 mit_only_ok=0` — both parsers accept and reject the same inputs.
Known intended difference (crafted input): duplicate tensor key -> upstream `Ok(1 tensor)` (last wins),
MIT `Err(duplicate tensor name "a")`.

## 3. Real binary end-to-end: galion-voice-serve on 127.0.0.1:21650
`GALION_REAL=1 cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox --test real_safetensors_mit -- --nocapture`
(new test `crates/galion-voice-cpu/tests/real_safetensors_mit.rs`; it starts the release binary, probes, kills it).
Baseline = same test file in a throwaway clone at 400e93a5 (upstream Apache safetensors).

| Run | Time | /health | STT (JFK 11 s WAV) | TTS "Hello from the Galion mouth." |
|---|---|---|---|---|
| upstream baseline | 19:31:58 | 200 after 0.70 s, rss 234 456 kB | 200, "...ask not what your country can do for you...", 1.64 s | 200, 140 044 B, 1.46 s |
| MIT reader | 19:32:27 | 200 after 0.45 s, rss 234 496 kB | 200, identical text, 1.79 s | 200, 140 044 B, 1.39 s, **byte-identical** to baseline |

WAV sha256 both runs: `c1a6ef38c18e6e10aff58783355cfdc2b43dff821b9a4fae4733bd2dfec53bb9`. Port 21650 free afterwards.
Release binary size: MIT 4 265 024 B vs upstream 4 269 528 B (-4.5 kB).

## 4. Chatterbox (3 GB of safetensors via MmapedSafetensors) with the MIT reader (19:32-19:33 +01:00)
`GALION_REAL=1 RAYON_NUM_THREADS=2 cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox --test real_chatterbox`
-> 2 passed (58.3 s): line round-trip heard back by Whisper "Hello, I am Samantha. It is lovely to meet you.";
worker frame protocol ok, model loaded in 4496 ms.

## 5. VPS (read-only, 19:35-19:37 +01:00)
- Nerve `GET 127.0.0.1:8670/state`: up; 12 healthy, 5 degrading, 2 failing, 1 degraded, 1 loading, 1 absent
  components (cortex events present; nothing related to this branch). Cortex is live and reporting.
- No voice/mouth service is active on the VPS (`systemctl is-active` inactive, nothing on :8003), so there is no
  live safetensors consumer to compare against; nothing was deployed.
- The VPS holds archived copies of the same model files (/srv/box-archive/2026-10-21/.../oom-models, 13 files).
  sha256 (first 16 hex) box vs VPS identical for ve, kitten-nano, kitten-voices, whisper-tiny.en:
  f0921cab452fa278, b45307e7cbb82025, 034554611b2ebc8b, db59695928ded604. So section 1 parity holds for the
  VPS copies byte for byte.

## 6. Gates
| Check | Result |
|---|---|
| `cargo-deny --offline check licenses bans sources` (no safetensors exception) | bans ok, licenses ok, sources ok |
| `cargo test -p safetensors -p galion-voice-cpu -p galion-chatterbox` (debug, default features) | 10 + 21 + 14 + 2 passed, 0 failed |
| `cargo clippy -p safetensors -p galion-voice-cpu -p galion-chatterbox --all-targets --all-features -- -D warnings` | ok |
| `cargo fmt --all --check`, vendor crate `cargo fmt --check` | ok |
| safetensors crate: build x86_64-musl + aarch64-musl (rust-lld), check aarch64-android + armv7-musleabihf | ok |
| `cargo build --release -p galion-voice-cpu --features whisper,kitten,chatterbox --target aarch64-unknown-linux-musl` (rust-lld) | ok (1 m 33 s) |

## 7. Workspace regression (box, 2026-10-27 +01:00)

| Run | Window | Result |
|---|---|---|
| `cargo-slot test -j 2 --offline --workspace --exclude openmind-sandbox --exclude cgroup-detect` (fail-fast) | 19:37-19:40 | 1570 passed, 1 failed: `openmind-nerve evt::runtime::tests::idle_is_quiet_and_burst_escalates_once` (timing flake under load; passed 3/3 re-run alone; unrelated to safetensors) |
| same, `--no-fail-fast` | 19:40:57-19:42:19 | 385 suites, 2026 passed, 0 failed, 18 ignored |
| `cargo run -p repo-check -- --root .` | 19:42:26 | PASS, 0 errors, 83 pre-existing warnings |

## 8. Re-verify after box reset (restart r2, 2026-10-27 20:39-20:47 CET +01:00)
Fresh box image (rustc 1.99.1), worktree head f4bfc21, models in /workspace/oom-models (4.9 GB).
| Check | Window | Result |
|---|---|---|
| `cargo fmt --all --check` | 20:39 | ok |
| `cargo deny --offline check licenses bans sources` (after `cargo fetch`; no safetensors exception) | 20:40 | bans ok, licenses ok, sources ok; lock `safetensors 0.7.0` is the path crate (no registry source) |
| `cargo test -p safetensors -p galion-voice-cpu -p galion-chatterbox` | 20:39 | 10 + 21 + 14 + 2 + 1 passed, 0 failed |
| `cargo clippy ... --all-targets --all-features -- -D warnings` | 20:40 | ok |
| `GALION_REAL=1 ... --test real_safetensors_mit` (release voice-serve on :21650) | 20:40-20:42 | 1 passed; health 200 after 0.77 s; STT JFK text identical; TTS 140 044 B, sha256 `c1a6ef38...53bb9` = byte-identical to the 19:31 upstream baseline; port free after |
| `GALION_REAL=1 RAYON_NUM_THREADS=2 ... --test real_chatterbox` | 20:42-20:43 | 2 passed (53.9 s) |
| `cargo-slot test --workspace --exclude openmind-sandbox --exclude cgroup-detect --no-fail-fast` | 20:43-20:46 | 385 suites, 2026 passed, 0 failed, 18 ignored |
| `repo-check --root .` / `scripts/no-python.sh` | 20:46 | PASS 0 errors 83 pre-existing warnings / ok |
| VPS read-only nerve `GET 127.0.0.1:8670/state` | 20:43 | 200 (38 kB): 12 healthy, 3 degrading, 2 failing, 1 degraded, 1 loading, 1 absent; voice service inactive, :8003 not listening (no live safetensors consumer; nothing deployed) |
