# galion-stt build and install — 2026-10-29 (Europe/Zagreb, CET +01:00)

Owner task t362u ("build and install the stt and connect to the Hermes research dictations").
Finished 09:30 CET (+01:00).

## What was built

- **Crate:** a new Rust crate and binary, `galion-stt`, in galion-studio/obsidian-open-mind.
  - Branch `swarm/galion-stt`, commit `e1dafdb172bc246be3570b5db8ae5e3cc7961db9`, pushed with push-branch.sh
    (no force).
  - **Base:** 6966c8da. This is the swarm's tree-equal copy of GitHub main 7ba85b60 (same tree a6f6c163), because
    main's signed commits are not on the box. The tree matches main, but the commit graph differs.
  - **Not merged:** CI status for the branch is unknown.
- **Service:** axum/tokio WebSocket service.
  - **Engine:** Moonshine Tiny Streaming English (MIT code and weights, 8-bit files quantized_26_08_21), called
    through the official libmoonshine C API v0.1.5. The library is loaded with dlopen, so CI builds without it.
  - **Planned later:** a pure-Rust candle port (noted in the docs).
  - **Pinning:** libmoonshine, ONNX Runtime 1.23.2 (byte-identical to Microsoft's release) and the 8 model files are
    sha256-pinned and checked at every start. Full list in docs/galion-stt.md.
- **Design:**
  - 2 preloaded transcribers act as the cap of 2 sessions. Each session has its own engine thread behind a bounded
    drop-oldest queue.
  - Sessions are capped at 10 min, the service binds to loopback only, and partials come about every 300 ms.
  - No audio or text is written to disk or logs; logs carry counts and timings only.
- **Two engine findings, both fixed:**
  1. With ONNX Runtime's default thread pools, libmoonshine gave garbled transcripts on 5 of 12 identical runs.
     Single-thread was correct 12 of 12 times, so the service now always runs single-threaded ORT.
  2. If the last partial had already used up all the audio, libmoonshine skipped its final re-decode at stop. The
     final then repeated the weaker partial. The service now feeds 100 ms of silence before stop.
- **Checks:**
  - Unit tests: 30 (protocol, audio conversion, queue, line tracker, pins, FFI layout).
  - Real tests: tests/real_stt.rs, gated by GALION_REAL=1 on ports 19651-19652.
  - `cargo fmt --check` and `cargo clippy --workspace --all-targets -D warnings` are clean. The full workspace
    tests pass after adding galion-stt to the Gaia ownership registry (counts 73→74 and 61→62).
  - repo-check: PASS. The dependency licences are all on the allowed list.

## Port and protocol (for the Hermes page / console)

- **Address:** `127.0.0.1:8651` (8650 is the ukulele tuner, which was not touched).
- **Health:** `GET /health` returns 200 JSON.
- **Stream:** `WS /stream`.
  - Client sends binary 16 kHz mono PCM16LE frames of 20–100 ms (10–250 ms accepted), then text `{"type":"stop"}`.
  - Server sends `{"type":"partial","line":N,"text":…}` about every 300 ms, and `{"type":"final","line":N,"text":…}`
    at end of speech or stop.
  - Server sends `{"type":"error","message":…}` for: busy (then close 1013), overload drops, time limit, or a bad
    frame.
  - After stop: the remaining finals, then close 1000.
- **Console wiring:** both consoles have `--stt 127.0.0.1:8651`. Both ports proxy `/api/stt/health` and
  `/api/stt/stream` and were tested end to end:
  - console-rust-primary :18080: health 200; the WS stream gives the correct transcript, close 1000.
  - console :8080: health 200; the WS stream gives the correct transcript, close 1000.
  - `/hermes` returns 200 on both ports.

## Speed and memory on the VPS (load average about 21–23)

- **Real-time factor:**
  - Real-time sessions: 0.42 (11 s clip) and 0.48 (44 s clip).
  - Fast offline bench under the unit's limits: 0.64 and 0.87.
- **Latency:**
  - First partial: about 0.7–0.8 s after the first audio frame.
  - Final after the speaker pauses (VAD): 0.26–0.59 s.
  - Final after stop: 0.16–0.30 s.
  - Two concurrent sessions: the same latencies, and no audio dropped.
- **Memory (RSS):**
  - 258 MB idle (2 engines loaded).
  - Peak 324 MB with 1 session and 399 MB with 2 (cgroup peak 368 MB).
  - Limit is MemoryMax=1G.

## Transcript test

- **JFK clip** ("ask not what your country…"):
  - Box: WER 4.5 %. The only error is "America" for "Americans".
  - VPS: the same in 7 of 7 sessions, apart from punctuation and "Americans" being correct in some.
- **44 s LibriVox clip** (A Tale of Two Cities): about 1 % word errors ("going to act" for "going direct").
- **Session cap:** a third session while two were busy got "busy" and close 1013.

## Privacy lock-down (verified)

- **User and unit:**
  - System user `galion-stt`.
  - `galion-stt.service` sets HF_HUB_OFFLINE, TRANSFORMERS_OFFLINE, HF_HUB_DISABLE_TELEMETRY, DO_NOT_TRACK and
    MOONSHINE_VOICE_CACHE.
  - Hardening: ProtectSystem=strict, ProtectHome, PrivateTmp, PrivateDevices, NoNewPrivileges, an empty capability
    set, and RestrictAddressFamilies.
  - Limits: MemoryMax=1G, CPUQuota=300%, Nice=5.
- **Files:** models and libraries are root-owned and read-only (0444 files, 0555 dirs).
- **Egress lock:**
  - nftables table `inet galion_stt_egress` with `meta skuid "galion-stt" oif != "lo" counter drop`.
  - Stored in /etc/nftables.d/galion-stt-egress.nft and included from /etc/nftables.conf (backed up first). The unit
    reloads it before every start, so a failed load stops the service from starting.
  - No other firewall rules were touched.
- **Verified:**
  - As `galion-stt`, `curl https://example.com` failed (exit 6) and a direct-IP curl failed (exit 28). As root,
    curl worked (200).
  - The nft counter dropped 9 packets, all from those curl tests.
  - Localhost health: 200. The only listener is 127.0.0.1:8651.
  - strace of the service during a full session showed 0 connect and 0 socket calls. The only sends went to the
    loopback client.

## Changed on the VPS

- **New:**
  - /opt/galion/stt-moonshine/{bin,lib,models,SHA256SUMS}
  - user galion-stt
  - /etc/systemd/system/galion-stt.service (enabled, active)
  - /etc/nftables.d/galion-stt-egress.nft
  - nft table inet galion_stt_egress
- **Edited (backups next to each file):**
  - /etc/nftables.conf: one include line; backup `.bak-galion-stt-20261029T0918`.
  - Both `63-hermes-ui-rebuild-a69e0e35.conf` drop-ins: `--stt 127.0.0.1:8651` plus a comment line; backups
    `.bak-galion-stt-20261029`.
  - console-rust-primary and console were restarted, and both are active.
- **Temp files** on the VPS (staging and bench clips) were removed.

## Rollback

1. Restore both console drop-ins from `…63-hermes-ui-rebuild-a69e0e35.conf.bak-galion-stt-20261029`, then run
   `systemctl daemon-reload` and `systemctl restart console-rust-primary`, and then `systemctl restart console`.
2. `systemctl disable --now galion-stt`
3. `nft delete table inet galion_stt_egress`, then restore /etc/nftables.conf from
   `/etc/nftables.conf.bak-galion-stt-20261029T0918`.
4. Only with an owner yes: remove the unit file, the .nft file and /opt/galion/stt-moonshine, and run
   `userdel galion-stt`.

## Notes

- I accidentally ran `python3 -c 1` in a shell once early on. The swarm shim blocked it, so no Python ran.
- On the box I kept /workspace/swarm/tmp/galion-stt/kit (78 MB: the pinned libs, models and 2 public-domain clips)
  for re-running the real tests. The rest of the temp files and the 15 GB debug build cache were removed.
- `cargo deny` is not installed. Licences were checked with `cargo tree`.
- No Cloudflare/DNS changes, no git on /opt/galion/openmind, cortex/nerve/hermes-rs not touched, nothing in
  /workspace/cargen, no approval-retry flags used.
