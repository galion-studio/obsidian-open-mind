<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve T3.6: consumer compatibility (wave-4 slot 180)

Base: swarm/nerve-wiring-rebase 8046c41 -> swarm/nerve-t36-consumer, 2026-10-21 (CEST +02:00).
Question: does every consumer of the nerve's endpoints keep working when the
live `galion-nerve` (built from 2f5a730) is replaced by `openmind-nerve`?

Tests: `crates/openmind-nerve/tests/consumer_compat.rs` (11 tests, real loopback
socket, run in every `cargo test`) and `crates/openmind-nerve/tests/real_consumer_compat.rs`
(`GALION_REAL=1`: release binary on the box, ports 20500-20502; plus
`GALION_REAL_VPS=1`: read-only GETs on the live VPS nerve). Both use the
consumers' **own** code (galion-realtest `live::VPS` + `live::judge` + `http`,
nerve-client `Client` + `body`, gaia-live `nerve::{post,tail,healthy}`,
galion-deploy `health_table` / `rollback_health`), so a change on either side
breaks a test. Evidence: docs/real-tests/nerve-t36-consumer.md.

## Consumer matrix

| # | consumer (crate / file) | endpoint(s) and contract | live galion-nerve (2f5a730, VPS) | openmind-nerve (this branch) |
|---|---|---|---|---|
| 1 | galion-realtest `live.rs` VPS rows `nerve-healthz`, `nerve-dashboard` | `GET /healthz` 200 Exact("ok"); `GET /` 200 Contains("galion-nerve") | pass (title `galion-nerve`) | pass; **was B3-fragile** before this branch (see B3) |
| 2 | galion-realtest `scenarios::nerve` (box stack, `--with-nerve`) | `/healthz` 200, `POST /signal` 202, `GET /signals` lists source, bad signal 400 | n/a (box only) | pass |
| 3 | nerve-client (hermes-rs, hermes-gateway via hermes-rs, openmind-memory) | `POST /signal` HTTP/1.0, kinds heartbeat/metric/event/error, sources `hermes-rs` / `openmind-memory` | not enabled (no `NERVE_URL` in units) | pass; 429 now visible (B4) |
| 4 | gaia-live `nerve.rs` (gaia `heal-tick --nerve`) | `POST /event` 200, `GET /events?n=` JSON array, `GET /health` `"status":"ok"` | **/health 404, /event 404, /events 404** (B1, B2) | pass with `--log`/`NERVE_EVENT_LOG`; `/event` 404 without (B2) |
| 5 | galion-deploy `health_table` (F4) | `GET /healthz` 200; rollback judged on `/healthz` | pass | pass (+ `/health` 200) |
| 6 | deploy/nerve/install.sh | `GET /healthz` 200 | pass | pass |
| 7 | scripts/real/nerve-capture-vps.sh, `openmind-nerve replay` | `/state`, `/self-eval`, `/self-eval.md`, `/`, `/health` | pass except `/health` 404 | pass (all 200) |
| 8 | gaia nerve-restarts (swarm/gaia-loop-decisions b40208a, not on this base) | `GET /state`: object, `components[].restarts_1h` count, `reflex.budgets[unit].live_1h` count; fail closed otherwise | pass (15 components, 10 budgets) | pass (real engine state); `null` before 1st cycle -> fails closed (D1) |
| 9 | guardian-tick `heal` -> `<state>/nerve.jsonl` (openmind_nerve::Writer); galion-one shows its last line | file, Writer JSONL | n/a | pass; torn tail healed (B5) |
| 10 | openmind-survival `nerve::append_nerve` (library fn, no caller today) | its own `{ts,source,kind,name,ok,detail}` lines | n/a | **not a nerve event** (B6) |
| 11 | `/state` top-level keys | live: alerts components cycle cycle_ms gaia interval_s mode probe_errors reflex signals started system tool ts version | - | superset (adds cortex, incidents, self_eval, ...) |

Not consumers: galion-desk (does not read 8670), galion-one (its "nerve" is an
in-process buffer; `-A galion-nerve` is only a curl User-Agent), console, tuner.

## Breaks found

| id | break | where it bites | status on this branch |
|---|---|---|---|
| B1 | live `GET /health` is 404, so gaia-live `healthy()` is false against the live nerve | gaia heal-tick pointed at 8670 before cutover | fixed by the cutover itself (openmind-nerve answers /health 200); nothing to change pre-cutover |
| B2 | `POST /event` / `GET /events` need an event log; `deploy/nerve/galion-nerve.service` runs `serve` with no `--log` / `NERVE_EVENT_LOG` -> gaia-live posts get **404** even after cutover | gaia heal-tick -> 8670 | **owner decision D2** (unit file change); not changed |
| B3 | dashboard contained "galion-nerve" only through the self component row: before the first cycle (state `null`) the page had none -> realtest `nerve-dashboard` fails right after every (re)start | galion-realtest live probe after a deploy | **fixed**: `<title>galion-nerve (openmind-nerve)</title>` (live title is `galion-nerve`) + unit test; real test phase 0 proves it on a 1,556-byte pre-cycle page |
| B4 | nerve-client counted any reply as `sent`: a 429 (120 signals/min/source) or 400 was invisible to the sender; and the nerve wrote its reply in several small TCP segments | hermes-rs 5xx storms | **fixed**: client reads the status line (until `\n`), new `rejected` counter (in the heartbeat msg too), no back-off on rejects; server writes each reply with one `write_all`. Real test: client rejected 10 == nerve dropped 10 |
| B5 | one unreadable line (torn tail after a crash, or a foreign JSONL line) made `GET /events` HTTP 500 for the whole log, and the next append was glued onto the torn line | gaia-live `tail()` | **fixed**: `/events` uses new `read_lossy` (skips + logs `<4>... skipped N`), `Writer::open` ends a torn last line. Real test: SIGKILL + torn line + restart -> 2 readable records, 200 |
| B6 | openmind-survival `nerve_line` format (`ok`,`detail`, no `severity`/`data`) is not a nerve event; strict `read_all` rejects it | only if someone appends it to a nerve log | documented + test; no caller today, not changed |

## Owner decisions

- **D1** `/state` (and `/incidents`, `/cortex`, `/ddos`) answer `200 null` until the
  first cycle finishes (seconds after a start). Consumers fail closed (gaia) or
  see `null`. Options: (a) keep; (b) answer `503 {"warming_up":true}` until
  cycle 1 (clearer, still fails closed); (c) serve an empty-but-valid document
  (NOT recommended: gaia would read "0 restarts" and stop failing closed).
  Recommendation: (b). Not implemented.
- **D2** Enable the event log in the nerve unit so gaia heal-tick can record to
  8670: `Environment=NERVE_EVENT_LOG=/var/lib/galion-nerve/events.jsonl`
  (inside `ReadWritePaths`; default rotation 8 MiB x 3 kept = at most 32 MiB
  of disk). Options: enable at cutover / keep off and
  let gaia use a file sink. Not changed (unit text, owner's call).
- **D3** nerve-client consumers are not enabled live (no `NERVE_URL` in the
  hermes-rs / openmind-memory units); enabling is the existing ws-nerve step 2.
