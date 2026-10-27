<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Real-life tests + decision memo: guardian-tick-wire (wave-4 slot 297)

Branch `swarm/guardian-tick-wire`, base `swarm/nerve-t24-wire-gaps-3` `97fb7204`
(slot 289). Closes the last nerve-coverage GAP (D-T24-3). Times are
Europe/Zagreb (CET +01:00). No deploy, no live service change, no CF DNS, no
HADES change, no secrets.

## What changed

| crate | change |
|---|---|
| `nerve-client` | new `flush(max)`: wait (bounded) until every queued signal is sent or failed. For one-shot CLIs. No-op/instant when `NERVE_URL` is unset. 2 new unit tests. |
| `guardian-tick` | depends on `nerve-client`. **Keeps** the `openmind-nerve` lib file writer (`<state>/nerve.jsonl`) unchanged and authoritative. Every heal event is additionally mirrored live: `crit` -> kind `error`, `info`/`warn` -> `event`, name = event kind, msg = JSON data. Every run pushes `started` (msg = report/heal/classify) and one `heartbeat` `tick` (msg `mode=… ok=…`), `tick_failed` error on failure, then `flush(1.5 s)`. Report mode also pushes metric `probes_failed` and one `probe_failed` error per failed probe. |
| `openmind-nerve` tests | new `tests/real_guardian_tick.rs` (GALION_REAL, ports 21670-21679). |

No new third-party crates. Without `NERVE_URL` the binary behaves exactly as
before (verified: report output identical to the live binary on the VPS).

`repo-check --only nerve --nerve-gaps`: `nerve coverage: 37 bins, 24 covered,
13 probe-only, 0 gap(s)` (was 23 covered / 1 gap). guardian-tick: `covered
(direct nerve-client)`.

## Box real test (2026-10-27 20:43 CET)

`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_guardian_tick -- --nocapture`

Real openmind-nerve serve on 21670, in-test healthy listener 21671, closed
probe port 21672, dead nerve 21679, real `command` restart backend.

```
== 2026-10-27T20:43:26+01:00 nerve up on 21670; guardian-tick ticks start
-- report tick 6.2ms total=4: heartbeat/tick, error/probe_failed "dies: connect: Connection refused", metric/probes_failed=1.0, event/started "report"
-- heal dry-run tick 10.0ms total=8: heartbeat/tick, event/heal-restart (dry-run, would restart), error/incident-opened (class down), event/started "heal"
-- heal apply tick 111.3ms total=11: heartbeat/tick, event/heal-restart (mode apply, restarted via command), event/started
-- dead-nerve tick 6.3ms
-- no-NERVE_URL tick 1.1ms total=11
== 2026-10-27T20:43:26+01:00 PASS: guardian-tick mirrors to live nerve, file writer kept (slot 297)
test result: ok. 1 passed
```

Asserted: every tick's signals arrive (totals 4 -> 8 -> 11, nothing lost at
exit), the file writer still gets `incident-opened` and `heal-restart`, the
dry-run never runs the backend, the apply runs it exactly once, a dead nerve
never fails or stalls a tick (< 4 s; real 6 ms), no `NERVE_URL` pushes nothing.

## VPS real test (read-only for live; 2026-10-27 20:45 CET)

Static musl build (971 616 B; live binary 893 648 B) copied to the throwaway
folder `/var/tmp/sandbox-test-gtw-20261027204529`, run as `nobody`. A
throwaway nerve (the live binary `openmind-nerve-9d1a4f3`, `nobody`, port
21670, data in the folder) received the pushes. The **real** live probe file
(`/opt/galion/etc/probes.toml`, copied, not printed) was used, so the probes
hit real VPS services. Live nerve :8670 and the live `guardian-tick.timer`
were not touched. Folder and processes removed afterwards.

