# Workstream: Galion voice-to-voice (`ws/voice`)

| Field | Value |
|---|---|
| Written | 2026-10-18 21:10 CEST (+02:00), Europe/Zagreb; updated 21:57 CEST |
| Branch | `ws/voice` (not merged, not pushed) |
| Crates | `crates/galion-voice-core` (new, M1), `crates/galion-voice-cpu` (M2 CPU pipeline), `crates/persona-plex-relay` (imported scaffold + adapter) |
| Builds on | `docs/persona-plex-v2-design.md`, `UNIVERSAL-CPU.md` (binding), rewrite plan tickets B46/B47, B62/B63, B65 |
| Status | M1 done; M2 items 1-4 done. The CPU pipeline runs STT/TTS on a worker thread (7.4). TTS option A works and is licence-clean (own G2P + KittenTTS nano in candle) but runs at RTF 0.7-0.8 on this box (7.5). Nothing deployed, no GPU, no money spent. Model files (Whisper tiny.en 151 MB, KittenTTS nano 59 MB, CMUdict 3.6 MB, Opus RFC 8251 test vectors 75 MB tarball) live in /workspace/oom-models, not in git. |

Labels: **[source]** checked on the cited page on 2026-10-18; **[measured]** measured on this box today; **[unknown]** not known yet, to measure.

## 1. Honest stance

A 7B full-duplex speech model (PersonaPlex-7B, Moshi architecture) cannot run in real time on a phone CPU. NVIDIA lists A100/H100 as supported hardware and an A100 80 GB as test hardware [source: HF model card `nvidia/personaplex-7b-v1`]; bf16 needs about 14 GB of VRAM, and 4-bit NF4 still needs 9.6 GB on a 12 GB consumer GPU [source: matzelle.co, 2026-02-15]. CPU offload on that same GPU box gave about 40 s latency, i.e. not real time [source: same]. A phone has neither the memory bandwidth nor the RAM.

So the plan has three parts:

1. a **portable Rust voice core** that runs everywhere (phone/Termux, Pi, laptop, VPS): framing, VAD, jitter buffer, the full-duplex session with barge-in, and a backend trait;
2. **pluggable backends** behind that trait: (a) remote PersonaPlex on a GPU through `persona-plex-relay` (best quality), (b) an on-device CPU pipeline (streaming STT -> local or remote text model -> TTS) for small chips, with lower quality and turn-based behaviour plus barge-in, (c) a mock for tests;
3. **persona** from the Persona Plex layer through a tiny interface (`PersonaSource`: text prompt + voice prompt). The `openmind-persona` crate on `ws/hermes` will implement it. We do not depend on it yet.

## 2. Architecture

```mermaid
flowchart LR
  subgraph Device["Phone / Pi / laptop (any CPU, portable Rust)"]
    MIC[mic PCM] --> FR[Framer]
    FR --> VAD[Energy VAD]
    VAD --> SES[Session state machine\nfull duplex + barge-in]
    SES -->|playout frame| SPK[speaker]
    JB[Jitter buffer\nreorder, PLC, flush on barge-in] --> SES
    PER[PersonaSource trait\n(openmind-persona later)] -.-> SES
  end
  SES <-->|VoiceBackend trait| B{backend}
  B --> MOCK[(c) MockBackend\ntests]
  B --> CPU[(b) CPU pipeline M2\nSTT -> text model -> TTS]
  B --> RB[(a) RelayBackend\nmoshi wire protocol]
  RB <-->|WSS, Opus/Ogg, kind 0x01 audio,\n0x02 text, 0x40.. relay private| RELAY[persona-plex-relay on VPS\nauth, persona injection,\nspeaker ID soft factor]
  RELAY <-->|WSS / SSH tunnel| GPU[PersonaPlex-7B moshi server\nGPU, 24 kHz, 80 ms frames\nM3, blocked on budget]
  B -.model audio.-> JB
```

Session states: `Idle -> Listening -> UserSpeaking -> AwaitingReply -> Speaking -> Listening`, plus `Fallback` (backend lost; resume or switch backend) and `Closed`. Barge-in: VAD `SpeechStart` while `Speaking` (or with model audio queued) flushes the jitter buffer on the same tick and calls `backend.interrupt()`. Full-duplex backends get the mic every tick; turn-based backends get speech frames only, plus `end_of_user_turn()` at VAD `SpeechEnd`.

