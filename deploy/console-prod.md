# Console production build — how it was made (staged, 2026-10-18T19:50:00+02:00)

Nothing here is installed or enabled. `console-prod.service` is not enabled and the
live console still runs the vite dev server.

## Build (in a copy, never in the live tree)

    cd /opt/galion/openmind
    rsync -a --exclude node_modules --exclude .git --exclude .worktrees \
      --exclude '.env' --exclude '*.env' --exclude '*secrets*' \
      --exclude '*.pem' --exclude '*.key' \
      /opt/galion/console-ide/ stage/console-src/
    cp -a /opt/galion/console-ide/node_modules stage/console-src/node_modules
    cd stage/console-src
    nice -n 19 /opt/galion/tooling/node/bin/node scripts/with-app-env.mjs \
      node_modules/vite/bin/vite.js build

`npm run build` is never used: that script is `vite build && npm run db:migrate`, and
the second half migrates the live database.

The build ran in the copy, in 6 seconds, exit 0. Vite 8 with the nitro preset the
project pins in `vite.config.ts` (`preset: "vercel"`, `serverDir: "./server"` so the
server middleware keeps running). `NITRO_PRESET=node-server` was tried and ignored:
the config pins the preset, so the output is the Vercel layout,
`.vercel/output/functions/__server.func/index.mjs` plus `.vercel/output/static/`.
Nitro's own preview command for that layout is
`srvx --static ../../static ./functions/__server.func/index.mjs`, which is what the
staged unit runs.

## Staged output

    stage/console-dist/server/       the nitro function bundle (index.mjs and its chunks)
    stage/console-dist/static/       public files, assets, manifest
    stage/console-dist/node_modules/srvx/   the 108 KB server wrapper the unit runs

Total 84 KB of application output plus the srvx wrapper. The build in
`stage/console-src` stays on disk for a rebuild; it is not committed.

## Measured, not assumed

Served on 127.0.0.1:18080 only, then killed. 18080 was free before and after.

    route            live dev 8080            built 18080
    /                302 -> /console          302 -> /console
    /status          200, 1705 bytes         200, 2342 bytes
    /soon            200, 1705 bytes         200, 2342 bytes
    /api/v1/health   401, 26 bytes           401, 26 bytes
    Host: galion.app 200, 1587 bytes         200, 1587 bytes  (coming-soon gate, unchanged)

    RSS, one page load each:
    live dev server (pid 2501884, vite dev)             517,024 KB  (505 MB)
    built server (pid 1231574, node + srvx)              58,112 KB  ( 57 MB)

The dev server serves the client-rendered shell for /status and /soon (1705 bytes,
same bytes for both), the built server renders them server-side (2342 bytes each).
The public-host gate and the health route answer the same on both, because the gate
lives in `server/middleware/00-public-host-gate.ts` and ships inside the build, not
only in the dev middleware.

## Switch and rollback

    bash deploy/switch-console.sh            # dry run, prints the diff and the plan
    bash deploy/switch-console.sh --apply    # installs the unit file, reloads systemd
    bash deploy/rollback-console.sh          # dry run of the restore
    bash deploy/rollback-console.sh --apply  # restores the newest backup

Neither script starts, stops or restarts a service. They install a unit file, copy the
previous one into `deploy/backup/` first, and print the `systemctl restart` line for
the operator. Today `console-prod` has no previous unit, so the rollback path is the
one to check by hand if the switch is made: the live unit is `console`, and the vite
dev unit should be copied aside before the switch.
