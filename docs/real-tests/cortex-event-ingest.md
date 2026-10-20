<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: cortex-event-ingest (T2.10, wave-4 slot 106, ports 19760-19769)

Branch `swarm/cortex-event-ingest`, base `swarm/cortex-sensor` e536446. All times Europe/Zagreb.

## Box: real nerve + real Gaia heal loop (2026-10-21 01:18 CEST +02:00)

```
GALION_REAL=1 GAIA_BIN=$CARGO_TARGET_DIR/release/gaia \
  cargo test --release -p openmind-nerve --test real_event_feed -- --nocapture
```

`openmind-nerve serve --log events.jsonl` on 127.0.0.1:19760 (interval 5 s, systemd/journal off) with
`NERVE_EVENT_FEED_FILE=events.jsonl,survival.jsonl`; test service port 19761. Output:

```
serve :19760 cycle 1: sensors ["listeners", "scanners", "edge", "components", "ddos", "event-feed"]
1. gaia heal-tick exit Some(4) outcome "refused" detail "no approval file given"
   cycle 2: "t106 is down (Gaia service probe)" | "t106: Gaia heal refused at stage approval"
2. cycle 3: "t106: Gaia restart cap reached (3 restarts in the last hour), still down" | incident "inc-252e9a5a" "critical"
   | event {"data":{"findings":2,"ids":["heal-capped-t106","service-down-t106"],"top":{"cause":"gaia:t106","component":"t106",
     "evidence_n":2,"fix":"...","id":"heal-capped-t106","title":"t106: Gaia restart cap reached (3 restarts in the last hour), still down"}},
     "kind":"bleeding","severity":"crit","source":"event:gaia-live","ts":"2026-10-21T01:18:15+02:00"}
   | score 72 | cycle_ms 22 event-feed us 37 rss 3396 KiB
3. gaia heal-tick exit Some(0) outcome "healthy" detail "service is up; nothing to do"
   cycle 4: all event findings cleared, score 71
4. cycle 5: "guardian probe disk is failing"
   cycle 6: survival probe cleared; 4 records in the nerve log; cycle_ms 22 event-feed us 32 rss 3464 KiB
test real_event_feed_on_the_box ... ok   (25.06 s)
```

What it proves:
1. The real producer path: `gaia heal-tick` (release build, this tree) probed the dead :19761, posted `service.probe`
   (warn) and `gaia.heal.refused` (warn, no approval file, so HADES refused and **nothing was started**) to the real
   nerve's `POST /event`; the nerve wrote them to its JSONL log; `event-feed` read the log back on the next cycle.
2. `gaia.heal.capped` (Gaia's exact record shape, posted by the test) became a **critical** bleeding finding, opened
   incident `gaia:t106`, and left as one B33 event `crit` per (source, kind). The probe detail
   (`connect 127.0.0.1:19761 ...`) stayed in loopback-only evidence; no address in titles or events.
3. A listener came up on :19761, a second real `heal-tick` posted `service.probe` info/up: all findings cleared.
4. A survival-guardian line (`{"kind":"probe","ok":false}`, no severity/data) in the second file raised
   `probe-failed-disk`; `ok:true` cleared it.
5. Budget: event-feed 32-44 us per cycle, whole cycle 22 ms, RSS 3.4 MiB (plan: < 50 ms, < 8 MiB).

Note: score 72 -> 71 after the clear comes from the incident staying open for its quiet cycles plus the
test's own polling on the nerve-api rate stream, not from the feed.

## VPS: read-only comparison (2026-10-21 01:20 CEST +02:00)

Read-only GETs over ssh on the live galion-nerve 127.0.0.1:8670 (old a63b730 build): `/healthz` 200, `/health`,
`/api/v1/health`, `/events`, `/api/v1/events` all 404. The live nerve has **no `/event` log**, so there is no live
producer to read until the cortex build replaces it (owner decision, cortex plan section 8 item 1). One temp file
(`/tmp/s106.out`, the 10-byte 404 body) was written and removed in the VPS /tmp by the probe; nothing else.

## Not covered

- Replay (`openmind-nerve replay`) runs with `live=false`, so `event-feed` reads no file there by design (the
  sensor contract); replay goldens are unchanged (`tests/cortex_replay.rs` green). A replay case for events needs
  an engine/replay change (event file in the capture dir) - T2.0 / engine owner.
- No producer posts boot-hardware events yet (galion-boot writes `hardware.json` only); any such record at
  error+ is handled by the generic rule.
