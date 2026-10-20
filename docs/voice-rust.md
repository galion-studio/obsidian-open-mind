<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# voice-rust: the Galion One mouth and ear in Rust (candle, CPU)

Swarm slug `voice-rust`, branch `swarm/voice-rust`, ports 19370-19379. Built on `swarm/voice-mouth` (5d776c1).
Started 2026-10-20 20:42 CEST (+02:00). Rust only; nothing deployed to the VPS.

## 1. The legacy contract (source of truth)

Read read-only on 2026-10-20 20:47 CEST (+02:00) with `ssh ... cat` of exactly two files on the VPS disk:
`/opt/galion/galion-light/tts/tts_server.py` (178 lines, Flask) and `/opt/galion/galion-light/tts/worker.py`
(76 lines). No other VPS file was opened. Neither runs on the VPS (they ran on the GPU pod).

### 1.1 `tts_server.py`: the HTTP mouth

| Item | Legacy behaviour |
|---|---|
| Bind | `TTS_HOST` (default `127.0.0.1`), `TTS_PORT` (default `8002`). Flask, `threaded=False` (one request at a time). |
| Model | `ChatterboxTTS.from_pretrained(device=cuda if available else cpu)` = Hugging Face `ResembleAI/chatterbox` (`t3_cfg`, `s3gen`, `ve`, `tokenizer.json`, `conds.pt`). Loaded once, lazily, under a lock; a load error is stored as `_err` (first 400 chars) and never retried. |
| Startup | `__main__` loads the model and exits with the error if it fails, then speaks `"Hello."` with `samantha` (warm-up), then serves. |
| Voices | `{"samantha": ".../voice-backend/assets/voices/samantha.wav", "jarvis": ".../jarvis.wav"}`. A voice is "ready" when its reference WAV exists. |
| Sample format | 24 000 Hz, mono, signed 16-bit little-endian PCM, 44-byte canonical RIFF/WAVE header (`fmt ` chunk size 16, PCM format 1, block align 2). Float samples are scaled by 32767 and clamped to [-32768, 32767]. |

**`GET /health`** -> always HTTP **200** (Flask `jsonify`), body:
```json
{"ok": <model loaded and no error>, "status": "ok" | "loading", "provider": "local", "model": "chatterbox",
 "loaded": <bool>, "error": <string or null>, "device": "cuda", "voices": ["samantha", "jarvis"], "bind": "127.0.0.1:8002"}
```
`status` is `"ok"` only when the model object exists; `voices` lists only voices whose reference WAV exists;
`device` echoes `TTS_DEVICE` (default `"cuda"`), not the device actually used.

**`POST /synthesize`**, body JSON (`request.get_json(silent=True)`; a non-JSON body counts as `{}`):
- `text` (string, required): stripped. Empty -> **400** `{"error": "text required"}`.
- `voice` (string, optional, default `"samantha"`): stripped and lower-cased; an unknown name silently becomes `"samantha"`.
- Text handling (`chunks`): collapse all whitespace to single spaces, strip, cap at **1200 chars**. Up to **300 chars** is
  one chunk. Longer text is split after `.`, `!` or `?` followed by whitespace, and greedily packed: a new chunk starts when
  the current one is at least 50 chars and adding the next sentence would pass 200 chars.
- Each chunk: `model.generate(chunk, audio_prompt_path=<voice wav>)` (Chatterbox defaults: `exaggeration=0.5`,
  `cfg_weight=0.5`, `temperature=0.8`, `repetition_penalty=1.2`, `min_p=0.05`, `top_p=1.0`). If the reference WAV is
  missing, `model.generate(chunk)` uses the built-in conditionals (`conds.pt`). Chunks are concatenated with no gap.
- Success -> **200**, `Content-Type: audio/wav`, body = the WAV above, headers `X-RTF: <wall seconds / audio seconds,
  3 decimals>` and `X-Duration: <audio seconds, 3 decimals>` (duration = `(len(wav) - 44) / 48000`, minimum 0.001).
- Any exception (including "model not loaded") -> **500** `{"error": "<first 300 chars of the exception>"}`.
- Other paths: Flask defaults (404 HTML page; 405 for a wrong method on a known path).

### 1.2 `worker.py`: the framed child protocol (Galion One used to spawn it)

