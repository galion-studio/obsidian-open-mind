<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# openmind-memory: "DB not written since Oct 13" diagnosis (wave-4 slot 388)

Branch `swarm/memory-db-stale-diag` off `memory-search-local-time` 9dda6146.
Probes 2026-10-28 13:27-13:35 CET (+01:00), VPS **read-only** (systemctl
status/cat/show, journalctl, ls/find mtimes, `ss`, loopback GET /health and
nerve GET /state, scp *copy* of the DB to the box). Nothing on the VPS was
written, restarted or reconfigured; `/opt/galion/openmind` not touched; no
secret values read (env file printed as names only).

## Short answer

1. **The flag read the wrong file.** The live service writes
   `/var/lib/memory-tencentdb/instances/galion/vectors.db`
   (`OPENMIND_MEMORY_INSTANCE_ID` is set, `Config::db_path` =
   `<data_dir>/instances/<id>/vectors.db`; pid 291 holds exactly that DB +
   WAL + SHM open). The "Oct 13" stamps (2026-10-13 11:04:38 / 11:19:53
   +02:00, slot 367) are the last of **4 rows in the legacy root
   `vectors.db`** (the `default`-instance path, 126,976 B, mtime
   2026-10-13 11:20:17 +02:00). Its `-shm` mtime looked fresh (2026-10-28
   13:22:03 CET) only because `galion-backup.timer` ran at that second and
   opened it as a reader.
2. **But the live store really is stale too, since 2026-10-18/19.** Live
   instance DB: 2,610 l0 rows, 216 l1 rows. Last l0 row 2026-10-19 14:57:55
   +02:00 (one 2-line `grok-bot-read-*` probe capture, = WAL mtime); last
   real traffic 2026-10-18 23:57:27 +02:00 (conversations/2026-10-18.jsonl,
   172 lines that day); last L1 record 2026-10-18 21:41:44 +02:00.
3. **Cause: the writer was retired, not the store.** The only regular
   writer was the Python Hermes `memory_tencentdb` plugin
   (`/opt/galion/hermes/config.yaml` still says `memory.provider:
   memory_tencentdb`, `memory_enabled: true`; plugin path
   `plugins/memory/memory_tencentdb` now only in the archive
   `/var/backups/hermes-legacy-20261021/tooling-hermes-agent.tar`).
   `hermes.service` stopped 2026-10-18 21:42:17 +02:00 (now masked);
   hermes-rs (live on :8642 since then) has **no memory client at all** (no
   reference to 8420, `/capture`, `/recall` or the `memory:` config block
   in `crates/hermes-*`). So since the cutover nothing captures and nothing
   recalls. Session-key mix on 2026-10-13 confirms who wrote: 1,097
   `api-*`, 150 `one-warm`, 88 `one-voice` (all Hermes gateway sessions).
4. **The server itself is fine.** `openmind-memory.service` active,
   NRestarts 0, started 2026-10-22 23:46:47 +02:00 (boot); GET /health 200
   `read_only:false fts:true`; nerve /state `openmind-memory` `healthy`
   since 2026-10-27 18:15:01 +01:00 (rss 660 kB, 0 errors). 0 established
   connections to :8420 in 15 samples over 30 s. Box real test: the same
   code writes the instance DB + JSONL on the first real `/capture`.
5. **Nobody noticed because nothing measures freshness.** nerve only sees
   process/HTTP health; galion-one's memory panel counts rows. Hence the
   staged tool below.

## Timeline (Europe/Zagreb)

