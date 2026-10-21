<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code, crates and model weights: `swarm/voice-rust`

Written 2026-10-20 21:40 CEST (+02:00). Licences checked on 2026-10-20 against the LICENSE files of the upstream
repositories (cloned read-only for reading, not vendored) and the Hugging Face model cards (`license:` front matter).

## 1. Crates

**No new crate was added.** The new crate `galion-chatterbox` depends only on `candle-core` 0.9, `candle-nn` 0.9
(both `default-features = false`: no cuda, metal, mkl, accelerate) and `serde_json`, which were all already in
`galion-voice-cpu`'s tree (see `docs/third-party/voice-mouth.md`). `galion-voice-cpu` gains the optional feature
`chatterbox = ["dep:galion-chatterbox"]` (a path dependency inside this repo).

Transitive tree of `galion-voice-cpu --features whisper,kitten,chatterbox` (target x86_64-unknown-linux-musl,
normal edges, from `cargo tree` + `cargo metadata`): 78 crates. Every licence is MIT, Apache-2.0, BSD-2-Clause,
Zlib, Unicode-3.0 or an OR of these; `Unlicense OR MIT` crates (byteorder, memchr) are used under MIT. Nothing
GPL/LGPL/MPL/SSPL/BUSL/unlicensed. No C code, no `-sys` crate, no openssl.

| Crate | Licence (SPDX) | Repository |
|---|---|---|
| allocator-api2 v0.2.21 | MIT OR Apache-2.0 | https://github.com/zakarumych/allocator-api2 |
| bitflags v2.13.2 | MIT OR Apache-2.0 | https://github.com/bitflags/bitflags |
| bytemuck v1.25.2 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| bytemuck_derive v1.12.1 | Zlib OR Apache-2.0 OR MIT | https://github.com/Lokathor/bytemuck |
| byteorder v1.5.0 | Unlicense OR MIT | https://github.com/BurntSushi/byteorder |
| candle-core v0.9.2 | MIT OR Apache-2.0 | https://github.com/huggingface/candle |
| candle-nn v0.9.2 | MIT OR Apache-2.0 | https://github.com/huggingface/candle |
| cfg-if v1.0.5 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| crc32fast v1.5.3 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crossbeam-deque v0.8.8 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-epoch v0.9.21 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-utils v0.8.23 | MIT OR Apache-2.0 | https://github.com/crossbeam-rs/crossbeam |
| dyn-stack v0.13.2 | MIT | https://codeberg.org/sarah-quinones/dyn-stack |
| dyn-stack-macros v0.1.3 | MIT | https://github.com/kitegi/dynstack/ |
| either v1.19.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/either |
| equivalent v1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| float8 v0.6.1 | MIT | https://github.com/EricLBuehler/float8 |
| foldhash v0.2.0 | Zlib | https://github.com/orlp/foldhash |
| galion-chatterbox v0.1.0 | MIT |  |
| galion-voice-core v0.1.0 | MIT |  |
| galion-voice-cpu v0.1.0 | MIT |  |
| gemm v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| gemm-c32 v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| gemm-c64 v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| gemm-common v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| gemm-f16 v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| gemm-f32 v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| gemm-f64 v0.19.0 | MIT | https://github.com/sarah-ek/gemm/ |
| getrandom v0.3.4 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom |
| half v2.7.1 | MIT OR Apache-2.0 | https://github.com/VoidStarKat/half-rs |
| hashbrown v0.16.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| hashbrown v0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| indexmap v2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| itoa v1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| libc v0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| libm v0.2.16 | MIT | https://github.com/rust-lang/compiler-builtins |
| memchr v2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| memmap2 v0.9.11 | MIT OR Apache-2.0 | https://github.com/RazrFalcon/memmap2-rs |
| num-complex v0.4.6 | MIT OR Apache-2.0 | https://github.com/rust-num/num-complex |
| num-traits v0.2.19 | MIT OR Apache-2.0 | https://github.com/rust-num/num-traits |
| num_cpus v1.17.0 | MIT OR Apache-2.0 | https://github.com/seanmonstar/num_cpus |
| once_cell v1.21.4 | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| paste v1.0.15 | MIT OR Apache-2.0 | https://github.com/dtolnay/paste |
| ppv-lite86 v0.2.21 | MIT OR Apache-2.0 | https://github.com/cryptocorrosion/cryptocorrosion |
| proc-macro2 v1.0.107 | MIT OR Apache-2.0 | https://github.com/dtolnay/proc-macro2 |
| pulp v0.22.3 | MIT | https://github.com/sarah-quinones/pulp/ |
| pulp-wasm-simd-flag v0.1.1 | MIT | https://github.com/sarah-quinones/pulp/ |
| quote v1.0.47 | MIT OR Apache-2.0 | https://github.com/dtolnay/quote |
| rand v0.9.5 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| rand_chacha v0.9.0 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| rand_core v0.9.5 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| rand_distr v0.5.1 | MIT OR Apache-2.0 | https://github.com/rust-random/rand_distr |
| raw-cpuid v11.6.0 | MIT | https://github.com/gz/rust-cpuid |
| rayon v1.12.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| rayon-core v1.13.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon |
| reborrow v0.5.5 | MIT | https://github.com/sarah-ek/reborrow/ |
| safetensors v0.7.0 | Apache-2.0 | https://github.com/huggingface/safetensors |
| seq-macro v0.3.6 | MIT OR Apache-2.0 | https://github.com/dtolnay/seq-macro |
| serde v1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_core v1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_derive v1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json v1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| stable_deref_trait v1.2.1 | MIT OR Apache-2.0 | https://github.com/storyyeller/stable_deref_trait |
| syn v2.0.119 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| syn v3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay/syn |
| synstructure v0.14.0 | MIT | https://github.com/mystor/synstructure |
| thiserror v2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| thiserror-impl v2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| typed-path v0.12.4 | MIT OR Apache-2.0 | https://github.com/chipsenkbeil/typed-path |
| unicode-ident v1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| yoke v0.8.3 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| yoke-derive v0.8.4 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zerocopy v0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| zerocopy-derive v0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | https://github.com/google/zerocopy |
| zerofrom v0.1.8 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zerofrom-derive v0.1.8 | Unicode-3.0 | https://github.com/unicode-org/icu4x |
| zip v7.4.0 | MIT | https://github.com/zip-rs/zip2.git |
| zmij v1.0.23 | MIT | https://github.com/dtolnay/zmij |