- stdout is the protocol channel (Python output is redirected to stderr). After the model loads the child writes the
  single byte `R`.
- Request: `u32 LE length n`, then `n` bytes `[voice byte][UTF-8 text]`. `n < 2` or `n > 2000` -> reply is just the end
  frame. Voice byte `0` = samantha, `1` = jarvis, anything else = samantha. Text is decoded with replacement, stripped
  and cut to **300 chars**.
- Generation uses `exaggeration=0.4, cfg_weight=0.4` (note: different from the HTTP server's 0.5/0.5).
- Reply: `u32 LE length` + raw PCM (24 kHz s16le mono, no WAV header) for the one clip, then a zero-length end frame.
  On an exception the clip frame is skipped and only the end frame is sent (the error goes to stderr as `mouth: ...`).
- Galion One (`crates/galion-one/src/speech.rs`) keeps the framing as pure functions (`frame_request`, `read_ready`,
  `read_frames`) and today talks HTTP to a remote mouth instead (`GALION_MOUTH`).

### 1.3 The ear (faster-whisper large-v3-turbo) as Galion One calls it

The ear's server source was not in scope (only the two files above were read). Its contract is therefore taken from the
caller, Galion One on `integration-1` 8fd26f6 (`crates/galion-one/src/main.rs`, `transcribe()` and the service probe):
- Address: `GALION_EAR`, default `127.0.0.1:8643`.
- `POST /v1/audio/transcriptions`, `Content-Type: multipart/form-data; boundary=onebound`, parts `file` (filename
  `speech.wav`, `audio/wav`, the browser recording as WAV), `language=en`, `fast=1`. 20 s timeout.
- Response: JSON; Galion One reads only the string field `text` (first 400 chars). Any HTTP error or unreachable socket
  means "No ear on this machine." The OpenAI-compatible shape `{"text": "..."}` is what faster-whisper servers return.
- `GET /health` on the ear address is the `whisper` row of Galion One's service probe.
- Recorded live settings (config/voice/galion-one-voice.toml on `swarm/settings-snapshot` b9ff9d4): faster-whisper 1.2.1,
  `large-v3-turbo`, `language=en`, `beam=1`, `temperature=0.0`, `condition_on_previous_text=false`.

### 1.4 How Galion One uses the mouth (integration-1 8fd26f6)

`GALION_MOUTH=<addr>` makes `speak()` send `POST /synthesize` with `{"text": ...}` only (no `voice`, so the server default
`samantha` applies), 30 s timeout, and use the body only when the status is 200 and the body is non-empty.
`:8002 /health` passes the remote `/health` status and body through (3 s timeout).

## 2. What was built (all Rust, candle 0.9 on CPU, no Python anywhere)

### 2.1 `crates/galion-chatterbox`: Chatterbox inference in candle
A complete re-implementation of the inference path of Resemble AI's Chatterbox (MIT code and weights, HF
`ResembleAI/chatterbox` @ 5bb1f6ee). No candle port existed (only ONNX ones), so every module was written from the
architecture and checkpoint tensor names:

