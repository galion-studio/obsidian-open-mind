# FUTO Keyboard voice model: can Galion use it? (research only)

Date: 2026-10-29, about 08:10 CET (Europe/Zagreb). Research only. No commits, no deploys, no service changes. The only VPS access was one read-only SSH command (nproc, lscpu, free -m, uptime).

## Short answer
- **What it is:** FUTO's voice input runs OpenAI **Whisper** through **whisper.cpp**. The built-in English model is **Whisper tiny.en**, fine-tuned by FUTO with a method it calls **ACFT** (audio-context fine-tuning) and quantised to q8_0 GGML (43.6 MB file). It only turns speech into text (STT). It does **not** translate and has **no voice of its own**.
- **Can it run on CPU?** **Yes, easily.** It is the same size as the Whisper tiny.en model we have already run on CPU on the box: 0.7 to 3.4 s per utterance, and peak memory of about 300 MB for STT alone.
- **Can we use it commercially?** **Yes for the model weights and the method.** The weights are Apache-2.0 and the ACFT repo is MIT. **No for FUTO's app code.** The keyboard and Voice Input apps use the FUTO Source First License, which bans commercial modification and redistribution. So take the model, not the app.
- **Recommendation:** Use FUTO's `acft-whisper-tiny.en` safetensors as a drop-in "ear" (the STT step) in our existing pure-Rust candle Whisper code (`crates/galion-voice-cpu`). Pair it with KittenTTS nano, which is already ported to candle, as the "mouth" (the TTS step). This is the turn-based CPU voice loop that `galion-voice-serve` already provides. It is a working voice, but it is not a replacement for Persona Plex's full-duplex conversation.

