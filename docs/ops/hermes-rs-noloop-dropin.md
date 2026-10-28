<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-rs: no-crash-loop drop-ins (wave-4 slot 370)

Written 2026-10-28 06:30 CET (+01:00). Branch `swarm/hermes-rs-noloop-dropin` off
`swarm/hermes-rs-crashloop-r3` d76b70df. **Staged only, nothing installed.** The VPS
was used read-only for the live unit plus throwaway transient units (as `nobody`);
evidence: `docs/real-tests/hermes-rs-noloop-dropin.md`.

## Why the live unit can loop forever

Live `hermes-rs.service` (VPS `systemctl show`, 06:21 CET): `Restart=on-failure`,
`RestartSec=3`, `StartLimitBurst=5`, `StartLimitIntervalUSec=10s` (systemd defaults),
`RestartPreventExitStatus=` empty. A start every ~3.2 s gives at most 4 starts in any
10 s window, so the default limit of 5 **can never trip**. Measured on the VPS with a
throwaway unit using exactly these settings: 26 starts in 80 s, still restarting when
stopped by hand. That is the 2026-10-27 19:45 CET pattern (13 restarts in 46 s).

## The staged drop-ins (`deploy/staged/hermes-rs.service.d/`)

| file | what | decision |
|---|---|---|
| `40-config-no-loop.conf` (slot 355) | `RestartPreventExitStatus=2`: a config error (exit 2) stops the unit after the first start | D-355-1 |
| `41-coding-on-error.conf` (new, option) | `HERMES_RS_CODING_ON_ERROR=disable`: a bad coding config logs and starts with coding tools OFF, chat keeps serving | D-355-2 |
| `45-start-limit.conf` (new) | `[Unit] StartLimitIntervalSec=120 StartLimitBurst=5`, `[Service] RestartSteps=4 RestartMaxDelaySec=30`: every other failure (exit 1, 226/NAMESPACE, panic, OOM) gets 4 retries with back-off 3 -> 30 s, then the unit stays failed | D-370-1 |

Measured on the VPS (systemd 255.4, transient units as nobody, 06:25-06:28 CET):

| case | settings | result |
|---|---|---|
| `sh -c 'exit 2'` | 40 + 45 | 1 start, NRestarts 0, failed |
| `sh -c 'exit 1'` | 40 + 45 | starts at +0, +3.2, +8.7, +18.5, +35.5 s; 6th refused at +65.7 s "Start request repeated too quickly"; failed |
| `sh -c 'exit 1'` | live-like (no drop-ins) | 26 starts in 80 s, never refused (stopped by hand) |
| real gateway d76b70df, ReadWritePaths hidden by PrivateTmp (226/NAMESPACE) | 40 + 45 | 5 starts in 65.7 s, then refused (a real non-2 loop, bounded) |
| real gateway, coding worktree under /var/lib (19:45 replay) | 40 + 45 | exit 2 once, NRestarts 0 |
| same | 40 + 41 + 45 | active, `listening on 127.0.0.1:22401`, `GET /health` 200, `/v1/models` 401 without key, journal "coding tools OFF, gateway continues" |
| same | live-like | exit 2, 7 restarts in 21 s, still looping (stopped by hand) |

`systemd-analyze verify` (as nobody) of the **live** unit + its 4 live drop-ins +
40/41/45: rc 0, no warnings. Negative controls: `RestartSteps=abc` and
`RestartMaxDelaySec` < `RestartSec` give warnings; `StartLimitIntervalSec=` in
`[Service]` gives "Unknown key ... ignoring" (that is why the file puts it in `[Unit]`;
the Rust test `start_limit_keys_sit_in_the_right_sections` enforces it). Note: verify
exits 0 even with these warnings, so read its output, not only the exit code.

## Install (owner-approved only)

```
install -m 0644 40-config-no-loop.conf 45-start-limit.conf /etc/systemd/system/hermes-rs.service.d/
# optional, D-355-2, needs a gateway >= d76b70df (live b7fbbc78 ignores it):
install -m 0644 41-coding-on-error.conf /etc/systemd/system/hermes-rs.service.d/
systemd-analyze verify /etc/systemd/system/hermes-rs.service   # output must be empty
systemctl daemon-reload
systemctl show hermes-rs -p RestartPreventExitStatus -p StartLimitBurst -p StartLimitIntervalUSec -p RestartSteps -p RestartMaxDelayUSec
```
40 and 45 need no restart (they apply to the next failure). 41 needs a restart at an
approved time. Watch nerve `/state` (hermes-rs healthy, restarts_1h 0).

## After the unit stopped on purpose

`systemctl status hermes-rs` shows failed; the journal has the reason (exit-2 log line
or "Start request repeated too quickly"). Fix the cause, then:
```
systemctl reset-failed hermes-rs && systemctl start hermes-rs
```
Cortex note: its R3 restart is refused while the unit is start-limited (until
`reset-failed` or 120 s after the first of the 5 starts). That is the intended
behaviour (no restart storm), but the cortex should hand off to a human instead of
counting that refusal as a new failure (follow-up F-370-1).

## Rollback

```
rm /etc/systemd/system/hermes-rs.service.d/40-config-no-loop.conf \
   /etc/systemd/system/hermes-rs.service.d/41-coding-on-error.conf \
   /etc/systemd/system/hermes-rs.service.d/45-start-limit.conf
systemctl daemon-reload
systemctl reset-failed hermes-rs   # only if it is in failed state
```
(restart only if 41 was installed and the unit was restarted with it; otherwise none).
Back to today's behaviour: 5-in-10 s default, which never trips.

## Owner decisions

- **D-370-1** Install `45-start-limit.conf` (5 starts per 120 s, back-off 3 -> 30 s)?
  Recommended: yes, together with 40 (D-355-1). Trade-off: after ~66 s of failures the
  unit stays down until someone runs `reset-failed` (or a restart after 120 s).
- **D-355-2** (unchanged) `41-coding-on-error.conf`. Recommended: yes, with the next
  gateway deploy that carries d76b70df.
- **F-370-1** Cortex: treat "start request repeated too quickly" / start-limited unit as
  "hand off", not as a new restart reason. Not done here.
- Same pattern applies to the other Rust units with `Restart=on-failure` and
  `RestartSec>=2` (default limit never trips). Not audited here.
