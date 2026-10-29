# Streaming speech-to-text to replace Whisper tiny (Hermes chatbox mic): research

Date: 2026-10-29, about 08:40 CET (Europe/Zagreb). Research only: nothing installed, downloaded, built or changed. Sources: web pages, the Hugging Face model API (metadata only), the GitHub and crates.io APIs (metadata only) and HTTP HEAD requests for file sizes.

## Short answer
- **Best MIT option: Moonshine v2 "Tiny Streaming" (English, 34M params).** It is from Moonshine AI, formerly Useful Sensors. The code is MIT and so are the weights. It is built for live microphone input. It caches encoder and decoder state as audio arrives, and it sends `LineTextChanged` events with partial text while you speak (every 500 ms by default, and you can change that). The final text arrives about 69 ms after you stop speaking on their "Linux x86" machine, against 1,141 ms for Whisper Tiny. On the Open ASR leaderboard average it is slightly more accurate than Whisper Tiny: 12.00% WER against 12.81%.
- **If accuracy matters more than CPU use: Moonshine v2 "Small Streaming" (123M, MIT).** Its WER is 7.84%, better than Whisper Small, and the final text arrives about 165 ms after speech ends on the same Linux x86 machine. It costs about 3.6 times the parameters of Tiny.
- **Rust path:** No pure-candle Moonshine *streaming* implementation exists yet. There are three options today:
  - (a) `transcribe-rs` (MIT): a Rust re-implementation of the Moonshine streaming pipeline on the `ort` crate, which links ONNX Runtime from C++. Its public API transcribes a whole buffer, so we would have to expose its internal chunk and state functions to get partial text.
  - (b) `moonshine-rs` (MIT OR Apache-2.0): a bindgen wrapper around the official C++ `libmoonshine` plus ONNX Runtime. It already has `add_audio()`, `poll()` and `finalize()` for partial results.
  - (c) Port Moonshine Streaming to candle inside `galion-voice-cpu`. This is the only route that stays pure Rust and keeps the binary under 2 MiB without shipping a `libonnxruntime.so`. Reference code exists: HF Transformers `MoonshineStreaming` (Apache-2.0), the `transcribe-rs` streaming.rs file (MIT), and a non-streaming candle Moonshine in `any-speech-to-text` (MIT OR Apache-2.0).
- **Best non-MIT runner-up: a sherpa-onnx streaming Zipformer transducer** (code and weights Apache-2.0, permissive but not MIT). It is a true frame-synchronous streaming design that emits words as you speak and runs very cheaply on CPU. The 20M English model is trained on LibriSpeech only, so it may be weaker on real microphone or conversational speech (we have not tested this). It also outputs uppercase text with no punctuation. It is not clearly better than Moonshine. NVIDIA's Parakeet realtime model is more accurate (9.30% avg WER) but uses the NVIDIA Open Model License, not MIT, and needs NeMo/PyTorch.

## Baseline (ours, measured earlier on our box)
Whisper tiny.en in candle: 39M params, 151 MB f32 (FUTO ACFT q8 is about 44 MB), RTF 0.12 to 0.31, about 310 MB RSS. It transcribes whole chunks, so no text appears until a chunk is finished.

## Comparison table
"Partial" means text appears while you are still speaking. Figures marked (vendor) are the project's own numbers. Hardware is noted where the source gives it. Nothing here was measured on our VPS.

