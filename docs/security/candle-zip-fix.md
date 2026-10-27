<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# candle-zip-fix: zip 7.4.0 -> 9.0.5 under candle (RUSTSEC-2026-0367 / -0368 / -0369)

Written 2026-10-21 01:50 CEST (+02:00). Branch `swarm/candle-zip-fix`, base `swarm/voice-rust` 03d21a9 (candle
lives there: `galion-voice-cpu` features `whisper`/`kitten`, and `galion-chatterbox`).

## Finding (from cortex-s2-a 9557eb3, T2.4 rustsec sensor)

`Cargo.lock` had `zip` 7.4.0, reached only through `galion-voice-cpu` / `galion-chatterbox` -> `candle-core`
0.9.2 (and `candle-nn` -> `candle-core`). Three advisories, all patched in `zip >= 9.0.0`:

| Advisory | Issue | CVSS3 |
|---|---|---|
| RUSTSEC-2026-0367 (GHSA-vv5m-jxxh-4627) | `extract()` keeps SUID/SGID/sticky bits | 7.3 |
| RUSTSEC-2026-0368 (GHSA-4v9h-p7m9-rh26) | `extract()` creates symlinks pointing outside the destination | none given |
| RUSTSEC-2026-0369 (GHSA-4684-h2q9-2vcp) | `extract()` / stream extract apply permissions through symlinks | 9.1 |

Real exposure was low: all three are in `ZipArchive::extract` / `ZipStreamReader::extract`. candle-core 0.9.2
never calls them; it only uses `ZipArchive::new`, `by_name`, `by_index`, `file_names` (reading `.pth`/`.npz`
tensors in `src/pickle.rs`, `src/npy.rs`) and `ZipWriter` (writing `.npz`). Our voice crates load safetensors
weights, not `.pth`/`.npz`. The advisory still makes every audit of the lock red, so it is fixed.

## Why "bump candle" is not enough (checked 2026-10-21 on crates.io and GitHub)

| candle-core | zip requirement |
|---|---|
| 0.9.2 (ours) | ^7.2.0 |
| 0.10.0 / 0.10.1 / 0.10.2 | ^7.2.0 |
| 0.11.0 (latest release, 2026-06-26) | ^8.6.0 |
| upstream main `Cargo.toml` | 8.6.0 |

No candle release, and not upstream main, accepts zip 9. Bumping candle alone would give zip 8.x, which is still
affected (patched is only `>= 9.0.0`), and would mean porting galion-chatterbox / whisper to a new candle API.

## Fix

- `vendor/candle-core/`: the published candle-core 0.9.2 crate, with three small source edits for the zip 9 API
  (`ZipFile::name()` and `ZipArchive::file_names()` now return `ZipResult<Cow<str>>`) and the zip requirement set
  to 9.0.5. Every change is listed in `vendor/candle-core/GALION-PATCH.md` and marked `galion candle-zip-fix` in
  the code. Licence stays MIT OR Apache-2.0 (original LICENSE kept, changes stated as Apache-2.0 section 4(b) asks).
- Workspace `Cargo.toml`: `[patch.crates-io] candle-core = { path = "vendor/candle-core" }` and `vendor` added to
  `exclude` (so fmt/clippy of the workspace do not lint upstream code). candle-nn 0.9.2 stays from crates.io and
  picks up the patched candle-core.
- `Cargo.lock`: zip 7.4.0 -> 9.0.5; candle-core now a path package; new lock entries `nix` 0.31.3 and
  `cfg_aliases` 0.2.2 (zip 9 uses nix only on macOS/BSD targets; nothing new is compiled on Linux/Android).
- `crates/galion-voice-cpu/tests/real_voice_serve.rs`: the older real test asserted `GET /health == 200` on the
  first probe, but since 1b9fd06 health is truthfully 503 "loading" until models load, so it failed (seen
  on this branch before the test change; the cause is the health contract, not zip). It now waits up to 120 s for 200. Test-only change.

Removal: drop `vendor/candle-core` and the patch entry when a candle release requires zip >= 9 (then also port to
that candle version), or upstream the 3-line change (owner decision below).

## Audit evidence

The cortex rustsec sensor's own code (`openmind_nerve::sensors::rustsec::{audit, lock_packages}` with its
embedded snapshot, cortex-t20-merge ee75f75), run from a scratch crate outside the repo:

```
base 03d21a9 Cargo.lock: 139 packages, 2 affected
  paste 1.0.15 ["RUSTSEC-2024-0436"]
  zip 7.4.0 ["RUSTSEC-2026-0367", "RUSTSEC-2026-0368", "RUSTSEC-2026-0369"]
swarm/candle-zip-fix Cargo.lock: 140 packages, 1 affected
  paste 1.0.15 ["RUSTSEC-2024-0436"]
```

`paste` (unmaintained notice, info only, via candle's `gemm`) is unchanged and out of scope.

## Owner decisions (Slavik)

- **C1 keep a vendored, patched candle-core?** (a) keep `vendor/candle-core` (1.8 MB, 3 source edits) until
  upstream moves to zip 9 (recommended: fixes the audit now, no behaviour change); (b) instead accept the advisory
  while the candle features stay off in deployed builds (no vendoring, audit stays red); (c) port to candle 0.11 and
  accept zip 8.6 (still affected, larger change).
- **C2 upstream report.** Opening an issue/PR on huggingface/candle asking for zip 9 is publishing under our name;
  not done. Yes/no.
