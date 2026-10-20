<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Health now

- DESK-ID: openmind.HEALTH-NOW
- Stamp: 2026-10-19T18:46:07+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: one GET per URL, no retries. Loopback only.
- Nothing was restarted.

| URL | HTTP status | Body |
| --- | --- | --- |
| http://127.0.0.1:8091/health | 200 | word ok |
| http://127.0.0.1:8420/health | 200 | JSON |
| http://127.0.0.1:8080/health | 404 | neither |

Nothing was restarted. No unit was started, stopped, or restarted.

The 8091 read was taken once at 2026-10-19T18:44:07+02:00. Curl saved 3 bytes and wrote no error. Those bytes are the word ok (a trailing newline only). That listener writes this body only with status 200. Ports 8420 and 8080 were read once each, just before the stamp. Bodies were classed in place and were not copied into this note.
