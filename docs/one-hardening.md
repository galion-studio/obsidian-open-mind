<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# galion-one hardening (swarm/one-hardening, on top of one-probe b44cd4f)

Recorded 2026-10-20 20:50 CEST (+02:00). Nothing here is deployed.

## What changed

1. **Timestamps are Europe/Zagreb with an offset.** The `at` field of
   `GET /api/v1/analytics`, and of the lines written to
   `one/session.jsonl` and `hermes/channels/one.jsonl`, was `unix % 86400`
   printed as `HH:MM:SS`: the UTC time of day with no offset, whatever `TZ`
   said. It is now RFC 3339 with the offset (`2026-10-20T20:48:21+02:00`,
   `+01:00` from 2026-10-25 03:00 local). The `unix` field in the jsonl lines
   is unchanged. Nerve event lines (`/api/v1/analytics` `nerve.events`) now
   start with `20:48:21+02:00` instead of the UTC `18:48:21`.
   Implementation: `src/zagreb.rs`, no dependency, does not read `TZ` or
   `/usr/share/zoneinfo`; tested against chrono-tz every hour 2020-2040.
   *Consumer note:* the analytics `at` grew from 8 to 25 characters; any UI
   that shows it as-is now shows the date too.
2. **Fewer shell-outs.**
   - `ps -eo args` is gone: the unit checks (cloudflare, jupyter, nginx,
     origin) read `/proc/<pid>/cmdline` (`src/host.rs`).
   - `nvidia-smi`: optional. `ONE_NVIDIA_SMI=off|0|none|false` never runs it;
     any other value is the binary path; unset = `nvidia-smi` from `PATH`.
     Missing binary, non-zero exit, bad output or > 3 s = no GPU (killed on
     timeout). If the GPU disappears, the next 5th-tick sample clears it
     instead of showing the last reading forever.
   - `pdftotext`: kept (no permissive pure-Rust option without SIMD crypto,
     see docs/third-party/one-hardening.md), but optional the same way
     (`ONE_PDFTOTEXT`), 5 s timeout, and the PDF goes to a fresh private
     temp file (`create_new`, 0600, unique name, removed afterwards) instead
     of the fixed shared `/tmp/galion-one-page.pdf`.
   - `curl`: kept for the HTTPS edge probes and the OpenRouter model list
     (pure-Rust TLS needs ring/aws-lc assembly). Already absent-safe.
3. **A failed pairing never creates the store.** Before: `POST /pair` with no
   device store wrote a fresh `admin-device.json` (`fails:1`) into the
   secrets dir. Now: no store (or an unreadable one) answers `{"error":"no"}`
   and writes nothing; later updates only rewrite an existing store, never
   create it.
4. **Console probe is configurable.** `ONE_CONSOLE=host:port` (default
   `127.0.0.1:8080`, today's behaviour) or `off` to drop the `console` pulse;
   `ONE_CONSOLE_PATH` (default `/`). A bad value falls back to the default.

Voice behaviour is unchanged (persona `samantha`, mouth `127.0.0.1:8002`,
same as config/voice/galion-one-voice.toml on swarm/settings-snapshot).

## New env vars (all optional; unset = old behaviour)

| var | default | meaning |
|---|---|---|
| `ONE_CONSOLE` | `127.0.0.1:8080` | console pulse target, or `off` |
| `ONE_CONSOLE_PATH` | `/` | path the console pulse GETs |
| `ONE_NVIDIA_SMI` | `nvidia-smi` | GPU query binary, or `off` |
| `ONE_PDFTOTEXT` | `pdftotext` | PDF text binary, or `off` |
