<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-stt: live dictation for the Hermes research page

Written 2026-10-29 09:25 CET (+01:00), Europe/Zagreb. Branch `swarm/galion-stt`, owner task t362u.

galion-stt is a small Rust service that turns live microphone audio into text while you speak.
It runs on the VPS, listens on loopback only, and the console proxies the browser's
`/api/stt/health` and `/api/stt/stream` to it (`--stt 127.0.0.1:8651` on both console units).

- **Engine:** Moonshine Tiny Streaming, English (moonshine-ai, MIT code and MIT weights), through the official
  libmoonshine C API (v0.1.5, header version 30000). The library is opened at run time (`libloading`), so the crate
  builds and unit-tests anywhere without the native files.
- **Later goal:** a pure-Rust candle port of the Tiny Streaming model, which would drop the C++/ONNX Runtime
  shared libraries. Not started; the C API path is the shipped one.

## Address and protocol

| Item | Value |
|---|---|
| Bind | `127.0.0.1:8651` (8650 is the ukulele tuner). The binary refuses any non-loopback bind. |
| `GET /health` | `200` JSON: `status`, `service`, `engine`, `sample_rate` 16000, `format` `pcm16le-mono`, `sessions_active`, `sessions_max`, `sessions_total`, `sessions_refused`, `partial_ms`, `max_session_secs` |
| `WS /stream` | One dictation per connection. |

Client → server:

- **Binary frames:** 16 kHz, mono, signed 16-bit little-endian PCM. Each frame is 20–100 ms; the accepted range is
  10–250 ms (160–4000 samples). An odd byte count or a frame outside that range gets a non-fatal `error` and is
  skipped.
- **Text `{"type":"stop"}`:** sent once, at the end. The server flushes, sends the remaining finals, then closes
  normally (1000).

Server → client (text JSON):

- `{"type":"partial","line":N,"text":"..."}` — the current guess for line N, about every 300 ms while new audio arrives.
- `{"type":"final","line":N,"text":"..."}` — line N is done. A final comes when Moonshine's VAD sees the end of
  speech (silence), or at stop. Line numbers start at 0 and only go up. A partial for line N can be followed by
  more partials, then exactly one final.
- `{"type":"error","message":"..."}` — non-fatal unless followed by a close. Cases:
  - `busy: …` + close 1013: both sessions are in use.
  - `overload: …`: the engine fell behind and the oldest queued audio was dropped.
  - `session time limit …` / `session audio limit …` (10 min): finishing.
  - Bad frame or bad text.
- **Close codes:** 1000 when done; 1013 when busy; 1011 for an internal error.

## Design

- **Server:** axum 0.8 + tokio. Each session gets one preloaded transcriber from a pool of 2 (the pool is the session
  cap) and one dedicated `std::thread` for the heavy work.
- **Queue:** the WebSocket side feeds a bounded queue (256 chunks, drop-oldest, with a warning in the log and to the
  client), so a slow engine never blocks the socket.
- **Partials:** every 300 ms (`--partial-ms`); Moonshine's own `transcription_interval` is 0.2 s.
- **Stop:** feeds 100 ms of silence, then `stop_stream` + `transcribe_stream`. Without the silence, libmoonshine skips
  its end-of-segment re-decode when the last partial tick already consumed all audio, and the final would just
  repeat the weaker partial (seen as "for the U." instead of "for your country").
- **ONNX Runtime threads:** always one per session (`MOONSHINE_ORT_SINGLE_THREAD=1`, set by the binary and the unit).
  With ORT's default thread pools, libmoonshine 0.1.5 gave garbled text (e.g. "I I I I…", "…ispecies…") on 5 of 12
  identical runs of the same clip on the box; single-thread gave the same correct text 12 of 12 times. Single-thread
  also suits the loaded VPS.
- **Privacy:**
  - No audio or transcript is ever written to disk.
  - `save_input_wav_path` is never set; `log_output_text`, `log_ort_run` and `return_audio_data` are explicitly off.
  - Logs are one start line and one end line per session, with counts and timings only.
- **Pinning:** every library and model file is sha256-checked at start (table below). There are no downloads at build
  or run time.

## Pinned files (checked at every start)

