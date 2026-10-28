<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# memory-search-local-time (wave-4 slot 367, F-354-1)

Branch `swarm/memory-search-local-time` off main-staging 38a07700 (box snapshot
3f015986, tree 4864589f byte-identical to GitHub 38a07700). Ports 22370-22379.
Nothing deployed. Times Europe/Zagreb (CET +01:00).

## Finding

F-354-1 (slot 354): `openmind-memory` `/search/conversations` printed the stored
`recorded_at` raw, e.g. `[2026-10-28T03:08:06.277Z]`. The L1 recall lines
(`format_memory_line`, opt-in `OPENMIND_MEMORY_RECALL_INCLUDE_L1`) also showed the
UTC wall clock (`2026-10-01 08:30` for a row written 10:30 CEST).

## Decision (D-367-1): store UTC, render Zagreb

* **Storage stays UTC** (`toISOString()`, `...sssZ`). Reasons: (1) 8420 wire
  parity, `recordedAt` in JSONL/`/capture` replies is asserted by the golden and
  real tests (`is_js_iso_utc`); (2) the store compares and sorts `recorded_at`
  as TEXT (`recorded_at > ?`, `ORDER BY recorded_at`), which is only correct with
  one fixed offset; mixed `+02:00`/`+01:00` strings would mis-order the repeated
  02:00-03:00 hour on 2026-10-25; (3) live data is already UTC (VPS DB stamps
  `2026-10-13T09:04:38.716Z`, `2026-10-13T09:19:53.385Z`), so no migration.
* **Display is Europe/Zagreb with explicit offset**, never `Z`/`+00:00`:
  * `/search/conversations`: `[2026-10-28T04:08:06+01:00]` (RFC 3339, seconds,
    same format as `zagreb_time::now_rfc3339`). Non-RFC 3339 legacy text is shown
    unchanged.
  * L1 lines: `2026-10-01 10:30 +02:00` (minutes + offset; the offset keeps the
    repeated 02:30 on 2026-10-25 unambiguous). Unchanged: date-only values,
    `T00:00` "date written as an instant" (stays `YYYY-MM-DD`, as before), and
    offset-less times (taken as already local).
* Zone: fixed Europe/Zagreb via the shared `zagreb-time` crate (rule 7); no new
  config knob (D-367-2: add `OPENMIND_MEMORY_TZ` only if a non-Zagreb deploy is
  ever needed).

## Code

* `zagreb-time`: `render_rfc3339(ts)` and `render_minutes(ts)` (chrono parse of any
  offset -> Zagreb). No new dependencies (chrono/chrono-tz already used).
* `openmind-memory` `engine.rs` `search_conversations` and `text.rs` `format_timestamp`.
* Tests: zagreb-time +2 (DST 2026-10-25 last CEST second / first CET second /
  repeated 02:30, +00:00 and +02:00 inputs, junk -> None, spring switch
  2026-03-29); text.rs +1 (DST, day rollover 22:15Z -> 00:15 +02:00, date-only,
  midnight, offset-less, F-354-1 sample); new `tests/local_time.rs` (engine +
  SQLite: both 02:30 rows, F-354-1 sample, offset row, legacy text; DB value
  stays `...Z`); golden.rs L1 line updated 08:30 -> `10:30 +02:00`.

## Real-life tests (box, release binary)

2026-10-28 06:27:02 +01:00, `GALION_REAL=1 OPENMIND_MEMORY_BIN=<release>
REAL_MEMORY_LOCALTIME_STAMPS="<2 live VPS stamps>=<GNU date oracle>"
cargo test --release -p openmind-memory --test real_local_time -- --nocapture`
(port 22370):

