# Freeze heal (gaia-cortex R13)

A service that systemd still shows as running (`active`) but that stopped answering:
it hangs or returns 503. The crash rules (R3) never see it because nothing exited.
The freeze watch in gaia-cortex restarts such a service, carefully, through the same
decide/act path as a crash heal.

## Who can be restarted

Only units on the existing restart allowlist (`openmind_nerve::reflex::ALLOW`, or
`restart_allow` in the cortex config when set) **minus** the protected units
(`guard`, e.g. cloudflared, gaia-cortex, galion-nerve, backup, ssh, DNS) and the
reflex DENY list, **and** only units the nerve `/state` lists with a loopback health
port. Nothing is added for freezes. On the VPS today:

| allowlisted | freeze target? |
|---|---|
| openmind-memory.service | yes (8420) |
| hermes-rs.service | yes (8642), already restartable by the crash path |
| galion-desk.service | yes (8646) |
| tuner.service | yes (8650) |
| galion-one.service | yes (8091) |
| console.service | yes (8080) |
| galion-health.service | yes (8660) |
| link-watch-rs.service | no (no health port) |
| galion-voice.service | no (not deployed) |
| cloudflared.service | no (protected) |

The cortex, the nerve, the gate and the backup are not on the allowlist and are never
probed or restarted.

## What counts as frozen

The watcher thread probes each target every `probe_s` (default 15 s, timeout 5 s) from
the cortex itself, because the event-mode nerve refreshes `/state` only every few
minutes. A probe is **bad** when there is no answer, it times out, or HTTP >= 500.
3xx / 4xx mean the process is alive and count as good.

A freeze fires only when **all** hold:

1. `fails` (default 6) bad probes **in a row**: one good probe resets the streak (a blip);
2. the streak spans at least `min_s` (default 90 s);
3. the unit is outside its cooldown (`cooldown_s`, default 300 s) that starts on any
   systemd lifecycle event of the unit (start, stop, crash, exit, restart scheduled,
   start limit, OOM: a deploy restart) and on every restart the cortex asked itself;
   bad probes inside the cooldown do not count;
4. a fresh nerve `/state` read says the unit is `active` (anything else: held);
5. it is the only watched service failing: 2+ at once is a dependency / host problem
   (**R13-freeze-wide**: alert only).

The event (`unit.frozen`, source `gaia-cortex-freeze`) goes through an internal inbox
only; the same kind arriving from outside (stream, POST /events) is dropped.

## Decision (rules R13)

| rule | when | effect |
|---|---|---|
| R13-frozen | allowlisted, circuit closed, under the caps | restart via nerve POST /actions + alert; verify by probe |
| R13-freeze-wide | 2+ targets failing together | alert only |
| R2-circuit-open | the unit's circuit is open (e.g. a failed heal) | alert only |
| R13-freeze-cap | already `per_unit_hour` (default 1) freeze restart(s) in the last hour | open the circuit, alert only |
| R3-cap / R3-cap-total | the usual restart caps (per unit / all units per hour) | alert only |

After the restart the heal check is judged by probing the same port at the deadline
(`heal_verify_s`): good = verified, still bad = **R12-heal-failed**: circuit open
(no more restarts for an hour), alert only (no escalation).

## Settings (env, no rebuild)

| env | default | range |
|---|---|---|
| `CORTEX_FREEZE` | off (`0`) | `1` / `0` |
| `CORTEX_FREEZE_PROBE_S` | 15 | 5..300 |
| `CORTEX_FREEZE_FAILS` | 6 | 3..100 |
| `CORTEX_FREEZE_MIN_S` | 90 | 60..3600 |
| `CORTEX_FREEZE_COOLDOWN_S` | 300 | 60..3600 |
| `CORTEX_FREEZE_PER_HOUR` | 1 | 1..3 |
| `CORTEX_FREEZE_TIMEOUT_S` | 5 | 1..30, below probe_s |
| `CORTEX_FREEZE_SKIP` | empty | comma list of `*.service` (max 16) the freeze watch leaves alone (crash heal unchanged) |
| `CORTEX_ALERT_RELAY` | off | `http://<loopback ip>:<port>/<path>`: failed-heal phone alert (below) |
| `CORTEX_ALERT_RELAY_PER_HOUR` | 6 | 1..30 |

A value out of range (or `CORTEX_FREEZE` not 0/1) refuses to start (exit 2). Turn it
off live: set `Environment=CORTEX_FREEZE=0` in the drop-in (or remove the line) and
restart the cortex. Visible in `/health` (`freeze_watch`, `freeze_targets`) and
`/heals` (`freeze`: targets, streaks, cooldowns, counts).

## Failed heal: phone alert only (Mate 2026-10-30 11:27 CET, decision 2)

A failed heal (`R12-heal-failed`, crash or freeze) is now a **critical** alert and is
never escalated to Hermes. The nerve folds every pushed cortex event into an incident of
at most `error` severity, and its ntfy webhook only forwards `crit`, so the nerve copy
alone never reached the phone. With `CORTEX_ALERT_RELAY` set, the cortex also hands each
`R12-heal-failed` alert to the same loopback relay the nerve webhook uses
(`galion-ntfy-relay`, 127.0.0.1:2586; the topic stays in the relay's env file, the cortex
never sees it): ntfy priority `urgent`, title `Gaia: heal failed: <unit>`, max 6/h.
No other rule goes to the phone. `/heals` -> `phone` shows sent / failed / rate-limited;
`/health` -> `phone_alerts`.

## Tests

* unit: `freeze::tests` (bounds, N + span, blip, cooldown, wide, event marker, targets, skip);
  `phone::tests` (loopback-only config, only R12 goes out, urgent, hourly cap);
* pipeline (mock nerve): `tests/freeze_heal.rs` (incl. failed heal -> critical alert -> mock relay, no escalation);
* real (`GALION_REAL=1`, ports 23220-23229, real nerve, fake systemctl, mock services):
  `tests/real_freeze_heal.rs`.
