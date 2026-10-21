<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: cortex-state-titles (wave 4 slot 105)

Branch `swarm/cortex-state-titles`, base cortex-sensor `e536446`. Box only, ports 19750-19759. VPS not touched.

## The leak

`/api/v1/state` `cortex_events` (B33 events, T1.2) built `data.top` from `Finding::to_nerve_event`, which
copied the finding's `title` and `fix`. The `listeners` sensor's titles name the bind address and the stray
ports (`port N listens on <addr> (not loopback)`, `... no monitored unit owns: :8652 :8653`), so the event
stream carried exactly what B41 keeps out of events (found by cortex-s2-d f3468ce, 23:26 CEST). The nerve
API is loopback only, so nothing left the host; the risk is any later consumer that forwards events
(the T2.10 feed already drops these fields; this closes the same hole at the source).

## Fix

`Finding::to_nerve_event` `data` = `{id, component, cause, evidence_n}`; `nerve_events` `top` inherits it.
The operator shape (`/api/v1/cortex`, `/api/v1/state` `cortex`, the self-eval report and markdown) is
unchanged: titles and fixes stay there. Event key set equals the T2.10 feed's `top`, so the feed in
cortex-s2-d keeps working unchanged after T2.0 merges.

## Goldens

- New: `crates/openmind-nerve/tests/fixtures/cortex-replay.expected.events.json` (replay fixture's
  `cortex_events`, 4 events, `top` keys exactly `cause, component, evidence_n, id`). Regenerate with
  `CORTEX_GOLDEN_WRITE=1`.
- Unchanged: `cortex-replay.expected.json` and both `.md` goldens (report shape did not change).
- Before the fix (old `cortex.rs`) the new golden test fails: `cortex_events differ from the golden`.

## Real test (`GALION_REAL=1`, release)

`crates/openmind-nerve/tests/real_cortex_state_titles.rs`: `openmind-nerve serve` on 127.0.0.1:19750, a live
service on :19751, a stray loopback listener :19754 and a wildcard listener on :19753 (never accepted, open
about 0.5 s, closed by the test).

After the fix, 2026-10-21 01:12:28 +02:00:

```
serve :19750 cycle 1 (501 ms to both findings), state 7137 bytes; cortex_events
[{"data":{"findings":2,"ids":["public-listener-19753","unmanaged-listeners"],
  "top":{"cause":null,"component":"host","evidence_n":1,"id":"public-listener-19753"}},
  "kind":"vulnerability","severity":"high","source":"listeners","ts":"2026-10-21T01:13:09+02:00"}]
test real_state_cortex_events_carry_no_titles ... ok
```

The same run's `cortex.vulnerabilities` still says `port 19753 listens on <wildcard> (not loopback)` and
`:19754` (asserted), so the operator keeps the detail.

Before the fix (same test, old `cortex.rs`, debug build), 2026-10-21 01:13:15 +02:00: fails at the `top`
assertion; the event's `top` held
`"title": "port 19753 listens on <wildcard> (not loopback)"` and
`"fix": "bind it to 127.0.0.1 and publish through the tunnel, or firewall it (owner approval)"`.

Regression: `real_cortex_sensor.rs` (slot 43, ports 19430-19435) still passes in release: replay fixture
21/F (= pre-trait golden), live capture replay1 61/D, stray finding appears and clears,
`NERVE_SENSORS_OFF=listeners` works; its event `top` is now `{cause, component, evidence_n, id}`.