Models: `https://download.moonshine.ai/model/tiny-streaming-en/quantized_26_08_21/` (8-bit, 2026-08-21; the
manifest's CRC32C values also match). Libraries: GitHub release `moonshine-voice-linux-x86_64.tar.gz` (v0.1.5,
tarball sha256 `9c3a87fea93ff2ad957938868f95a0a366dce9ff8ad86bde6cdcf5a4cadb51df`); its `libonnxruntime.so.1` is
byte-identical to Microsoft's `onnxruntime-linux-x64-1.23.2.tgz` (sha256
`1fa4dcaef22f6f7d5cd81b28c2800414350c10116f5fdd46a2160082551c5f9b`).

| File | Bytes | sha256 |
|---|---:|---|
| lib/libmoonshine.so | 12157928 | 9d516b03b178f01b6f986ae29a6d1b9adc8f49bb6e4d202672b22c0181cc9e00 |
| lib/libonnxruntime.so.1 (ORT 1.23.2) | 22326072 | 13ab8084954fa4a47c777880180b90810d6020f021441395712b48a75b74c68b |
| models/adapter.ort | 1319664 | 22ecc949e146c49667fda28d102d4e30749a107dc88a396292aa8f277ef1347c |
| models/cross_kv.ort | 1287544 | 143a36667b8d05fd9d04e8c337b7ee121f37ef299aea6b3d82bdb3d3401950b4 |
| models/decoder_kv.ort | 32583720 | 8852553f312adb6c9aa4d17418015049b30f412209ee569d336548c0044627de |
| models/encoder.ort | 7675440 | a8414e1a5dedf9f2093d7680601dd8a9b0433e7020260eafe0e370ead91134ca |
| models/frontend.model.ort | 23344 | 5121b561417b638afce0c6c31b760e37c93cf97f80d9b0031aad1fe7b6f25d61 |
| models/frontend.weights.ort | 2093464 | 217da24ac6f522ebf02da8ef288e77d1ac68d50d4a6821433182e4fbf4204bbd |
| models/streaming_config.json | 509 | 74fe5ddebd63b17caf59e8a3b18c17547ff7bce1642050edbb1c3962674f8950 |
| models/tokenizer.bin | 249974 | 6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d |

The installed binary `bin/galion-stt` (glibc x86_64, needs ≤ GLIBC_2.34) has sha256
`13ab8eb6eb23e0fede620d2167ccacee590893f20b7c643471902d21ed1a0bfd`. The VPS keeps a list in
`/opt/galion/stt-moonshine/SHA256SUMS`.

## Install layout (VPS)

- **Files:**
  - `/opt/galion/stt-moonshine/{bin,lib,models}` are root-owned. Files are 0444 (the binary is 0755), and
    `lib/` and `models/` are 0555.
- **User and unit:**
  - System user `galion-stt` (nologin, no home).
  - Unit `galion-stt.service` (copy in `crates/galion-stt/deploy/`):
    - Env: offline/no-telemetry variables, `MOONSHINE_VOICE_CACHE`, single-thread ORT.
    - Hardening: `ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `NoNewPrivileges`, an empty
      capability set, `RestrictAddressFamilies`.
    - Limits: `MemoryMax=1G`, `CPUQuota=300%`, `Nice=5`.
- **Egress lock:**
  - nftables table `inet galion_stt_egress` with rule `meta skuid "galion-stt" oif != "lo" counter drop`.
  - It lives in `/etc/nftables.d/galion-stt-egress.nft`, is included from `/etc/nftables.conf`, and is reloaded by
    the unit's `ExecStartPre` (fail closed).
  - `IPAddressDeny` is also set but is not enforced in this LXC, so the nft rule is the real lock.
- **Console wiring:**
  - `--stt 127.0.0.1:8651` is in both `63-hermes-ui-rebuild-a69e0e35.conf` drop-ins (console-rust-primary :18080
    and console :8080).

## Rollback

1. **Console wiring:** restore each `63-hermes-ui-rebuild-a69e0e35.conf` from its `.bak-galion-stt-20261029` next to
   it, then `systemctl daemon-reload && systemctl restart console-rust-primary` (and then `console`).
2. **Service:** `systemctl disable --now galion-stt`.
3. **Egress table:** `nft delete table inet galion_stt_egress`, then restore `/etc/nftables.conf` from
   `/etc/nftables.conf.bak-galion-stt-20261029T0918` (it only adds the include line).
4. **Only with an owner yes:** remove `/etc/systemd/system/galion-stt.service`,
   `/etc/nftables.d/galion-stt-egress.nft`, `/opt/galion/stt-moonshine`, and `userdel galion-stt`.
