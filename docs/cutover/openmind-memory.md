<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: openmind-memory (127.0.0.1:8420)

- Status: **CUT OVER, live since 2026-10-21 09:45:26 CEST (+02:00)** (index [README.md](README.md) §2 C10b + C10d).
- **Approved: Yes.** Binary swap: Slavik ~09:35 (deploy approved, t175 permission), relayed by Mate 09:35; NERVE_URL drop-in: Mate t184 09:25:15 (D3).
  Record: `swarm/nerve-eventlog-deploy@0ddce9bafe29771ca5dd1e16d2c965a5ba95155a:docs/ops/nerve-eventlog-nerveurl-20261021.md`; CALL-RECORD 09:25:15, 09:32:33, 09:43:55.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 `docs-cutover-missing` from the record above and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00),
  evidence in [`../real-tests/docs-cutover-missing.md`](../real-tests/docs-cutover-missing.md). Nothing was changed on the VPS to write it.

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/openmind-memory.service` (1,095 B, sha256 `42517408ae24c5af9829ec8d0c3f6d307f441d4ee814f1cb7301421abe055902`, unchanged since 2026-10-18 21:41), enabled, active, `Restart=always` 5 s, `User=root`, `MemoryMax=64M`, `TasksMax=64`, `Conflicts=memory-gateway.service` |
| exe | `/opt/galion/rust/bin/openmind-memory-aa77864`, 1,702,640 B, sha256 `18e35ab49a76702b47742b8c5d4f95f29c3956839979e7df7a6ede671f1d2bb6`, RSS 3.4 MB, NRestarts 0 |
| source | `swarm/nerve-eventlog-deploy` aa77864c2744fad56d11e84476d5bfab1fd25bf7 (= nerve-t36-consumer 334beaa + 364d266 + slot-157 DST fix), x86_64 musl static, carries `nerve-client` |
| data | `/var/lib/memory-tencentdb` (the only `ReadWritePaths`), same data as the 2026-10-18 build |
| old binary (rollback) | `/opt/galion/rust/bin/openmind-memory`, 1,980,096 B, sha256 `473ae4a3b13ceabba007a89f7e3389df1c91fb6e7986bf2efd5890304ff12e37`, untouched (rust-v1 2f7d48f line, installed 2026-10-18) |

Drop-ins (both 0644 root):

| File | sha256 | Content |
|---|---|---|
| `openmind-memory.service.d/50-rust-bin.conf` (09:45:26) | `8ea8565d025899182e0661a3144e91567ecfccc489ad4b91b40e12771b1e6b64` | `ExecStart=` reset, `ExecStart=/opt/galion/rust/bin/openmind-memory-aa77864`, `SyslogIdentifier=openmind-memory` (added at the 09:45:26 second restart so the journal ident stays the unit name) |
| `openmind-memory.service.d/60-nerve-url.conf` (09:32:33) | `fdd150c7377f0f492b52f12e8e3045812fd1e755642aaea99c919a2187bc2642` | one `Environment=` line, `NERVE_URL` = the loopback nerve (`http://127.0.0.1:8670`, per the record). Its comment still says "inert"; it is active since 09:45 (stale comment, vps-unit-hygiene P6) |

Backups: `/var/backups/openmind-memory.service-20261021-093233` (unit before the NERVE_URL drop-in),
`/var/backups/openmind-memory.service-20261021-094509` + `.d/` (holds `60-nerve-url.conf`) + bin-sha. All present at 17:22.

## 2. Verify (on the VPS, GET only)

```
systemctl show openmind-memory -p ActiveState -p NRestarts -p DropInPaths
sha256sum /proc/$(systemctl show openmind-memory -p MainPID --value)/exe   # 18e35ab4…1d2bb6
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8420/health   # 200
curl -s -m 5 http://127.0.0.1:8670/state | grep -c '"openmind-memory'        # > 0: the nerve sees it
```

At 17:22: active, NRestarts 0, both drop-ins, `/health` 200 (277 B), nerve component `openmind-memory:healthy`;
galion-health `/status.json` lists `openmind-memory` ok.

## 3. Rollback (on the VPS, as root)

Level 1, back to the 2026-10-18 binary (from the record; old binary still in place):

```
rm /etc/systemd/system/openmind-memory.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart openmind-memory
```

Level 1b, also drop NERVE_URL (harmless with the old binary, which ignores it):

```
rm /etc/systemd/system/openmind-memory.service.d/60-nerve-url.conf && systemctl daemon-reload && systemctl restart openmind-memory
```

Not recommended: `/root/galion-switch-20261018-2140/rollback.sh memory` goes back to the Node `memory-gateway.service` (pre-Rust, 2026-10-18).
It has not been rehearsed since and the Node service is disabled. Use it only on an explicit owner decision.

## 4. Open points

- The 60-nerve-url.conf comment is stale. Fixing it is proposal P6 on `swarm/vps-unit-hygiene` b985e43, which needs an owner decision.
- The unit runs as `User=root`, like the 2026-10-18 install. Hardening it is a separate change and is not proposed here.
