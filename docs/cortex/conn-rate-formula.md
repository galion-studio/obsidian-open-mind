<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# conn-rate: the B41 formula gap and the corrected formula (slot 178, cortex-connrate-fix)

Branch `swarm/cortex-connrate-fix`, base `swarm/cortex-merge-all` 6bcb882. 2026-10-21 CEST (+02:00).

## Was it already fixed in cortex-merge-all 6bcb882?

No. `crates/openmind-nerve/src/sensors/connrate.rs` is byte-identical between cortex-s2-b 51100e1 and
cortex-merge-all 6bcb882 (`git diff 51100e1 6bcb882 -- crates/openmind-nerve/src/sensors/connrate.rs` is empty; the
merge only touched `tests/real_cortex_s2b.rs` for the `edge-rate` sensor name). The sensor already raised alarms
from a *new-identity* count (correct on loopback), but the B41 formula itself (docs/cortex.md 3.2 step 2: growth of
ESTAB + TIME-WAIT per interval) was still computed as-is and still reads ~0 under any steady load. No count-only
formula existed.

## Why the B41 delta reads ~0

TIME-WAIT holds every closed connection for 60 s (`TCP_TIMEWAIT_LEN`, fixed in the kernel). Under a constant rate
`r`, the pool is flat at `60 * r`: each interval adds `r * dt` and expires `r * dt`. The delta is the difference of
two equal flows, so it is 0 for every constant rate (B41 "a flat TIME-WAIT count means zero new connections" is
wrong; it means a *constant* rate).

## Corrected formula (`b41flow`)

```
rate = max(0, (ESTAB+TW)_now - (ESTAB+TW)_prev  +  TW_prev * min(dt, 60) / 60) / dt
```

The second term estimates the TIME-WAIT entries that aged out during the interval (entries spread evenly over the
60 s window). Counts only, no identities and no addresses, so it also works from `ss -s`-style totals.
Properties (unit test `b41_flow_rate_math`): steady 10/s -> 10.0; ramp-up from idle -> the delta alone; load
stopped -> 0 (pool drains, never negative); long-lived ESTAB only -> 0; dt > 60 s -> lower bound.

## Shipped off by default

`NERVE_CONNRATE_FORMULA` (only read when `NERVE_CONNRATE_PORTS` turns the sensor on):

| value | severity driven by | output |
|---|---|---|
| unset / `identity` / unknown | new-identity count (cortex-s2-b, unchanged) | byte-identical to 6bcb882 (3 evidence lines) |
| `b41flow` (`flow`, `b41-flow`) | corrected B41 count formula | + evidence line 4 `formula=b41flow identity_rate=… b41flow_rate=…` |
| `max` | larger of the two | + evidence line 4 |

Thresholds are unchanged (B41 4.3, 1/10/50 per s, env may only tighten).

## Evidence

- VPS read-only sample (3 reads of `/proc/net/tcp*`, 30 s apart, 2026-10-21T09:24:42+02:00), redacted fixture
  `crates/openmind-nerve/tests/fixtures/conn-rate-vps/` (watched ports only, all rows loopback, uid/inode zeroed):
  hermes-rs :8642 identity 1.37/s, old B41 0.00-0.03/s, b41flow 1.35-1.43/s; galion-one :8091 identity 1.03-1.07/s,
  old 0.00-0.03/s, b41flow 0.93-1.08/s. Details: docs/real-tests/cortex-connrate-fix.md.
- Box real test `tests/real_connrate_fix.rs` (two release nerves, default vs `b41flow`, steady ~20/s): see the
  real-test record.

## Limits

- Both formulas undercount when the cycle is longer than 60 s (TIME-WAIT entries come and go between reads);
  the default 30 s cycle is fine.
- Non-loopback peers that close first leave no TIME-WAIT on our side (LAST-ACK -> gone): neither formula sees
  those connections. Today every watched VPS port is loopback-only.
- `b41flow` assumes a roughly steady arrival rate inside the 60 s window; a burst that ended < 60 s ago is
  smeared over the next intervals (the identity count is exact for it).

## Owner decisions (Slavik)

1. Amend B41 3.2 step 2 (docs/cortex.md) to the corrected formula, or keep the identity count as the definition.
2. Default formula once the sensor is turned on live: `identity` (today), `b41flow`, or `max`.
3. Ports/thresholds for :8642 (1.37-1.44/s) and :8091 (~1.05/s): with B41 memory-gateway thresholds they warn
   (open from cortex-s2-b; vps-traffic-hermes slot 116 explains the steady probe traffic).
