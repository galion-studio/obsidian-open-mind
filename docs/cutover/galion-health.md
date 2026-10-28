<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: galion-health (127.0.0.1:8660)

- Status: **NOT CUT OVER.** The 2026-10-18 v0.1.0 binary runs unchanged since 2026-10-18 22:14:38 CEST (+02:00). No cutover was executed on 2026-10-21.
- **Approved: No.** The v0.2.0 replacement (`swarm/health-fixes` 62ddad812921a1832c0fc714f37140116c1b8a4b) awaits the owner ("deploy v0.2.0 + accept behaviour changes", CALL-RECORD 07:21:12).
  The 2026-10-18 install was part of the ws/foundation session. CALL-RECORD starts on 2026-10-20 and holds no approval line for it.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from CALL-RECORD, `docs/real-tests/health-land.md` and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/galion-health.service` (872 B, sha256 `e7a8d40260c9ac5d2eaad32a4bf6d3d0753f70d35d56a27d18059782d04f0d91`, 2026-10-18 22:14), enabled, active, **no drop-ins**, `User=root` (so `systemctl is-active` works), empty capability set, `MemoryMax=16M`, `TasksMax=8`, `CPUQuota=10%`, `Restart=always` 5 s |
| exe | `/opt/galion/rust/bin/galion-health`, 503,512 B, sha256 `6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a`, RSS 0.3 MB, NRestarts 0 |
| source | **proven**: byte-identical rebuild of ws/foundation e43d7ab609dd46521b27e16aac9ad9b440a9f737 `crates/galion-health` with `deploy/cross-foundation.sh` (rustc 1.99.1, x86_64 musl) ([../real-tests/health-land.md](../real-tests/health-land.md)). The repo unit `deploy/foundation/galion-health.service` matches the live one on every non-comment line |
| checks | 12 entries in `/status.json`: openmind-memory, hermes-rs, link-watch-rs, galion-desk, tuner, tuner-quick, one, console, cloudflared, journald, ssh, galion-backup.timer |

## 2. Verify (on the VPS, GET only)

```
systemctl show galion-health -p ActiveState -p NRestarts -p DropInPaths
sha256sum /proc/$(systemctl show galion-health -p MainPID --value)/exe    # 6afe668c…d8e01a
curl -s -m 5 http://127.0.0.1:8660/healthz                                 # 200 "ok"
curl -s -m 5 http://127.0.0.1:8660/status.json | grep -o '"ok":[a-z]*' | head -1   # "ok":true
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8660/       # 200
```

At 17:22: `/healthz` 200 (3 B), `/status.json` 200 (1,394 B, `ok:true`, 12 names as above), `/` 200 (1,950 B); nerve `galion-health:healthy`.
`/health` is 404 on this binary. The endpoint is `/healthz`.

## 3. Prepared cutover to v0.2.0 (NOT executed, needs approval)

Same pattern as [galion-one.md](galion-one.md) §3: build 62ddad8 for x86_64 musl, VPS sandbox run on a temp port as a non-root user (sandbox_vps_only),
then copy to `/opt/galion/rust/bin/galion-health-62ddad8`, back up the unit to `/var/backups/galion-health.service-<ts>`, and add
`/etc/systemd/system/galion-health.service.d/50-rust-bin.conf` with `ExecStart=` + `ExecStart=/opt/galion/rust/bin/galion-health-62ddad8`, then daemon-reload and restart.
v0.2.0 starts 5 threads (the unit's `TasksMax=8` fits). Behaviour changes (431/408/400 on bad requests, 503 busy) are listed in `swarm/health-fixes:docs/health-fixes.md`.

Rollback for that future cutover (composed here, not rehearsed): `rm /etc/systemd/system/galion-health.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart galion-health`.
Today there is nothing to roll back. If the binary were ever lost, rebuild e43d7ab with `deploy/cross-foundation.sh` to get `6afe668c…` again.

## 4. Open points

- The health page does not list galion-nerve, openmind-queue, tanga-proto or galion-nerve-shadow (its check list is from 2026-10-18).
- v0.1.0 known issues (split-request reads, single thread up to ~48 s): CALL-RECORD 23:08 (20 Oct) and 07:21:12. They are fixed in v0.2.0, which is not deployed.
