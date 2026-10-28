<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover runbook: galion-one (127.0.0.1:8091 text, 127.0.0.1:8002 mouth)

- Status: **CUT OVER, live since 2026-10-21 08:39:24 CEST (+02:00)** from `swarm/one-no-shellout` 40454f90db9023fdbe0fb5939148f76266c5d760 (index [README.md](README.md) §2 C9).
- **Approved: Yes**, Slavik via Mate t179, 2026-10-21 08:36:04: deploy only after the sandbox tests pass, and keep it only if it works.
  Record: `swarm/galion-one-deploy@a93218de347d968b64e72c9ea316f7f5fef34d14:docs/ops/galion-one-deploy.md`; CALL-RECORD 08:43:12.
- [../one-probe-cutover.md](../one-probe-cutover.md) lists the port and probe differences. This file is the runbook: what runs, how the 08:39 cutover was done, and how to repeat it or roll it back.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from the record and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/galion-one.service` (1,851 B, sha256 `cff8a7c151cbfb89ce150c7d1599e9bd2018b3afe726b99462b6fd006e5cdeaf`, unchanged since 2026-10-18 22:13:56), enabled, active, `Restart=always` 5 s, `MemoryMax=128M`, `TasksMax=512`, `Nice=5`, `WorkingDirectory=/opt/galion/galion-light`, stdout/stderr append to `/var/log/galion-one.log` |
| exe | `/opt/galion/rust/bin/galion-one`, 963,904 B, sha256 `24cb4976b1325ae0f5a100a7165a1e2e0bbd29f3f2c0e584dbd3a1cdd03b7ca3`, MainPID unchanged since 08:39:24, NRestarts 0 |
| drop-in | `galion-one.service.d/50-rust-bin.conf` (08:39:24, sha256 `467d894b661cfe8e4fb0fb6ecc0f9f15c77dd2b60a55f702ac81692d0f85ff8b`): `ExecStart=` reset, `ExecStart=/opt/galion/rust/bin/galion-one` (no arguments, as before) |
| old binary (rollback) | `/opt/galion/openmind/dist/bin-ws/galion-one`, 828,640 B, sha256 `090c15d0fbb32d372edd87ea3cb154a5be40cbadbb3ea3d8bb4a7a5880e50191`, untouched (inside the working copy, which stays at 2f5a730) |
| backup | `/var/backups/galion-one-20261021/` (0700): `galion-one.old` (828,640 B), `galion-one.service`, `before-state.txt`, `SHA256SUMS` |
| public | ingress `galion.app`, `www.galion.app` → `http://127.0.0.1:8091` (§2 C8, 08:32:32); `one.galion.app` → :8091 (with `/tanga` on :8671, see [tanga-proto.md](tanga-proto.md)) |

Unit-file quirks (vps-unit-hygiene, `swarm/vps-unit-hygiene` b985e43, proposal P1): the fragment header still says "STAGED, NOT INSTALLED" and has no `[Install]` section.
It is enabled by a hand-made link. `After=hermes.service` points at the masked legacy unit.

## 2. Verify (on the VPS, GET only)

```
systemctl show galion-one -p ActiveState -p NRestarts -p DropInPaths
sha256sum /proc/$(systemctl show galion-one -p MainPID --value)/exe       # 24cb4976…03b7ca3
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8091/health   # 200
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8091/         # 200
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8002/health   # 503 today (mouth "loading", known, not caused by the cutover)
```

At 17:22: active, NRestarts 0, 8091 `/health` 200 (3 B), `/` 200 (205,402 B), 8002 `/health` 503 (86 B), nerve `galion-one:healthy` and `galion-one-mouth:degrading`.
The 503 is a hardcoded stub, and no voice service exists on the VPS (slot 181 mouth-loading-diag, CALL-RECORD 09:02). It was 503 both before and after C9.

## 3. How the 08:39 cutover was done (repeat for the next galion-one build)

1. Build on the box: `cargo build --locked --release -p galion-one --target x86_64-unknown-linux-musl` (static-pie, stripped). Check `git merge-base --is-ancestor` for the commits the build must contain.
2. Box tests: `cargo test -p galion-one`, fmt, clippy `-D warnings`; run the binary on a slot port with a temp home and store and warm ping off. `/health`, `/` and `/api/v1/services` must be 200, and it must still be alive after 12 s.
3. VPS sandbox (sandbox_vps_only): temporary non-root user, `/var/tmp/sandbox-test-<ts>`, loopback test ports. Check `/health` and `/` at 2 s and 32 s, then remove the user, folder and processes.
4. Owner approval for the live step (`docs/cutover-approval.md`).
5. Live: copy the binary to `/opt/galion/rust/bin/galion-one` (root 755) and check its sha256. Back up to `/var/backups/galion-one-<date>/` (old binary, unit, before-state, SHA256SUMS).
   Write `50-rust-bin.conf`, run `systemctl daemon-reload && systemctl restart galion-one`, and never overwrite the old binary.
6. After: sample 8091 `/health` and `/` every 15 s for 2 min (no restarts), check the public `galion.app`, `www`, `one.galion.app` and `one.galion.app/tanga/` from the box,
   and check that the nerve shows `galion-one` http 200. "Degrading: 1 restart(s)" from the planned restart clears after 1 h.

## 4. Rollback (on the VPS, as root; from the record)

```
rm /etc/systemd/system/galion-one.service.d/50-rust-bin.conf
systemctl daemon-reload
systemctl restart galion-one
```

This runs the old `/opt/galion/openmind/dist/bin-ws/galion-one` again (`090c15d0…`). If that file were ever lost, `/var/backups/galion-one-20261021/galion-one.old` is the same bytes (SHA256SUMS in the folder).

## 5. Open points

- Mouth 503 fix options (slot 181) and keep-warm pings that get Hermes 402 (about 4,300 a day) are owner decisions; nothing is changed here.
- P1 unit refresh (header, `[Install]`, `After=`) is staged on `swarm/vps-unit-hygiene` b985e43 and not installed (D-U1).
