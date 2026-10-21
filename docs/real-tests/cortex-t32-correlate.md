<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: cortex T3.2 cross-sensor correlation (slot 179), 2026-10-21 +02:00

Branch `swarm/cortex-t32-correlate` on `swarm/cortex-merge-all` 6bcb882. Box ports 20490-20499 only.
Design and owner decisions: docs/cortex/correlation.md.

## Box, real binary (`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_cortex_t32`)

Run 2026-10-21 09:21 +02:00 with `NERVE_T32_CAPTURES=replay179,replay158,replay1` (15.6 s, pass):

| step | result |
|---|---|
| `openmind-nerve replay fixtures/cortex-replay` | 16/F in 6 ms; every incident has `correlation`; `edge` members `bleeding:edge-dead-origins` + `bleeding:error-rate-cloudflared` |
| replay of live capture replay179 (VPS 09:15-09:16) | 66/D in 12 ms (base 6bcb882: 66/D) |
| replay of live capture replay158 (VPS 08:23) | 66/D in 22 ms (unchanged) |
| replay of live capture replay1 (VPS 2026-10-20 19:25) | 56/D in 11 ms (base 61/D: scanner member floor) |
| no member charged twice (all 3 captures) | 0 double charges; members never in `components` |
| `serve` :20490 live, `edge`-group component on :20491 down, surface on :20492 (closed) + `t32..invalid`, sshd on a scratch auth log +1.1 MiB | after 5.5 s: one `edge` incident [critical], sensors `surface`, members `bleeding:surface-dns` + `bleeding:surface-http-down`, both `covered` by `edge`; score 67/D: -25 edge incident, -8 auth-log bleed (no `ssh-flood` incident: a finding alone opens none); cycle_ms 22, RSS 3848 KiB |
| origin :20491 comes up | `edge` resolved after 2 cycles; score 84/B: -8 auth-log bleed, -8 `surface-dns` "same cause `edge`, scored once: also bleeding surface-http-down" |

The box resolver is a fake-IP proxy (198.18.0.53 answers every well-formed name), so the DNS failure is
the empty-label name `t32..invalid` (fails offline, no packet leaves the box).

## Box, in-process engine (`cargo test -p openmind-nerve --test cortex_t32_correlate`, 3 tests)

- golden fixture: correlation sections, scanner deduction 10 with the member-floor text, all other
  deductions equal to the pre-trait golden (sum +5), 4 `- correlated:` markdown lines.
- live-mode engine (sshd sensor statting a scratch auth log in the temp dir), 400 failed passwords from 60
  TEST-NET-1 sources, +1.1 MiB auth log, host at 92 % disk: one `ssh-flood` incident [critical], components
  `host` + `lxc-host`, rules `same-cause` + `ssh-disk-bleed`, members `bleeding:sshd-auth-log-growth` +
  `ddos:sshd-credential-flood`; cost 28 (25 + 3); after 2 more cycles only `ssh-flood` is open, one
  deduction, score 72, pass false.
- same without the bleed: the disk stays its own incident (2 deductions).

## Existing real tests re-run on this branch (release, `--test-threads=1`, 09:12-09:22 +02:00)

real_box, real_cortex_merge_all, real_cortex_s2_c, real_cortex_s2_d, real_cortex_s2a, real_cortex_s2b,
real_cortex_sensor, real_cortex_state_titles, real_cortex_t20, real_cortex_t32, real_edge_rate,
real_event_feed, real_score_v2: **13/13 pass** (each on its own historic slot range; ports checked free, no
other nerve process running). Updated expectations, all from the scanner member floor: fixture 21/F -> 16/F
(real_cortex_sensor, real_score_v2, real_edge_rate with `NERVE_EDGE_HOSTS=0`), replay1 61/D -> 56/D
(real_cortex_sensor, real_score_v2, real_edge_rate). The T2.12 edge-flood capture prints 6/F (was 11/F).

## Real server (VPS, read-only, 2026-10-21 09:15:05-09:16:51 +02:00)

`ssh root@VPS`: 3x `GET 127.0.0.1:8670/state` 30 s apart (cycles 7041-7043, mode `live`, status `degrading`:
10 healthy, 3 degrading, 2 absent), `GET :8660/status.json`, `ss -ltn`, `journalctl -o json` 30 min
(1026 cloudflared, 307 ssh, 13 galion-nerve-shadow, 9 galion-nerve lines), `systemctl show galion-nerve`
(MainPID 1466797, NRestarts 0, active since 2026-10-18 22:35:04 +02:00). `/api/v1/state` and
`/api/v1/cortex/events` 404: **the cortex is not deployed**; observed through the nerve.

Live nerve picture: cloudflared degrading (tunnel origin errors 85/99 requests in 10 min, 1 restart in the
last hour), galion-one degrading (1 restart in the last hour: the 08:39 planned deploy), galion-one-mouth
degrading (503), voice-worker and gaia absent.

Replay of that capture by this branch (`NERVE_SSHD=1 NERVE_SSHD_AUTH_LOG=0`): 66/D, same as base 6bcb882.
Deductions: 15 `edge` (8 public hostnames -> 7 closed origin ports; members edge-dead-origins (error, sensor
`edge`) + error-rate-cloudflared (warn, `components`)), 15 galion-one (mouth stuck loading; member
stuck-loading-galion-one-mouth), 4 unowned loopback listeners :8672 :19120. The 307 ssh lines hold no
failed-password flood (password auth is off), so no `ssh-flood` incident on live data.

Nothing written on the VPS. Capture kept on the box at `/workspace/swarm/nerve-capture/replay179` (not
committed: journal lines carry addresses).
