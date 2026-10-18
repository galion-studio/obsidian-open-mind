# STAGED — awaiting Slavik approval

Nothing in this file is enabled, installed or running. Nothing here restarts a service.
Each entry is a proposal for the operator to approve or reject.

## T3b — runtime Python removed from the ported binaries (2026-10-18T19:47:11+02:00)

Behaviour changes that need approval before switch-over:

1. galion-desk no longer spawns `python3 /opt/galion/bin/desk-hermes.py`. It runs the Rust
   `desk-hermes` binary instead: path from `DESK_HERMES_BIN`, default
   `/opt/galion/openmind/dist/bin/desk-hermes`. The stdout JSON is unchanged (checked against
   the live desk below).
2. galion-one's warm loop no longer shells out to `python3 -c`. That call DELETEd rows in
   Hermes state.db, which is forbidden, so it is removed with no replacement. The warm loop
   still transcribes; it just no longer prunes.
3. `one/models.py` and the voice worker path are off by default, gated behind `ONE_LEGACY_PY=1`
   plus `ONE_LEGACY_PY_BIN` for the interpreter. No interpreter path is hard-coded any more.
   The voice worker had no torch on this box and failed anyway.
4. `ONE_NO_WARM=1` disables the warm loop. Test instances only; production keeps the warm loop.
   It is deliberately absent from the staged galion-one unit.

Net effect once the units are switched: no Python process and no Python spawn is left in the
desk, the One loop or the link watch.

## T5 — staged units and switch scripts (2026-10-18T19:47:11+02:00)

- `deploy/galion-desk.service`, `deploy/galion-one.service`, `deploy/link-watch.service`:
  staged unit files pointing at `/opt/galion/openmind/dist/bin/*`. All three deliberately
  omit the `[Install]` section so they cannot be enabled. galion-desk and link-watch replace
  the live units that still run `python3` today; galion-one is a new unit for a process that
  is currently started outside systemd.
- `deploy/switch.sh`, `deploy/rollback.sh`: dry run by default, `--apply` required, and they
  copy the live unit into `deploy/backup/` before installing. Neither script starts, stops or
  restarts a service — it prints the `systemctl restart` line for the operator to run.
- `dist/bin/`: the five release binaries, copied (not symlinked), with `SHA256SUMS`.
  Verified with `sha256sum -c`: all five OK.

## T6 — console production build, staged, not switched (2026-10-18T19:50:57+02:00)

- Built in a copy, never in the live tree: `/opt/galion/openmind/stage/console-src`, with a
  real copy of `node_modules` (a symlink to the live tree left behind by an interrupted run
  was replaced, so the build could not write into `/opt/galion/console-ide`).
  Build command: `nice -n 19 node scripts/with-app-env.mjs node_modules/vite/bin/vite.js build`,
  exit 0, 6 seconds. `npm run build` is never used — it ends in `db:migrate` against the live DB.
- Output staged at `stage/console-dist/` (server bundle, static files, the 108 KB srvx wrapper).
  The service that would run it: `deploy/console-prod.service`, server on 127.0.0.1:8080
  instead of the dev server's 0.0.0.0:8080. Not enabled, no `[Install]` section.
- `deploy/switch-console.sh` and `deploy/rollback-console.sh` (thin wrappers, dry run by
  default, `--apply` required, never start or stop a service). `deploy/console-prod.md` records
  the exact build command and the measurements.
- Behaviour change to approve: the console stops being served by the vite dev server and starts
  being served as a built bundle. Measured on the spare port 18080: `/` 302 to `/console` on both,
  `/api/v1/health` 401 on both, the public-host gate (galion.app) answers the same 1587 bytes on
  both, and `/status` and `/soon` come straight from the server build instead of the dev shell.
  RAM drops from 517,024 KB (dev server, 505 MB) to 58,112 KB (57 MB) for the same page loads.
- Known limit to decide on: the build uses the project's pinned `vercel` preset, so it runs here
  through the srvx wrapper rather than `node .output/server/index.mjs`. A pod-native
  `node-server` preset build would need a config override in the copy, which was not done.

