<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Room cutover: box rehearsal of docs/cutover/room.md (swarm/room 5bb27e1)

- Rehearsed: 2026-10-20 21:14:57–21:15:36 CEST (+02:00), swarm slot 33, branch `swarm/deploy-sim`. **5/5 checks pass.** Full log: `docs/real-tests/deploy-sim/room-run5.log`.
- Re-run: `scripts/real/deploy-sim-build.sh && scripts/real/deploy-sim.sh room` (port 19332).
- The plan (room.md) lives on `swarm/room`. This file records how it behaved in the sim and what to change. Nothing touched the VPS. A read-only GET at 21:03 CEST confirmed that nothing answers on 127.0.0.1:4020 (`000`), as room.md says.

## Sim setup

| VPS (room.md) | sim |
|---|---|
| no room unit, nothing on :4020 | simsup with an empty unit dir, nothing on :19332 |
| `deploy/staged/room.service` → `/etc/systemd/system/room.service`, `daemon-reload` | the same file (from 5bb27e1) with `ExecStart=$R/opt/galion/rust/bin/room --listen`, `ROOM_BIND=127.0.0.1:19332`, `ROOM_ROOT=$R/workspace`; `ctl daemon-reload`. The hardening lines are parsed and not enforced (the sim cannot test `ProtectHome=true` against a `/workspace` root; room.md's go/no-go already asks for `systemd-analyze security`). |
| `/tmp/room.new` | `$R/var/tmp/galion-cutover/room.new` = swarm/room 5bb27e1 musl build, sha `05075415…0949` |
| data under `/workspace/galion-light/docs`, `/workspace/galion-src` | three small sample files under `$R/workspace/…` |

## Step by step

| room.md step | result in the sim |
|---|---|
| state before | `listeners: 0; Unit room.service could not be found.` |
| install the unit + `daemon-reload` | reloaded. The unit stays **inactive** (daemon-reload does not start anything, and the unit has no `[Install]`) |
| `galion-deploy room /tmp/room.new http://127.0.0.1:4020/health` | **fails: `cannot resolve installed binary for room`, exit 1 in 40 ms.** galion-deploy needs an installed binary to back up. Nothing was changed. |
| fallback: `install -m 755 room.new /opt/galion/rust/bin/room && systemctl start room` | started. The first `/health` 200 came **316 ms** after `install` began (includes the ctl round trip) |
| health: `/health`, `/api/objects`, `/api/read?path=/etc/passwd`, `ss -ltn` | `ok` (exactly), `{"objects":[`, **404**, `127.0.0.1:19332` only (never 0.0.0.0) |
| upgrade later via galion-deploy (same binary) | exit 0, **5 ms** of refused connections |
| upgrade with a broken binary | automatic rollback, `rolled back ok`, 20.2 s of failed health, then healthy on the previous room |
| manual rollback to today's state: `systemctl stop room; rm unit; daemon-reload` | `listeners on 19332: 0; Unit room.service could not be found.` |

## What this changes in room.md

1. **Make the manual first install the primary path, not the fallback.** galion-deploy cannot do a first install by design (step 1 resolves and copies the current binary), so `galion-deploy room …` on the first day always exits 1 with `cannot resolve installed binary`. The correct first-day command is:
   ```
   install -m 0644 deploy/staged/room.service /etc/systemd/system/room.service && systemctl daemon-reload
   install -m 0755 /var/tmp/galion-cutover/room.new /opt/galion/rust/bin/room && systemctl start room
   curl -s -m 3 http://127.0.0.1:4020/health        # ok
   ```
   From the second deploy on, `galion-deploy room <new> http://127.0.0.1:4020/health` works (rehearsed: 5 ms swap, automatic rollback of a broken build).
2. Use `/var/tmp/galion-cutover` (0700) instead of `/tmp/room.new` for the copied binary, as the tuner and nerve plans do.
3. `room --version` exits without binding (both main 13a296c and 5bb27e1), so galion-deploy's version step is safe for room. The old main build also only serves with `--listen`.
4. The rollback is exact: stop, remove the unit, daemon-reload, and nothing is on the port. room keeps no state.
5. There is no expected downtime for anyone. Nothing depends on :4020 today, and room is a new service.
