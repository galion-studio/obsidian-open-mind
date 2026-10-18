# Workstream: Galion voice-to-voice (`ws/voice`)

| Field | Value |
|---|---|
| Written | 2026-10-18 21:10 CEST (+02:00), Europe/Zagreb |
| Branch | `ws/voice` (not merged, not pushed) |
| Crates | `crates/galion-voice-core` (new, M1), `crates/persona-plex-relay` (imported scaffold + adapter) |
| Builds on | `docs/persona-plex-v2-design.md`, `UNIVERSAL-CPU.md` (binding), rewrite plan tickets B46/B47, B62/B63, B65 |
| Status | M1 done (code + tests). Nothing deployed, no GPU provisioned, no money spent, no model weights downloaded. |

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
| (b1) STT: Whisper tiny/tiny.en | Good English for its size [unknown for Croatian] | whisper.cpp tiny.en on Pi 5: ~6 s per 10 s chunk (RTF ~0.6) [source: Medium Pi 5 test]; ~1.5 s per short clip [source: openHAB]. Those are C with NEON/AVX. **Pure-Rust scalar (candle/tract without SIMD flags): [unknown], expected slower**; M2 measures it | 39 M params (OpenAI paper); RSS in a pure-Rust runtime [unknown] | Whisper weights and code MIT; candle MIT/Apache-2.0; tract MIT/Apache-2.0 | Maybe: Pi 5 / recent phone at RTF < 1 if scalar speed holds [unknown] |
| (b2) TTS: Piper / VITS ONNX | Natural for its size; 16 kHz (x_low/low) or 22.05 kHz (medium/high) [source: cekura.ai on Piper voice index] | RTF 0.153 on an Intel i7-1255U (onnxruntime) [source: cekura.ai citing an arXiv study]; tract scalar [unknown] | Voice files about 21-28 MB (x_low), ~63 MB (low/medium), 63-137 MB (high) [source] | **Engine: rhasspy/piper MIT but archived; active piper1-gpl is GPL-3.0; espeak-ng phonemiser is GPL.** Voices: per-voice MODEL_CARD [source]. Using only the ONNX voice with our own phonemiser avoids GPL code. Needs a licence review | Maybe (x_low/low voices) [unknown] |
| (b3) text model | Local small LLM, or remote through openmind-provider / llama-server (B63) | [unknown] | [unknown] | per model | Remote: yes. Local: only very small models [unknown] |
| (c) Mock | Tone only | 1 frame per tick | ~2.2 MB RSS [measured] | MIT (ours) | Yes |

### Codec (Opus) trade-off

The wire protocol needs Opus in Ogg pages. `audiopus`/`opus` crates link libopus (C, BSD-3), which goes against the "no C build deps" rule. On 2026-10-18 crates.io lists several pure-Rust Opus crates [source: crates.io]:

- `opus-pure` 0.2.2 (BSD-3-Clause): encoder + decoder, SILK/CELT/hybrid, **Ogg mux/demux included**, "no C, no FFI, no build.rs, no dependencies", MSRV 1.88. **First choice to evaluate.**
- `opus-rs` 0.1.37 (BSD-3-Clause): port of libopus 1.6, zero deps by default, no_std option. No Ogg layer.
- `rusty-opus` 1.0.2 (BSD-3-Clause): runtime-dispatched AVX2/SSE2/NEON kernels with a scalar fallback. Its `unsafe` SIMD conflicts with "no SIMD intrinsics" unless the scalar path can be forced. Second choice.
- `ruopus` 0.1.2 (MIT): `std::arch` SIMD hot loops. Same concern.

None is audited yet (unsafe/SIMD grep, conformance, size on armv7). That audit is the first M2 task. The core stays codec-free (PCM frames), so the choice stays inside the backend/transport.

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
5. Merge order: `ws/voice` adds `galion-voice-core` as a workspace member and makes `persona-plex-relay` depend on it. Also, `UNIVERSAL-CPU.md` is untracked in this worktree. Who commits it?
