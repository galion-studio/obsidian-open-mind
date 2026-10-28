<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve heartbeat: cycles/h vs load (wave-4 slot 369, D-344-1 prep)

In event mode (`serve --mode event`, the live VPS unit) the nerve runs a full
engine cycle at start, on every liveness heartbeat, and debounced after stack
events (journal unit failures, pidfd exits, inotify, `POST /event`). With
nothing happening the heartbeat is the only thing that re-checks health, so it
sets how long a *quiet* failure (a component that hangs or degrades without a
journal/pidfd event, e.g. HTTP 503 "stuck loading") can go unseen.

## Knob

`NERVE_EVT_HEARTBEAT` seconds, **default 900 (unchanged; owner decides)**.

| value | effect |
|---|---|
| unset | 900 s |
| 120..=900 | used as is |
| < 120 or > 900 | clamped to 120 / 900, `<4>` warning at start, `/stats` `config.heartbeat_clamped_from` = requested value (a typo never crash-loops a deploy) |
| not a number (`abc`, `300s`, `-1`) | start-up error, exit 2 (same as every other numeric knob) |
| `NERVE_EVT_HEARTBEAT_TEST_FLOOR=1` | floor 60 s for box real tests only (warned at start, `config.heartbeat_test_floor`); never on a deployed unit |

`/stats` `config` now also shows `heartbeat_bounds_s` `[120, 900]` and
`heartbeat_cycles_per_hour` (3600 / heartbeat). The previous range was
60..=3600 silently clamped. Bounds rationale: above 900 s a quiet failure
waits > 15 min; below 120 s the heartbeat cycle (probe walk + `systemctl
list-units` inventory + threshold checks) runs 30+/h on a host that already
gets event-triggered cycles.

## Trade-off (measured)

| heartbeat | quiet cycles/h (theory / measured box) | worst-case quiet-failure detection | box CPU per extra cycle | box process CPU/h (idle nerve, incl. 1 req/s test polling) |
|---|---|---|---|---|
| 900 s (default, live) | 4 / 3.87 | 15 min | - | 1.06 cpu-s/h |
| 300 s | 12 / 11.61 | 5 min | ~0.025 cpu-s | 1.26 cpu-s/h |
| 120 s (floor) | 30 / 29.03 | 2 min | ~0.028 cpu-s | 1.76 cpu-s/h |

Box numbers come from `tests/real_heartbeat_tune.rs` (31-min window, see
`docs/real-tests/nerve-heartbeat-tune.md`). The box has no systemd, so its
inventory step is cheaper than on the VPS: the live nerve's `cycle_ms` is
~0.9-1.0 s wall on a host with load average ~20-24 on 8 CPUs (06:21-06:57
+01:00), and the live process used 5 cpu-s total over 12.7 h (60 cycles +
all events). Even at a pessimistic 0.1 cpu-s per live cycle, 300 s adds
~0.8 cpu-s/h and 120 s ~2.6 cpu-s/h: negligible CPU either way. The real cost
of a faster heartbeat is **more `systemctl list-units` + probe traffic on an
already loaded VPS and more `nerve.heartbeat` events in the ring/stream**
(one per heartbeat, deduped by key), not nerve CPU.

Recommendation for the owner (D-344-1): 300 s (12 cycles/h, 5-min worst-case
quiet detection) is the balanced choice if 15 min is too slow for health
verdicts; it is a unit `Environment=NERVE_EVT_HEARTBEAT=300` change at the
next nerve deploy. Not changed here.
