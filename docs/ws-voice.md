# Workstream: Galion voice-to-voice (`ws/voice`)

| Field | Value |
|---|---|
| Written | 2026-10-18 21:10 CEST (+02:00), Europe/Zagreb |
| Branch | `ws/voice` (not merged, not pushed) |
| Crates | `crates/galion-voice-core` (new, M1), `crates/persona-plex-relay` (imported scaffold + adapter) |
| Builds on | `docs/persona-plex-v2-design.md`, `UNIVERSAL-CPU.md` (binding), rewrite plan tickets B46/B47, B62/B63, B65 |
| Status | M1 done; M2 items 1, 2 and 4 done, item 3 (TTS) stopped at the licence check (section 7, updated 2026-10-18 21:25 CEST). Nothing deployed, no GPU, no money spent. Model files (Whisper tiny.en 151 MB, Opus RFC 8251 test vectors 75 MB tarball) live in /workspace/oom-models, not in git. |

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
3. TTS: `tract-onnx` running one Piper x_low/low voice (~21-28 MB). Measure RTF. Phonemiser licence decision (GPL espeak-ng vs our own rules / a permissive G2P).
4. `CpuPipelineBackend` implementing `VoiceBackend` (HalfWithBargeIn). Swap it into the loopback test.
5. Optional: a small neural VAD if energy VAD gives false barge-ins in the room (needs echo cancellation on native clients).

**M3: GPU PersonaPlex smoke test. BLOCKED** on a RunPod top-up (balance about -$0.01). Nothing gets provisioned and the API key is not used until Slavik approves a budget. Then: one A100/4090-class pod with the PersonaPlex server, relay on the VPS, a native client running `Session<RelayBackend>` with Opus, and the PWA. Measure end-to-end latency, interruption stop time and cost per hour, then tear down.

## 6. Needs Slavik

1. **Voiceprint consent and retention policy** (speaker ID as a soft second factor): who may enrol, explicit consent text, where embeddings live, how long they are kept, deletion on request, and whether raw enrolment audio is ever stored (proposed: never, embeddings only).
2. **GPU budget** for M3: RunPod top-up amount, a cap per hour/month, and an auto-shutdown rule. Currently about -$0.01, so blocked.
3. **HTTPS host** for the phone PWA (mic needs a secure context): which hostname and TLS front end. Until then, an SSH tunnel to localhost only.
4. **Licence calls**: OK to use NVIDIA Open Model License weights; Piper engine GPL-3.0/espeak-ng GPL vs a permissive phonemiser.
5. **SIMD exception**: opus-pure (Opus) and gemm (candle matmul) both use runtime-dispatched SIMD kernels internally, with scalar fallbacks on x86 and baseline NEON on aarch64, and no build flags. Accept this as compliant with UNIVERSAL-CPU.md ("optional arch path behind cfg with a portable fallback"), or require scalar-only forks?
6. **TTS route**: our own permissive G2P (CMUdict) vs GPL espeak-ng in a separate process.
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

### 7.3 TTS licence check (stopped before a spike)

| Option | Weights | Size | Phonemes come from | Licence-clean today? |
|---|---|---|---|---|
| Piper (VITS ONNX) | per-voice MODEL_CARD (many MIT / CC) | 21-137 MB | espeak-ng (GPL-3.0); engine piper1-gpl is GPL-3.0 | No |
| Kokoro-82M | Apache-2.0 | 88 MB int8, 310 MB f32 ONNX | misaki, with espeak-ng (GPL) fallback; espeak-free mode needs a transformer G2P (torch) | No (as shipped) |
| KittenTTS nano 0.8 | Apache-2.0 | 25 MB int8, 56 MB f32 | espeak-ng IPA via phonemizer (GPL-3.0) | No (as shipped) |

[source: HF cards, kokoro issue #247, KittenTTS issue #17, kokoro-onnx releases, 2026-10-18]

Every small neural TTS has permissive weights but expects espeak-style IPA phonemes, and the only shipped producer of those is GPL. tract-onnx was also not used: tract-linalg compiles assembly kernels in its build script, which breaks the no-C/asm-build rule. So, per the instructions, **no TTS spike this run**. `BeepTts` remains the placeholder.

The licence-clean route is our own English grapheme-to-phoneme step:
- CMUdict (permissive BSD-style licence) plus an ARPAbet-to-IPA table and simple letter-to-sound rules for unknown words.
- That feeds KittenTTS nano (Apache-2.0, 14 M params, a candle-ready safetensors conversion exists) or Kokoro.
- That is the M2b proposal. It needs Slavik's OK on that approach, or on accepting GPL espeak-ng as a separate process (GPL FAQ, "mere aggregation").

### 7.4 CpuPipelineBackend

`galion-voice-cpu::pipeline::CpuPipelineBackend<S: SpeechToText, M: TextModel, T: TextToSpeech>` implements `VoiceBackend` in HalfWithBargeIn mode.
- A turn runs as: utterance → resample to 16 kHz → STT → text model → TTS → resample to the session rate → frames, with text events (`user: …`, then the reply) and per-turn timings.
- Barge-in clears the queued reply.
- Mocks: `MockStt`, `EchoModel` (no network, no paid API) and `BeepTts`.
- The test drives a full `Session`: a turn, then a barge-in.

M2 core fixes that came out of this work:
- A half-duplex pre-roll: the VAD onset frame is no longer clipped, and the 12 + 6 frames reach STT exactly.
- `end_of_user_turn()` now fires after the last frame has been sent.

Known limit: STT runs synchronously inside the tick that ends the turn (about 1.3-2 s here). A worker thread is the next step.

### 7.5 Next

1. M2b: licence-clean English G2P (CMUdict + rules) and KittenTTS nano in candle. Measure RTF and RSS.
2. A worker thread for STT/TTS so ticks never block. A streaming partial transcript is optional.
3. Run `stt-bench` on a real aarch64 device (phone/Termux or Pi) for the phone-viable answer.
4. Test `OpusOgg` against the PWA's real Opus/Ogg packets once the PWA owner can share a capture. Read-only; no VPS changes.
5. M3 (GPU PersonaPlex) is still blocked on budget.