## 2. Code we re-implemented (read, not copied)

No upstream source file was copied. The Rust code in `crates/galion-chatterbox` was written from the published
architecture and checkpoint tensor names; the upstream Python was read (read-only clones outside the repo) to match
shapes, constants and numerics. The upstream licences are permissive either way.

| Upstream | Revision read | Licence | URL | What we matched |
|---|---|---|---|---|
| Resemble AI Chatterbox | 5de7a54aa4e5e2baadb0182dde554908b48b85c2 (2026-07-21) | MIT ("Copyright (c) 2025 Resemble AI") | https://github.com/resemble-ai/chatterbox | T3 (Llama backbone, conditioning encoder, Perceiver, CFG sampling), S3Gen glue, voice encoder, text normalisation, tokenizer use |
| CosyVoice (inside Chatterbox's `s3gen/`) | as vendored in Chatterbox | Apache-2.0 | https://github.com/FunAudioLLM/CosyVoice | Conformer encoder with relative positions, conditional flow matching decoder (U-Net + transformer blocks), HiFT vocoder, CAM++ x-vector |
| S3Tokenizer | 9bf5d845b5e043ffaf4657f4942939091c7697a2 (2025-12-22) | Apache-2.0 | https://github.com/xingchensong/S3Tokenizer | v2 speech tokenizer: log-mel, FSMN attention blocks, FSQ codes |
| candle Whisper model code | (already vendored by ws-voice) | MIT OR Apache-2.0 | https://github.com/huggingface/candle | `src/whisper/` (unchanged); `whisper_stt.rs` adds the multilingual prompt and 128-bin filters (our code) |
| librosa mel filter construction | n/a (algorithm) | ISC | https://github.com/librosa/librosa | Slaney mel scale and normalisation re-derived in Rust (`dsp::mel_filters_slaney`, `whisper_stt::mel_filters_slaney`) |

Not implemented: Resemble's Perth implicit watermark (`resemble-perth`, MIT, https://github.com/resemble-ai/Perth)
that the Python package applies to every output. See docs/voice-rust.md section 6.

## 3. Model weights (not in git; downloaded once, free, from Hugging Face)

| Model | Files | Revision | Licence | Source URL | sha256 |
|---|---|---|---|---|---|
| ResembleAI/chatterbox (English, 0.5 B) | t3_cfg.safetensors (2.1 GB), s3gen.safetensors (1.06 GB), ve.safetensors, tokenizer.json, conds.pt, README.md | 5bb1f6ee58e50c3b8d408bc82a6d3740c2db6e18 | MIT (model card `license: mit`) | https://huggingface.co/ResembleAI/chatterbox | see below |
| openai/whisper-large-v3-turbo | model.safetensors (1.6 GB, fp16), tokenizer.json, config.json, README.md | 41f01f3fe87f28c78e2fbf8b568835947dd65ed9 | MIT (model card `license: mit`) | https://huggingface.co/openai/whisper-large-v3-turbo | see below |
| openai/whisper-tiny.en (already present, voice-mouth) | model.safetensors, config.json, tokenizer.json, melfilters.bytes | see voice-mouth doc | MIT | https://huggingface.co/openai/whisper-tiny.en | see voice-mouth doc |
| KittenTTS nano 0.8 (stand-in reference clip only) | kitten-official-jasper-3.wav, made earlier by the Rust KittenTTS port | see voice-mouth doc | Apache-2.0 | https://huggingface.co/KittenML/kitten-tts-nano-0.8 | n/a |

```
    914cb1696f47527fe8852ca8f1fe1fa63cb34f76f9c715e84e067b744dd0da81  chatterbox/t3_cfg.safetensors
    2b78103c654207393955e4900aac14a12de8ef25f4b09424f1ef91941f161d4e  chatterbox/s3gen.safetensors
    f0921cab452fa278bc25cd23ffd59d36f816d7dc5181dd1bef9751a7fb61f63c  chatterbox/ve.safetensors
    d71e3a44eabb1784df9a68e9f95b251ecbf1a7af6a9f50835856b2ca9d8c14a5  chatterbox/tokenizer.json
    6552d70568833628ba019c6b03459e77fe71ca197d5c560cef9411bee9d87f4e  chatterbox/conds.pt
    542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1  whisper-large-v3-turbo/model.safetensors
    297b13372ac43916285644fb9687add3cc62ee2a1adb60da3dc25cc94c1871fd  whisper-large-v3-turbo/tokenizer.json
    c5b526b3e3cd64cd8940dabb45e8ba726629e22d8ed389c29b552f9140daf04a  whisper-large-v3-turbo/config.json
```

`galion-voice-serve` loads a model directory only when its `README.md` front matter (or LICENSE) declares a licence
on the allow list (`serve::ALLOWED_MODEL_LICENCES`); both new directories declare `mit`.

### Reference voices
- `samantha.wav` (the legacy clip at `/workspace/galion-src/voice-backend/assets/voices/samantha.wav`) lived on the
  GPU pod and was **not** available: only the two Python files could be read. For the tests a 6 s stand-in was made
  by this Rust Chatterbox with its built-in voice (`conds.pt`, MIT). `jarvis.wav` stand-in: a KittenTTS clip
  (Apache-2.0 model output). Neither stand-in is committed. The real clips (and their provenance and licence) are
  needed from Slavik; drop them into `--voices DIR` as `samantha.wav` / `jarvis.wav`.