## 1. What FUTO's voice model is
| Item | Finding | Source |
|---|---|---|
| Engine | whisper.cpp (GGML `.bin` models) | Keyboard source `Models.kt`: https://github.com/futo-org/android-keyboard/blob/007394af/voiceinput-shared/src/main/java/org/futo/voiceinput/shared/Models.kt |
| Built-in model | `tiny_en_acft_q8_0.bin` (shipped renamed as `.bin.not.tflite`), shown in the UI as "English-39" (39M parameters) | same file; https://keyboard.futo.tech/voice-input-models |
| Optional downloads | English-74 / -244 (base.en / small.en ACFT), Multilingual-39 / -74 / -244 (tiny / base / small ACFT) | same two links |
| File sizes (q8_0 GGML, measured from HTTP Content-Length today) | tiny.en 43,550,795 B; base.en 81,781,811 B; small.en 264,477,561 B; tiny 43,537,450 B; base 81,768,602 B; small 264,464,624 B | https://voiceinput.futo.org/VoiceInput/tiny_en_acft_q8_0.bin (redirects to dl.voiceinput.futo.org) |
| Architecture | Whisper encoder-decoder transformer. tiny.en has d_model 384, 4 encoder and 4 decoder layers, 80 mel bins and a vocabulary of 51,864 tokens (from FUTO's config.json). Same shape as OpenAI tiny.en (37.76M params) | https://huggingface.co/futo-org/acft-whisper-tiny.en |
| "57.7M params" on Hugging Face | The HF page says 57,676,032 F32 parameters. The difference from 37.76M is exactly 51,864 × 384 = 19.9M, so the output projection is probably stored as a separate copy of the token embedding. It is not a bigger model. (This is my inference and is unverified.) | HF API for both repos |
| What ACFT changes | Whisper normally processes a fixed 30 s window, even for 2 s of speech. ACFT fine-tunes the model so the encoder can run on just the audio length (`audio_ctx`) without the decoder looping or repeating. That cuts latency on short clips. FUTO's reported word error rates (WER) on LibriSpeech clean for tiny.en: 4.73% for the stock model at full context; 225.99% for the stock model with dynamic context (it breaks down and repeats); 4.96% / 5.50% for the ACFT model at full / dynamic context | https://github.com/futo-org/whisper-acft |
| Task | Speech to text only. The English models are English-only. The multilingual models auto-detect among the languages you enable. FUTO Voice Input lists 17 UI languages, those with more than 1,000 Whisper training hours. FUTO documents no translation feature. (Whisper multilingual *can* translate into English, but I found no sign FUTO uses that. This is not verified in code.) | https://docs.keyboard.futo.tech/settings/languagesmodels ; https://github.com/futo-org/voice-input |
| Also in FUTO Voice Input? | Yes. ACFT was built for FUTO Voice Input v1.3.2+, and the keyboard now has that voice input built in | https://github.com/futo-org/whisper-acft ; https://github.com/futo-org/voice-input |
| Standalone download | Yes. Safetensors checkpoints on HF (`futo-org/acft-whisper-{tiny.en,tiny,base.en,base,small.en,small}`) and GGML q8_0 files at voiceinput.futo.org | links above; collection https://huggingface.co/collections/futo-org/whisper-acft-667c430f8de3a22b73151d74 |

### Licenses
| Part | License | Commercial use? |
|---|---|---|
| FUTO Keyboard app code | FUTO Source First License 1.1: modification only for non-commercial purposes; distribution only free of charge and for non-commercial purposes | **No.** Do not copy its code. https://github.com/futo-org/android-keyboard/blob/master/LICENSE.md |
| FUTO Voice Input app code | FUTO Source First License 1.0 | **No.** https://github.com/futo-org/voice-input |
| ACFT weights (HF) | Apache-2.0 (model card + HF tag) | **Yes** (keep the notice) |
| ACFT method repo (notebooks) | MIT (GitHub license API) | Yes |
| OpenAI Whisper code + weights | MIT (repo); HF card for openai/whisper-tiny.en is tagged apache-2.0 | Yes |
| whisper.cpp | MIT | Yes |
| GGML `.bin` files on voiceinput.futo.org | No separate license stated on the download page. They are conversions of the Apache-2.0 weights. **Not verified.** It is safer to use the HF safetensors, which carry a clear license | n/a |

## 2. Can it run on CPU?
**Yes.** The numbers below are all tiny.en sized.

- **Our own measurement (box Xeon, candle, stock OpenAI tiny.en, f32):** from `docs/ws-voice.md` §7 in the repo (worktree `wt/voice-mouth-stuck`).
  - 11 s clip, full 30 s window: 3,410 ms on 1 thread (real-time factor RTF 0.310); 2,293 ms on 4 threads (RTF 0.208).
  - 11 s clip, trimmed context: 1,703 ms (RTF 0.155) / 1,303 ms (RTF 0.118). The transcript was exact.
  - 2 s clip, trimmed: 703 to 943 ms, but the text **degraded** ("my fellow Merrick and").
  - Peak RSS for STT was 308 MB. The STT+TTS service peaked at about 450 MB.
  - **The 2 s trimmed-context degradation is exactly the failure ACFT was built to fix.** That is the concrete benefit of FUTO's model for us. It is not yet measured with the ACFT weights.
- **whisper.cpp published numbers:** tiny uses about 75 MiB on disk and about 273 MB of memory; base uses 142 MiB and about 388 MB (https://github.com/ggml-org/whisper.cpp README). Encoder-only times on a full 30 s window from 2022 (https://github.com/ggerganov/whisper.cpp/issues/89):
  - Ryzen 9 3900X, AVX2, 8 threads: tiny 422 ms, base 880 ms.
  - Ryzen 9 5950X, 8 threads: tiny 197 ms, base 421 ms.
  - These are old commits, encoder only, and do not include decoding.
- **VPS (read-only SSH, 2026-10-29 07:08 CET):**
  - CPU: AMD Ryzen 7 3800X (Zen 2, AVX2). `nproc` 8, lscpu lists 16 CPUs (2 threads per core).
  - Memory: `free -m` shows 16,384 MB total, 116 MB used and 16,267 MB available. The "used" figure looks like a container view; treat it with care.
  - **Load average 23.79 / 22.33 / 22.81 on 8 usable cores.** The box is heavily oversubscribed, so real latency on the VPS will be worse than the box figures until load drops. VPS latency has **not been measured**.
  - Rough expectation (my inference, not measured): on an idle Zen 2 at 4 threads, tiny.en takes about 0.5 to 2 s per short utterance.

## 3. Fit for the Galion voice service
**What the service expects (from the repo):**
- Galion One needs **both** an ear (STT) and a mouth (TTS):
  - ear: `POST /v1/audio/transcriptions` returns `{"text"}`, legacy GPU port 8643, 20 s timeout;
  - mouth: `POST /synthesize` returns a WAV, legacy :8002, 30 s timeout.
  - Source: `docs/voice-mouth-503.md`, `docs/voice-rust.md`, `config/voice/galion-one-voice.toml`.
- The current live :8002 returns 503 "loading chatterbox", which is a hard-coded stub. Branch `mouth-honest-stub-rs` (d552fc4) changes it to an honest "voice not deployed".

**What already exists (not deployed):**
- `crates/galion-voice-cpu`, binary `galion-voice-serve`:
  - pure-Rust candle Whisper STT (feature `whisper`);
  - KittenTTS nano 0.8 in candle with our own G2P, the text-to-phoneme step (feature `kitten`);
  - optional Chatterbox (feature `chatterbox`).
- `crates/galion-voice-core` adds energy-based voice activity detection (VAD) and barge-in (the user talking over the reply) for a turn-based session.
- The x86_64 musl binary is **1,840,616 B, under the 2 MiB rule**. Weights load at runtime (`docs/real-tests/voice-mouth.md:89`).
- KittenTTS measured RTF 0.69 to 0.80 on the box; the first reply audio arrived 6.0 to 6.7 s after the user stopped talking (`docs/ws-voice.md`).
- Chatterbox on CPU is too slow: RTF 6 to 11, 4.2 GB RSS (`docs/voice-rust.md`).

**So FUTO's model is an STT upgrade, not a new pipeline.** A full voice loop still needs:
1. **Ear:** FUTO acft-whisper-tiny.en (or base.en for more accuracy) in our candle Whisper, run with trimmed or dynamic audio context.
2. **Brain:** Hermes (hermes-rs :8642). Note that the docs record **HTTP 402 at the mind step** (OpenRouter credit), so a voice turn fails there regardless. Nerve/cortex never use OpenRouter; the Hermes billing is a separate owner decision.
3. **Mouth (CPU TTS options):**
   - **KittenTTS nano:** Apache-2.0 (KittenML/KittenTTS repo), already in candle.
   - **Kokoro-82M:** Apache-2.0 weights (HF hexgrad/Kokoro-82M). Better quality, but more compute, and no candle port in the repo. CPU speed not measured by us.
   - **Piper:** the original rhasspy/piper is MIT but **archived**; the active successor OHF-Voice/piper1-gpl is **GPL-3.0**. The voices repo rhasspy/piper-voices is tagged MIT on HF, but each voice has its own model card. Piper normally uses the GPL espeak-ng phonemiser. Only usable license-clean as ONNX voices plus our own G2P; needs review.

**Rust integration options:**
| Option | License | Notes |
|---|---|---|
| Our candle Whisper (`galion-voice-cpu`) | candle MIT OR Apache-2.0 (crates.io 0.11.0; repo pins 0.9) | **Best fit.** Already built, pure Rust, under 2 MiB, tested. Loads `model.safetensors` + `config.json` + `tokenizer.json`. **FUTO's HF repo has no `tokenizer.json`** (only vocab.json/added_tokens.json), so reuse OpenAI tiny.en's tokenizer.json (same 51,864 vocabulary; unverified that it is byte-identical). Our loader ties the output projection to the token embedding; whether FUTO's separate output projection differs is **unverified** and needs a transcript test. Uses runtime AVX2 matmul (the SIMD exception, decision A, is still pending) |
| whisper-rs (bindings to whisper.cpp) | Unlicense; whisper.cpp MIT | Can load FUTO's q8_0 `.bin` files directly, but builds C/C++ (against the Rust-first rule; whisper.cpp C++ is already flagged for a ruling). The GitHub repo is archived and development moved to https://codeberg.org/tazz4843/whisper-rs (0.16.0, 2026-03) |
| sherpa-onnx / sherpa-rs | Apache-2.0 / MIT | C++ plus onnxruntime; it would need an ONNX export of the ACFT model. Heavier dependencies, not Rust-first |

**Trade-offs vs Persona Plex** (NVIDIA PersonaPlex: 7B, Moshi-based, full-duplex speech-to-speech; code MIT, weights under the NVIDIA Open Model License with a gated HF license; https://github.com/NVIDIA/personaplex):
| | FUTO tiny ACFT + LLM + KittenTTS (CPU) | Persona Plex (GPU) |
|---|---|---|
| Hardware | CPU only. About 300 to 450 MB RAM for STT+TTS, plus the brain | GPU with a lot of VRAM. Its README offers `--cpu-offload`, but CPU-only use of a 7B speech model is not realistic for real time (unmeasured) |
| Latency | Turn-based: STT about 1 to 3 s, then the LLM, then TTS (first audio 6 to 6.7 s on the box today). Slower on the loaded VPS | Designed for low-latency real-time conversation |
| Interruptions | Barge-in only via our VAD in galion-voice-core. No true talking over each other, no backchannel ("mm-hm") | Full-duplex natively |
| Voice persona | The STT has no voice. The persona voice comes from TTS: KittenTTS has 8 stock voices, **no Samantha cloning**. Cloning needs Chatterbox, which is too slow on CPU | Voice and role conditioning built in |
| Quality | tiny.en WER about 5% clean / about 17% noisy speech (FUTO numbers). base.en is better at about 2× the cost | Higher, but not measured by us |
| Languages | English models are English-only; multilingual tiny/base/small are weaker per language. KittenTTS is English | Not verified |
| License | All parts permissive (Apache-2.0 / MIT) **if we avoid FUTO app code and GPL Piper/espeak** | Weights are not MIT: NVIDIA Open Model License (commercial terms not reviewed here) |

## Recommendation
1. Treat FUTO's model as **"Whisper tiny.en, fine-tuned to be fast on short clips"**. It is a good, free, commercially usable upgrade for our existing CPU ear. It is not a voice engine on its own.
2. Proposed setup:
   - Fetch `futo-org/acft-whisper-tiny.en` safetensors (Apache-2.0), with base.en as the accuracy option.
   - Run it in `galion-voice-serve --features whisper,kitten` with dynamic/trimmed audio context, with KittenTTS nano as the mouth.
   - Point Galion One's `GALION_MOUTH` at it.
   - Keep Persona Plex as the future GPU path. This needs owner decisions already on file: SIMD exception A, new unit B, galion-one restart C (`docs/voice-mouth-503.md` §4); the Hermes 402 fix; and model files in `/var/lib/galion/voice-models`.
3. **Before any deploy, test on the box:** ACFT tiny.en vs stock tiny.en on short (2 to 4 s) clips with trimmed context: accuracy, latency and RSS. Also check the tokenizer and output-projection compatibility noted above.

## Not verified
- FUTO ACFT weights have not been run in our candle code. Tokenizer and output-projection compatibility are unverified.
- Real latency on the VPS: not measured, and the VPS is heavily loaded (load about 23 on 8 cores).
- The license of the GGML `.bin` files on voiceinput.futo.org: none stated; only the HF safetensors are clearly Apache-2.0.
- Whether FUTO ever uses Whisper's translate task: not found in its docs; source code not inspected.
- PersonaPlex VRAM needs and its weight-license commercial terms: the HF model card is gated and was not read.
- Kokoro CPU speed in Rust: no measurement or port in the repo.
