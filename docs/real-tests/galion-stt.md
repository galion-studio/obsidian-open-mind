<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: galion-stt

Written 2026-10-29 09:25 CET (+01:00), Europe/Zagreb. Branch `swarm/galion-stt`. Moonshine Tiny Streaming
(English) only; no other engine was compared (owner steer).

## Box: `cargo test --release -p galion-stt --test real_stt` (GALION_REAL=1, ports 19651-19652)

Run with `GALION_STT_LIB_DIR`, `GALION_STT_MODEL_DIR` and `GALION_STT_WAV=jfk.wav`, and `-- --test-threads 1`.
The last 3 of 3 runs passed (4 passed, 0 failed each), 2026-10-29 09:12 CET (+01:00).

- **`real_engine_transcribes_jfk`:** the engine directly, in 100 ms chunks.
  - Transcript: "And so my fellow America, Ask not. What your country can do for you? Ask what you can do for your
    country."
  - WER 4.5 % (one word, "America" vs "Americans").
- **`real_ws_stream_jfk_and_caps`:** the release binary serving on 127.0.0.1:19651.
  - Real-time WS session: first partial at about 680 ms, the same transcript, WER 4.5 %, close 1000, line numbers
    never go backwards.
  - Two held sessions, then a third: the third gets `busy…` and close 1013, and the two finish with 1000.
  - An odd-length frame gives a non-fatal error, then a normal close.
  - `/health` afterwards shows `sessions_active` 0.
- **`real_refuses_non_loopback_bind`:** `--bind 0.0.0.0:19652` exits non-zero with "loopback only".
- **`wer_math`:** checks the WER helper.
- **Pass bar:** WER ≤ 25 % against the reference plus the anchor words "fellow", "ask not" and "country".

## Box finding: multi-thread ONNX Runtime is not reliable

On the box, 12 identical fast runs of `galion-stt transcribe jfk.wav` used ORT's default thread pools:

- 7 runs were correct.
- 5 were garbled, e.g. "I I and I I I…", "…air for what you can do for for you…", "…ispecies…".

12 runs with `MOONSHINE_ORT_SINGLE_THREAD=1` were all correct and identical. The service now always runs
single-threaded ORT.

## VPS (8 vCPU Ryzen 7 3800X, load average about 21-23 during the runs), 2026-10-29 09:17-09:22 CET (+01:00)

All runs used the installed binary and files. The engine benches ran as user `galion-stt` under the unit's limits
(`systemd-run -p CPUQuota=300% -p MemoryMax=1G -p Nice=5`). The WS runs went against the live `galion-stt.service`.

| Measure | jfk.wav (11.0 s) | two_cities (44.4 s) |
|---|---:|---:|
| RTF, real-time WS session (engine ms / audio ms) | 0.42 (4603 / 11000) | 0.48 (21281 / 44374) |
| RTF, fast bench (one decode per 300 ms of audio, back to back) | 0.64 | 0.87 |
| Time to first partial, WS client, from the first audio frame | 0.80 s (0.70 s in later runs) | 1.68 s (first word starts later in the clip) |
| Final after VAD end of speech (real-time bench, 3 lines) | 0.26 / 0.58 / 0.59 s | — |
| Stop → last final, WS client | 0.16–0.30 s | VAD final already sent 1.7 s before stop |
| Largest single decode call | 0.46–0.58 s | 0.62 s (1.41 s in the fast bench) |
| Model load per transcriber | 0.15 s (2 loaded at start in 0.32 s) | |
| Dropped audio (overload) | 0 ms | 0 ms |

Memory (service with 2 preloaded transcribers):

| State | RSS |
|---|---:|
| Idle after start | 258 MB |
| During 1 session | peak 324 MB |
| During 2 concurrent sessions | peak 399 MB (VmHWM), cgroup MemoryPeak 368 MB |
| Idle after sessions | 283–317 MB (cgroup MemoryCurrent 283 MB) |
| One engine alone (bench) | 155 MB loaded, 200–210 MB peak |

All of these are well under `MemoryMax=1G`.

Transcript, VPS, real-time WS:

- **jfk:** "And so my fellow America(ns), Ask not. What your country can do for you? Ask what you can do for your
  country."
  - In 7 of 7 sessions it differed only in "America"/"Americans" and punctuation.
- **two_cities:** "It was the best of times. it was the worst of times. It was the age of wisdom, It was the age of
  foolishness, It was the epoch of belief, It was the epoch of incredulity. It was the season of light, It was the
  season of darkness. It was the spring of hope, it was the winter of despair. We had everything before us. We had
  nothing before us. We were all going direct to heaven, We were all going to act the other way. In short,, the
  period was so far like the present period, That some of its noisiest authorities insisted on its being received
  for good or for evil in the superlative degree of comparison only."
  - Against the text, the only clear error is "going to act the other way" for "going direct the other way" (about
    1 % of words).

Session cap: with two sessions busy, a third got `busy: all 2 dictation sessions are in use` and close 1013.

## VPS privacy lock-down

- `runuser -u galion-stt -- curl -m 5 https://example.com` → exit 6 (could not resolve).
- The same against a public IP → exit 28 (timeout).
- Root curl → 200, which shows the network works and the block is per-user.
- The nft counter on `galion_stt_egress` showed 9 packets dropped (only from those curls), and nothing more after the
  sessions.
- `curl http://127.0.0.1:8651/health` → 200.
- The only listening socket is 127.0.0.1:8651.
- strace on the service (`connect,sendto,sendmsg,socket`, all threads) during a full WS session showed:
  - 0 `connect` and 0 `socket` calls;
  - only `sendto` on the one accepted loopback client socket (the partial/final JSON).
- The journal holds counts and timings only, for example `session 1 end: wall_ms=11258 audio_ms=11000 chunks=550
  dropped_ms=0 … partials=14 finals=4 first_text_ms=736 stop_to_final_ms=274`. It holds no text.
- One ORT start-up warning is logged: it failed to read `/sys/class/drm/card0/device/vendor` (GPU discovery). This is
  a local file read and is harmless.

## Through the console proxy (VPS)

| Port | `/api/stt/health` | WS `/api/stt/stream` with jfk.wav |
|---|---|---|
| console-rust-primary :18080 | 200 `{"ok":true,…,"service":"galion-stt","sample_rate":16000}` | first partial 0.72 s, stop → final 0.16 s, 4 finals, close 1000, correct text |
| console :8080 | 200 (same body) | first partial 0.69 s, stop → final 0.16 s, 4 finals, close 1000, correct text |

`/hermes` returns 200 on both ports after the restarts. `tuner.service` stays active on 8650 and was not touched.
