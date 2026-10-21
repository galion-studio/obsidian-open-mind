<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code, crates and model weights: `swarm/voice-mouth`

Written 2026-10-20 19:30 CEST (+02:00). Licences were checked on the cited pages/APIs on 2026-10-20.

## 1. New in this branch's own commits
**No new crate was added.** `galion-voice-serve` and `serve.rs` use std only (plus the crate's existing optional
candle features). `scripts/voice/fetch-models.sh` uses curl and sha256sum.

## 2. Brought in by merging `vps/ws-voice` @ 202bde2 (9 commits; authored on Oct 18, re-audited here)
| Item | Version | Licence (SPDX) | Source | Used for | Why not our own |
|---|---|---|---|---|---|
| candle-core, candle-nn | 0.9.2 | MIT OR Apache-2.0 | https://github.com/huggingface/candle | CPU tensor ops for Whisper and KittenTTS (features `whisper`, `kitten`, off by default) | A tensor library with conv/matmul/softmax is far beyond a small rewrite; candle is pure Rust, no C, no `-sys` crates |
| serde, serde_json | 1 | MIT OR Apache-2.0 | crates.io | Whisper `config.json` (feature `whisper`) | already a workspace dependency |
| num-traits | 0.2 | MIT OR Apache-2.0 | crates.io | Whisper model code | small, standard |
| opus-pure | 0.2.2 | BSD-3-Clause | https://crates.io/crates/opus-pure | Opus/Ogg in persona-plex-relay (feature `opus`, off by default) | Opus codec; no C |
| Vendored kitten-core model code | tts-web @ 4ac4680a | MIT | https://github.com/idle-intelligence/tts-web (`crates/kitten-core`) | KittenTTS nano model in candle (`src/kitten/`) | Keeps its notice in `crates/galion-voice-cpu/LICENSE-kitten-core.txt` |
| Vendored candle Whisper model code | candle 0.9 examples | MIT OR Apache-2.0 | https://github.com/huggingface/candle | `src/whisper/` | as above |
| CMUdict notice | cmudict @ 7479086 | BSD-2-Clause style | https://github.com/cmusphinx/cmudict | G2P dictionary (data, not in git) | Notice in `NOTICE-cmudict.txt` |

Transitive tree of `galion-voice-cpu --features whisper,kitten` (75 crates, from `cargo tree`): every licence is
MIT, Apache-2.0, BSD-2/3, Zlib, Unicode-3.0 or an OR of these. `byteorder` and `memchr` are `Unlicense OR MIT`
(used under MIT). Nothing GPL/LGPL/MPL/SSPL/BUSL/no-licence. Full list:

| Crate | Licence |
|---|---|
| allocator-api2 v0.2.21 | MIT OR Apache-2.0 |
| bitflags v2.13.2 | MIT OR Apache-2.0 |
| bytemuck v1.25.2 | Zlib OR Apache-2.0 OR MIT |
| bytemuck_derive v1.12.1 | Zlib OR Apache-2.0 OR MIT |
| byteorder v1.5.0 | Unlicense OR MIT |
| candle-core v0.9.2 | MIT OR Apache-2.0 |
| candle-nn v0.9.2 | MIT OR Apache-2.0 |
| cfg-if v1.0.5 | MIT OR Apache-2.0 |
| crc32fast v1.5.3 | MIT OR Apache-2.0 |
| crossbeam-deque v0.8.8 | MIT OR Apache-2.0 |
| crossbeam-epoch v0.9.21 | MIT OR Apache-2.0 |
| crossbeam-utils v0.8.23 | MIT OR Apache-2.0 |
| dyn-stack v0.13.2 | MIT |
| dyn-stack-macros v0.1.3 | MIT |
| either v1.19.0 | MIT OR Apache-2.0 |
| equivalent v1.0.3 | Apache-2.0 OR MIT |
| float8 v0.6.1 | MIT |
| foldhash v0.2.0 | Zlib |
| gemm v0.19.0 | MIT |
| gemm-c32 v0.19.0 | MIT |
| gemm-c64 v0.19.0 | MIT |
| gemm-common v0.19.0 | MIT |
| gemm-f16 v0.19.0 | MIT |
| gemm-f32 v0.19.0 | MIT |
| gemm-f64 v0.19.0 | MIT |
| getrandom v0.3.4 | MIT OR Apache-2.0 |
| half v2.7.1 | MIT OR Apache-2.0 |
| hashbrown v0.16.1 | MIT OR Apache-2.0 |
| hashbrown v0.17.1 | MIT OR Apache-2.0 |
| indexmap v2.14.2 | Apache-2.0 OR MIT |
| itoa v1.0.18 | MIT OR Apache-2.0 |
| libc v0.2.191 | MIT OR Apache-2.0 |
| libm v0.2.16 | MIT |
| memchr v2.8.3 | Unlicense OR MIT |
| memmap2 v0.9.11 | MIT OR Apache-2.0 |
| num-complex v0.4.6 | MIT OR Apache-2.0 |
| num-traits v0.2.19 | MIT OR Apache-2.0 |
| num_cpus v1.17.0 | MIT OR Apache-2.0 |
| once_cell v1.21.4 | MIT OR Apache-2.0 |
| paste v1.0.15 | MIT OR Apache-2.0 |
| ppv-lite86 v0.2.21 | MIT OR Apache-2.0 |
| proc-macro2 v1.0.107 | MIT OR Apache-2.0 |
| pulp v0.22.3 | MIT |
| pulp-wasm-simd-flag v0.1.1 | MIT |
| quote v1.0.47 | MIT OR Apache-2.0 |
| rand v0.9.5 | MIT OR Apache-2.0 |
| rand_chacha v0.9.0 | MIT OR Apache-2.0 |
| rand_core v0.9.5 | MIT OR Apache-2.0 |
| rand_distr v0.5.1 | MIT OR Apache-2.0 |
| raw-cpuid v11.6.0 | MIT |
| rayon v1.12.0 | MIT OR Apache-2.0 |
| rayon-core v1.13.0 | MIT OR Apache-2.0 |
| reborrow v0.5.5 | MIT |
| safetensors v0.7.0 | Apache-2.0 |
| seq-macro v0.3.6 | MIT OR Apache-2.0 |
| serde v1.0.229 | MIT OR Apache-2.0 |
| serde_core v1.0.229 | MIT OR Apache-2.0 |
| serde_derive v1.0.229 | MIT OR Apache-2.0 |
| serde_json v1.0.154 | MIT OR Apache-2.0 |
| stable_deref_trait v1.2.1 | MIT OR Apache-2.0 |
| syn v2.0.119 | MIT OR Apache-2.0 |
| syn v3.0.9 | MIT OR Apache-2.0 |
| synstructure v0.14.0 | MIT |
| thiserror v2.0.21 | MIT OR Apache-2.0 |
| thiserror-impl v2.0.21 | MIT OR Apache-2.0 |
| typed-path v0.12.4 | MIT OR Apache-2.0 |
| unicode-ident v1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| yoke v0.8.3 | Unicode-3.0 |
| yoke-derive v0.8.4 | Unicode-3.0 |
| zerocopy v0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerocopy-derive v0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerofrom v0.1.8 | Unicode-3.0 |
| zerofrom-derive v0.1.8 | Unicode-3.0 |
| zip v7.4.0 | MIT |
| zmij v1.0.23 | MIT |

