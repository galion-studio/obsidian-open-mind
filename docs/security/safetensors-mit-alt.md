<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# AP-1 follow-up: MIT replacement for safetensors (owner decision memo)

Written 2026-10-27 19:40 CET (+01:00), slot 295, branch `swarm/safetensors-mit-alt` off `swarm/cargo-deny-gate`
400e93a5. Nothing is merged; RULES.md unchanged. For Slavik / Mate.

## Problem
`safetensors` 0.7.0 (Hugging Face) is the **only Apache-2.0-only crate** in Cargo.lock
(docs/security/cargo-deny-gate-20261027.md "Offenders"). candle-core and candle-nn 0.9.2 depend on it directly,
so every voice build (Whisper STT, KittenTTS, Chatterbox) pulls it in. The cargo-deny gate passes today only
through the OWNER-PENDING AP-1 exception. Upstream 0.8.0 is still Apache-2.0-only.

## What this branch does (prototype, option C)
- `vendor/safetensors-mit/`: Galion-written, MIT, `#![forbid(unsafe_code)]` reader/writer for the safetensors
  format, same public API subset candle uses (`tensor::*`, `slice::IndexOp`). ~1000 lines + 345 lines of tests.
  Deps: serde + serde_json only (already in the lock, taken under MIT).
- Root `Cargo.toml` `[patch.crates-io] safetensors = { path = "vendor/safetensors-mit" }` (same mechanism as the
  existing candle-core and paste patches). candle sources untouched.
- `deny.toml`: AP-1 exception removed; gate passes with **zero** Apache-2.0-only crates.
- Cargo.lock: registry safetensors 0.7.0, hashbrown 0.16.1, allocator-api2 0.2.21 removed; nothing added.

## Evidence (details: docs/real-tests/safetensors-mit-alt.md)
- All 7 real model files (4.97 GB, 4017 tensors): 0 differences vs upstream in names, dtypes, shapes, byte ranges,
  metadata and 3098 slice checks; re-serialization byte-identical for the 4 files under 200 MB.
- 200 000 mutated headers: upstream and MIT accept/reject exactly the same inputs.
- Real galion-voice-serve on :21650: STT text identical, TTS WAV **byte-identical** to the upstream build
  (sha256 c1a6ef38...53bb9). Chatterbox real tests 2/2 pass with 3 GB of weights through the MIT reader.
- Release binary 4.5 kB smaller. Builds x86_64/aarch64 musl; checks android/armv7.

## Differences from upstream (deliberate)
1. Duplicate JSON keys are rejected (upstream keeps the last one). Removes a parser-differential trick where two
   tools read different weights from the same file. No real model file has duplicates.
2. `__metadata__` is written with sorted keys (upstream: HashMap order), so saved files are deterministic.
3. Only the API candle 0.9.2 uses is provided. A future candle that calls more of the upstream API would fail to
   compile (loud, not silent) and the shim would need the extra items.

## Risks / costs
- We now own a parser for untrusted model files. Mitigation: no unsafe, overflow-checked sizes, 100 MB header cap,
  hostile-input tests, the differential fuzz above. Recommend adding it to the fuzz/SAST target list.
- Package is named `safetensors` (required by `[patch]`); README and Cargo.toml say clearly it is not Hugging Face's.
- Must be re-checked when candle is bumped (same as the existing candle-core/paste patches).

## Options for AP-1
| Option | Effect |
|---|---|
| A. Keep upstream Apache-2.0 safetensors (exception stays) | No code to own; release must ship Apache-2.0 LICENSE/NOTICE for it. |
| B. Drop candle/voice from the MIT build | Loses STT/TTS; not recommended. |
| **C. Adopt this MIT shim (this branch)** | Zero Apache-only crates; gate needs no exception; ~1000 lines to maintain. |

## Owner decisions
- **ST-1** Pick AP-1 option A or C (recommendation: C — fully MIT, parity proven on real models, rollback is one line).
- **ST-2** If C: OK that the duplicate-key behaviour is stricter than upstream (recommendation: yes).
- **ST-3** If C: add `vendor/safetensors-mit` to the cargo-fuzz target list (header parser) in the next fuzz slot.
- **ST-4** Merge path: this branch is off `swarm/cargo-deny-gate` 400e93a5, which is itself not on main-staging;
  merge cargo-deny-gate first, then this branch (no conflicts expected: Cargo.toml patch block, deny.toml
  exceptions, Cargo.lock).

Rollback (if needed later): remove the patch line, `cargo update -p safetensors`, restore
`{ allow = ["Apache-2.0"], crate = "safetensors" }` in deny.toml.
