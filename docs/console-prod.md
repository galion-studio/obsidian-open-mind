<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Console production switch (staged checklist)

- DESK-ID: openmind.B61
- Written: 2026-10-19T17:12:04+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Live tree, not edited: /opt/galion/console-ide
- Build record: deploy/console-prod.md (T6, 2026-10-18)
- Staged unit: deploy/console-prod.service (no install section)
- The switch is not done.

## Status

Port 8080 is still the vite dev server from the live `console` unit. This ticket only writes the checklist. It does not rebuild, does not install a unit, does not stop vite, and does not start the production server.

One read, body discarded:

    curl -sf -o /dev/null -w "%{http_code}" http://127.0.0.1:8080/health

Observed code: **404**.

## What is live

Read from `/etc/systemd/system/console.service`. The file was not changed.

- WorkingDirectory is `/opt/galion/console-ide`.
- ExecStart runs `scripts/with-app-env.mjs` and `node_modules/vite/bin/vite.js dev --host 127.0.0.1 --port 8080`.
- Restart is always, with a 3 second gap, and the unit is wanted by multi-user.target.

`/etc/systemd/system/console-prod.service` is not installed. The socket on 127.0.0.1:8080 is node, the same vite dev process T6 measured. The comment in `deploy/console-prod.service` that says the live bind is 0.0.0.0 is stale. The installed unit binds 127.0.0.1.

## What is already staged

T6 left this on disk. It is not the process on 8080.

- `stage/console-dist/server/index.mjs` (nitro bundle)
- `stage/console-dist/static/`
- `stage/console-dist/node_modules/srvx/` (the srvx wrapper)
- `deploy/console-prod.service` would run that bundle with node on 127.0.0.1:8080
- `deploy/switch-console.sh` and `deploy/rollback-console.sh` are dry-run unless `--apply` is passed

T6 served the build on 127.0.0.1:18080 and then stopped that spare listener. 8080 stayed on vite. RSS was about 505 MB for vite and about 57 MB for the built server. On that day `/` was 302 to `/console` on both, and `/api/v1/health` was 401 with a 26 byte body on both. Those 26 bytes are the phone-gate JSON `{"error":"phone required"}`, not `healthPayload()`.

## What /health should return

`/health` is not a console route. The generated route tree registers `/api/v1/health` and `/api/hermes/health` only. The catalog row titled "Compat health" names a galion-backend path, not this app. `src/lib/origin-script.ts` is a paste script for another origin. It is not the process on 8080.

`GET /health` on 127.0.0.1:8080 should return **404** and no health JSON. That is the code recorded above. The production build must keep that. This switch does not add a `/health` alias, and it does not edit `/opt/galion/console-ide`.

The console health document is `GET /api/v1/health` (`src/routes/api/v1/health.ts` calls `healthPayload()` in `src/lib/catalog.ts`).

When Host is a console host, including `127.0.0.1` and `localhost` (`src/lib/host.ts`, phone middleware in `src/start.ts`), that route should return status 200, content-type `application/json`, and this body:

- `status`: `"healthy"`
- `service`: `"galion-control"`
- `version`: `"3.8.3-board"`
- `project`: `42`
- `phi`: `1.618`
- `mode`: `"light"`
- `zones.active`: `"running intent"`
- `zones.chill`: `"hibernate"`
- `github`: `"galion-studio"`
- `privateVault`: `"sealed"`
- `note`: `"CPU Galion console. ZeroAI. No models. Hibernation not loaded."`
- `target`: the machine record. Keys are provider, profile, mode, pod, podId, ssh user, ssh host, ssh direct, ssh port, public address, gpu, httpPort 80, originPort 8080, connected. Addresses are not copied into this file.

When Host is not a console host and the path starts with `/api/`, the phone middleware returns 401 and `{"error":"phone required"}` (26 bytes) and never calls `healthPayload()`. The in-app guardian list treats 200 and 401 as ok for `http://127.0.0.1:8080/api/v1/health`. A 404 on bare `/health` is not that probe. `deploy/survival/probes.toml` names the console path `/api/v1/health`, not `/health`.

This ticket did not call `/api/v1/health`. A 404 on `/health` is not a reason to restart the console.

## Checklist

Nothing below was executed except the single `/health` read and the read of the live unit file.

### A. Leave the live process alone

- [ ] Do not edit `/opt/galion/console-ide`.
- [ ] Do not run `npm run build` in that tree. The script is `vite build && npm run db:migrate`, and the second half migrates the live database (see `deploy/console-prod.md`).
- [ ] Do not stop the vite dev process on 8080 while writing or reviewing this page.

### B. Rebuild only if the staged bundle is missing or stale

Skip this while `stage/console-dist` from T6 is the bundle you intend to serve.

- [ ] Copy sources into `stage/console-src` with the rsync exclusions already written in `deploy/console-prod.md` (no env files, no keys).
- [ ] Build inside that copy with `nice -n 19` and `node scripts/with-app-env.mjs node_modules/vite/bin/vite.js build`, as that file records.
- [ ] Leave the output under `stage/console-dist`. Do not commit `stage/`.

### C. Dry run only, until the operator approves

- [ ] Run `bash deploy/switch-console.sh` with no `--apply`. It prints the plan for a new unit named `console-prod`. It does not replace `console.service`. It does not stop vite. It does not listen on 8080.
- [ ] Run `bash deploy/rollback-console.sh` with no `--apply`. There is no installed `console-prod` unit, so this restores nothing. The unit to copy aside by hand is `console`.

### D. Cutover, operator only, after approval

The switch is not done. Do not treat this section as already run.

- [ ] Copy `/etc/systemd/system/console.service` aside before any install. The rollback wrapper only knows `console-prod`.
- [ ] Read `deploy/console-prod.service`. It has no install section. ExecStart is node plus srvx, host 127.0.0.1, port 8080, working directory `stage/console-dist`.
- [ ] Free port 8080 from vite, then serve the staged bundle on that same address. Installing `console-prod` while `console` still holds 8080 does not switch the site. Both units want the same port. `deploy/switch.sh` only installs a unit file and prints a restart line. It does not perform the cutover.
- [ ] After the listener is the built server, check: `/` is 302 to `/console`; `/health` is still 404; `/api/v1/health` with Host 127.0.0.1 is 200 and the JSON above; a non-console Host on `/api/` is still 401 and 26 bytes. The public-host gate should match T6 (Host galion.app, 200, the coming-soon page).
- [ ] RSS should sit near the T6 built figure (about 57 MB), not the vite figure (about 505 MB).

### E. Rollback, operator only, if the cutover is bad

- [ ] Put the copied vite unit back and give 8080 back to `vite dev`. Do not delete `stage/console-dist`.

## Not done

The switch is not done. Port 8080 is still vite dev. `console-prod.service` is not installed. No process was restarted for this ticket.
