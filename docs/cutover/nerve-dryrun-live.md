<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Deploy record: live nerve reflexes to dry-run (D-T32-R1), 2026-10-21 +02:00

Owner decision: Mate, 2026-10-21 17:04 +02:00 (D-T32-R1). The live galion-nerve (127.0.0.1:8670) only
simulates its reflexes: it logs and reports what it would do and never restarts, rolls back or changes a
live service. Live auto-fix comes back only after dry-run proves reliable, by a separate decision.

## Finding: the running build already has the switch

Running build: `/opt/galion/rust/bin/openmind-nerve-aa77864` (334beaa + slot-157 DST fix), sha256
`f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68`, via drop-in `50-rust-bin.conf`.

Source at aa77864 (`crates/openmind-nerve`):

- `config.rs`: `NERVE_DRY_RUN` (default 1) -> `Config::dry_run_env`.
- `reflex.rs` `Reflex::dry_run()`: `<data_dir>/mode` = `live` / `dry-run` overrides the env at run time,
  else `NERVE_DRY_RUN`.
- `reflex.rs` `execute()`: in dry-run a restart records `would run: systemctl restart --no-block <unit>` and a
  rollback `would run: <galion-deploy> <svc> <backup> (via systemd-run)`; neither `systemctl` nor
  `systemd-run` is spawned; the live restart and rollback budgets are untouched. Audit lines, alerts and
  `/state.reflex.recent` still carry the action with `"mode": "dry-run"`; `/state.mode` reports `dry-run`.
- No other code path in the nerve changes a service (the remaining `run()` calls are `systemctl show`,
  `journalctl`, `df` and `gaia status`, all read-only). The JSONL event log (`POST /event`, `GET /events`)
  is independent of the reflex mode.
- c1a9be8 (slot 186, `swarm/nerve-t32-reflex-safety`) adds `force_dry` for `replay --reflex` only and a
  separate dry-run rollback limit; neither is needed for a safe live dry-run, so no new binary was built.

The unit sets `Environment=NERVE_DRY_RUN=0`; `/var/lib/galion-nerve/mode` was absent. So the change is one
drop-in, no code change. This branch (`swarm/nerve-dryrun-live`, base aa77864) carries only this record.

## Steps (VPS, all times +02:00)

1. 17:06:37 backup `/var/backups/galion-nerve.service-20261021-170637/`: `galion-nerve.service`,
   `galion-nerve.service.d/{50-rust-bin.conf,60-nerve-eventlog.conf}`, `binary.sha256`, `pre-state.txt`
   (MainPID 87026, NRestarts 0).
2. 17:07:06-17:07:51 sandbox: temporary system user `sbxdry1707`, `/var/tmp/sandbox-test-20261021-170706`
   (0700), copy of the same binary (same sha256), `127.0.0.1:22040`, `NERVE_DRY_RUN=1`, interval 5 s,
   fail cycles 2, component file with one component on the real allow-listed unit `link-watch-rs.service`
   whose health port is a dead `:22041` (forced reflex condition), deploy tool/backups/switch dir pointed at
   non-existent sandbox paths, journal off, own event log. Result: `/health` 200, `/events` 200, mode
   `dry-run`; audit 17:07:12, 17:07:17, 17:07:22 `would run: systemctl restart --no-block link-watch-rs.service`
   then 17:07:27 `rate-limited` (budget 3/h, dry counter); alerts `[dry-run] restart ...`;
   link-watch-rs MainPID 1359406 / NRestarts 0 before and after (unchanged). Cleanup: processes 0, user and
   group removed, directory removed, port free.
3. 17:08 drop-in `/etc/systemd/system/galion-nerve.service.d/70-nerve-dryrun.conf` (0644, sha256
   `3fe865c9fa1f1bc32e1015038141d36fdb132b2c2877e6db48fc19ede68030f1`):

   ```
   [Service]
   Environment=NERVE_DRY_RUN=1
   ```

   plus comments (decision, the `mode` file caveat, backup path, rollback).
4. `systemctl daemon-reload`; effective env `NERVE_DRY_RUN=1`, `NERVE_EVENT_LOG=/var/lib/galion-nerve/events.jsonl`.
5. 17:08:27 `systemctl restart galion-nerve`: active, MainPID 606906, NRestarts 0, ExecStart still
   `/opt/galion/rust/bin/openmind-nerve-aa77864`; journal `listening on 127.0.0.1:8670 interval=30s mode=dry-run ... components=15`.

## Verification

| check | 17:09:55 | 17:25:01 |
|---|---|---|
| `/health` | 200 | 200 |
| `/events` | 200 | 200 |
| `/state.mode` | dry-run | dry-run |
| cycle | 3 | 34 |
| hermes-rs signals (last_seen, total since restart) | 17:09:22, 5 | 17:24:53, 82 |
| openmind-memory signals | 17:09:26, 2 | 17:24:26, 32 |
| `reflex.recent` | empty | empty |
| live / dry budget used (all units) | 0 / 0 | 0 / 0 |
| `audit.jsonl`, `reflex.json`, `mode` in /var/lib/galion-nerve | absent | absent |
| journal since restart: restart / rollback / systemd-run / `[live]` lines | 0 | 0 |
| `galion-nerve-rollback-*` units | 0 | - |

Service MainPID / NRestarts before (17:06:37) and after (17:25:01): hermes-rs 89484/0, openmind-memory
88565/0, link-watch-rs 1359406/0, tuner 4004553/0, galion-one 29782/0, console 2501864/0, galion-health
1374810/0, galion-desk 231/0 - unchanged. cloudflared 22835 -> 611846 at 17:08:54: a separate root ssh
session restarted it (`forge-link` task, approved by Mate 17:06 +02:00, logged in CALL-RECORD at
17:10:18); the nerve's journal and audit show no action, and it was in dry-run from 17:08:27.

Not changed: any other unit, budgets, HADES mode, Cloudflare DNS, the paid model, `/opt/galion/openmind`.

## Rollback

```
rm /etc/systemd/system/galion-nerve.service.d/70-nerve-dryrun.conf
systemctl daemon-reload
systemctl restart galion-nerve
```

(returns to `NERVE_DRY_RUN=0` = live reflexes; full original unit set in the backup directory above.)

## Caveat for the owner

`/var/lib/galion-nerve/mode` containing `live` overrides the env at run time (by design in aa77864), so
anyone with root can switch live reflexes back on without touching the unit. It is absent now. If the
dry-run must not be overridable, options: (a) also write `dry-run` to that file (same override strength);
(b) a small code change so `NERVE_REFLEX_DRYRUN=1` (or c1a9be8's `force_dry`) wins over the file. Neither
was done here.