```
**[user]** Session: dst [2026-10-25T02:30:00+02:00]
**[user]** Session: dst [2026-10-25T02:30:00+01:00]
**[user]** Session: dst [2026-10-13T11:04:38+02:00]
**[user]** Session: dst [2026-10-13T11:19:53+02:00]
**[user]** Session: live [2026-10-28T06:27:02+01:00]
real VPS stamp 2026-10-13T09:04:38.716Z -> [2026-10-13T11:04:38+02:00] ok
real VPS stamp 2026-10-13T09:19:53.385Z -> [2026-10-13T11:19:53+02:00] ok
stored recorded_at 2026-10-28T05:27:02.519Z (UTC, unchanged wire/storage format)
test result: ok. 1 passed
```

Oracle: `TZ=Europe/Zagreb date -d <stored> +%FT%T%:z`. The live row is captured
over HTTP `/capture`, its stamp passes `zagreb_time::require_rfc3339` and lies
in the before/after `now_rfc3339` window; port closed after stop.

Existing real suite on the new binary, `OPENMIND_REAL_PORT_BASE=22371`
(22371-22376): `real_memory` 7/7 ok (every route, auth contract, migrations,
kill -9 crash recovery 1422 acked / 1423 rows integrity ok, perf 10k,
read-only); `real_live_8420_get_contract_readonly` (GALION_REAL_VPS=1) 1/1 ok.

## VPS (read-only)

06:26-06:27 +01:00: `openmind-memory.service` active, MainPID 291, NRestarts 0,
binary `/opt/galion/rust/bin/openmind-memory-aa77864` on 127.0.0.1:8420;
`GET /health` 200; `POST /search/conversations` without key -> 401 (key not read,
so live search text not compared; stored-format check done instead by grepping
only timestamp patterns from `/var/lib/memory-tencentdb/vectors.db`: 12 stamps,
all `...sssZ`, newest 2026-10-13 11:19:53 +02:00 (that is the last write of the
**legacy root DB**, not of the live store; see correction below)). Nerve
8670 `/state` 200 cycle 58; cortex live on 8674 `/health` 200. Nothing changed.

## Open

* D-367-1 (above) owner ack: display-only, storage UTC.
* D-367-2: no TZ knob.
* Live text changes only after an owner deploy of openmind-memory (not done).
* ~~The live memory DB has not been written since 2026-10-13~~ **Corrected
  2026-10-28 17:30 CET (+01:00), slot 406 (D-388-4), per the slot 388
  finding:** the grep above read the legacy root `vectors.db` (default-instance
  path, 4 rows, last row 2026-10-13 11:19:53 +02:00), which the live service
  does not use. The live store is
  `/var/lib/memory-tencentdb/instances/galion/vectors.db`: last l0 row
  2026-10-19 14:57:55 +02:00 (one `grok-bot-read-*` probe), last real capture
  2026-10-18 23:57:27 +02:00, last L1 record 2026-10-18 21:41:44 +02:00. It
  IS stale, because its only writer (the Python Hermes `memory_tencentdb`
  plugin) stopped with `hermes.service` on 2026-10-18 21:42:17 +02:00 (see
  docs/ops/memory-db-stale-diag.md, D-388-1). All 4 legacy-root rows are also
  in the live instance DB (docs/ops/legacy-vectors-archive.md).

## Gates (box, 06:28-06:33 +01:00)

`cargo fmt --check` ok; `cargo clippy --workspace --all-targets` (excl
openmind-sandbox, cgroup-detect) `-D warnings` ok; `cargo test --workspace`
(excl openmind-sandbox, cgroup-detect, hermes-tools) 379 suites 2009 passed /
0 failed / 18 ignored (base 378 / 2005); `scripts/no-python.sh` ok;
`scripts/spdx-scan.sh` 752 scanned, 0 missing; repo-check PASS 0 errors / 83
warnings (same as base); openmind-memory release x86_64 + aarch64 musl static
(1747696 / 1708816 B); armv7-musleabihf check ok; aarch64-android check ok
(zagreb-time plain; openmind-memory with a temporary zig cc wrapper for the
bundled SQLite C, not committed). No new dependencies, Hermes not used.
