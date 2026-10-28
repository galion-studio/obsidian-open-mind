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


## Optional crash-loop fail-fast (`GALION_DEPLOY_FAILFAST=N`, off by default)

The automatic (L1) rollback of a build that crashes at start used to take the whole
20 × 1 s health window, about 20.2 s of outage. With `GALION_DEPLOY_FAILFAST=N` (N > 0)
the forward health loop also reads the unit's state every 250 ms and gives up as soon
as the new build has crashed N times since the deploy's own restart, then rolls back as
before (same `health failed, rolling back` line and exit 3/4, preceded by
`crash loop: …; failing fast`). A unit that sits in `failed` on two polls in a row
(`Restart=no`, or the start limit was hit) also ends the window.

- Crashes counted = growth of `NRestarts` since the deploy restart + 1 if
  `SubState=auto-restart` (a crash systemd has not restarted yet). systemd:
  `systemctl show -p ActiveState -p SubState -p NRestarts <u>.service`; direct backend:
  `$GALION_DEPLOY_CTL show <u>` (simsup has `NRestarts` but no `SubState`, so there a
  crash counts RestartSec later).
- If the backend gives no `NRestarts` (systemd < 235, a ctl without `show`), it logs
  `fail-fast off: cannot read NRestarts; using the full health window` and behaves
  exactly as without the flag.
- Off (unset or 0) is byte-for-byte the old behaviour: no extra calls (asserted in
  `tests/systemd_backend.rs` and the unit tests). The rollback's own health check
  never fails fast.
- It does not help a build that stays up without becoming healthy (sim-hang): that
  still takes the full window. N = 1 also rolls back a good build whose first start
  fails once (port still held, slow dependency); see
  `docs/real-tests/galion-deploy-fastfail.md` for the numbers and the N to pick.

Test knobs (do not use in production): `GALION_DEPLOY_HEALTH_TRIES` (20),
`GALION_DEPLOY_HEALTH_INTERVAL_MS` (1000).

## Pre-cutover backup

```
galion-deploy backup [--root DIR] [--dest DIR] [--unit NAME]... [--no-data | --galion-backup BIN]
galion-deploy backup-verify DIR [--live-root DIR]
```

Writes `/opt/galion/backups/<YYYYMMDD-HHMMSS+zzzz>/` (Europe/Zagreb, 0700, files 0600):
`backup.tar` (ustar overlay), `MANIFEST.tsv`, `SHA256SUMS`, `VERIFY.txt` and `data/`. Default
units are `tuner` and `galion-nerve`. **One definition of a backup lives in galion-backup**
(swarm/rust-ports, `galion-backup run`): the data part is that binary, run with
`GALION_BACKUP_DEST=<snapshot>/data GALION_BACKUP_KEEP=1` (default
`/opt/galion/rust/bin/galion-backup`, env `GALION_DEPLOY_GALION_BACKUP`). galion-deploy adds
only what a binary swap can break: the unit binaries, unit files and drop-ins, the tuner
quick-tunnel transient unit, the tuner static/source tree, `/var/lib/galion-nerve`, and the
deploy tool itself. Secret trees and secret-like names are never read. Exit 0 written and
verified, 1 failed, 2 usage. Runbook: `docs/ops/cutover-tuner-nerve.md`.

## Static tree install (`dist-install`, slot 379)
```
galion-deploy dist-install SRC DEST    # dirs 755, files 644, executables 755; swap in; keep DEST.old-<stamp>
galion-deploy dist-check DIR           # group/other-writable or set-id entries -> exit 1
galion-deploy dist-rollback DEST OLD   # OLD back to DEST, current -> DEST.failed-<stamp>
```
Never copies a source mode (the live console-dist was unpacked 666/777).
Refuses absolute/escaping symlinks and special files; a failed copy leaves DEST
untouched. See docs/ops/world-writable-dirs-stage.md (D-356-3).

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
GALION_REAL=1 cargo test --release -p galion-deploy --test real_failfast -- --nocapture --test-threads 2   # fail-fast numbers, ports 19910-19911
```

The rehearsals use ports 19390-19393 and the dead port 19399, all on the box. Results are in `docs/real-tests/deploy-rust.md`.
