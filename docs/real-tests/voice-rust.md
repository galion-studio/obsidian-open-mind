<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: `swarm/voice-rust`

Box: the swarm build box (8 vCPU x86_64, 15 GB RAM, shared with other agents, so load average 3-5 during the runs).
CPU only, release binaries, no GPU, nothing on the VPS. Ports: ear 19371, mouth 19372, Galion One 19374 (+ its
mouth proxy 19375). 19370 was already bound by another agent's `galion-console-static` during the run, so the
mouth moved to 19372. Every process the scripts started was killed at the end.

Reproduce: `ONE_BIN=<galion-one from integration-1 8fd26f6> scripts/real/voice-rust.sh [models] [voices]`, and
`MOUTH_ARGS="--flow-steps 5 --prompt-seconds 4"` for the fast profile. Galion One was built from
`git archive 8fd26f6` (integration-1) into a scratch directory and started with `GALION_MOUTH=127.0.0.1:19372`,
`GALION_EAR=127.0.0.1:19371`, a temp `GALION_HOME`, and a throw-away pairing store (`ONE_DEVICE_STORE`) holding the
sha256 of a test code, so `/pair` hands out a cookie and `/hear` can be called. The mind (LLM) was not involved
(`/reply` would call it); nothing paid was called.

"words" = share of the script's words found in the transcript (case and punctuation ignored).

## Run 1: reference settings (10 flow steps, 10 s S3Gen prompt cap; the samantha stand-in clip is 6 s)