Phone PWA: `apps/phone-voice/` on the VPS (owned by another job, read only here) already speaks the relay protocol, so the browser path needs no core changes. The Rust core is for native clients (Termux, Pi, desktop) and for the relay itself.

## 3. What M1 delivered

`crates/galion-voice-core` (std only, zero third-party deps, `#![forbid(unsafe_code)]`, scalar maths only):

| Module | What |
|---|---|
| `audio` | `FrameSpec` (moshi 24 kHz/80 ms, STT 16 kHz/20 ms), `Frame`, `Framer` (odd-sized chunks to fixed frames), RMS/dBFS, deterministic synth (tone, xorshift noise) |
| `vad` | `EnergyVad`: level vs adaptive noise floor (+12 dB margin, -45 dBFS absolute floor), onset 2 frames, hangover 6 frames |
| `jitter` | `JitterBuffer`: prebuffer, reorder by seq, silence concealment, late/duplicate drop, max depth bound, `flush()` for barge-in; a gap before a new talk spurt is not concealed |
| `backend` | `VoiceBackend` trait (start/send_audio/end_of_user_turn/interrupt/poll/close), `Capabilities` incl. `DuplexMode::{Full, HalfWithBargeIn}`, `BackendEvent` |
| `persona` | `PersonaSource` trait + `StaticPersona` (integration point for `openmind-persona`) |
| `session` | `Session<B>`: the state machine above, one `tick(mic) -> playout + events` per frame |
| `mock` | `MockBackend`: 440 Hz reply after each turn, one frame per poll, interrupt, failure injection |
| `tests/loopback.rs` | synthetic user audio up and model audio down on every tick, barge-in mid reply (half- and full-duplex), backend loss to fallback and resume |
| `examples/voice-loopback.rs` | 60 s of simulated audio (750 ticks) with 14 barge-ins, used for size/RSS |

`persona-plex-relay`: new `backend` module with `RelayBackend<T: Transport, C: AudioCodec>`, which implements `VoiceBackend` over the existing `WireMessage` protocol. It builds the upstream URL with the persona via `upstream_url()` and sends audio only, so `forwardable_upstream()` holds. Handshake, text and colored text are mapped, and Error, RelayFallback and RelayBye become `Unavailable`. `interrupt()` sends nothing, because the PersonaPlex server takes only audio from the client; local playout is flushed by the core. The codec is pluggable. `PcmPassthrough` is for tests only; a real server needs Opus in Ogg (see section 5).

### Results (measured 2026-10-18 21:00-21:10 CEST, rustc 1.99.1)

| Check | galion-voice-core | persona-plex-relay |
|---|---|---|
| tests | 9 pass (6 unit + 3 loopback) | 26 pass (24 imported + 2 adapter) |
| clippy `--all-targets -D warnings` | clean | clean |
| `cargo fmt --check` | clean | clean |
| build x86_64-unknown-linux-musl (release, rust-lld) | ok | ok |
| build aarch64-unknown-linux-musl (release, rust-lld) | ok | ok |
| check aarch64-linux-android (tier 2) | ok | ok |
| check armv7-unknown-linux-musleabihf (tier 2) | ok | ok |

Sizes (release profile: opt-level z, lto, codegen-units 1, panic abort, strip):

| Artifact | x86_64 musl | aarch64 musl |
|---|---|---|
| `persona-plex-relay` binary | 479,448 B | 424,096 B |
| `voice-loopback` example binary (core + mock) | 400,376 B | 355,528 B |
| `libgalion_voice_core.rlib` | 344,096 B | 346,836 B |

Max RSS on x86_64, measured with wait4 `ru_maxrss`: `voice-loopback` 60 s run 2,204 KB; `persona-plex-relay --config persona-plex-relay.example.toml` (prints plan, exits) 2,060 KB. The voice core keeps one jitter buffer at no more than 25 frames x 1,920 samples x 2 B, about 96 KB.

## 4. Backend comparison

Only figures from a source or our own measurement. Hardware-specific figures name their hardware.

