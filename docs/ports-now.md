<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Ports now

- DESK-ID: openmind.PORTS-NOW
- Stamped: 2026-10-20T09:56:22+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: one `ss -lnt`. No curl. Nothing was restarted.

The stamp offset is numeric. It is not a trailing UTC designator.

## Asked ports

| Port | Listening | Address |
| --- | --- | --- |
| 8091 | listening | 127.0.0.1:8091 |
| 8002 | listening | 127.0.0.1:8002 |
| 8080 | listening | 127.0.0.1:8080 |
| 8420 | listening | 127.0.0.1:8420 |
| 8646 | listening | 127.0.0.1:8646 |
| 8650 | listening | 127.0.0.1:8650 |
| 8642 | listening | 127.0.0.1:8642 |

All seven asked ports are listening. Each address is the local address from that one `ss -lnt`.

Nothing was restarted. No unit was started, stopped, or restarted. Nothing was committed. Nothing was pushed.
