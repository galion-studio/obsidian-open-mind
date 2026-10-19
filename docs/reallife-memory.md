<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real life memory

- DESK-ID: openmind.REAL-MEM
- Written: 2026-10-19T22:28:34+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Probed: 2026-10-19T22:26:57+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: one GET. Loopback only. No POST. No second server. This ticket did not bind 8420.

## Not a replacement test

The openmind-memory crate was not switched in front of the live gateway, so this is not a replacement test.

The process already listening on 127.0.0.1:8420 is pid 1374875, command /opt/galion/rust/bin/openmind-memory. ps lstart reports Sun Oct 18 20:14:40 2026. The /proc exe symlink mtime is 2026-10-18T20:14:40+00:00. The binary file mtime is 2026-10-18T19:41:22+00:00. No file named openmind-memory was found under target/ or dist/. crates/openmind-memory refuses to bind port 8420. That crate was not placed in front of this listener. Nothing was restarted. systemctl was not used. Nothing was committed. Nothing was pushed.

## Probe

| URL | HTTP status | Body |
| --- | --- | --- |
| http://127.0.0.1:8420/health | 200 | JSON |

One GET of http://127.0.0.1:8420/health. Status code 200. The body is JSON. Content-Type was application/json. Length was 277 bytes. The body was not printed.