| Candidate | Code licence | Weights licence | Params / files | Languages | Speed / latency (source) | WER (source) | Partial results? | Rust path | Maturity |
|---|---|---|---|---|---|---|---|---|---|
| **Moonshine v2 Tiny Streaming** | MIT (except core/third-party) | **MIT** | 34M (card); HF safetensors f32 176 MB; transcribe-rs ONNX tarball 33.0 MB compressed | EN; also Tiny Streaming for es, de, ar, ja, zh, vi, tl | Final text 69 ms after speech ends on "Linux x86" (CPU model not stated; docs say this column is old and pessimistic). Whisper Tiny on the same machine: 1,141 ms (vendor). Paper, M3: 50 ms, 8.0% compute load, against 289 ms for Whisper Tiny | Open ASR avg 12.00% (Whisper Tiny 12.81%, vendor). LS-clean 4.49% float, 4.83% shipped int8 | **Yes**: LineStarted, LineTextChanged and LineCompleted events, default 500 ms updates | C++ core + ONNX Runtime; `moonshine-rs` (FFI); `transcribe-rs` (ort). No candle streaming port | 11.3k stars, pushed 2026-10-02; arXiv 2602.12241 |
| **Moonshine v2 Small Streaming** | MIT | **MIT** | 123M; f32 561 MB; tarball 104.8 MB | EN, es, de, ja | Linux x86: 165 ms; Whisper Small: 3,425 ms (vendor) | Open ASR avg 7.84%; LS-clean 2.49% | Yes | same as Tiny | same |
| Moonshine v2 Medium Streaming | MIT | MIT | 245M; tarball 201.8 MB | EN | Linux x86: 269 ms (vendor) | 6.65% (better than Whisper Large v3 at 7.44%, vendor) | Yes | same | same |
| Moonshine v1 Tiny / Base (non-streaming) | MIT | MIT (EN) | 26M / 58M; Tiny f32 108 MB | EN | Paper, M3: 27 / 44 ms | 12.66% / 10.07% | No (re-runs; no cache) | candle port exists (`any-speech-to-text`, Crane) | older |
| sherpa-onnx streaming Zipformer EN 20M (k2-fsa) | Apache-2.0 | Apache-2.0 (HF tag) | ~20M; encoder int8 42.8 MB (fp32 88.8 MB) | EN | sherpa-ncnn docs: RTF 0.071 with 2 threads (hardware not stated, unverified) | LS test-clean 3.88% (LibriSpeech-only training, so real-mic WER unknown) | **Yes** (transducer, streams token by token; endpointing built in) | `sherpa-rs` (MIT) binds the C++ sherpa-onnx + ONNX Runtime | sherpa-onnx 15.6k stars, very active |
| sherpa-onnx streaming Zipformer EN 2023-06-26 | Apache-2.0 | Apache-2.0 | int8 encoder 71 MB | EN | not found | not checked | Yes | same | same |
| Vosk small en-us 0.15 (Kaldi) | Apache-2.0 | Apache-2.0 | 40 MB zip (41.2 MB measured) | EN (+ ~20 other small models) | Vendor: "Android and RPi" class; no number found | LS-clean 9.85%, TED-LIUM 10.38% | **Yes** (PartialResult) | `vosk` crate (MIT) binds libvosk (C++) | 15.2k stars; older Kaldi technology |
| NVIDIA Parakeet realtime EOU 120M | NeMo Apache-2.0 | **NVIDIA Open Model License** (not MIT, not OSI) | 120M; .nemo 460 MB | EN, no punctuation | 80 ms chunks; end-of-utterance latency p50 160 ms (GPU pipeline) | Open ASR avg 9.30% (160 ms streaming) | Yes | PyTorch/NeMo; no Rust path found | Nov 2025 |
| NVIDIA Nemotron speech streaming 0.6B | Apache (NeMo) | NVIDIA licence ("other") | 600M; 2.47 GB (q8 GGUF 700 MB) | EN + punctuation | GPU-oriented | not checked | Yes | none found | 2026 |
| Kyutai STT 1B en/fr (delayed-streams-modeling) | MIT (Python) / Apache-2.0 (Rust server) | **CC-BY-4.0** | ~1B; 1.98 GB + Mimi 385 MB | EN, FR | GPU-oriented (L40S: 64 streams at 3x real time). Third-party q8 GGUF on candle: about 1.8x real time on an Apple M CPU (unverified) | not checked here | Yes (0.5 s delay, semantic VAD) | **Native Rust/candle** (moshi-server) | 3k stars |
| Kyutai STT 2.6B en | same | CC-BY-4.0 | 2.6B; 5.2 GB | EN | 2.5 s delay; too big for our CPU | not checked | Yes | candle | same |
| Mistral Voxtral Mini 4B Realtime | Apache-2.0 | Apache-2.0 (HF tag) | 4B | multi | Too big for a loaded 8-core CPU (assessment, not measured) | not checked | Yes | `candle-transformers` has a `voxtral` module (realtime support not confirmed) | 2026 |
| whisper_streaming (ÚFAL) / SimulStreaming / WhisperLive | MIT / MIT / MIT | Whisper MIT | same as Whisper | multi | Re-runs Whisper on a growing buffer, so CPU cost multiplies on an already loaded box. Latency comes from confirming text across passes (LocalAgreement / AlignAtt); figures not re-checked | as Whisper | Pseudo-partials | Python; we would re-implement the policy around our candle Whisper | mature |
| Distil-Whisper (distil-small.en) | MIT | MIT | 166M (unverified); 332 MB fp16 | EN | faster than Whisper small, but not streaming | not checked | No | candle Whisper loads it | maintenance mode |
| wav2vec2-base-960h (CTC) | Apache-2.0 | Apache-2.0 | ~95M (unverified); 378 MB | EN | full-attention encoder, so not really streaming | not checked | No (chunked only) | candle has no wav2vec2 module | old |
| Silero STT | — | **CC BY-NC 4.0** (non-commercial) | — | — | — | — | — | — | Excluded |
| april-asr | **GPL-3.0** | (icefall LSTM-based) | — | EN | — | — | Yes | C | Excluded (GPL; rewrite in progress) |