| When | What | Source |
|---|---|---|
| 2026-10-13 11:19:53 +02:00 | last row in legacy root `vectors.db` (4 rows total) | box copy, `freshness default` |
| 2026-10-13 13:47 +02:00 | 1,979 conversation lines that day (Node memory-gateway era) | conversations/*.jsonl line counts |
| 2026-10-18 21:41:44 +02:00 | last L1 record (records/2026-10-18.jsonl) | mtime + l1 max(updated/created) |
| 2026-10-18 21:42:17 +02:00 | Python `hermes.service` stopped (exit 1 on SIGTERM), later masked | journalctl -u hermes |
| 2026-10-18 21:42:49 +02:00 | Rust openmind-memory started on 127.0.0.1:8420, instance DB | journalctl -u openmind-memory |
| 2026-10-18 23:57:27 +02:00 | last real capture (sessions openmind-boss-*, uuid, gaps-verify-*) | conversations/2026-10-18.jsonl |
| 2026-10-19 14:57:55 +02:00 | last l0 row: 1 user + 1 assistant, `grok-bot-read-*` probe | conversations/2026-10-19.jsonl, WAL mtime |
| 2026-10-21 09:45 +02:00 | 2 restarts, binary -> `openmind-memory-aa77864` (nerve-client) | journal |
| 2026-10-22 23:46:47 +02:00 | current pid 291 (boot), NRestarts 0 | systemctl show |
| 2026-10-28 13:22:03 +01:00 | galion-backup run; touched legacy root `vectors.db-shm` (reader) | timers + mtime |

## Staged fix (this branch; NOT deployed)

`openmind-memory freshness <data_dir> [instance_id] [max_age_hours]`
(`crates/openmind-memory/src/freshness.rs`), read-only:

- uses the server's own path rule (`Config::db_path`), prints which DB it
  read as `db[instance]=...`, and reports a root `vectors.db` next to an
  instance store as `stray_root_db=yes` / `db[legacy-root]` that **never
  counts** toward freshness;
- newest write = max of l0 `MAX(recorded_at)`, l1 `MAX(updated/created)`,
  DB mtime, non-empty WAL mtime, newest `conversations/` and `records/`
  JSONL; an **empty** `-wal` is marked `reader-touched, ignored` (SQLite
  readers and galion-backup create/touch it);
- Europe/Zagreb times with offset; counts and file names only, never
  content or session keys; exit 0 fresh, 3 stale, 1 error, 64 usage.

Verdict on the copy of the live data dir (13:32 CET):

```
db[instance]=.../instances/galion/vectors.db
  db_mtime=2026-10-18T23:42:30+02:00
  wal_mtime=2026-10-19T14:57:55+02:00
  l0_rows=2610 l0_last=2026-10-19T14:57:55+02:00
  l1_rows=216 l1_last=2026-10-18T21:41:44+02:00
conversations_last_file=2026-10-19.jsonl mtime=2026-10-19T14:57:55+02:00
records_last_file=2026-10-18.jsonl mtime=2026-10-18T21:41:44+02:00
stray_root_db=yes (not the live instance; ignored for freshness)
db[legacy-root]=.../vectors.db
  db_mtime=2026-10-13T11:20:17+02:00
  wal_mtime=2026-10-28T13:32:23+01:00 (empty; reader-touched, ignored)
  l0_rows=4 l0_last=2026-10-13T11:19:53+02:00
  l1_rows=0 l1_last=none
newest_write=2026-10-19T14:57:55+02:00 age_h=215.6
state=stale (max_age_h=24)          (exit 3)
```

## Owner decisions

- **D-388-1 restore a writer (main one).** (a) give hermes-rs a memory
  client: `/recall` before the model call and `/capture` after the reply
  against 127.0.0.1:8420 with the existing Bearer (same wire as the
  `memory_tencentdb` plugin; behind a flag, default off, its own slot);
  (b) accept frozen memory and set `OPENMIND_MEMORY_READ_ONLY=1` until V2;
  (c) retire openmind-memory. Recommended: (a) - without it Hermes has had
  no long-term memory since 2026-10-18 21:42 +02:00, and the config still
  claims it does.
- **D-388-2 legacy root `vectors.db`** (126,976 B, 4 rows, 2026-10-13):
  archive and remove (live file change, owner), or keep and rely on
  `freshness` labelling it. galion-backup currently backs it up and
  touches its SHM every run.
  -> slot 406: read-only check + owner runbook in
  docs/ops/legacy-vectors-archive.md (D-406-1..4); nothing changed live.
- **D-388-3 freshness alert:** deploy this build and have nerve/cortex (or
  a timer) run `freshness /var/lib/memory-tencentdb galion 24`, alerting on
  exit 3. Needs a deploy; not done here.
- **D-388-4 record fix:** slot 367 / the 07:08 flag said "last write Oct 13";
  correct is: legacy DB Oct 13, live DB last write 2026-10-19 14:57:55
  +02:00 (probe), last real capture 2026-10-18 23:57:27 +02:00.
  -> done in slot 406 (docs/real-tests/memory-search-local-time.md).
