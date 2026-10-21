# galion-one deploy record, 2026-10-21

Owner decision: Slavik via Mate (t179), 2026-10-21 08:36 +02:00. Deploy the newer galion-one only after sandbox tests pass; keep it only if it works.

## Source

- Branch `swarm/one-no-shellout`, commit `40454f90db9023fdbe0fb5939148f76266c5d760`
- Ancestry checked with `git merge-base --is-ancestor`: `fed3de4` (one-hardening) and `b44cd4f` (one-probe) are both ancestors.
- Build: `cargo build --locked --release -p galion-one --target x86_64-unknown-linux-musl` (static-pie, stripped, no dependencies)

## Box tests (2026-10-21 08:36-08:37 +02:00)

- `cargo test -p galion-one`: 78/78 unit tests passed, plus 1 + 2 + 1 other test targets passed
- `cargo fmt --check`: ok. `cargo clippy --all-targets -D warnings`: clean
- Runtime: 127.0.0.1:20390 (mouth 20391), temp home and store, warm ping off. `/health` 200, `/` 200, `/api/v1/services` 200, alive after 12 s

## VPS sandbox (2026-10-21 08:37:39-08:38:20 +02:00)

- Temporary user `sbx179`, dir `/var/tmp/sandbox-test-20261021-083739`, binary sha256 matched
- 127.0.0.1:20392 (mouth 20393): `/health` 200, `/` 200 at 2 s and again after 32 s, did not crash
- Process killed, user and dir removed, no leftover processes

## Binaries

| | path | size (bytes) | sha256 |
|---|---|---|---|
| old | `/opt/galion/openmind/dist/bin-ws/galion-one` (left untouched) | 828640 | `090c15d0fbb32d372edd87ea3cb154a5be40cbadbb3ea3d8bb4a7a5880e50191` |
| new | `/opt/galion/rust/bin/galion-one` (root:root 755) | 963904 | `24cb4976b1325ae0f5a100a7165a1e2e0bbd29f3f2c0e584dbd3a1cdd03b7ca3` |

Backup: `/var/backups/galion-one-20261021` (700). It holds the old binary, the unit file (600, sha256 `cff8a7c1…eaf`), the before state and SHA256SUMS. Before this deploy there were no drop-ins.

## Drop-in `/etc/systemd/system/galion-one.service.d/50-rust-bin.conf`

```
[Service]
ExecStart=
ExecStart=/opt/galion/rust/bin/galion-one
```

The arguments are the same as before (none). Environment, the voice/mouth settings (no GALION_MOUTH/EAR change), the tunnel, DNS, HADES mode and the Hermes model were not changed.

## Before and after

- Before: MainPID 1373612, NRestarts 0, active since 2026-10-18 22:13:56 +02:00
- Restart: 2026-10-21 08:39:24 +02:00
- After: MainPID 29782, NRestarts 0, `/proc/<pid>/exe` = `/opt/galion/rust/bin/galion-one`
- 127.0.0.1:8091 `/health` 200 and `/` 200, sampled every 15 s for 2 min (08:40:30-08:42:15 +02:00) with no restarts
- Public (from the box): galion.app 200, www.galion.app 200, one.galion.app 200, one.galion.app/tanga 301 to /tanga/ (200). Same as before the deploy.
- hermes-gateway :8642 `/health` 200
- nerve :8670: galion-one http 200, galion_health_ok true. Status is "degrading: 1 restart(s) in the last hour" because of this planned restart. It should clear after an hour.

## Voice (read-only)

:8002 is galion-one's own mouth listener (ONE_MOUTH_BIND), not a separate Python process. No Python listener or voice unit is running on the VPS. `/health` returns 503 `{"status":"loading","model":"chatterbox","loaded":false}` both before and after. Nerve galion-one-mouth: degrading (HTTP 503, "model loading"), same as before. Nothing was changed.

## Rollback

```
rm /etc/systemd/system/galion-one.service.d/50-rust-bin.conf
systemctl daemon-reload
systemctl restart galion-one
```