```
== 2026-10-27T20:45:37+01:00 VPS throwaway nerve (openmind-nerve-9d1a4f3) :21670 up=yes user=nobody
-- live binary (unchanged) report:
guardian-tick 2026-10-27T20:45:37+01:00 host_ok=false probes=11 failed=5
-- new binary, NERVE_URL=throwaway nerve:
guardian-tick 2026-10-27T20:45:37+01:00 host_ok=false probes=11 failed=5
exit=0 ms=41
SAME_PROBES_live_vs_new
{"total":8,"dropped":0,"recent":[heartbeat/tick "mode=report ok=true",
  error/probe_failed "load: load1/core 361.6% crit (load1 28.93, cores 8)",
  error/probe_failed "origin: connect: Connection refused",
  error/probe_failed "voice: status 503",
  error/probe_failed "vision: connect: Connection refused", ...]}
-- new binary, dead nerve (:21679): exit=0 ms=39
-- heal dry-run (no --allow), state in throwaway:
guardian-tick heal 2026-10-27T20:45:38+01:00 mode=dry-run cap=3/h
{"total":15,"dropped":0,"kinds":["heartbeat/tick","event/incident-opened","error/incident-opened",...]}
file writer (state/nerve.jsonl) lines: 5
cleaned /var/tmp/sandbox-test-gtw-20261027204529
no leftover procs
live nerve :8670 /state=200
guardian-tick.timer active; live bin sha 007720e3a583
```

Report tick: 8 = started + metric + 5 `probe_failed` + heartbeat. Heal
dry-run: 7 more = started + 5 `incident-opened` + heartbeat, matching the 5
file-writer lines. 0 dropped.

## Live finding (read-only)

The live `guardian-tick.service` (timer, every 60 s) runs **report mode only**
(`--probes /opt/galion/etc/probes.toml --out …/guardian-tick.json`), no
`heal`, no `--jsonl`, no `NERVE_URL`. So today the file writer of D-T24-3 is
not even used live, and **the live nerve sees nothing** from guardian-tick
while it reports `failed=5` of 11 probes every minute (load crit, origin,
voice 503, vision down, one more). The nerve's sources right now are only
gaia-cortex, hermes-rs and openmind-memory.

## Decision memo for Slavik (D-T24-3)

**Question:** guardian-tick writes nerve events to a file through the
`openmind-nerve` lib, not nerve-client. Count it as coverage, migrate, or both?

**Recommendation: A — dual sink (this branch).** Keep the file writer
(authoritative audit trail, works with the nerve down, heal's append-only
history) and mirror every event live through nerve-client. Zero behaviour
change unless `NERVE_URL` is set; a dead nerve costs ~1 refused connect and
never fails a tick (39 ms measured on the VPS).

| option | what | pro | con |
|---|---|---|---|
| **A dual sink (built, tested)** | file writer kept + nerve-client mirror + flush | nerve sees guardian-tick live; file trail unchanged; no-op without env; 0 gaps | +78 KB binary; two copies of heal events |
| B accept file as coverage | allowlist guardian-tick in repo-check | no code | the live nerve stays blind to guardian-tick (it does not even write the file in report mode) |
| C replace file with nerve-client | drop the `openmind-nerve` lib dep | one path | loses the offline audit trail; heal history lost when nerve is down |

**To go live (needs Slavik's OK; NOT done here):**
1. Merge `swarm/guardian-tick-wire` via the normal staging path.
2. Install the new binary as `/opt/galion/rust/bin/guardian-tick-<sha>` and
   point the unit at it (keep the old one for rollback).
3. Drop-in `/etc/systemd/system/guardian-tick.service.d/nerve.conf`:
   `[Service]` `Environment=NERVE_URL=http://127.0.0.1:8670`, then
   `systemctl daemon-reload`. Report mode stays; no heal, no `--allow`.
4. Watch `GET 127.0.0.1:8670/api/v1/signals` for source `guardian-tick`
   (expect 1 heartbeat/min + `probe_failed` errors for the 5 failing probes).
   Rollback: remove the drop-in (pure no-op path) or revert the binary.

Open question for the same decision: with the mirror on, the 5 live
`probe_failed` errors per minute will reach the nerve/cortex as real signals.
Decide whether the cortex should triage them (useful, real incidents) or
whether the load probe threshold should be tuned first (load1/core 361% is
host noise per the 19:57 CET panel note).
