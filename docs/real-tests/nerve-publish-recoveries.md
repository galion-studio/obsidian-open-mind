<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: nerve-publish-recoveries (slot 246, D-237-2)

Branch `swarm/nerve-publish-recoveries`, based on nerve-event-driven bb287cc
(the live nerve). Box only. Nothing was changed on the VPS; the only VPS
access was read-only (GET, `journalctl` reads, `ps`/`/proc` reads).

## The bug, seen live (VPS, read-only, 2026-10-22 02:04-02:11 +02:00)

- The nerve's `journalctl -f -o json` child (pid 946277, parent
  openmind-nerve 938448) has **17 `UNIT=` matches, 0 `.path` units, no
  postfix, no ssh**. Failures reach the nerve for every unit (match group 1),
  but `Started` arrives only for units in that list. So the recovery of
  postfix, ssh, a `.path` unit or any other untracked unit can never be
  published.
- Live example from `GET /evt` (ring) and `journalctl`:
  `gaia-hermes-handoff.path` failed with `trigger-limit-hit` at 23:06:25 and
  23:08:13 (+02:00, plus `Failed to start`) and was fixed at 23:08:38
  (`Started`). The nerve ring holds `unit.failed` + `unit.start_failed` for
  the path unit and **no `unit.started`** for it, which is why the cortex
  incident stayed open (slot 237 D-237-2: sticky TTL).
- At the same time the engine `incident.resolved` carried only `{"id"}`
  (slot 233 P-233-2) and the keys `incident.opened:<cause>` /
  `incident.resolved:<cause>` meant that a cause opening again within the
  600 s hub dedupe window was silently dropped.
- Live nerve at 02:04: `GET /incidents` showed 2+ open engine incidents
  (galion-one mouth stuck loading 3.0 h, edge/cloudflared 240 errors in 5 min).
  postfix, ssh, cron, galion-nerve and gaia-cortex were all `active`.

## What changed (crates/openmind-nerve)

1. `evt/recover.rs` (new): unit fault incidents. The first failure of a
   non-transient, non-sandbox unit publishes `incident.opened` (source
   `units`, `id` = `cause` = `unit:<unit>`). Repeats only count, so there
   are no duplicate opens. A `Started` / `Deactivated successfully` (journal) or
   an `active` / `inactive` row in the inventory starts a recovery hold
   (`NERVE_EVT_RECOVER_HOLD`, default 30 s, 0..=3600). If a new failure
   arrives inside the hold, the recovery is cancelled, so a crash loop never
   flaps between open and resolved. When the hold passes, the nerve publishes
   `unit.state` (`to`, `recovered: true`) and then `incident.resolved`
   `{id, cause, unit, resolved_by, state, opened, open_s, faults}`. A
   `.service` missing from the service listing counts as gone. `.path`,
   `.timer` and `.socket` units never appear in that listing, so only their
   journal `Started` resolves them.
2. `evt/journal.rs`: faulted units join the journal filter (the child
   restarts after its last cursor, so a `Started` that happened meanwhile is
   still read). Their `Started` / `Deactivated successfully` lines are parsed
   (`parse_entry_with`).
3. `evt/runtime.rs`: faults are seen before the hub dedupe (a repeat failure
   inside the dedupe window still cancels a pending recovery). The hold is a
   loop deadline (wake label `recovery`), so no timer polling is added. Unit
   incident events are not offered to the escalation coalescer, because the
   journal event behind them already was. Engine incident events now carry
   `cause`, `components`, `opened` and `resolved_by: "engine"`, and their keys
   carry the open time.
4. `GET /incidents` (event mode) adds open and recently resolved unit incidents
   (`source: "units"`), so a consumer that backfills from it (cortex 237)
   sees the same causes as on the stream. `GET /stats` has `units`
   (`opened_total`, `resolved_total`, `repeat_faults`,
   `recoveries_cancelled`, `hold_s`, open/resolved lists) and
   `config.recover_hold_s`. `/state` and the score are unchanged.

## Real test 1: `real_t246_unit_recoveries_published` (box, GALION_REAL=1)

```
GALION_REAL=1 GALION_NERVE_BASE_BIN=/workspace/swarm/artifacts/openmind-nerve-bb287cc \
CARGO_TARGET_DIR=/workspace/swarm/target/nerve-publish-recoveries \
cargo-slot test --release -p openmind-nerve -j 2 --test real_publish_recoveries -- --nocapture --test-threads 1
```

The release `openmind-nerve serve --mode event` runs on 127.0.0.1:21160. A journal FIFO
(`replay:`) uses the same parser as the live child. A `systemctl` stand-in in the
test's temp dir prints a unit table that the test edits (the box has no
systemd). One NDJSON `/events/stream` subscriber listens from seq 0. Hold 2 s.

Run at 02:05:44-02:06:21 +02:00 (same result in runs at 02:03:24 and later):

