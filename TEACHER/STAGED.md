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

## T6 — console production build (in progress, to be added when it passes)

Still building. This section will carry the prod unit, the build command, the measured RAM
against today's dev server, and the switch and rollback scripts before Slavik sees it.
