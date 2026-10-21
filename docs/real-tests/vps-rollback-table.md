<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-test record: vps-rollback-table (wave-4 slot 210)

Docs-only slot; the "real test" is checking every fact in `docs/ops/rollback-table.md` against the live VPS, read-only.
All times CEST (+02:00), 2026-10-21. Method: `ssh slavik-vps 'nice -n 10 bash -s' < <batch>` (same alias as slots 182/184/203).
Raw outputs (box only, no secrets): `/workspace/swarm/logs/vps-rollback-table/{probe1,probe2,probe3,probe4-timers,probe5,recheck}.txt`, manifest `manifest.sha256`.

## Probes

| Batch | Time | What (all read-only) |
|---|---|---|
| probe1 | 20:20:39–20:20:41 | `systemctl show` of 15 units (state, MainPID, NRestarts, drop-in paths), `/proc/<pid>/exe` sha256, sha256 + mode + mtime of every `*.service.d/*.conf`, the 4 new unit files, cloudflared config (sha, mode, 38 hostname rules), `ls` of all backup paths, sha256 of `/opt/galion/rust/bin/*`, 13 loopback GETs, nerve `/state` head, cortex `/health`, `ss -ltn`, WC HEAD |
| probe2 | 20:21:18–20:21:19 | comment (`#`) lines of every drop-in and new unit (rollback lines, approval notes; secret-like words filtered), `find` listings of 12 backup dirs, sha256 of the unit/conf copies in them, the 3 `.bin-sha` files, tuner/galion-deploy backups, names/modes in `/etc/gaia-cortex` + `/var/lib/gaia-cortex` (**`river.env` not read**), journal start/stop lines of gaia-cortex and galion-nerve |
| probe3 | 20:21:51 | `grep -c` of switch strings in binaries (`ONE_NO_WARM`, `NERVE_DRY_RUN`, `NERVE_EVT_ACTIONS`, `events/stream`, `HERMES_RS_DAILY_USD_CAP`), old galion-one sha, base unit `ExecStart`/`NERVE_DRY_RUN`, cortex `llm-spend.json` (counts only), last 2 cortex decisions, running nerve env switches |
| probe4 | ~20:22 | `systemctl list-timers`, `galion-backup.timer` `OnCalendar`, `timedatectl` (VPS = Etc/UTC) |
| probe5 | 20:22:56 | sha256 of remaining backups (original cloudflared config, legacy hermes unit, galion-one / nerve / hermes-rs / memory unit copies) and the live base unit files; `galion-deploy --version`; tuner `/` |
| recheck | 20:26:36–20:26:37 | `sha256sum -c` of the 52-line manifest (every sha256 the doc cites, with its path); existence of all 61 paths named in the doc; running exe + NRestarts of 9 units; 13 GETs; nerve `/state`; cortex `/health` |

## Results

- **sha256: 52/52 OK** (`SHA_ALL_OK 52`). Every full sha256 in the doc (37 distinct values) is in the manifest or a probe log: 0 unprobed.
- **Paths: 60/61 present**; the one absent is `/var/lib/galion-nerve/mode`, which the doc says must stay absent (it would override dry-run).
- **Running binaries 20:26:37** (exe sha256 first 16): galion-nerve `c15be071f8263175` (bb287cc, event mode, PID 772576), openmind-memory `18e35ab49a76702b`,
  hermes-rs `eb691b2967eb0375`, galion-one `24cb4976b1325ae0`, openmind-queue `8a7f7faaaa864e0a` (NRestarts 1), gaia-cortex `fbe83e136789b37b`,
  galion-nerve-shadow `3a566c56dfea79cf`, tuner `89c7fcbcb54c5108`, cloudflared `03f1f25d1cc93b9a`. MainPID and NRestarts identical to 20:20:39 (nothing moved while writing).
- **GETs 20:26:37**: 8670 /health /events /stats /inventory 200; 8420 200; 8642 200; 8091 200; 8002 **503** (old "loading" stub, unchanged); 19120 /healthz 200;
  8674 /health 200; 8672 200; 8650 200; 20241 /ready 200 (`cloudflared_tunnel_ha_connections 4` at 20:21).
- **Nerve** (`/state`): openmind-nerve, mode dry-run, cycle 7 (event-mode heartbeat), score 66 grade D. Env: `NERVE_DRY_RUN=1 NERVE_MODE=event NERVE_EVT_ACTIONS=dry-run NERVE_EVT_ESCALATE=delegate`.
- **Cortex** (live since 19:06:55): `{"mode":"dry-run","llm_enabled":true,"nerve":true,"events_in":14,"incidents_open":0,"escalations_open":0}`; LLM ledger 1 call,
  $0.000031, 166 in / 23 out tokens, at 19:06:45; 9 decision lines, all `decided_by: rules`, `dry_run: true`, no actions.
- **Base unit files** of galion-nerve, hermes-rs, openmind-memory, galion-one = their backups byte for byte (all changes are drop-ins).
- **Stack facts used in §2**: running nerve exe is bb287cc although `50-rust-bin.conf` names aa77864 (80 overrides 50); `galion-nerve.service` has `NERVE_DRY_RUN=0`;
  `NERVE_DRY_RUN` string present in all 3 nerve binaries; `ONE_NO_WARM` present in both galion-one binaries; `events/stream` absent from aa77864 (so cortex is blind after an R22 rollback).
- **Timers**: galion-backup `*-*-* 00/6:17:00` UTC (+5 min random) → 02:17 CEST on 2026-10-25, inside F1; e2scrub_all 2026-10-25 03:10 UTC = 04:10 CET, just outside.

## Not done / limits

- No rollback line was run (VPS read-only; G2). Their correctness is checked only as "every file they name exists and has the stated sha256".
- C8/R17 cloudflared contents were compared by sha256 only (config not printed).
- `/etc/gaia-cortex/river.env` and every `.env` were not opened.
- Disclosure: the recheck batch wrote the 52-line sha256 manifest (hashes + paths, no secrets) to `/tmp/.s210m` on the VPS for `sha256sum -c` and removed it in the same batch
  (`ls` afterwards: "No such file"). Nothing else was written on the VPS.
- Cortex live state: observed through its own `/health` and files; the nerve was observed through `/state`, `/stats`, `/inventory`.