```
A opened seq 6 in 0 ms key "incident.opened:unit:postfix.service:1792627544" sev "high"
A unit.started seq 10 group "infra"; resolved seq 12 after 2001 ms (hold 2s) by "journal:started" faults 4 open_s 4
  key "incident.resolved:unit:postfix.service:1792627544"
B crash loop: 3 start/exit pairs, recoveries cancelled 3, unit.started published 1 (hub dedupe 600 s),
  0 resolved during loop; stable start -> resolved after 2000 ms, faults 4
C reopen: opened key "incident.opened:unit:postfix.service:1792627556" -> resolved key "incident.resolved:unit:postfix.service:1792627556"
  (first episode "...:1792627544" / "...:1792627544")
D inventory: resolved by "inventory:active" after 3144 ms (next event cycle + hold)
summary: stream events 31, units opened 4 resolved 4 repeat_faults 6 cancelled 3;
  engine opens ["incident.opened:lxc-host:1792627544", "incident.opened:s246-dead:1792627544"]; escalations 3
BASE .../openmind-nerve-bb287cc: postfix failure events 2, unit.started 0, incident.resolved 0 -> recovery never published
```

Checks made in the test, all passing:

- A: 4 failures produced exactly 1 open. The open shows in `/incidents`.
  `unit.started` was published for untracked postfix. `unit.state` came
  before `incident.resolved`, which carries `id` and `cause` and was sent no
  earlier than the hold. Afterwards the incident is in `/incidents` resolved
  and no longer open.
- B: 0 resolved during the crash loop, 1 open, at least 3 cancelled recoveries.
- C: a second postfix fault inside the 600 s dedupe window opens a new
  incident with a new key, which is resolved again (2 opens / 2 resolves).
- D: with no journal recovery line, the inventory row `active` resolves the
  incident (`inventory:active`). While the row says `failed`, the incident
  stays open.
- E: `run-u4242.service` and `sbx246-test.service` failures open nothing.
- Whole stream: every unit incident goes open -> resolved, never two opens in
  a row, and all end resolved. Engine open keys carry the open time.
- Base bb287cc with the same postfix input: 0 `unit.started`, 0
  `incident.resolved`. This reproduces D-237-2.

## Real test 2: `real_t246_vps_journal_replay` (real VPS journal data)

Fixture `crates/openmind-nerve/tests/fixtures/vps-journal-handoff-20261021.ndjson`:
82 entries read-only from the VPS, `journalctl -o json`, PID 1, units
`gaia-hermes-handoff.path` and `.service`, 23:00-23:15 +02:00 on
2026-10-21. Only these fields are kept: `__REALTIME_TIMESTAMP`, `MESSAGE_ID`,
`UNIT`, `MESSAGE`, `UNIT_RESULT`, `JOB_TYPE`, `JOB_RESULT`, `_PID`,
`_TRANSPORT`. Host, boot and machine ids and cursors were dropped. The fixture
contains no secrets. Port 21162.

```
VPS replay: opened seq 9 sev "high" kind "unit.failed" (first fault in the VPS journal: 2026-10-21 23:06:25 +02:00)
VPS replay: failure events published 2; resolved seq 13 by "journal:started" faults 3; opens 1 resolves 1;
  /incidents open ["inc-86dc8ee4", "inc-8c387b14"]   (engine incidents of the dummy component only)
```

The real incident (trigger-limit-hit twice, then `Failed to start`, then
`Started` at 23:08:38) now gives 1 open and 1 resolve, with 3 faults counted.
Only 2 failure events were published because the hub dedupe dropped the
second `unit.failed`. The book counted it anyway.

## Other tests

- `cargo fmt --all --check` ok; `cargo clippy -p openmind-nerve --all-targets --release -- -D warnings` 0 warnings.
- `cargo test --release -p openmind-nerve -p guardian-tick` (the only crate
  depending on openmind-nerve): 162 passed / 0 failed. That includes 5 new
  `evt::recover` unit tests and 1 new journal parser test. `evt_mode` 3/3
  and `consumer_compat` 11/11 are unchanged.
- `GALION_REAL=1 real_consumer_compat` 2/2, including `vps_live` (read-only).
- Cross builds: x86_64 musl release static-pie 1,463,072 B (bb287cc 1,415,176 B,
  +47 KB); aarch64 musl release static (rust-lld); armv7-musleabihf and
  aarch64-android `cargo check` ok.
- The full workspace test was not run (box disk at 95 %). The change touches
  only openmind-nerve and its single dependent.

## Owner decisions (Slavik)

- **D-246-1 deploy**: not deployed. To go live, build bb287cc + this branch
  and replace the live nerve (a VPS service change, needs your approval).
  Expected effect: postfix, ssh, `.path` and other unit faults get
  `incident.resolved` 30 s after the unit runs again, and cortex
  `incident_sticky_max_s` can stay 0.
- **D-246-2 hold**: `NERVE_EVT_RECOVER_HOLD` default 30 s. Longer means fewer
  flaps and slower closes.
- **D-246-3 cortex**: unit incidents use cause `unit:<unit>` with no
  top-level `unit`, so the cortex files them as `nerve-incident:unit:<u>`,
  next to its own `unit:<u>` subject, which now also clears because
  `unit.started` / `unit.state` are published. If you want one cortex
  incident per unit fault, the cortex could fold `nerve-incident:unit:*`
  into the unit subject (cortex change).
- **D-246-4 failed at start**: units already `failed` when the nerve starts
  open no unit incident (only failure events do). Open them from the first
  inventory too?
