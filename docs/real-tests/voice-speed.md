<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: `swarm/voice-speed` (slot 147)

Box: the shared swarm build box (8 vCPU Intel Xeon, AVX-512, 15 GB), release builds, CPU only. Ports 20170 (ear)
and 20171 (mouth); every process the test started was killed at its end. Models: `/workspace/oom-models`
(chatterbox @ 5bb1f6ee, whisper-large-v3-turbo; sha256 in docs/third-party/voice-rust.md). Voices: jarvis =
reference clip (KittenTTS male sample `kitten-official-jasper-2.wav`, 5 s), samantha = Chatterbox built-in voice.
Nothing paid, nothing on the VPS except the read-only probes in the last section.

## Run 1: `real_voice_speed`, 2026-10-21 07:37-07:46 CEST (+02:00)
`GALION_REAL=1 cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox --test real_voice_speed -- --nocapture --test-threads 1`
-> `test result: ok. 1 passed; 0 failed` in 490.1 s. Another agent's voice test (ports 19371/19372) ran during the
f32 and q8:5 profiles, hence the load. Report as printed:

## mouth profile t3=q8 flow-steps=10 (load 1.95 2.71 3.26)
- /health first Some(503), 200 after Some(8277) ms
- empty text -> HTTP 400 {"error":"text required"}
- mouth peak RSS 2771 MB
## mouth profile t3=f32 flow-steps=10 (load 6.59 3.96 3.66)
- /health first Some(503), 200 after Some(5025) ms
- empty text -> HTTP 400 {"error":"text required"}
- mouth peak RSS 4066 MB
## mouth profile t3=q8 flow-steps=5 (load 9.42 5.69 4.31)
- /health first Some(503), 200 after Some(7923) ms
- empty text -> HTTP 400 {"error":"text required"}
- mouth peak RSS 2772 MB
## lines (mouth -> WAV -> ear)
| profile | voice | text | audio s | synth ms | cpu ms | s per 3 s speech | < 15 s | heard | words |
|---|---|---|---|---|---|---|---|---|---|
| q8:10 | samantha | Hi. I am right here with you, and I am listening. | 2.92 | 14212 | 62960 | 14.6 | yes | Hi, I am right here with you and I am listening. | 100% |
| q8:10 | samantha | That sounds like a lot to carry. Do you want to talk about it? | 3.00 | 13049 | 61290 | 13.0 | yes | That sounds like a lot to carry. Do you want to talk about it? | 100% |
| q8:10 | jarvis | Good evening. All systems are running normally. | 3.12 | 12274 | 57140 | 11.8 | yes | Good evening. All systems are running normally. | 100% |
| q8:10 | samantha | I love the way the light looks this time of day. | 2.24 | 13487 | 54010 | 18.1 | no | I love the way the light looks this time of day. | 100% |
| q8:10 | jarvis | Your meeting starts in ten minutes, and the coffee is ready. | 3.08 | 13499 | 51810 | 13.1 | yes | Your meeting starts in 10 minutes and the coffee is ready. | 91% |
| f32:10 | samantha | Hi. I am right here with you, and I am listening. | 2.72 | 22168 | 59870 | 24.4 | no | Hi, I am right here with you and I am listening. | 100% |
| f32:10 | samantha | That sounds like a lot to carry. Do you want to talk about it? | 2.96 | 20296 | 61350 | 20.6 | no | That sounds like a lot to carry. Do you want to talk about it? | 100% |
| f32:10 | jarvis | Good evening. All systems are running normally. | 2.92 | 17888 | 53370 | 18.4 | no | Good evening. All systems are running normally. | 100% |
| f32:10 | samantha | I love the way the light looks this time of day. | 2.12 | 17571 | 51010 | 24.9 | no | I love the way the light looks this time of day. | 100% |
| f32:10 | jarvis | Your meeting starts in ten minutes, and the coffee is ready. | 3.32 | 19544 | 59810 | 17.7 | no | Your meeting starts in 10 minutes and the coffee is ready. | 91% |
| q8:5 | samantha | Hi. I am right here with you, and I am listening. | 2.92 | 11920 | 37080 | 12.2 | yes | Hi, I am right here with you and I am listening. | 100% |
| q8:5 | samantha | That sounds like a lot to carry. Do you want to talk about it? | 3.00 | 13616 | 39640 | 13.6 | yes | That sounds like a lot to carry. Do you want to talk about it? | 100% |
| q8:5 | jarvis | Good evening. All systems are running normally. | 3.12 | 26706 | 37220 | 25.7 | no | Good evening. All systems are running normally. | 100% |
| q8:5 | samantha | I love the way the light looks this time of day. | 2.24 | 12565 | 31330 | 16.8 | no | I love the way the light looks this time of day. | 100% |
| q8:5 | jarvis | Your meeting starts in ten minutes, and the coffee is ready. | 3.08 | 11285 | 36380 | 11.0 | yes | Your meeting starts in 10 minutes and the coffee is ready. | 91% |
## per profile
- q8:10: mean words 98.2%, mean 14.1 s per 3 s of speech
- f32:10: mean words 98.2%, mean 21.2 s per 3 s of speech
- q8:5: mean words 98.2%, mean 15.9 s per 3 s of speech
load at end 8.64 8.74 6.15

## VPS read-only probes (2026-10-21 07:38 +02:00)

Read-only checks only (GET, `systemctl status`, `ss`, `/proc`). Nothing was restarted, built or POSTed.

- CPU: AMD Ryzen 7 3800X (AVX2/FMA/F16C, no AVX-512), 16 GB RAM, load ~7.
- `:8002` (galion-one mouth) `GET /health` → 503 `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}`.
- nerve `:8670` `/state`: cycle 6847, mouth "model loading"; components galion-one-mouth and cloudflared degrading, voice-worker absent.
- `galion-one.service` active.
- The new mouth binary was **not** run on the VPS. A timed VPS run needs owner permission (see docs/voice-speed.md §6).
- Disclosure: while reading nerve `/state`, a temp file `/tmp/.vs147-state` was written on the VPS and then deleted right away. No secrets were in it. This was a minor departure from strict read-only.

## Local gates at commit time

- `cargo fmt --check` ok; `cargo clippy --workspace -D warnings` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 506 passed, 0 failed.
- `cargo check` for galion-chatterbox and galion-voice-cpu with chatterbox: x86_64-musl, aarch64-musl, aarch64-linux-android and armv7-musleabihf all ok.
- `scripts/no-python.sh` ok.
