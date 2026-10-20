<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Health snap

- DESK-ID: openmind.HEALTH-SNAP
- Probed: 2026-10-19T18:09:57+02:00 (Europe/Zagreb)
- Written: 2026-10-19T18:10:57+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: one GET per URL. Status code only. A body under 40 bytes is shown. Larger bodies were not printed. Nothing was restarted.

| URL | Code | Body |
| --- | --- | --- |
| http://127.0.0.1:8091/health | 200 | `ok` (3 bytes) |
| http://127.0.0.1:8420/health | 200 | withheld (277 bytes) |
| http://127.0.0.1:8080/health | 404 | withheld (2928 bytes) |

Listeners, read with `ss` only. No unit was started, stopped, or restarted.

| Bind | Process |
| --- | --- |
| 127.0.0.1:8091 | galion-one |
| 127.0.0.1:8420 | openmind-memory |
| 127.0.0.1:8080 | node |

The 404 on port 8080 is the recorded code. It is not a reason to restart that process.