| Backend | Quality | Latency | CPU / RAM | Licence | Phone-viable |
|---|---|---|---|---|---|
| (a) PersonaPlex-7B on GPU via relay | Best: full duplex, back-channel, persona voice. FullDuplexBench smooth-turn takeover 0.908, interruption takeover 0.950 [source: model card, as summarised by rywalker.com] | 0.170 s smooth turn-taking, 0.240 s user interruption (benchmark, model side, excluding network) [source: same]. End-to-end over the VPS relay [unknown] | GPU: A100/H100 listed; ~14 GB VRAM bf16, 9.6 GB 4-bit [source]. Phone side: only the voice core (~2 MB RSS measured with mock) + Opus [unknown] | Code MIT; weights NVIDIA Open Model License (gated), Moshi base CC-BY-4.0 [source] | **Yes as a client** (the model runs remotely). **No** on-device |
| (b1) STT: Whisper tiny.en | Correct transcript of the 11 s JFK clip [measured]; short trimmed windows cut mid-word degrade [measured, section 7] | **candle 0.9 CPU, this box (Xeon, AVX2 runtime-dispatched by gemm): RTF 0.155 (1 thread) / 0.118 (4 threads) on 11 s, trimmed encoder context; 0.310 / 0.208 with the full 30 s window [measured]**. For reference, whisper.cpp tiny.en on a Pi 5 takes about 6 s per 10 s chunk [source: Medium]. aarch64 builds but was not run (no device). | 39 M params; **peak RSS 308 MB** (f32 weights mmapped) [measured] | Weights Apache-2.0 (HF card) / MIT (openai/whisper repo); candle MIT/Apache-2.0 | Likely on a recent phone or Pi 5 (aarch64 NEON path in gemm) [unknown until run on a device] |
| (b2) TTS: Piper / VITS ONNX | Natural for its size; 16 kHz (x_low/low) or 22.05 kHz (medium/high) [source: cekura.ai on Piper voice index] | RTF 0.153 on an Intel i7-1255U (onnxruntime) [source: cekura.ai citing an arXiv study]; tract scalar [unknown] | Voice files about 21-28 MB (x_low), ~63 MB (low/medium), 63-137 MB (high) [source] | **Engine: rhasspy/piper MIT but archived; active piper1-gpl is GPL-3.0; espeak-ng phonemiser is GPL.** Voices: per-voice MODEL_CARD [source]. Using only the ONNX voice with our own phonemiser avoids GPL code. Needs a licence review | Maybe (x_low/low voices) [unknown] |
| (b2a) TTS: KittenTTS nano 0.8 + our G2P (candle) | Intelligible: Whisper tiny.en transcribes 4/4 test sentences correctly apart from one name ("Galian") and punctuation [measured, 7.5]; 24 kHz | **RTF 0.74-0.80 (1 thread) / 0.69-0.75 (4 threads) on this Xeon** [measured]; first reply audio 6.0-6.7 s after the user stops in the full pipeline [measured] | 14 M params, 56 MB f32 weights; peak RSS 280 MB TTS only [measured] | Weights Apache-2.0; model code MIT (vendored); CMUdict BSD-style; no GPL | Needs about 2x speed-up first [unknown on device] |
| (b3) text model | Local small LLM, or remote through openmind-provider / llama-server (B63) | [unknown] | [unknown] | per model | Remote: yes. Local: only very small models [unknown] |
| (c) Mock | Tone only | 1 frame per tick | ~2.2 MB RSS [measured] | MIT (ours) | Yes |

### Codec (Opus) trade-off

The wire protocol needs Opus in Ogg pages. `audiopus`/`opus` crates link libopus (C, BSD-3), which goes against the "no C build deps" rule. On 2026-10-18 crates.io lists several pure-Rust Opus crates [source: crates.io]:

- `opus-pure` 0.2.2 (BSD-3-Clause): encoder + decoder, SILK/CELT/hybrid, **Ogg mux/demux included**, "no C, no FFI, no build.rs, no dependencies", MSRV 1.88. **First choice to evaluate.**
- `opus-rs` 0.1.37 (BSD-3-Clause): port of libopus 1.6, zero deps by default, no_std option. No Ogg layer.
- `rusty-opus` 1.0.2 (BSD-3-Clause): runtime-dispatched AVX2/SSE2/NEON kernels with a scalar fallback. Its `unsafe` SIMD conflicts with "no SIMD intrinsics" unless the scalar path can be forced. Second choice.
- `ruopus` 0.1.2 (MIT): `std::arch` SIMD hot loops. Same concern.

Audit result: see section 7.1. Short version: licence OK, no C, passes the RFC 8251 vectors, but it carries runtime-dispatched SIMD kernels, so it sits behind a non-default `opus` feature. The core stays codec-free (PCM frames), so the choice stays inside the backend/transport.

## 5. Milestones

