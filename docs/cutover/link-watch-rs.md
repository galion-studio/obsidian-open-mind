<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: link-watch-rs (no listener)

- Status: **NOT CUT OVER on 2026-10-21.** The Rust link-watch replaced the Python `link-watch.service` on 2026-10-18 21:57:48 CEST (+02:00) and runs unchanged since.
- **Approved: not recorded.** The 2026-10-18 switch logs on the VPS (`/root/galion-switch-20261018-2140/SWITCH-LOG*.md`) carry no approval line, and CALL-RECORD starts 2026-10-20.
  No 2026-10-21 cutover is proposed or approved.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) and base.git `vps/ws-switch-gaps` ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/link-watch-rs.service` (1,177 B, sha256 `1abe890e57f2b0232065fa93ae1870ff6c92fa70a1af4c6f72271752d937bb23`, 2026-10-18 21:57), enabled, active, no drop-ins, `Restart=always` 3 s, `MemoryMax=32M`, `TasksMax=16`, `ProtectSystem=strict`, `ReadWritePaths=/opt/galion/galion-light/state`, `Conflicts=link-watch.service`. The file is **byte-identical** to `deploy/link-watch-rs.service` on base.git `vps/ws-switch-gaps` 14595cc3801ede34cfbbb6808981fd6775cc58ce |
| exe | `/opt/galion/rust/bin/link-watch`, 508,248 B, sha256 `2f5874f72243d24f52eb767f660d57ab14959c205679547b51366fa865f61116`, RSS 0.3 MB, NRestarts 0 |
| source | `crates/link-watch` (oom-rust ws/switch-gaps). **Not proven by bytes**: `dist/bin/link-watch` on `vps/ws-switch-gaps` and ws/foundation e43d7ab is `6b9d04de6bcaaae8…`, not the live `2f5874f7…`. The live binary was built from the same crate but the build recipe is not recorded |
| behaviour | writes `/opt/galion/galion-light/state/link.json` every 30 s (mtime 17:23:34 at the 17:23:35 probe); `link-log.json` unchanged since 2026-10-18 19:36:50; probes one remote host set by an `Environment=` line in the unit (value not copied here) |
| legacy | `link-watch.service` (Python) still installed, **disabled, inactive**. Starting it would stop link-watch-rs (`Conflicts=`) |
| journal | 0 lines since 2026-10-21 00:00 |

## 2. Verify (on the VPS, read-only)

```
systemctl show link-watch-rs -p ActiveState -p NRestarts
sha256sum /proc/$(systemctl show link-watch-rs -p MainPID --value)/exe    # 2f5874f7…f61116
stat -c %y /opt/galion/galion-light/state/link.json                       # within the last ~30 s
curl -s -m 5 http://127.0.0.1:8660/status.json | grep -o '"name":"link-watch-rs"'   # galion-health checks it
```

At 17:22–17:23: active, NRestarts 0, link.json fresh, nerve `link-watch-rs:healthy`, galion-health lists it ok.
At 17:07 the nerve dry-run sandbox wrote "would run: systemctl restart --no-block link-watch-rs.service" against a forced test target (CALL-RECORD 17:15:45). That was a sandbox simulation; the live unit was not touched.

## 3. Rollback

There is no 2026-10-21 change to roll back. The 2026-10-18 switch script goes **back to Python** and is not recommended:
`/root/galion-switch-20261018-2140/rollback-gaps.sh link-watch` (disable/stop link-watch-rs, delete its unit, `systemctl enable --now link-watch.service`).
Use it only on an owner decision; it was not rehearsed after 2026-10-18.

A future Rust-to-Rust cutover would follow [galion-one.md](galion-one.md) §3 with a `50-rust-bin.conf` drop-in. The rollback would be
`rm /etc/systemd/system/link-watch-rs.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart link-watch-rs` (composed here, not rehearsed).

## 4. Open points

- Provenance: rebuild `crates/link-watch` from `vps/ws-switch-gaps` with candidate recipes and compare against `2f5874f7…`, as health-land did for galion-health.
  Until then the live file is the only copy (back it up before any change).
- Retiring the Python `link-watch.service` unit is vps-unit-hygiene D-U3 (owner).
