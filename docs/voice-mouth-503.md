<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Why the mouth (127.0.0.1:8002) answers 503, and the proposed fix

Written 2026-10-20 19:25 CEST (+02:00) by swarm slot 06 (`swarm/voice-mouth`).
The VPS was only probed read-only (`systemctl status/cat/show`, `ss -ltn`, `journalctl`, a read of
`/var/log/galion-one.log`, HTTP GETs). Nothing on the VPS was changed or restarted.
**The live fix below needs Slavik's approval.**

## 1. What the VPS shows (2026-10-20 18:50 and 19:23 CEST)
- `galion-one.service` active since 2026-10-18 22:13:56 CEST, 0 restarts, pid 1373612,
  `ExecStart=/opt/galion/openmind/dist/bin-ws/galion-one` (sha256 prefix 090c15d0: the ws/ui musl build).
  It listens on 127.0.0.1:8091 (text) and 127.0.0.1:8002 (mouth). The unit sets no `GALION_MOUTH` and no `GALION_EAR`.
- `GET 127.0.0.1:8002/health` -> **503** `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}`.
  Every other GET path on 8002 is 404 (only `/health` and `POST /synthesize` exist).
- `journalctl -u galion-one`: only the start line. stdout/stderr go to `/var/log/galion-one.log`.
- `/var/log/galion-one.log`: 7,780 Python tracebacks `tts/worker.py ... ModuleNotFoundError: No module named 'torch'`
  plus `voice: failed to fill whole buffer`, all from the binaries before the current one; since the current start
  there is only `voice: listening` and 7,303 lines of `warm rust skipped mouth=0 mind=402`.
- No GPU (`nvidia-smi` absent); CPU is an AMD Ryzen 7 3800X (8 cores, AVX2), 16 GB RAM.
- Nothing listens on 8643 (the ear) or 8003.
- galion-nerve (8670) marks `galion-one-mouth` (AETHER) as **degrading: health probe HTTP 503**.

## 2. Root cause
1. **There is no text-to-speech backend at all.** B14 removed the spawn of the Python Chatterbox worker
   (`galion-light/tts/worker.py`, which needed torch + CUDA; torch is not installed and the VPS has no GPU, so it
   never worked here either: the 7,780 tracebacks). What is left in `crates/galion-one/src/speech.rs` is the
   `Mouth` trait with `local()` hard-wired to the `Unavailable` stub, whose `ready()` is always false.
2. **`/health` was misleading.** `tts_http` in `crates/galion-one/src/main.rs` answered a hard-coded
   Chatterbox/CUDA body, so with the stub it reports `"status":"loading"` forever. The 503 is permanent, not a warm-up.
3. **Even a remote mouth would not clear it.** `GALION_MOUTH` already sends `POST /synthesize` to another service,
   but the old `/health` only looks at `speech::local().ready()`, so the live binary would still answer 503 with a
   working remote mouth.
4. Separate, not the 503: the keep-warm ping to Hermes (8642) gets **HTTP 402** (`mind=402`, payment required),
   so even with a mouth, a full voice turn would fail at the "mind" step until the OpenRouter credit question is settled.

## 3. What this branch changes (code only, nothing deployed)
- `galion-one`: `:8002 /health` tells the truth. With `GALION_MOUTH` set it passes the remote service's `/health`
  through (one GET, 3 s timeout; 503 `"status":"unreachable"` if it is down). Without it: 503
  `"status":"unavailable"` with the reason. No more fake Chatterbox/CUDA body.
- New `galion-voice-serve` (crate `galion-voice-cpu`, merged from `vps/ws-voice` 202bde2): a std-only HTTP service
  that implements both Galion One contracts, the ear (`POST /v1/audio/transcriptions`, multipart or raw WAV ->
  `{"text":...}`) and the mouth (`POST /synthesize` `{"text":...}` -> 24 kHz WAV), plus `/v1/voice` (round trip)
  and `/health`. Engines sit behind the `SpeechToText` / `TextToSpeech` traits: Whisper tiny.en (STT) and
  KittenTTS nano 0.8 with our own licence-clean G2P (TTS), on the CPU via candle. It never downloads anything and
  loads a model directory only when its declared licence is on the allow list.
- Box measurements: STT 1.7 s for 11 s of speech (4 threads), TTS 3.1 s for 5.8 s of audio, peak RSS ~450 MB
  (docs/real-tests/voice-mouth.md).

## 4. Proposed live fix (AWAITING SLAVIK'S APPROVAL; not done)
Decisions needed first:
- **A. SIMD exception.** candle's matmul (`gemm`, `pulp`, `raw-cpuid`) picks AVX2/NEON kernels at run time. Our code
  uses no intrinsics and no target-cpu flags, but RULES.md says "no SIMD". Without this exception there is no
  usable CPU speech path today (a scalar-only matmul is not written or measured). The features are opt-in
  (`whisper`, `kitten`); default builds do not pull candle.
- **B. A new unit** (`galion-voice.service`) and about 215 MB of model files in `/var/lib/galion/voice-models`.
- **C. One galion-one restart** to load the new binary and two env lines.

Steps, one service at a time, after the branch is merged to main:
1. On the box: `scripts/voice/fetch-models.sh <dir>` (pinned revisions, sha256-checked), and build static musl
   from merged main: `cargo build --release --target x86_64-unknown-linux-musl -p galion-voice-cpu --features whisper,kitten --bin galion-voice-serve`
   and `-p galion-one`. No build on the VPS.
2. Copy the models to `/var/lib/galion/voice-models/` and the binary to `/opt/galion/rust/bin/galion-voice-serve`.
3. Install and start this unit (new service, nothing live replaced):
   ```ini
   [Unit]
   Description=Galion voice (CPU STT + TTS for Galion One): 127.0.0.1:8003
   After=network-online.target
   [Service]
   ExecStart=/opt/galion/rust/bin/galion-voice-serve --bind 127.0.0.1:8003 --models /var/lib/galion/voice-models
   Environment=RAYON_NUM_THREADS=2
   Nice=10
   CPUQuota=200%
   MemoryMax=768M
   Restart=always
   RestartSec=5
   NoNewPrivileges=true
   ProtectSystem=strict
   ProtectHome=true
   PrivateTmp=true
   [Install]
   WantedBy=multi-user.target
   ```
   Check: `curl -s 127.0.0.1:8003/health` -> 200 `"status":"ok"`.
4. galion-one drop-in `/etc/systemd/system/galion-one.service.d/voice.conf`:
   `Environment=GALION_MOUTH=127.0.0.1:8003` and `Environment=GALION_EAR=127.0.0.1:8003`, then install the new
   galion-one with `/opt/galion/rust/bin/galion-deploy galion-one <new-binary> http://127.0.0.1:8091/health`
   (backup, atomic install, 20 s health check, automatic rollback).
5. Check once (no loop): `8091/health` stays `ok`; `8002/health` -> 200 with the voice service's JSON; the nerve
   row `galion-one-mouth` turns healthy.
6. Rollback: remove the drop-in and `galion-deploy`'s backup restores the old binary; `systemctl disable --now
   galion-voice` removes the new service. The old state is exactly today's 503.

Cheaper partial option: only steps 1-3 plus the drop-in on the **old** binary would make `POST /synthesize` work,
but `/health` would keep answering 503 (root cause 3), so the nerve would still flag it.
