<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: `swarm/voice-mouth` (slot 06, ports 19060-19069)

All times Europe/Zagreb. Box: 8 shared vCPUs (about 21 swarm agents running, load 3.6), release builds
(opt-level z), models from `/workspace/oom-models` (verified by `scripts/voice/fetch-models.sh`: all 11 files "ok").
Every process started was stopped afterwards (checked: no listener on 19060-19069, no `galion-voice-serve` left).

## 1. Live VPS, read-only (for comparison)
2026-10-20 18:50 CEST and 19:23 CEST (+02:00), over ssh, GET only:
```
GET 127.0.0.1:8002/health -> 503 {"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}
GET 127.0.0.1:8002/{,healthz,status,voice,v1/voice,mouth,api/voice} -> 404 "not found"
GET 127.0.0.1:8643/health (ear) -> 000 (nothing listening)
systemctl show galion-one: ActiveEnterTimestamp 2026-10-18 22:13:56 CEST, NRestarts=0
/var/log/galion-one.log tail: "warm rust skipped mouth=0 mind=402" (7,303 times)
```
Root cause and fix proposal: `docs/voice-mouth-503.md`.

## 2. `scripts/real/voice-mouth.sh` (2026-10-20 19:16 CEST (+02:00))
Builds both binaries, runs galion-voice-serve on 127.0.0.1:19060 (1 and then 4 candle threads), then a patched
galion-one on 19061/19062 with and without `GALION_MOUTH=127.0.0.1:19060`. Input: the real 11.0 s JFK speech
WAV (16 kHz mono, 352,078 B). Output (verbatim):
```
# 2026-10-20 19:16:16 CEST (+02:00) on grok-bot-vm-303083841, 8 CPUs, load 3.60 3.59 3.05
## RAYON_NUM_THREADS=1
health: {"ok":true,"status":"ok","provider":"galion-voice-cpu","device":"cpu","stt":"whisper-tiny.en","tts":"kitten-tts-nano-0.8","voices":["bella","bruno","hugo","jasper","kiki","leo","luna","rosie"],"stt_missing":"","tts_missing":"","rss_kb":229644,"hwm_kb":305172} -> 200
stt #1 (11.0 s JFK WAV): {"text":"And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.","audio_s":11.00,"ms":3036,"cpu_ms":3450} -> 200 3.037180s
stt #2 (11.0 s JFK WAV): {"text":"And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.","audio_s":11.00,"ms":2924,"cpu_ms":3320} -> 200 2.924929s
stt #3 (11.0 s JFK WAV): {"text":"And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.","audio_s":11.00,"ms":2870,"cpu_ms":3270} -> 200 2.871711s
tts #1: 200 280044 B 3.426617s audio 5.83 s, wall 3425 ms, cpu 3420 ms
tts #2: 200 280044 B 3.249599s audio 5.83 s, wall 3248 ms, cpu 3250 ms
round trip /v1/voice: 200 436044 B 7.878792s stt 2803 ms, tts 5073 ms, reply audio 9.08 s, cpu 8260 ms
transcript header: And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.
process: VmHWM: 442888 kB VmRSS: 336424 kB 
## RAYON_NUM_THREADS=4
health: {"ok":true,"status":"ok","provider":"galion-voice-cpu","device":"cpu","stt":"whisper-tiny.en","tts":"kitten-tts-nano-0.8","voices":["bella","bruno","hugo","jasper","kiki","leo","luna","rosie"],"stt_missing":"","tts_missing":"","rss_kb":229772,"hwm_kb":305300} -> 200
stt #1 (11.0 s JFK WAV): {"text":"And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.","audio_s":11.00,"ms":1707,"cpu_ms":3580} -> 200 1.708761s
stt #2 (11.0 s JFK WAV): {"text":"And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.","audio_s":11.00,"ms":1696,"cpu_ms":3700} -> 200 1.697751s
stt #3 (11.0 s JFK WAV): {"text":"And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.","audio_s":11.00,"ms":1675,"cpu_ms":3630} -> 200 1.677194s
tts #1: 200 280044 B 3.121150s audio 5.83 s, wall 3120 ms, cpu 3640 ms
tts #2: 200 280044 B 3.054957s audio 5.83 s, wall 3054 ms, cpu 3660 ms
round trip /v1/voice: 200 436044 B 6.250057s stt 1687 ms, tts 4561 ms, reply audio 9.08 s, cpu 9190 ms
transcript header: And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.
process: VmHWM: 447984 kB VmRSS: 341092 kB 
## galion-one (patched) with GALION_MOUTH=127.0.0.1:19060
mouth /health: {"ok":true,"status":"ok","provider":"galion-voice-cpu","device":"cpu","stt":"whisper-tiny.en","tts":"kitten-tts-nano-0.8","voices":["bella","bruno","hugo","jasper","kiki","leo","luna","rosie"],"stt_missing":"","tts_missing":"","rss_kb":341100,"hwm_kb":447984} -> 200
mouth /synthesize: 200 215644 B 2.298410s
## galion-one (patched) without GALION_MOUTH (the live configuration)
mouth /health: {"ok":false,"status":"unavailable","provider":"unavailable","loaded":false,"reason":"no speech backend configured; set GALION_MOUTH to a galion-voice-serve address"} -> 503
# done; all started processes stopped
```

