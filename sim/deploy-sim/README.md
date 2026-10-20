<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# simsup: cutover rehearsals without systemd

The swarm box runs `tini` as PID 1, so a `systemctl restart` cannot be rehearsed
there. `simsup` is a small std-only supervisor that behaves like systemd for the
unit subset our services use. `deploy/foundation/galion-deploy` talks to it
through its `direct` backend.

```
simsup --root DIR daemon                    # start every DIR/units/*.service
simsup --root DIR ctl VERB [UNIT] [N]       # known exec-path cat show is-active start stop
                                            # restart reset-failed journal status daemon-reload shutdown
simsup probe 127.0.0.1:PORT /health 2 STOPFILE 300   # downtime probe (ms between tries)

GALION_DEPLOY_SM=direct GALION_DEPLOY_CTL="simsup --root DIR ctl" \
GALION_DEPLOY_BACKUP_DIR=DIR/backups galion-deploy <svc> <new> <health-url>
```

Modelled: `ExecStart` (absolute path, quoted args), `Environment=`, `Restart=always|on-failure|no`,
`RestartSec`, `TimeoutStopSec` (SIGTERM to the process group, then SIGKILL),
`StartLimitIntervalSec`/`StartLimitBurst` (systemd's fixed window; it also refuses manual
restarts until `reset-failed`), `WorkingDirectory`, Type=simple exec failure (logged as
`status=203/EXEC`; the start job still succeeds), a clean environment, a per-unit journal
(`DIR/journal/<unit>.log`, what `journalctl -o cat` shows), and `daemon-reload` (new units are
added inactive; running processes keep running).

Parsed and **not** enforced (listed in `ctl show … IgnoredKeys`): `User=`, `Nice=`,
`CPUQuota=`, `MemoryMax=`, all sandboxing (`ProtectSystem=` …), `StateDirectory=`,
`[Install]`. Not modelled: other `Type=`s, socket activation, dependencies, D-Bus timing.
A rehearsal therefore proves the deploy *logic* and the binaries' start/stop behaviour. It
does not prove the unit's sandbox.

`sim-broken` (answers `--version`, exits 1 as a service) and `sim-hang` (never answers
`--version`, never binds) are the deliberately bad builds.

Build and run: `scripts/real/deploy-sim-build.sh`, then `scripts/real/deploy-sim.sh all`.
Results: `docs/real-tests/deploy-sim.md`.