| Module | What it is |
|---|---|
| `text.rs` | `punc_norm` (the reference text clean-up) and the English BPE tokenizer read from `tokenizer.json` |
| `t3.rs` | T3: 30-layer Llama (1024 wide, 16 heads, Llama-3 RoPE scaling) with KV cache; conditioning = speaker projection + Perceiver resampler over the 150 prompt speech tokens + emotion (exaggeration) projection; classifier-free guidance batch; repetition penalty, temperature, min-p, top-p sampling |
| `flow.rs` | S3Gen token-to-mel: token embedding, conformer encoder with ESPnet relative-position attention, pre-lookahead, 2x upsampling; conditional flow matching decoder (U-Net, causal conv blocks, 56 transformer blocks), cosine time schedule, CFG 0.7, Euler steps |
| `hift.rs` | HiFT vocoder: F0 predictor, neural source filter (sine harmonics), STFT source branch, upsampling with Snake resblocks, iSTFT to 24 kHz |
| `ve.rs` | Voice encoder (3-layer LSTM, 40-bin mel partials) for the T3 speaker embedding |
| `s3tok.rs` | S3Tokenizer v2 (FSMN attention, FSQ) to tokenise the reference clip |
| `campplus.rs` | CAM++ x-vector (Kaldi fbank, 2-D residual head, densely connected CAM blocks, stats pooling) for S3Gen speaker conditioning |
| `conds.rs` | Built-in voice from `conds.pt` (torch zip pickle, read with candle's pickle reader) or a voice from any reference WAV; prompt-length cap |
| `model.rs` | `Chatterbox::load(dir)`, `generate(text, conds, options)`, the reference 40 ms start trim/fade |
| `dsp.rs`, `wav.rs`, `rng.rs` | FFT/STFT, Slaney mel banks, Kaldi fbank, resampling, silence trim; WAV I/O; seeded RNG |

Examples: `chatterbox-say MODEL_DIR TEXT OUT.wav [--ref voice.wav] [--seed N] [--steps N]`, `voice-sim` (speaker
similarity check). `unsafe` is used once: the read-only mmap of safetensors (`nnx::load_vb`, SAFETY comment).

### 2.2 `galion-voice-serve` (crate `galion-voice-cpu`), new feature `chatterbox`
- `--tts chatterbox` serves the legacy mouth: voice table `samantha`/`jarvis` -> `<--voices>/<name>.wav`, built-in
  voice when a clip is missing, lower-cased voice name, unknown voice -> `samantha`, 1200-char cap, the legacy
  300/200/50 chunking (`chatterbox_tts::legacy_chunks`), float->s16 conversion with torch's truncation, 24 kHz mono
  44-byte WAV, `X-RTF` and `X-Duration` headers, `{"error": ...}` JSON errors. Reference clips are analysed once at
  load (the legacy server re-analysed per call; same result).
- `--worker`: the legacy `worker.py` frame protocol on stdin/stdout (`R`, `u32` frames, voice byte, 300-char cut,
  exaggeration/cfg 0.4 as worker.py).
- `--stt whisper-large-v3-turbo`: the legacy ear's model. `whisper_stt.rs` now handles multilingual checkpoints
  (prompt `<|startoftranscript|><|en|><|transcribe|><|notimestamps|>`), 128 mel bins (Slaney bank computed in Rust when
  `melfilters.bytes` is absent; verified equal to the shipped 80-bin file to 1e-5), the `language` form field, and
  suppresses end-of-text/space as the first token. Same `POST /v1/audio/transcriptions` contract.
- Health is truthful (section 3). The port is bound before loading, so a starting service answers 503 `loading`.
- `--flow-steps` and `--prompt-seconds`: speed knobs (defaults = reference 10/10).
- Release profile: per-package `opt-level = 3` for candle, gemm and the two voice crates (the workspace default `z`
  made inference ~1.45x slower). Other binaries are unaffected.

## 3. Contract conformance and deliberate deviations

| Item | Legacy | Rust service | Note |
|---|---|---|---|
| `POST /synthesize` request/response, WAV format, voices, chunking, `X-RTF`, `X-Duration`, 400 `text required` | as section 1.1 | same | `X-Audio-S`, `X-Voice`, `X-Wall-Ms`, `X-Cpu-Ms` are extra headers |
| `GET /health` fields | `ok,status,provider,model,loaded,error,device,voices,bind` | all present (`provider` "local", `device` "cpu" = real device) | extra: `runtime,stt,tts,stt_missing,tts_missing,rss_kb,hwm_kb` |
| `/health` HTTP status | always 200 | **200 only when every requested engine is loaded; 503 while loading or broken** | deliberate: Galion One's mouth row and the nerve read the status code, and "always 200" hid a dead mouth (the voice-mouth 503 root cause). Body still says `status:"loading"` while loading |
| Synthesis while not loaded | 500 | 503 `{"error":"no text-to-speech: ..."}` | more precise code; Galion One only checks for 200 |
| Non-JSON body | treated as `{}` -> 400 | 400 `text required` | same |
| Other paths | Flask HTML 404/405 | JSON 404/405 | |
| Concurrency | Flask `threaded=False` | one synthesis at a time (engine mutex), health answers concurrently | |
| Perth watermark | applied by the Python package | **not applied** | gap, section 6 |
| Ear | faster-whisper large-v3-turbo, beam 1, temp 0 | candle Whisper large-v3-turbo, greedy (= beam 1, temp 0), no `condition_on_previous_text` | `fast=1` accepted and ignored; >30 s audio is windowed |

## 4. Running it
```
galion-voice-serve --bind 127.0.0.1:8002 --models /var/lib/galion/voice-models --tts chatterbox --stt none \
    --voices /var/lib/galion/voices            # mouth (legacy :8002 contract)
galion-voice-serve --bind 127.0.0.1:8643 --models /var/lib/galion/voice-models --stt whisper-large-v3-turbo --tts none
                                               # ear (legacy :8643 contract)
```
Build: `cargo build --release -p galion-voice-cpu --features whisper,kitten,chatterbox --bin galion-voice-serve`.
Model dirs `chatterbox/` (5 files + README.md) and `whisper-large-v3-turbo/` (3 files + README.md), see
docs/third-party/voice-rust.md for URLs, revisions and sha256. Nothing was deployed anywhere.

## 5. Measured (details in docs/real-tests/voice-rust.md)
On the shared 8-vCPU build box:
- Mouth, reference settings: 25-34 s wall for 2.4-3.4 s of speech (RTF 8.9-11.5; T3 6-10 s, flow 17-22 s, HiFT 2 s).
  Fast profile (5 steps, 4 s prompt): 15.6-19.2 s (RTF 5.9-7.0). Load 4-6 s, plus ~1.2 s per reference clip. Peak RSS 4.2 GB.
- Ear (large-v3-turbo, full 30 s window): 12.7-14.0 s per utterance whatever its length. Peak RSS 4.8 GB.
- Round trip: 10 of 12 transcriptions word-perfect; the other 2 are the same line in both runs, "carry" -> "Carrie" (homophone).
- Galion One (integration-1 8fd26f6) end to end: `/hear` -> our ear -> correct text in 13.9-15.9 s (14.3 s in run 3) (its timeout is
  20 s); its mouth proxy `/synthesize` -> our mouth -> 200 WAV in 12.6 s (fast) / 20.7-21.4 s (reference) for a 2 s line
  (its timeout is 30 s), and that WAV transcribes back correctly; `/health` passes our body and status through.

## 6. Open issues and gaps (honest)
1. **CPU speed.** Chatterbox on this CPU is 6-11x slower than real time. With the reference settings a 2 s reply fits Galion One's 30 s mouth
   timeout but 3 s replies already took 34 s (run 3); the fast profile (15-19 s for 2-3 s) fits, longer replies
   (several chunks) will not. The ear takes ~13 s per call (fits the 20 s timeout, not
   pleasant). Options (need a decision, see 7): a GPU (not allowed now), quantised/bf16 weights, fewer flow steps,
   streaming chunk-by-chunk in Galion One, or keep KittenTTS/whisper-tiny for latency.
2. **Real `samantha.wav` / `jarvis.wav` missing.** The tests used stand-ins (Chatterbox built-in voice, KittenTTS).
   Conditioning demonstrably works (speaker similarity 0.87-0.91 to the reference vs 0.46-0.60 cross).
3. **No Perth watermark.** Resemble's Python package watermarks every output; this port does not (no Rust Perth
   exists; it would be its own port).
4. **Numerical parity with PyTorch not measured** (no Python allowed): validation is functional (Whisper transcripts,
   speaker similarity), not a tensor-by-tensor diff. Sampling RNG differs from torch, so outputs are not bit-identical.
5. Whisper large-v3-turbo has no `suppress_tokens` in its config (they are in `generation_config.json`, not
   downloaded); greedy decoding has been clean on the test lines. "Trimmed" encoder context hallucinates with turbo,
   so the ear always uses the 30 s window.
6. The S3Gen prompt from a reference clip is computed with our own resampler (windowed-sinc), not librosa's
   soxr_hq; small spectral differences are possible.
7. Port 19370 of this slot was occupied by another agent's `galion-console-static` during the run.

## 7. Needs Slavik's approval / input
- The real `samantha.wav` (and `jarvis.wav`) reference clips with their provenance/licence, to be placed in `--voices`.
- Whether the mouth may ship without the Perth watermark (Resemble applies it by default; the MIT licence does not
  require it).
- The `/health` deviation (503 when not ready instead of always 200).
- Which latency trade-off to take for production (section 6.1), and any deployment: nothing was deployed.
- The per-package `opt-level = 3` override in the workspace `Cargo.toml` (an exception to the size-first profile).
