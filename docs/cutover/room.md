<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Cutover plan: room (alternative first cutover candidate)

- Written: 2026-10-20T19:04:51+02:00 (Europe/Zagreb), branch `swarm/room`, based on 13a296c
- Status: **PLAN ONLY. Nothing has been cut over, installed, copied to the VPS, or restarted.**
- Approval: needs Slavik's explicit go (docs/cutover-approval.md). This file is not an approval.

## What room is

room is a read-only page of operator files. It is **not** a sandbox and runs no
commands. `room` prints the page JSON and exits. `room --listen` serves it on
`ROOM_BIND` (default `127.0.0.1:4020`) with four GET routes: `/health`, `/`,
`/api/objects` and `/api/read?path=`. `ROOM_ROOT` (default `/workspace`) picks the
data tree. Crate docs: `cargo doc -p room`.

## Current live state (read-only probe, 2026-10-20T18:52:20+02:00)

| check | result |
|---|---|
| `ss -ltnp` for :4020 | no listener |
| systemd units matching `room` | none |
| room process | none |
| `GET 127.0.0.1:4020/health` | connection refused (000) |
| `/opt/galion/openmind/dist/bin/room` | 344304 bytes, Oct 18 17:43 (old build that always listens) |
| data roots `/workspace/galion-light/docs`, `/workspace/galion-src`, `/workspace/galion-light/room/index.html` | present |

So **this would start a new service, not replace one.** Nobody depends on :4020
today. That is why room is the lowest-risk first candidate: if it fails, the
rollback returns to exactly what runs now, which is nothing on :4020.

## Behaviour changes compared with the old dist binary (Oct 18 build)

1. The old binary always listened. The new one listens only with `--listen`, so the unit must pass `--listen`.
2. Unknown flags, a missing `--fixture` path, or `--listen` followed by extra arguments now exit 2 with usage and never serve.
3. Manifests that do not exist are no longer listed as phantom `library` rows.
4. Walking no longer follows symlinks and stops at depth 32. The old code could recurse forever on a symlink loop.
5. `/api/read` now resolves symlinks and refuses targets outside the read roots.
6. A malformed `%` escape in `/api/read` used to panic. Under `panic = "abort"` that killed the whole server. Now it is a 404.
7. Non-200 replies have standard reason phrases ("Not Found" and so on, not "No"). There is a `nosniff` header. An empty request gets 400. More than 64 connections at once get 503.
8. Rows are sorted by name within each directory, so the order is deterministic.

## Command (only after the go)

On the **box**, never on the VPS:
```
git -C <clone> checkout <merged-main-sha>    # must contain this branch
export PATH=/workspace/swarm/bin:$PATH CARGO_TARGET_DIR=/workspace/swarm/target/room
nice -n 10 cargo build --release --locked -p room -j 4 --target x86_64-unknown-linux-musl
scripts/real/room.sh                          # must end with "leftover room processes: none"
sha256sum $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/room
```
Then on the VPS, as the operator, with the approval quoted in the log:
```
scp room  ->  /tmp/room.new                   # copy only the binary; compare sha256
install -m 644 deploy/staged/room.service /etc/systemd/system/room.service
systemctl daemon-reload
/opt/galion/rust/bin/galion-deploy room /tmp/room.new http://127.0.0.1:4020/health
```
`galion-deploy` backs up any previous binary, installs atomically, restarts, checks
health for 20 s, and rolls back on failure (infra/DEPLOY.md). **Check first** that
it installs to `/opt/galion/rust/bin/room`, the path the staged unit uses. That
path is outside the `/opt/galion/openmind` working copy on purpose. If
galion-deploy cannot handle a unit that never ran before, do it by hand:
`install -m 755 /tmp/room.new /opt/galion/rust/bin/room && systemctl start room`.
The staged unit has no `[Install]`, so it does not survive a reboot until
someone deliberately adds `WantedBy=multi-user.target`. That is a second
decision.

## Health check (one read each, no retry loop)

```
curl -s -m 3 http://127.0.0.1:4020/health                      # exactly "ok"
curl -s -m 3 http://127.0.0.1:4020/api/objects | head -c 12     # {"objects":[
curl -s -m 3 -o /dev/null -w '%{http_code}\n' 'http://127.0.0.1:4020/api/read?path=/etc/passwd'   # 404
ss -ltn 'sport = :4020'                                         # 127.0.0.1:4020 only, never 0.0.0.0
systemctl status room --no-pager                                # active (running)
```
Also re-read the neighbours once, so the switch is shown to touch nothing else:
`127.0.0.1:8091/health` is still `ok`, and `8420/health` is still JSON.

## Rollback

- Automatic: galion-deploy rolls back if `/health` fails within 20 s.
- Manual (returns to today's state, nothing on :4020):
  ```
  systemctl stop room
  rm /etc/systemd/system/room.service && systemctl daemon-reload
  ss -ltn 'sport = :4020'      # empty again
  ```
  If an older room binary existed under `/opt/galion/rust/bin`, galion-deploy's backup restores it. Today there is none.
- room writes no data and keeps no state, so there is nothing to restore.

## Go / no-go

Go only if **every** box is true:
- [ ] Slavik approved room by name (not "a candidate").
- [ ] This branch is merged to `main` and CI is green (fmt, clippy -D warnings, test, musl tier-1, repo-check, no-Python).
- [ ] `scripts/real/room.sh` passes on the box against the exact binary to be copied (sha256 recorded).
- [ ] The decision on exposure is written down: room stays loopback-only. No tunnel or ingress route to :4020 without a separate approval, because `/api/read` serves `.md`/`.txt` from `/workspace/galion-src` and `/workspace/galion-light/docs`.
- [ ] Someone checked `/workspace/galion-src` and `/workspace/galion-light/docs` on the VPS for sensitive `.md`/`.txt` files whose names avoid the filter (`secret`, `.env`, `token`, `id_ed25519`). The name filter is a guard, not a classifier.
- [ ] `systemd-analyze security room.service` is reviewed, and the hardening lines do not stop room from reading `/workspace`.
- [ ] Port 4020 is still free right before the switch.

No-go if any box is false, if :4020 is taken, or if any health check above fails.
A failed check means rollback, not retry.