**M2: on-device CPU spike (no GPU, no spend).**
1. Opus: audit `opus-pure` (grep for `unsafe`/`std::arch`, build on all 4 targets, size delta, encode/decode round-trip through Ogg against the PWA's packets). Replace `PcmPassthrough` in `RelayBackend` with it.
2. STT: `candle` (default-features = false, CPU, no `target-feature`) running Whisper tiny/tiny.en, streaming in 2-4 s windows driven by VAD. Measure RTF and RSS on x86_64, plus aarch64 if a device is available. Weights are about 75 MB in f16 [unknown exact size; stay under the 100 MB/run rule].
3. TTS: done as option A (section 7.5): our own CMUdict-based G2P + KittenTTS nano in candle. tract-onnx was dropped (asm in its build script).
4. `CpuPipelineBackend` implementing `VoiceBackend` (HalfWithBargeIn). Swap it into the loopback test.
5. Optional: a small neural VAD if energy VAD gives false barge-ins in the room (needs echo cancellation on native clients).

**M3: GPU PersonaPlex smoke test. BLOCKED** on a RunPod top-up (balance about -$0.01). Nothing gets provisioned and the API key is not used until Slavik approves a budget. Then: one A100/4090-class pod with the PersonaPlex server, relay on the VPS, a native client running `Session<RelayBackend>` with Opus, and the PWA. Measure end-to-end latency, interruption stop time and cost per hour, then tear down.

## 6. Needs Slavik

1. **Voiceprint consent and retention policy** (speaker ID as a soft second factor): who may enrol, explicit consent text, where embeddings live, how long they are kept, deletion on request, and whether raw enrolment audio is ever stored (proposed: never, embeddings only).
2. **GPU budget** for M3: RunPod top-up amount, a cap per hour/month, and an auto-shutdown rule. Currently about -$0.01, so blocked.
3. **HTTPS host** for the phone PWA (mic needs a secure context): which hostname and TLS front end. Until then, an SSH tunnel to localhost only.
4. **Licence calls**: OK to use NVIDIA Open Model License weights? For TTS, option A (section 7.5) avoids GPL entirely: Apache-2.0 KittenTTS weights (third-party safetensors conversion, provenance by ONNX hash), MIT model code, BSD-style CMUdict notice shipped. Confirm that is acceptable.
5. **SIMD exception**: opus-pure (Opus) and gemm (candle matmul) both use runtime-dispatched SIMD kernels internally, with scalar fallbacks on x86 and baseline NEON on aarch64, and no build flags. Accept this as compliant with UNIVERSAL-CPU.md ("optional arch path behind cfg with a portable fallback"), or require scalar-only forks?
6. **TTS route**: our own permissive G2P (CMUdict) is now built and intelligible (section 7.5); GPL espeak-ng is not needed. Still to decide: accept KittenTTS at its current CPU speed, or keep the beep until it is faster.
7. Merge order: `ws/voice` adds `galion-voice-core` as a workspace member and makes `persona-plex-relay` depend on it. (`UNIVERSAL-CPU.md` is now committed on ws/voice.)

## 7. M2 results (measured 2026-10-18 21:10-21:25 CEST)

### 7.1 opus-pure 0.2.2 audit

| Check | Result |
|---|---|
| Licence | BSD-3-Clause. Derived from rusty-opus, then opus-rs, both BSD-3, ports of libopus. The Xiph/Skype/Octasic notices are kept in LICENSE and ATTRIBUTION.md. Compatible with MIT. **Pass** |
| C / FFI / build.rs / deps | none (`[dependencies]` empty, `build = false`). **Pass** |
| `unsafe` | Only in SIMD kernels (`#![allow(unsafe_op_in_unsafe_fn)]`). The packet parser and range decoder are safe Rust |
| SIMD / arch code | **Present**, despite the README's "cross-compiles anywhere": `std::arch` kernels in 11 files (silk/sigproc_fix, resampler, nsq_del_dec; celt mdct, kiss_fft, pitch, pvq, bands, lpc, mod). On x86_64 it uses runtime `is_x86_feature_detected!` (SSE/AVX/AVX2) with a scalar fallback. On aarch64 it uses NEON unconditionally; NEON is baseline there, so it runs on every aarch64 CPU, but no scalar path is selectable. armv7 and other targets get scalar. No `asm!`, no build flags needed. **This breaks the letter of UNIVERSAL-CPU.md ("no arch-specific SIMD intrinsics").** The other pure-Rust Opus crates are worse: opus-rs has about 278 `unsafe` sites plus arch code, ruopus has a `simd.rs`, rusty-opus is explicitly SIMD |
| Its own suite (`cargo test --release`, x86_64) | 154 unit + 21 integration files, all pass. 3 ignored tests are dump/seed helpers |
| Official RFC 8251 decoder vectors (our own harness, 12 vectors, 48 kHz stereo) | All 12 match the reference `.dec` output within ±1 LSB. Vectors 02-04 are bit-exact; the rest have SNR 98.6-110.2 dB |
| Fuzz (our own harness) | 300,000 random packets: 117,727 rejected, no panic. 20,000 random or `OggS`-prefixed streams through `OggOpusReader`: all rejected, no panic |

Decision: a **conditional pass**. `OpusOgg` is in `persona-plex-relay` behind the non-default `opus` feature, so default builds stay strictly compliant until Slavik accepts the exception (see section 6).
- `OpusOgg`: 24 kHz mono, 20 ms Opus packets (4 per 80 ms core frame), one Ogg page per core frame.
- The OpusHead and OpusTags header pages go out on the first call.
- The Ogg mux and demux are ours: streaming, CRC-checked and resyncing. opus-pure's writer holds a packet back (one frame of extra latency) and its reader needs a blocking `Read`.
- 4 tests:
  - Ogg CRC known value.
  - Our stream is readable by opus-pure's reader.
  - opus-pure's writer output decodes through our demux when fed in 97-byte chunks.
  - Round trip with one corrupted page: exactly that page is lost, the rest decodes, garbage never panics.

The phone PWA's real packets have not been tested yet; the VPS was not touched.

### 7.2 Whisper tiny.en via candle (`galion-voice-cpu`, feature `whisper`)

- **Stack:** candle-core/candle-nn **0.9.2**, CPU only, default-features off, no target-feature flags.
  - candle 0.10 and 0.11 pull `tokenizers` with `onig` (a C regex library) into candle-core itself, so they were rejected.
  - candle-transformers was avoided for the same reason. Its Whisper model and mel code (about 800 lines, MIT/Apache-2.0) are vendored with attribution.
  - The tokenizer decode is our own GPT-2 byte-level table, so no `tokenizers` crate.
  - Dependency tree: no `-sys` crates, no `cc`.
  - Caveat: `gemm`/`pulp` (candle's matmul) do runtime SIMD dispatch internally (AVX2/FMA on this box, NEON on aarch64), the same category as opus-pure. No scalar-only switch exists. armv7 compiles to scalar but could not be run here.
- **Model:** `openai/whisper-tiny.en`, safetensors f32, 151,060,136 B (sha256 prefix db59695928ded604), in `/workspace/oom-models/whisper-tiny.en`. Licence: Apache-2.0 on the HF card, MIT in the openai/whisper repo.
- **Test audio:** `samples_jfk.wav` (11.0 s, 16 kHz). Machine: this box's Intel Xeon (AVX2/AVX-512 present), shared with other jobs, so treat these as indicative.
- **Greedy decoding with guards:** at most about 8 tokens per second of audio, and repetition-loop detection that drops the repeated tail.

| Run (x86_64 musl release) | 1 thread | 4 threads | Transcript |
|---|---|---|---|
| Model load (mmap) | 251 ms | 257 ms | |
| 11 s, full 30 s window | 3,410 ms, **RTF 0.310** | 2,293 ms, RTF 0.208 | exact |
| 11 s, trimmed context | 1,703 ms, **RTF 0.155** | 1,303 ms, RTF 0.118 | exact (one comma lost) |
| first 4 s, trimmed | 1,097 ms, RTF 0.274 | 749 ms, RTF 0.187 | "And so my fellow Americans asked and" |
| first 2 s, trimmed | 943 ms, RTF 0.471 | 703 ms, RTF 0.351 | "And so my fellow Merrick and" |
| first 4 s, full window | 2,344 ms, RTF 0.586 | 1,318 ms, RTF 0.329 | "And so my fellow Americans asked" |
| first 2 s, full window | 2,172 ms, RTF 1.086 | 1,269 ms, RTF 0.635 | "And so my fellow American" |
| Peak RSS (whole run) | 307,616 kB | 309,284 kB | |
| Through `Session<CpuPipelineBackend>` (24 kHz mic frames, VAD-cut, 11.44 s utterance) | STT 1,970 ms | 1,335 ms | exact |

What this shows:
- The full window has a fixed cost of about 2.2 s per call on one thread, whatever the clip length.
- The trimmed window scales with the audio but loses accuracy on clips cut mid-word. Without the guards it looped; that is fixed now.
- So the CPU path should send whole VAD utterances, not fixed windows.

Binary size of `stt-bench`: 1,361,672 B (x86_64), 1,079,088 B (aarch64). Weights are separate.

Targets:
- Tier 1: x86_64 and aarch64 musl release builds ok.
- Tier 2: `cargo check --all-targets` ok on aarch64-linux-android and armv7-unknown-linux-musleabihf.
- aarch64 RTF: **not measured** (no aarch64 device here; no emulator installed).

### 7.3 TTS licence check (21:25 CEST, before the spike)

| Option | Weights | Size | Phonemes come from | Licence-clean as shipped? |
|---|---|---|---|---|
| Piper (VITS ONNX) | per-voice MODEL_CARD (many MIT / CC) | 21-137 MB | espeak-ng (GPL-3.0); engine piper1-gpl is GPL-3.0 | No |
| Kokoro-82M | Apache-2.0 | 88 MB int8, 310 MB f32 ONNX | misaki, with espeak-ng (GPL) fallback | No |
| KittenTTS nano 0.8 | Apache-2.0 | 25 MB int8, 56 MB f32 | espeak-ng IPA via phonemizer (GPL-3.0) | No |

[source: HF cards, kokoro issue #247, KittenTTS issue #17, kokoro-onnx releases, 2026-10-18]. tract-onnx was rejected: tract-linalg compiles assembly kernels in its build script. Section 7.5 shows how KittenTTS was made licence-clean (option A).

### 7.4 CpuPipelineBackend on a worker thread (measured 21:29-21:55 CEST)

`galion-voice-cpu::pipeline::CpuPipelineBackend<S, M, T>` implements `VoiceBackend` (HalfWithBargeIn).
- **Worker thread:** STT, text model and TTS run on one `std::thread`. Jobs and results go over `std::sync::mpsc`; there is no async runtime. `end_of_user_turn()` only hands the utterance over, and `poll()` drains the results.
- **Cancellation by generation number:** `interrupt()` (barge-in) and a newer turn bump the generation. The worker checks it before each stage and between TTS chunks, and `poll()` drops stale results. A Whisper call that has already started finishes, but its result is thrown away.
- **TTS streaming:** `TextToSpeech::chunks()` cuts the reply. KittenTTS cuts per sentence, and sentences over 60 bytes again at `,`/`:`. Each chunk's frames go out as soon as that chunk is made.
- **Failure handling:** a worker panic shows up as `Unavailable`, so the session falls back. A stage error shows up as `Err(Protocol)` from `poll()`. `close()` does not block.
- **Tests (6 pipeline tests, all pass):**
  - A session turn followed by a barge-in.
  - The end-of-turn tick stays under 100 ms while STT takes 400 ms.
  - An interrupt during STT drops that reply.
  - A newer turn supersedes the pending one.
  - Chunked TTS streams, and a barge-in after the first audio stops the remaining chunks.
  - Worker errors surface, and `close()` takes under 5 ms.

`stt-bench` drives `Session<CpuPipelineBackend<WhisperStt, EchoModel, _>>` with the 11.44 s JFK utterance as 24 kHz mic frames, ticking in real time after the speech (x86_64 musl release):

| TTS | threads | end-of-turn tick | worst tick | STT | TTS first chunk / all | speech end → first reply frame | peak RSS |
|---|---|---|---|---|---|---|---|
| BeepTts | 1 | 31 µs | 64 µs | 1,923 ms | 2 ms | 2,085 ms | 307,776 kB |
| BeepTts | 4 | 31 µs | 120 µs | 1,214 ms | 2 ms | 1,366 ms | 308,972 kB |
| KittenTTS (clauses) | 1 | 47 µs | 88 µs | 1,676 ms | 4,936 / 7,064 ms | 6,734 ms | 408,604 kB |
| KittenTTS (clauses) | 4 | 21 µs | 94 µs | 1,278 ms | 4,610 / 6,636 ms | 6,013 ms | 410,220 kB |
| KittenTTS (sentences only) | 1 | 53 µs | 1,001 µs | 1,700 ms | 6,578 ms | 8,414 ms | 460,308 kB |

- Before this change, the tick that ended the turn blocked for the whole STT call (1.3-2 s, section 7.2).
- The reply is "Galion heard: <22-word transcript>" with 117 frames of 80 ms. All frames played with 0 concealed, 0 dropped and 0 flushed.

### 7.5 TTS option A: our own G2P + KittenTTS nano in candle (measured 21:30-21:55 CEST)

**G2P (`galion_voice_cpu::g2p`, std only, always built).**
- **Dictionary: the CMU Pronouncing Dictionary** (cmusphinx/cmudict @ 7479086, `cmudict.dict`, 3,618,488 B, sha256 81917843…).
  - Licence: checked in its LICENSE file. "Copyright (C) 1993-2015 Carnegie Mellon University", a BSD-2-Clause-style permissive licence: keep the notice in source, and reproduce it in docs for binaries. The file is deemed source code.
  - The notice is in `crates/galion-voice-cpu/NOTICE-cmudict.txt`.
  - The dictionary sits in `/workspace/oom-models/cmudict`, not in git.
- **Letter-to-sound rules** for unknown words: our own simplified English rules (digraphs, magic e, soft c/g, open/closed syllables, first-syllable stress).
- **Text handling:** acronym spelling (GPU → `dʒˌiːpˌiːjˈuː`) and a cardinal number expander.
- **IPA output in espeak-ng en-us style:**
  - the stress mark sits before the vowel;
  - `ː` on long vowels, `ɚ`/`ɜː`, `ɾ` for flapped t, and unstressed `i`;
  - function words are unstressed (`ɐ`, `ðə`/`ði`, `tə`).
- **Token ids:** mapped like KittenTTS 0.8's `TextCleaner`: punctuation split off by spaces, then `[0] + ids + [10, 0]`.
- **Symbol table:** the official 178-symbol table, confirmed by the model's 178-row embedding matrices. tts-web's table has an extra `ʓ`, which shifts `ˈ ˌ ː` by one. We do not use it.
- No GPL code, no espeak data.

| G2P measurement (x86_64 musl release) | Result |
|---|---|
| CMUdict load | 126,052 words, 109 ms, VmHWM 17,328 kB |
| Phonemise one sentence | 8-72 µs |
| Letter-to-sound rules alone vs the dictionary (every 50th word, 2,315 words, stress ignored) | 27.8 % exact, phone error rate 23.6 %. Weak; fine only as a fallback for the few unknown words per reply |
| Binary `g2p-check` | 471,728 B x86_64, 413,368 B aarch64 |

**KittenTTS nano 0.8 in candle 0.9.2 (`galion-voice-cpu`, feature `kitten`).** It is feasible: it runs with no new crates and no Cargo.lock change. It uses the same candle-core/candle-nn 0.9.2 as Whisper, and the dependency tree has no `-sys`, `cc` or `cmake` crates.
- **Model code:** vendored from idle-intelligence/tts-web `crates/kitten-core` @ 4ac4680a (MIT; `LICENSE-kitten-core.txt`). It covers ALBERT, the text encoder, the predictor and the iSTFTNet decoder.
- **Changes for Galion:**
  - anyhow replaced by candle errors;
  - weights loaded by mmap;
  - our own ConvTranspose1d (one matmul plus overlap-add), tested equal to a textbook reference. It is about 7 % faster here.
  - In one test run, candle's CPU `conv_transpose1d` disagreed with the reference for the iSTFT shape (padding 0, k=20, stride 5). A later run with new random inputs did not reproduce it. This is not root-caused.
- **Weights:** `idle-intelligence/kitten-tts-nano-safetensors`, in `/workspace/oom-models/kitten-tts-nano`, not in git.
  - Files: `kitten-nano.safetensors` 56,068,376 B (sha256 b45307e7…), 458 f32 tensors; `kitten-voices.safetensors` 3,277,776 B (sha256 03455461…), 8 voices.
  - Provenance: the file's metadata names the source ONNX sha256 `320564d2…`. That equals the LFS oid of the official `KittenML/kitten-tts-nano-0.8-fp32` `kitten_tts_nano_v0_8.onnx`.
  - Licence: Apache-2.0 (KittenML).
  - We did not convert or compare the ONNX ourselves (no Python), so the conversion is trusted on that metadata.
- **Intelligibility:** Whisper tiny.en transcribed our own TTS output (G2P → KittenTTS → resample to 16 kHz → Whisper):

| Input text | Whisper heard |
|---|---|
| Hello, world. | "Hello world." |
| Obsidian Open Mind, made by Galion, is listening. | "Obsidian Open Mind made by Galian is listening." |
| This high quality speech model works without a graphics card. | "This high-quality speech model works without a graphics card." |
| And so my fellow Americans, ask not what your country can do for you, ask what you can do for your country. | exact, apart from one comma |

**Speed and memory:** `tts-bench`, voice jasper, x86_64 musl release with opt-level z, this shared Xeon box:

| Sentence (tokens → audio) | 1 thread | 4 threads |
|---|---|---|
| #0 (20 → 2.59 s) | 1,926 ms, RTF 0.743 | 1,913 ms, RTF 0.738 |
| #1 (62 → 4.84 s) | 3,751 ms, RTF 0.775 | 3,365 ms, RTF 0.695 |
| #2 (72 → 5.02 s) | 3,914 ms, RTF 0.780 | 3,473 ms, RTF 0.692 |
| #3 (122 → 7.89 s) | 6,341 ms, RTF 0.804 | 5,906 ms, RTF 0.748 |
| Load (mmap + voices + CMUdict) | 182 ms, VmHWM 111,792 kB | 184 ms |
| Peak RSS, TTS only | 280,172 kB (after #3) | 280,240 kB |

- The same bench built with `opt-level=3` (separate target dir, before the ConvTranspose change) gave RTF 0.659-0.708 on 1 thread.
- Stage split for #3 on 1 thread (before the ConvTranspose change):

  | Stage | Time |
  |---|---|
  | BERT | 331 ms |
  | Text encoder | 23 ms |
  | Predictor | 62 ms |
  | Decoder | 6.0 s (94 %) |

  Inside the decoder, the HiFi-GAN generator's second stage (64 channels at 60× frame rate: noise block, upsampling, ResBlocks) takes most of the time. Extra threads barely help.
- Binary `tts-bench`: 1,786,720 B x86_64, 1,319,984 B aarch64. With Kitten, `stt-bench` is 1,852,256 B x86_64 and 1,373,960 B aarch64. Weights are separate.

**Verdict:**
- **Licence:** clean (Apache-2.0 weights, MIT model code, BSD-style CMUdict, our own rules).
- **Quality:** intelligible.
- **Speed:** not yet good enough for conversation on this CPU. RTF is about 0.7-0.8, so a 5 s clause needs about 4 s before its first sample, and first reply audio comes 6-6.7 s after the user stops. `BeepTts` stays the default; Kitten is opt-in behind `kitten`.

### 7.6 Targets (galion-voice-cpu, 2026-10-18 21:55 CEST)

- **aarch64-unknown-linux-musl:** release build ok for `stt-bench`, `tts-bench` and `g2p-check` with `--features kitten,whisper`. It needs `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`; the default `cc` rejects `--fix-cortex-a53-843419`.
- **Tier 2:** `cargo check --all-targets` is ok on aarch64-linux-android and armv7-unknown-linux-musleabihf, both with default features and with `kitten,whisper`.
- **Not measured:** aarch64 or armv7 RTF/RSS (no device or emulator here).
- **Checks:** clippy `--all-targets -D warnings` is clean for default, `whisper`, `kitten` and `kitten,whisper`. `cargo fmt --check` is clean.
- **Tests:** 13 with default features, 14 with `kitten`, 21 with `kitten,whisper`. All pass.

### 7.7 Still open / next

1. **SIMD exception: open for Slavik and unchanged.** opus-pure (Opus) and gemm/pulp (candle matmul, now used by both Whisper and KittenTTS) dispatch SIMD kernels at run time. There are scalar fallbacks on x86, baseline NEON on aarch64, and no build flags. See section 6 item 5. Nothing was changed here.
2. **KittenTTS speed.** Ideas, none measured yet:
   - opt-level 3 for the candle crates only (via `profile.release.package`; whole-binary opt-level 3 measured 18-22 % faster);
   - a direct conv1d in place of candle's im2col for the long 64-channel generator convolutions;
   - shorter first chunks;
   - the int8 ONNX weights (25 MB), but candle has no int8 conv path.
3. Better letter-to-sound rules, or a small permissive dictionary for names and brand words (`Phonemizer::add_word` exists). A text normaliser for dates, currencies and units.
4. Run `stt-bench` and `tts-bench` on a real aarch64 device (phone/Termux or Pi).
5. Test `OpusOgg` against the PWA's real Opus/Ogg packets. Read-only; no VPS changes.
6. M3 (GPU PersonaPlex) is still blocked on budget.
