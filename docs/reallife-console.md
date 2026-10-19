<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real life console

- DESK-ID: openmind.REAL-CONSOLE
- Written: 2026-10-19T22:28:42+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Live tree, not edited: /opt/galion/console-ide
- The stamp offset is numeric. It is not a trailing UTC designator.

## One read

One GET, no retry, loopback only. The body was not copied into this file.

    curl -sS -D /tmp/reallife-health.hdr -o /tmp/reallife-health.body -w "%{http_code}" --max-time 10 http://127.0.0.1:8080/health

| item | value |
| --- | --- |
| URL | http://127.0.0.1:8080/health |
| Taken | 2026-10-19T22:27:55+02:00 |
| HTTP status | 404 |
| content-type | text/html; charset=utf-8 |
| body bytes | 2928 |

The response Date header was in GMT. That header is not the stamp for this note.

## Listener

Port 8080 was not freed. The socket stayed on 127.0.0.1:8080.

| item | value |
| --- | --- |
| process | node |
| pid | 2501884 |
| cwd | /opt/galion/console-ide |
| exe | /opt/galion/tooling/node/bin/node |
| command | node /opt/galion/console-ide/node_modules/vite/bin/vite.js dev --host 127.0.0.1 --port 8080 |

That command is the vite dev server. Vite was not restarted.

## Not a replacement test

A production console was not switched in, so this is not a replacement test.

- `/etc/systemd/system/console-prod.service` is not installed.
- `/etc/systemd/system/console.service` is present and was not edited.
- No process named console-prod was running. No srvx process was running.
- The staged files are still only files: `deploy/console-prod.service` and `stage/console-dist/server/index.mjs`.
- console-prod was not started.

The 404 above is the live vite dev server. It is not a result from the staged production bundle. This note does not claim the production server would answer the same way, because that server was not put on port 8080.

## Not done

Vite was not restarted. console-prod was not started. Port 8080 was not freed. `/opt/galion/console-ide` was not edited. Nothing was committed. Nothing was pushed. systemctl was not used. cargo was not run.
