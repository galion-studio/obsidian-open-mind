<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: hermes-rs (hermes-gateway, 127.0.0.1:8642)

- Status: **CUT OVER, live since 2026-10-21 09:46:22 CEST (+02:00)** (index [README.md](README.md) §2 C10b + C10e). Legacy Python `hermes.service` **masked** 08:24:00 (§2 C6).
- **Approved: Yes.** Binary swap: Slavik ~09:35 (t175 permission, relayed by Mate 09:35); NERVE_URL drop-in: Mate t184 09:25:15 (D3); legacy mask: Slavik t172 08:23:38.
  Records: `swarm/nerve-eventlog-deploy@0ddce9bafe29771ca5dd1e16d2c965a5ba95155a:docs/ops/nerve-eventlog-nerveurl-20261021.md`,
  `swarm/hermes-legacy-retire@7622b8bc9ed188ae4746e08aedde6c76eeb634d9:docs/ops/hermes-legacy-retire.md`; CALL-RECORD 08:24:55, 09:32:33, 09:43:55.
- Paid model: **not touched**. The build has no `coding` feature, and config.yaml, caps and MCP are unchanged. Nothing in this doc turns on Hermes's paid AI model.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from the records and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| unit | `/etc/systemd/system/hermes-rs.service` (1,822 B, sha256 `acf5dae845d4a0438647abbaecab4d15daf9f95cdbb8696e27d72d5c2cc4736c`, unchanged since 2026-10-18 21:41), enabled, active, `Restart=on-failure` 3 s, `MemoryMax=64M`, `TasksMax=64`, `StateDirectory=hermes-rs`, `ReadOnlyPaths=/opt/galion/hermes`, `Conflicts=hermes.service` |
| exe | `/opt/galion/rust/bin/hermes-gateway-aa77864`, 1,841,904 B, sha256 `eb691b2967eb03758dcc6b2b6f46fe0a9d45d779b4d4bf937a28f37b19633e2f`, RSS 3.3 MB, NRestarts 0 |
| source | aa77864c2744fad56d11e84476d5bfab1fd25bf7, `--no-default-features --features sqlite` (no `coding`), x86_64 musl static, carries `nerve-client` |
| old binary (rollback) | `/opt/galion/rust/bin/hermes-gateway`, 2,173,696 B, sha256 `cd344b167dc65425b63bcb3765f439ee70a0bf47f355126cac8a75f3192ac0f6`, untouched |
| legacy | `hermes.service` → `/dev/null` (masked), unit kept in `/var/backups/hermes-legacy-20261021/` (sha256 `50d882e2f52b59f5…`) |

Drop-ins (0644 root):

| File | sha256 | Content |
|---|---|---|
| `hermes-rs.service.d/50-rust-bin.conf` (09:46:21) | `910cddb2eb9fc187fe440857c89a1308b14d5d18ca2a011253109396e3d9f383` | `ExecStart=` reset, `ExecStart=/opt/galion/rust/bin/hermes-gateway-aa77864`, `SyslogIdentifier=hermes-gateway` |
| `hermes-rs.service.d/60-nerve-url.conf` (09:32:33) | `833acdfbb2a68367e95b706f29478d13bc6065a950d6a508f35ab404a96d9850` | one `Environment=` line (`NERVE_URL`, loopback nerve). Its comment still says "inert"; it is active since 09:46 (stale, vps-unit-hygiene P6) |

Backups: `/var/backups/hermes-rs.service-20261021-093233`, `/var/backups/hermes-rs.service-20261021-094621` + `.d/` + bin-sha. All present at 17:22.

## 2. Verify (on the VPS, GET only)

```
systemctl show hermes-rs -p ActiveState -p NRestarts -p DropInPaths
sha256sum /proc/$(systemctl show hermes-rs -p MainPID --value)/exe       # eb691b29…633e2f
curl -s -o /dev/null -w '%{http_code}\n' -m 5 http://127.0.0.1:8642/health   # 200
systemctl show hermes.service -p UnitFileState                            # masked
```

At 17:22: active, NRestarts 0, `/health` 200 (122 B), nerve component `hermes-rs:healthy`, `hermes.service` masked.
The nerve also reports `hermes-rs: spend cap hit (provider calls refused)`: the existing cap is refusing paid provider calls. This doc changes nothing about it.

## 3. Rollback (on the VPS, as root)

Level 1, back to the 2026-10-18 binary (from the record):

```
rm /etc/systemd/system/hermes-rs.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart hermes-rs
```

Level 1b, also drop NERVE_URL:

```
rm /etc/systemd/system/hermes-rs.service.d/60-nerve-url.conf && systemctl daemon-reload && systemctl restart hermes-rs
```

**Do not use** `/root/galion-switch-20261018-2140/rollback.sh hermes`. It runs `systemctl enable --now hermes.service`, which fails while the unit is masked.
If the unit were unmasked, starting it would stop hermes-rs (`Conflicts=`) and bring back the uncapped paid Python Hermes (hermes-legacy-retire trap).
Undoing the mask has its own line in [README.md](README.md) §2 C6 and needs an owner decision.
`rollback-gaps.sh memory` is also out of date: it reinstalls a pre-gaps 2026-10-18 binary over `/opt/galion/rust/bin/hermes-gateway` and restores an old `.env`.

## 4. Open points

- Under load, hermes-gateway RSS grows about +9.5 MB/min (slot 187, CALL-RECORD 13:19:26). With `MemoryMax=64M`, a long burst would hit the limit. Slot 198 `hermes-mem-growth` is measuring this; it was still running at 17:19.
- Deploying the 1 s nerve-client deadline (slot 187 D-T33-1) and the early `--version/--check` flags (slot 194) needs a new binary and a new cutover. Neither is approved.
