# Deploy path (ws/foundation)
Installed: `/opt/galion/rust/bin/galion-deploy` on the VPS.

```
galion-deploy <service> <new-binary> [health-url]
```
1. Resolves the installed binary from the unit's ExecStart.
2. Checks the new file is ELF.
3. Copies the current binary to `/var/backups/galion/deploy/<svc>-<stamp>.bin` (keeps 5 per service) and runs a full data snapshot (`galion-backup.sh`; skip with `SKIP_DATA_BACKUP=1`).
4. Atomic install (`install` to `.new`, then `mv`), `systemctl restart`.
5. Health: up to 20 s for `systemctl is-active` and HTTP 2xx on the health URL (defaults: memory 8420/health, hermes 8642/health, desk 8646/health, tuner 8650/health, galion-health 8660/status.json; link-watch-rs: unit only).
6. On failure: prints the last journal lines, restores the old binary, restarts, re-checks. Exit 1 = rolled back, other non-zero = rollback also failed (manual action).

Build and ship (box side, static musl, zig cc):
```
cd /workspace/oom-ws/foundation && ./deploy/cross-foundation.sh
tar -cf - -C $CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release galion-health | \
  ssh -i ~/.ssh/id_ed25519_vps -o IdentitiesOnly=yes root@51.38.137.193 'tar -C /root/stage -xf - && /opt/galion/rust/bin/galion-deploy galion-health /root/stage/galion-health'
```
Tested 2026-10-18 22:14 CEST: `/bin/false` deployed onto galion-health -> health failed -> rolled back ok (exit 1); same-binary deploy -> ok (exit 0).

Backups: `systemctl start galion-backup.service`; restore test: `/opt/galion/rust/bin/galion-restore-test.sh [archive]`.
Unit files and scripts live in `deploy/foundation/`.