### Latency and CPU summary
| Call (input) | 1 thread: wall / CPU | 4 threads: wall / CPU | Real-time factor (4 thr) |
|---|---|---|---|
| STT, 11.0 s WAV -> text | 2.87-3.04 s / 3.3-3.5 s | 1.68-1.71 s / 3.6-3.7 s | 0.15 |
| TTS, 61 chars -> 5.83 s of 24 kHz audio | 3.25-3.43 s / 3.3-3.4 s | 3.05-3.12 s / 3.6-3.7 s | 0.53 |
| Round trip `/v1/voice` (WAV -> text -> "I heard: ..." -> 9.08 s WAV) | 7.88 s / 8.3 s | 6.25 s / 9.2 s | 0.69 |
| Through galion-one `:19062/synthesize` -> voice service | | 2.30 s (4.5 s of audio) | |
Load: Whisper 260-305 ms, KittenTTS + CMUdict 190-262 ms. Memory: RSS 230 MB idle after load, peak (VmHWM)
443-448 MB. Static musl x86_64 binary, 2 threads (19:22 CEST): STT 3.07 s wall / 4.3 s CPU, TTS 2.43 s for 2.82 s
of audio, VmHWM 362 MB. CPU ms come from `/proc/self/stat` (utime+stime) around each call (`X-Cpu-Ms`).

Intelligibility check: our TTS output for "Hello Slavik. The mouth on port eight thousand and two can speak again,
on the CPU only." was fed back into our STT, which heard "Hello Slavic. The mouth on port 8,000 and two can speak
again on the CPU only." (19:11 CEST).

## 3. Rust real test (gated)
`GALION_REAL=1 cargo test --release -p galion-voice-cpu --features whisper,kitten --test real_voice_serve`
(19:15 CEST): starts the real binary on 127.0.0.1:19063, `GET /health` 200, real WAV in -> text contains
"ask not what your country can do for you", text in -> RIFF WAV out (140,044 B), kills the process.
```
voice: stt whisper-tiny.en (apache-2.0) loaded in 260 ms
voice: tts kitten-tts-nano-0.8 (apache-2.0) + cmudict (bsd-2-clause) loaded in 190 ms
voice: POST /v1/audio/transcriptions -> 200 (352078 B in, 157 B out)
voice: POST /synthesize -> 200 (39 B in, 140044 B out)
test real_wav_in_text_out_and_text_in_wav_out ... ok
```

## 4. Behaviour comparison: live vs this branch
| Request | Live VPS (ws/ui binary) | Branch, no GALION_MOUTH | Branch, GALION_MOUTH -> galion-voice-serve |
|---|---|---|---|
| `GET :8002/health` | 503 `"loading"`, claims chatterbox/cuda | 503 `"unavailable"` + reason | 200 with the voice service's real JSON |
| `POST :8002/synthesize` | 500 `{"error":"voice"}` | 500 `{"error":"voice"}` | 200 audio/wav 24 kHz |
| Voice service down | n/a | n/a | 503 `"unreachable"` (tested with 127.0.0.1:19068) |

## 5. Builds
x86_64 and aarch64 musl release builds OK (rust-lld): galion-voice-serve 1,840,616 / 1,371,280 B (weights
separate), galion-one 822,824 / 732,680 B. `cargo check` OK on aarch64-linux-android and
armv7-unknown-linux-musleabihf (galion-voice-cpu with whisper+kitten, galion-one, openmind-speech).
aarch64 speed is not measured (no aarch64 device on the box).

> **2026-10-21 (realscript-port-3):** the shell driver for this evidence was ported to Rust and deleted; re-run with the matching test in `crates/galion-realtest/tests/` (see `docs/real-tests/realscript-port-3.md`).
