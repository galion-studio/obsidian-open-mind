<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: memory-freshness nerve sensor (wave-4 slot 405, D-388-3 prep)

Branch `swarm/memory-freshness-sensor`, base `1d7164b4` (memory-db-stale-diag).
All times are Europe/Zagreb (CET, +01:00). **Nothing deployed.** The sensor is off by default.

## What it does
The new staged nerve sensor `memory-freshness` runs `openmind-memory freshness <data_dir> <instance> <max_age_h>`
(slot 388 CLI, read-only SQLite open, counts + times only, no content) as a child process.
It uses argv only, env_clear, a fixed PATH, null stdin/stderr, stdout capped at 16 KiB, and a kill on timeout,
and parses only allow-listed keys. A finding needs exit code and `state=` to agree:
- exit 3 + `state=stale` → Bleeding `memory-stale` warn finding plus a warn Signal → incident → alert
  (journald + `alerts.json`; Telegram only if NERVE_TELEGRAM_ENV is set).
- exit 0 + `state=fresh` → silent.
- anything else (error, mismatch, timeout) → info `memory-freshness-unknown`, no incident.
- `NERVE_MEMFRESH_BIN` unset → the sensor is not registered (default off). It is appended last, so existing
  sensor order and incident ids are unchanged.

## Box run (2026-10-28 ~17:20–17:42)
`GALION_REAL=1 cargo test --release -p openmind-memory --test real_memory_freshness_sensor` — real release nerve and
memory binaries, real HTTP, throwaway DBs under a temp dir, ports 22750–22759.
- Leg A, stale throwaway store: `memory-stale` at cycle 2 (5.0 s), title "memory store 'galion' not written for 219.5 h …",
  incident opened, warn alert `incident:<id>:warn` in alerts.json, no path / "vectors.db" leaks in findings/alerts.
- Leg B, a real `/capture` through the memory server → the store is fresh → finding gone, incident resolved (cycle 5).
- Leg C, fresh store → no memory finding in 3 cycles.
- Leg D, `/bin/false` as BIN → info `memory-freshness-unknown`, no alert.
- Leg E, sensor unset → not registered.
- Sensor cost: cycle_ms 28, sensor ≈46 µs (the child runs on a background thread).
- Result: 2 passed (the live-copy test skips without `GALION_MEMORY_SNAPSHOT_DIR`).

Mutation control (17:41): exit 3 mapped away from Stale → leg A **FAILED** as expected; reverted.

## VPS run (2026-10-28 17:39:37–17:40:19, throwaway, as `nobody`)
x86_64 musl binaries in `/var/tmp/memfresh-405-<ts>` (mode 700). The live store was **copied** (`cp -p` of the instance
vectors.db + wal + shm and the stray root DB); conversations/records were recreated as **empty** files with the same
mtimes (no content copied). Run with `runuser -u nobody -- env -i …`.
- Live copy: CLI exit 3, `newest_write=2026-10-19T14:57:55+02:00 age_h=219.7 state=stale (max_age_h=24)`;
  nerve on :22755 raised `memory-stale` at cycle 2 (5.3 s), "not written for 219.7 h (9.2 days, limit 24 h)",
  evidence `instance_l0_rows=2610`, "stray legacy root DB present (ignored for freshness)". No paths leaked.
- Throwaway legs A–E: all ok (stale → incident at cycle 2; capture → fresh 17:39:43 → resolved cycle 5; fresh silent;
  unknown info; off not registered). `test result: ok. 2 passed; 41.34 s`.
- After: live DB file size+mtime unchanged (diff empty), openmind-memory MainPID 291 NRestarts 0 unchanged, live nerve
  `/state` 200, no `nobody` processes, no listeners on 22750–22759, temp dir removed.
- Note: an earlier attempt at 17:37:38 passed the live-copy leg but failed leg A with "dummy port free: AddrInUse"
  (the test's dummy listener was bound twice in one process). Fixed with a `Once` guard and re-run as above. A run at 17:38:58
  exited 126 before any test ran (it never started); re-staged and re-run.
- Not from this slot: the live galion-nerve was switched by another agent to `openmind-nerve-9baa450`
  (binary mtime 17:37:46, restart 17:40:13, NRestarts 0) during this window. This slot installed and restarted nothing.

## Gates
fmt ok; workspace clippy `-D warnings` (excluding openmind-sandbox, cgroup-detect) ok; workspace tests 2047 pass /
0 fail / 18 ignored; x86_64 + aarch64 musl release build ok; armv7 musleabihf check ok; android (aarch64/armv7)
check ok for openmind-nerve. For openmind-memory it fails only because the NDK clang for bundled libsqlite3-sys is missing (box
toolchain, same as base). no-python ok; SPDX headers present.
