<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Enabling the memory-freshness nerve sensor (staged, NOT installed)

Owner decision D-405-1 / D-388-3. Do this only at a planned nerve deploy.

1. Install an openmind-memory build ≥ `1d7164b4` side by side as `/opt/galion/rust/bin/openmind-memory-freshness`
   (the live `aa77864` memory build has no `freshness` subcommand). Do not replace the running memory service binary.
2. Deploy a nerve built from this branch (or a later one that contains the sensor).
3. Copy `deploy/staged/memory-freshness/galion-nerve-memfresh.conf` to
   `/etc/systemd/system/galion-nerve.service.d/`, run `systemctl daemon-reload`, then restart galion-nerve.
4. Expect an **immediate warn alert**: the store has been stale since 2026-10-19 14:57 (Zagreb) until D-388-1
   (a memory writer) lands. It resolves on its own once a write happens.
5. In event mode the verdict lags up to one heartbeat (900 s).

Settings: `NERVE_MEMFRESH_BIN` (absolute; unset = off), `_DATA_DIR` (/var/lib/memory-tencentdb), `_INSTANCE` (galion),
`_MAX_AGE_H` (24), `_EVERY_S` (900), `_TIMEOUT_MS` (10000), `_COMPONENT` (openmind-memory).
Rollback: remove the drop-in, daemon-reload, restart galion-nerve.
The CLI opens SQLite read-only. A reader may still create `-shm` or an empty `-wal` (known since slot 388).
