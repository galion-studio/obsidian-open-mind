<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# galion-deploy (Rust)

```
galion-deploy <service> <new-binary> [health-url]
galion-deploy --recover <service>
```

Backup, atomic install, restart, health check, automatic rollback, for one systemd
unit (or a `simsup` unit on the box). It replaces the bash script that was at
`deploy/foundation/galion-deploy`. That script is kept, not installed and not run, at
`docs/reference/galion-deploy.bash`. std only: no crates, no libc crate.

## Same as the bash version

- **CLI.** Same arguments. A `.service` suffix is stripped. A missing or empty third
  argument means the table URL, otherwise `none`. Fewer than 2 arguments print
  `usage: galion-deploy <service> <new-binary> [health-url]` and exit 2.
- **Environment.** `GALION_DEPLOY_SM=systemd|direct`, `GALION_DEPLOY_CTL`,
  `GALION_DEPLOY_BACKUP_DIR` (default `/var/backups/galion/deploy`),
  `GALION_DEPLOY_DATA_BACKUP` (default `/opt/galion/rust/bin/galion-backup.sh`, run
  if executable, failure ignored), `SKIP_DATA_BACKUP=1`.
- **Backend calls.** systemd: `systemctl cat|show -p ExecStart --value|restart|is-active --quiet|reset-failed <u>.service`
  and `journalctl -u <u>.service -n 20 --no-pager -o cat`. The ExecStart path is
  parsed exactly like the bash `sed`. direct: `$GALION_DEPLOY_CTL known|exec-path|restart|is-active|journal <u> 20|reset-failed`,
  plus `stop` for the first-install undo. `tests/systemd_backend.rs` asserts the
  exact call sequence for each of the ten old equivalence scenarios.
- **On disk.** `umask 077`. The backup dir and its parent are chmod 0700. The backup
  is `<dir>/<unit>-YYYYmmdd-HHMMSS+zzzz.bin` (Europe/Zagreb), copied like `cp -p`
  (mode, owner, times). The install goes through `<target>.new`, mode 0755, then a
  rename. Only the newest 5 backups per unit are kept, by mtime as with `ls -t`.
- **Log lines.** `ts=<Zagreb ISO> svc=galion-deploy level=… msg="…" …`. The messages
  `backup`, `deployed`, `health failed, rolling back`, `rolled back ok` and
  `ROLLBACK ALSO UNHEALTHY for <u>; manual action needed (backup …)` are unchanged.
  The unit journal is printed as `  | …`, cut to 240 bytes.
- **Health loop.** 20 tries, 1 s apart. A try counts only while the unit is active.
  The URL must answer 2xx within 3 s. With `none`, the unit must be active on try 5.
  The `galion-health` `healthz-self` URL becomes `status.json`.

## Fixed (deploy-sim findings)

| | bash | Rust |
|---|---|---|
| F1 | `<new> --version` waits forever. A binary that ignores argv (old tuner, galion-desk) serves the port outside the unit and the deploy hangs | Bounded, default `GALION_DEPLOY_VERSION_TIMEOUT=5` (`0` skips the probe). stdio goes to /dev/null and the probe runs in its own process group, which is SIGKILLed as a whole |
| F2 | A start-limit window that is already full refuses the rollback restart, and `set -e` then kills the deploy with the unit `failed` | `reset-failed` before every restart is on by default (`GALION_DEPLOY_RESET_FAILED=0` turns it off). A refused restart is retried once after `reset-failed` |
| F3 | Exit 1 for refused, rolled back, died mid-way and rollback failed alike | 0 deployed, 1 refused (nothing changed), 2 usage, 3 rolled back and healthy, 4 rollback unhealthy (manual action), 5 interrupted and restored, 6 interrupted and restore unhealthy |
| F4 | No `galion-nerve` entry. A deploy judged on `/health` also judged the rollback to the old nerve on `/health`, which returns 404 | Table entries `galion-nerve` → `:8670/healthz` and `room` → `:4020/health`. The rollback is judged on the unit's own path, at the host:port of the URL in use (`rollback_health=` is logged). `GALION_DEPLOY_ROLLBACK_HEALTH` overrides |
| F5 | No first install (`cannot resolve installed binary`) | A known unit whose ExecStart binary does not exist yet gets a first install: no backup, restart, health. On failure the unit is stopped and the binary removed. The tool refuses if the unit is active without its binary |
| F6 | Ctrl-C, a dropped ssh session or `timeout` after the install left the new binary with no rollback | SIGINT, SIGTERM and SIGHUP are caught. Before the install they leave nothing changed; after it they trigger a rollback (exit 5 or 6). Signals during a rollback are ignored until it finishes. A rollback journal `<backup-dir>/<unit>.journal`, written atomically before every phase, covers SIGKILL and power loss: the next run or `--recover` restores the backup. The journal is also the per-unit lock (created with link(2)), so a second deploy of the same unit is refused with exit 1 |

Also fixed:
- A refused forward restart now rolls back. Bash died with the new binary installed.
- Every child runs in its own process group with stdin on /dev/null, so a terminal
  Ctrl-C reaches galion-deploy and not a half-done `systemctl restart`.
- Writes to a closed stdout are ignored, and every log line is also appended to
  `<backup-dir>/<unit>.log`.
- Pruning keeps what bash kept (the newest 5 files) plus the newest copy of each of the
  5 newest distinct binaries, and also runs after a successful rollback. Under the bash
  rule, five failed deploys (five copies of the same binary) pushed the L2 rollback
  target out. Pruning only
  touches `<unit>-<stamp>.bin`; bash's `tuner-*.bin` glob also matched a unit named
  `tuner-x`.
- HTTP health checks run in-process for `http://`. Other schemes still use curl.

Test knobs (do not use in production): `GALION_DEPLOY_HEALTH_TRIES` (20),
`GALION_DEPLOY_HEALTH_INTERVAL_MS` (1000).

## Callers to know about

- openmind-nerve (swarm/nerve) launches `/opt/galion/rust/bin/galion-deploy` through
  `systemd-run` and detects a running deploy by `galion-deploy ` in a cmdline. Both
  still hold for the Rust binary. Nerve does not read the exit code.
- docs/cutover/*.md say "after a non-zero exit, check is-active and the sha". That
  advice still holds, and the exit code now says which case occurred.

## Tests

```
cargo test -p galion-deploy                                   # 28 unit + systemd backend scenarios
GALION_REAL=1 cargo test --release -p galion-deploy --test real_build       # rehearsal inputs pinned
GALION_REAL=1 cargo test --release -p galion-deploy --test real_rehearsal -- --nocapture --test-threads 4
```

The rehearsals use ports 19390-19393 and the dead port 19399, all on the box. Results are in `docs/real-tests/deploy-rust.md`.