**SIMD flag (open, needs Slavik):** `gemm`, `gemm-common`, `pulp` and `raw-cpuid` choose AVX2/AVX-512/NEON kernels
at run time. No `target-cpu`/`target-feature` flags are set and our code has no intrinsics, but RULES.md hard rule 1
says "no SIMD". This was already noted as "SIMD exception still open" on `ws/voice`. The heavy features stay
non-default; default workspace builds do not compile candle.

## 3. Model weights (not in git; fetched by a human with `scripts/voice/fetch-models.sh`, sha256-pinned)
| Model | Files used | Licence of weights | Licence of upstream code | Source (pinned) | Verdict |
|---|---|---|---|---|---|
| Whisper tiny.en | model.safetensors 151,060,136 B (sha256 db596959...), config.json, tokenizer.json | Apache-2.0 (HF model card `license: apache-2.0`; HF API) ; OpenAI's original release is MIT | openai/whisper MIT | huggingface.co/openai/whisper-tiny.en @ 87c71024 | **allowed** |
| Mel filters | melfilters.bytes 64,320 B | MIT OR Apache-2.0 (candle repo) | same | huggingface/candle @ a3b16994 | **allowed** |
| KittenTTS nano 0.8 (safetensors conversion) | kitten-nano.safetensors 56,068,376 B (sha256 b45307e7...), kitten-voices.safetensors 3,277,776 B | Apache-2.0 (HF card of the conversion and of `KittenML/kitten-tts-nano-0.8-fp32`, both checked via HF API) | KittenML/KittenTTS Apache-2.0 | huggingface.co/idle-intelligence/kitten-tts-nano-safetensors @ 3e76949c | **allowed** (conversion provenance trusted on its metadata; we did not re-convert, that would need Python) |
| CMU Pronouncing Dictionary | cmudict.dict 3,618,488 B | BSD-2-Clause style ("Copyright (C) 1993-2015 Carnegie Mellon University") | n/a | github.com/cmusphinx/cmudict @ 7479086 | **allowed** (notice kept) |

The service also enforces this at run time: `serve::check_model_dir` reads the model card's `license:` (or a
LICENSE text) and refuses anything not on `ALLOWED_MODEL_LICENCES`; a directory without a declared licence is not loaded.

## 4. Options researched and not used
| Option | Code licence | Weights / data licence | Why not |
|---|---|---|---|
| whisper-rs / whisper-rs-sys | **Unlicense** (crates.io API) | Whisper (MIT/Apache) | Unlicense is not on the allow list; it also builds whisper.cpp (C/C++, SIMD) through a `-sys` crate, against "Rust only / no C" |
| whisper.cpp | MIT | same | C/C++ with hand-written SIMD; would only be usable as an external process, not as Rust |
| Piper (rhasspy/piper) | MIT, but the repo is **archived** | per voice: e.g. `en_US-lessac` trained on the Blizzard 2013 Lessac data under its own restrictive licence (not commercial-friendly); `en_US-ljspeech` is public domain data | Phonemes come from espeak-ng (**GPL-3.0**); the maintained fork `OHF-Voice/piper1-gpl` is **GPL-3.0**; ONNX needs onnxruntime (C++) or tract (assembly kernels in tract-linalg) |
| Kokoro-82M | Apache-2.0 | Apache-2.0 | G2P is misaki with espeak-ng (GPL) fallback; 310 MB f32; not ported to candle here |
| KittenTTS (official Python/ONNX) | Apache-2.0 | Apache-2.0 | The official package phonemises with espeak-ng via `phonemizer` (GPL-3.0) and runs on Python + onnxruntime. We use the weights only, with our own G2P (CMUdict + own rules) in Rust: option A, licence-clean |
| NVIDIA PersonaPlex-7B | MIT | NVIDIA Open Model License (not MIT) | GPU only (A100/H100); spend not approved |