# voice-rust real test 2026-10-20 21:18:39 CEST (+02:00), 8 CPUs, load 3.72 2.43 3.20
mouth args: <defaults: flow steps 10, prompt 10 s>
## health while loading
- :19371 -> {"ok":false,"status":"loading","provider":"local","model":null,"loaded":false,"error":null,"device":"cpu","voices":[],"bind":"127.0.0.1:19371","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":null,"stt_missing":"","tts_missing":"",... HTTP 503
- :19372 -> {"ok":false,"status":"loading","provider":"local","model":null,"loaded":false,"error":null,"device":"cpu","voices":[],"bind":"127.0.0.1:19372","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":null,"stt_missing":"","tts_missing":"",... HTTP 503
both healthy after 8167 ms
## health when loaded
- :19371 -> {"ok":true,"status":"ok","provider":"local","model":"whisper-large-v3-turbo","loaded":true,"error":null,"device":"cpu","voices":[],"bind":"127.0.0.1:19371","runtime":"galion-voice-cpu (Rust, candle)","stt":"whisper-large-v3-turbo","tts":null,"stt_missing":"","tts_missing":"disabled (--tts none)",... HTTP 200
- :19372 -> {"ok":true,"status":"ok","provider":"local","model":"chatterbox","loaded":true,"error":null,"device":"cpu","voices":["samantha","jarvis"],"bind":"127.0.0.1:19372","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":"chatterbox","stt_missing":"disabled (--stt none)","tts_missing":"",... HTTP 200
    voice: stt whisper-large-v3-turbo (mit, multilingual true) loaded in 4819 ms
    voice: ready on 127.0.0.1:19371 (stt whisper-large-v3-turbo, tts none)
    voice: tts chatterbox (mit) ready in 9078 ms: model loaded in 6437 ms; samantha: reference /workspace/swarm/target/voice-rust/voices/samantha.wav (1191 ms); jarvis: reference /workspace/swarm/target/voice-rust/voices/jarvis.wav (1449 ms)
    voice: ready on 127.0.0.1:19372 (stt none, tts chatterbox)
## Samantha lines: mouth -> WAV -> ear
| # | voice | text | audio s | synth ms | RTF | ear ms | heard | words |
|---|---|---|---|---|---|---|---|---|
| 1 | samantha | Hi. I am right here with you, and I am listening. | 2.800 | 32148 | 11.479 | 12695 | Hi, I am right here with you, and I am listening. | 100% |
| 2 | samantha | That sounds like a lot to carry. Do you want to talk about it? | 3.240 | 28726 | 8.863 | 13432 | That sounds like a lot to Carrie. Do you want to talk about it? | 93% |
| 3 | jarvis | Good evening. All systems are running normally. | 2.760 | 30720 | 11.126 | 12688 | Good evening. All systems are running normally. | 100% |
| 4 | samantha | I love the way the light looks this time of day. | 2.400 | 25310 | 10.542 | 14026 | I love the way the light looks this time of day. | 100% |
## contract checks
- empty text -> {"error":"text required"} HTTP 400
- non-JSON body -> {"error":"text required"} HTTP 400
- ear without audio -> {"error":"multipart has no \"file\" part"} HTTP 400
    chatterbox: 49 chars -> 70 tokens, 2.80 s audio (t3 8353 ms, flow 21988 ms, hift 1798 ms)
    chatterbox: 62 chars -> 81 tokens, 3.24 s audio (t3 9735 ms, flow 16909 ms, hift 2069 ms)
    chatterbox: 47 chars -> 69 tokens, 2.76 s audio (t3 6706 ms, flow 22304 ms, hift 1697 ms)
    chatterbox: 48 chars -> 60 tokens, 2.40 s audio (t3 6105 ms, flow 17633 ms, hift 1560 ms)
## Galion One (integration-1 8fd26f6) end to end
- one :mouth /health pass-through -> {"ok":true,"status":"ok","provider":"local","model":"chatterbox","loaded":true,"error":null,"device":"cpu","voices":["samantha","jarvis"],"bind":"127.0.0.1:19372","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":"chatterbox","stt_missing":"disabled (--stt none)","tts_missing":"",... HTTP 200
- one :mouth /synthesize -> HTTP 200, 86444 bytes, 20704 ms (Galion One's own timeout to the mouth is 30 s)
  - that WAV through the ear: "Hello, I can hear you now."
- one /hear (paired, line 1 WAV) -> {"say":"Hi, I am right here with you, and I am listening."} in 15864 ms (Galion One's ear timeout is 20 s)
peak RSS: ear "hwm_kb":4768340, mouth "hwm_kb":4161384

## Run 2: fast profile (`--flow-steps 5 --prompt-seconds 4`)

# voice-rust real test 2026-10-20 21:22:41 CEST (+02:00), 8 CPUs, load 6.45 4.30 3.78
mouth args: --flow-steps 5 --prompt-seconds 4
## health while loading
- :19371 -> {"ok":false,"status":"loading","provider":"local","model":null,"loaded":false,"error":null,"device":"cpu","voices":[],"bind":"127.0.0.1:19371","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":null,"stt_missing":"","tts_missing":"",... HTTP 503
- :19372 -> {"ok":false,"status":"loading","provider":"local","model":null,"loaded":false,"error":null,"device":"cpu","voices":[],"bind":"127.0.0.1:19372","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":null,"stt_missing":"","tts_missing":"",... HTTP 503
both healthy after 7655 ms
## health when loaded
- :19371 -> {"ok":true,"status":"ok","provider":"local","model":"whisper-large-v3-turbo","loaded":true,"error":null,"device":"cpu","voices":[],"bind":"127.0.0.1:19371","runtime":"galion-voice-cpu (Rust, candle)","stt":"whisper-large-v3-turbo","tts":null,"stt_missing":"","tts_missing":"disabled (--tts none)",... HTTP 200
- :19372 -> {"ok":true,"status":"ok","provider":"local","model":"chatterbox","loaded":true,"error":null,"device":"cpu","voices":["samantha","jarvis"],"bind":"127.0.0.1:19372","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":"chatterbox","stt_missing":"disabled (--stt none)","tts_missing":"",... HTTP 200
    voice: stt whisper-large-v3-turbo (mit, multilingual true) loaded in 4207 ms
    voice: ready on 127.0.0.1:19371 (stt whisper-large-v3-turbo, tts none)
    voice: tts chatterbox (mit) ready in 8581 ms: model loaded in 5978 ms; samantha: reference /workspace/swarm/target/voice-rust/voices/samantha.wav (1212 ms); jarvis: reference /workspace/swarm/target/voice-rust/voices/jarvis.wav (1387 ms)
    voice: ready on 127.0.0.1:19372 (stt none, tts chatterbox)
## Samantha lines: mouth -> WAV -> ear
| # | voice | text | audio s | synth ms | RTF | ear ms | heard | words |
|---|---|---|---|---|---|---|---|---|
| 1 | samantha | Hi. I am right here with you, and I am listening. | 3.200 | 19248 | 6.012 | 13413 | Hi, I am right here with you and I am listening. | 100% |
| 2 | samantha | That sounds like a lot to carry. Do you want to talk about it? | 3.040 | 17833 | 5.863 | 13056 | That sounds like a lot to Carrie. Do you want to talk about it? | 93% |
| 3 | jarvis | Good evening. All systems are running normally. | 2.560 | 15556 | 6.073 | 13593 | Good evening, all systems are running normally. | 100% |
| 4 | samantha | I love the way the light looks this time of day. | 2.240 | 15633 | 6.974 | 13026 | I love the way the light looks this time of day. | 100% |
## contract checks
- empty text -> {"error":"text required"} HTTP 400
- non-JSON body -> {"error":"text required"} HTTP 400
- ear without audio -> {"error":"multipart has no \"file\" part"} HTTP 400
    chatterbox: 49 chars -> 80 tokens, 3.20 s audio (t3 8510 ms, flow 8673 ms, hift 2054 ms)
    chatterbox: 62 chars -> 76 tokens, 3.04 s audio (t3 8676 ms, flow 7188 ms, hift 1956 ms)
    chatterbox: 47 chars -> 64 tokens, 2.56 s audio (t3 7309 ms, flow 6786 ms, hift 1450 ms)
    chatterbox: 48 chars -> 56 tokens, 2.24 s audio (t3 7699 ms, flow 6611 ms, hift 1311 ms)
## Galion One (integration-1 8fd26f6) end to end
- one :mouth /health pass-through -> {"ok":true,"status":"ok","provider":"local","model":"chatterbox","loaded":true,"error":null,"device":"cpu","voices":["samantha","jarvis"],"bind":"127.0.0.1:19372","runtime":"galion-voice-cpu (Rust, candle)","stt":null,"tts":"chatterbox","stt_missing":"disabled (--stt none)","tts_missing":"",... HTTP 200
- one :mouth /synthesize -> HTTP 200, 96044 bytes, 12554 ms (Galion One's own timeout to the mouth is 30 s)
  - that WAV through the ear: "Hello, I can hear you now."
- one /hear (paired, line 1 WAV) -> {"say":"Hi, I am right here with you and I am listening."} in 13927 ms (Galion One's ear timeout is 20 s)
peak RSS: ear "hwm_kb":4768960, mouth "hwm_kb":4161740

## Voice conditioning check (reference clip -> output)

`cargo run --release -p galion-chatterbox --example voice-sim -- <chatterbox dir> <ref.wav> <outputs...>` computes
cosine similarity of speaker embeddings with the model's own voice encoder and the CAM++ x-vector model. Outputs are
the run 1 WAVs. Each output is much closer to its own reference than to the other voice:

| Reference | Output | voice-encoder cos | CAM++ cos |
|---|---|---|---|
| jarvis stand-in (KittenTTS male) | line 3, voice jarvis | 0.875 | 0.883 |
| jarvis stand-in | line 1, voice samantha | 0.590 | 0.455 |
| jarvis stand-in | line 4, voice samantha | 0.596 | 0.529 |
| samantha stand-in (Chatterbox built-in voice) | line 1, voice samantha | 0.910 | 0.868 |
| samantha stand-in | line 4, voice samantha | 0.890 | 0.802 |
| samantha stand-in | line 3, voice jarvis | 0.585 | 0.566 |
| (the two references against each other) | | 0.652 | 0.542 |

## Gated Rust tests (`GALION_REAL=1`)

`GALION_REAL=1 cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox --test real_chatterbox -- --test-threads 1`
on 2026-10-20 21:31 CEST (+02:00): **2 passed** in 38.9 s.
- `chatterbox_line_round_trips_through_whisper`: built-in voice, seed 1, 5 flow steps -> 73 speech tokens, 2.92 s
  audio (T3 8.7 s, flow 8.1 s, HiFT 1.9 s) -> whisper-tiny.en heard "Hello, I am Samantha. It is lovely to meet you."
- `worker_frame_protocol`: the binary in `--worker` mode wrote `R`, answered a 1-byte request with only the end
  frame, answered voice byte 1 + "All systems are running." with one PCM frame (1.60 s, 24 kHz s16le) and the end frame.

## Standalone CLI timings (`chatterbox-say`, built-in voice, seed 1, same 47-char line)
| Build | Flow steps | Wall | T3 | Flow | HiFT | Audio | RTF |
|---|---|---|---|---|---|---|---|
| workspace opt-level "z" (first run) | 10 | 48.1 s | 13.2 s | 32.3 s | 2.6 s | 2.92 s | 16.5 |
| opt-level 3 for candle/gemm/voice crates | 10 | 33.4 s | 7.9 s | 23.8 s | 1.7 s | 2.92 s | 11.4 |
| opt-level 3 | 5 | 19.5 s | 6.3 s | 11.6 s | 1.6 s | 2.92 s | 6.7 |
Same seed gave the same 73 tokens in all three runs (deterministic sampling).

Whisper large-v3-turbo in `stt-bench` on a 2.92 s clip: full 30 s window 14.4 s (correct text); "trimmed" encoder
context 2.9 s but it hallucinated repetitions, so the service keeps the full window (as trained).
