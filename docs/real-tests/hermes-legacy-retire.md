<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-server evidence: hermes-legacy-retire (slot 163)

Docs-only slot: no binary built, no box port used (range 20330-20339 untouched). All checks were read-only probes of the live VPS,
2026-10-21 08:11-08:18 CEST (+02:00). Full table and outputs: `docs/ops/hermes-legacy-retire.md` section 7.

| Probe | Result |
|---|---|
| `systemctl is-enabled/is-active hermes.service` | disabled / inactive since 2026-10-18 21:42:17 +02:00 |
| `systemctl is-active hermes-rs`, `GET 127.0.0.1:8642/health` | active, 200 in 0.4 ms |
| nerve `GET 127.0.0.1:8670/state` (cortex not deployed; nerve used per standing rule) | hermes-rs http 200, 0 systemd restarts, 4.7 MB of 64 MB |
| Python processes | networkd-dispatcher, fail2ban-server only (no Hermes) |
| `/proc/<hermes-rs pid>/fd` | legacy `/opt/galion/hermes/state.db` open (read-only layer) |
| files written in `/opt/galion/hermes` since 2026-10-19 00:00 +02:00 | 0 |