## Why Moonshine fits our problem specifically
- Whisper always pads input to a 30 s window and caches nothing, so every partial update recomputes everything. Moonshine v2 uses a sliding-window encoder (80 ms lookahead) with no padding, and it caches the encoding of audio it has already seen. Partial updates are therefore cheap, which matters on a VPS that is already heavily loaded.
- It has MIT on both code and weights, and covers the same size class (34M) as Whisper tiny (39M).
- Caveats:
  - The decoder is autoregressive, so a partial line can still change until `LineCompleted`. The chatbox UI should show the active line as provisional text.
  - The card warns about hallucination and repetition on short or noisy clips.
  - The int8 build shipped before 2026-07-30 lost accuracy (7.57% on LS-clean); use current files.
  - The vendor's Linux x86 numbers come from an unnamed CPU.

## Recommended plan (for decision only, nothing done)
1. Pick **Moonshine v2 Tiny Streaming** for the English mic. Keep Small Streaming as the upgrade if Tiny's accuracy feels weak.
2. For a quick trial, use `moonshine-rs` (FFI to libmoonshine) or `transcribe-rs` (`ort`) behind a feature flag. This brings in a ONNX Runtime shared library (tens of MB, size not checked), which breaks the "pure Rust, binary under 2 MiB" rule. Use it only to confirm latency and accuracy on the VPS.
3. Long term, port the Moonshine Streaming architecture to candle inside `galion-voice-cpu`, loading the MIT safetensors from `moonshine-ai/moonshine-streaming-tiny`. The architecture is a conv frontend, a 6-layer sliding-window encoder, an adapter, and a 6-layer RoPE decoder with KV cache. Partial text would go to the chatbox over the existing WebSocket. This keeps the stack pure Rust.
4. Before committing, measure on our VPS: RTF, partial-update latency, RSS, and WER on a few of the owner's own recordings, compared against FUTO ACFT tiny.en.

## Sources
- Moonshine repo and licence: https://github.com/moonshine-ai/moonshine , https://github.com/moonshine-ai/moonshine/blob/main/LICENSE
- Models and WER: https://github.com/moonshine-ai/moonshine/blob/main/docs/models/available-models.md , https://moonshine-voice.readthedocs.io/en/latest/models/accuracy/
- Moonshine vs Whisper latency table: https://moonshine-voice.readthedocs.io/en/stable/moonshine-vs-whisper/ ; benchmark method: https://github.com/moonshine-ai/moonshine/blob/main/docs/using/benchmarks.md
- Streaming events: https://github.com/moonshine-ai/moonshine/blob/main/docs/using/transcription.md
- Paper: https://arxiv.org/abs/2602.12241 ; card: https://huggingface.co/moonshine-ai/moonshine-streaming-tiny (MIT tag)
- Rust: https://github.com/cjpais/transcribe-rs (MIT) ; https://docs.rs/moonshine-rs (MIT OR Apache-2.0) ; https://github.com/TM9657/any-speech-to-text ; https://github.com/lucasjinreal/Crane (MIT) ; https://github.com/moonshine-ai/moonshine/issues/91
- sherpa-onnx: https://github.com/k2-fsa/sherpa-onnx ; https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17 ; https://k2-fsa.github.io/sherpa/ncnn/pretrained_models/zipformer-transucer-models.html ; https://github.com/thewh1teagle/sherpa-rs
- Vosk: https://alphacephei.com/vosk/models ; https://github.com/alphacep/vosk-api ; https://github.com/Bear-03/vosk-rs
- Parakeet: https://huggingface.co/nvidia/parakeet_realtime_eou_120m-v1 ; https://huggingface.co/nvidia/nemotron-speech-streaming-en-0.6b
- Kyutai: https://github.com/kyutai-labs/delayed-streams-modeling ; https://huggingface.co/kyutai/stt-1b-en_fr ; https://huggingface.co/stephvax/kyutai-stt-1b-en_fr-candle-gguf (third party)
- Voxtral Realtime: https://huggingface.co/mistralai/Voxtral-Mini-4B-Realtime-2602
- Whisper streaming: https://github.com/ufal/whisper_streaming ; https://github.com/ufal/SimulStreaming ; https://github.com/collabora/WhisperLive
- Distil-Whisper: https://huggingface.co/distil-whisper/distil-small.en ; Silero: https://github.com/snakers4/silero-models ; april-asr: https://github.com/abb128/april-asr
