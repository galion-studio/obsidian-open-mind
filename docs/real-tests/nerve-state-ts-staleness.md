<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# nerve-state-ts-staleness (wave-4 slot 448)

Branch `swarm/nerve-state-ts-staleness`, base f66974c3 (the live nerve
`openmind-nerve-f66974c`).

## Problem

In event mode the nerve only re-runs a cycle on an event or on the heartbeat
(`NERVE_EVT_HEARTBEAT`, default 900 s). A quiet stack therefore leaves `/state`
`ts` unchanged for up to 15 minutes, while `/health` `ts` is the request time.
Live probe 2026-10-29T23:49+01:00 Europe/Zagreb: `/state` ts held at
23:45:31+01:00 over three probes in 40 s while `/health` ts moved. A reader that
treats an unchanged `/state` ts as "nerve stalled" false-alarms; one that never
checks misses a nerve whose loop has really died behind a live HTTP thread.

## Change (additive, no removed fields)

The nerve publishes its own limit, so readers stop guessing:

* `/state` gains `freshness: {max_age_s, stale_after, stale_after_unix, basis}`.
* `/health` gains `state_ts`, `state_age_s`, `state_max_age_s`, `state_fresh`.
* Limit: `NERVE_STATE_MAX_AGE` (10..=86400 s) or auto: event mode
  heartbeat + max_wait + 60 s (1080 s with defaults), poll mode
  3 x interval + 60 s. A bad value refuses to start (exit 2).
* `freshness::is_stale(&state, now)` is the reader helper.

The cortex 5026d9cf candidate already uses stream keepalives (660 s) and the
console does not parse `/state` ts, so no reader changes are needed.

## Real test

```text
GALION_REAL=1 cargo test --release -p openmind-nerve \
  --test real_state_freshness -- --nocapture --test-threads=1
```

Ports 23180-23189. Run 2026-10-29T23:55+01:00 Europe/Zagreb on the box:

```text
A event quiet: state ts 23:54:55+01:00 -> 23:54:55+01:00, health ts 23:55:03+01:00, age 8 max 124 fresh true
B poll stalled: state ts 23:55:03+01:00, age 12 max 10 fresh false basis "NERVE_STATE_MAX_AGE"
C NERVE_STATE_MAX_AGE=5 exit Some(2)
test result: ok. 1 passed; 0 failed
```

No new dependencies.
