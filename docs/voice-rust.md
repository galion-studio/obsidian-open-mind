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
