<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Listen now

- DESK-ID: openmind.PORT-NOW
- Stamped: 2026-10-19T18:43:19+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: `ss -lnt` once. Loopback listeners only. Public addresses are not recorded.
- Nothing was restarted. No unit was started, stopped, or restarted.

## Asked ports

| Port | Listening | Bind |
| --- | --- | --- |
| 8091 | yes | 127.0.0.1:8091 |
| 8002 | yes | 127.0.0.1:8002 |
| 8080 | yes | 127.0.0.1:8080 |
| 8420 | yes | 127.0.0.1:8420 |
| 8646 | yes | 127.0.0.1:8646 |
| 8650 | yes | 127.0.0.1:8650 |

All six asked ports are listening on loopback.

## Loopback listeners

From one `ss -lnt`. Every row below was LISTEN. Public addresses are omitted.

| Local address | Port |
| --- | --- |
| 127.0.0.54 | 53 |
| 127.0.0.1 | 20241 |
| 127.0.0.1 | 20242 |
| 127.0.0.53%lo | 53 |
| 127.0.0.1 | 8660 |
| 127.0.0.1 | 8670 |
| 127.0.0.1 | 8671 |
| 127.0.0.1 | 8642 |
| 127.0.0.1 | 8646 |
| 127.0.0.1 | 8650 |
| 127.0.0.1 | 8652 |
| 127.0.0.1 | 8653 |
| 127.0.0.1 | 8432 |
| 127.0.0.1 | 8433 |
| 127.0.0.1 | 8420 |
| 127.0.0.1 | 8431 |
| 127.0.0.1 | 25 |
| 127.0.0.1 | 8080 |
| 127.0.0.1 | 8091 |
| 127.0.0.1 | 8002 |
| [::1] | 25 |

Nothing was restarted.
