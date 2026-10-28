<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: hermes-rs no-crash-loop drop-ins (wave-4 slot 370)

All times Europe/Zagreb CET (+01:00), 2026-10-28. VPS ip50, systemd 255 (255.4-1ubuntu8).
Throwaway dir `/var/tmp/noloop-test-20261028062409` (removed 06:29:08), transient
units `noloop370-*` with `User=nobody` (all stopped/reset, 0 left), ports 22400-22402
on VPS loopback (0 listening after). No live unit, file or service changed. Live
hermes-rs MainPID 255539, NRestarts 0 before (06:21) and after (06:28:59); nerve
`/state` hermes-rs healthy, restarts_1h 0, reflex_restarts_per_hour 0; cortex :8674
/health 200; hermes-rs :8642 /health 200. Test API key: random, generated on the VPS
into a 0600 file, never printed, deleted with the dir.

Binary: `hermes-gateway` from this branch (= d76b70df code), x86_64 musl release,
sha256 c3b9475ca12bcd16...

## systemd-analyze verify (as nobody, 06:24)

`SYSTEMD_UNIT_PATH=<dir>: systemd-analyze verify <dir>/hermes-rs-noloop-test.service`
(copy of live unit + live drop-ins 50/60/70/90, renamed so live drop-ins don't mix in)

| composition | rc | output |
|---|---|---|
| live + 40 + 41 + 45 | 0 | (empty) |
| live only | 0 | (empty) |
| + 99-bad.conf (`RestartSteps=abc`, `RestartMaxDelaySec=1`) | 0 | `Failed to parse unsigned value, ignoring: abc`; `RestartMaxDelaySec= has a value smaller than RestartSec=` |
| `StartLimitIntervalSec=` in `[Service]` | 0 | `Unknown key name 'StartLimitIntervalSec' in section 'Service', ignoring.` |

(First try without the trailing `:` in SYSTEMD_UNIT_PATH failed with "Unit
sysinit.target not found"; the default path must be appended.)

## Transient units (props taken from the staged files with grep, not retyped)

Base: `-p User=nobody -p Restart=on-failure -p RestartSec=3 -p NoNewPrivileges=yes -p ProtectSystem=strict ...`
Staged: `-p StartLimitIntervalSec=120 -p StartLimitBurst=5 -p RestartSteps=4 -p RestartMaxDelaySec=30 -p RestartPreventExitStatus=2`

| unit | ExecStart | props | outcome (systemctl show / journal) |
|---|---|---|---|
| t1 06:25:08 | `sh -c 'exit 2'` | staged | failed, NRestarts=0, ExecMainStatus=2 |
| t2 06:25:08 | `sh -c 'exit 1'` | staged | Started 08.24, 11.44, 16.94, 26.69, 43.69; 06:26:13.92 "Start request repeated too quickly"; failed, NRestarts=5 |
| t3 06:25:08 | `sh -c 'exit 1'` | live-like | 26 starts by 06:26:28, activating/auto-restart, never refused; stopped 06:26:38 |
| t4/t5 06:27:00 | gateway, PrivateTmp=yes hides ReadWritePaths in /var/tmp | staged | 226/NAMESPACE; Started 00.20, 03.44, 08.94, 18.55, 35.69; 06:28:05.92 repeated too quickly; failed (unplanned but real non-2 loop, bounded) |
| t4b 06:28:36 | gateway, `HERMES_RS_CODING_WORKTREE=/var/lib/dpkg` | staged | `coding: config error: worktree /var/lib/dpkg overlaps the denied path /var/lib (...)`, status=2/INVALIDARGUMENT, failed, NRestarts=0 |
| t5b 06:28:36 | same + `HERMES_RS_CODING_ON_ERROR=disable` (from 41) | staged | active/running, `listening on 127.0.0.1:22401 mock=true loopback=true`, journal `HERMES_RS_CODING_ON_ERROR=disable: coding tools OFF, gateway continues`; `GET /health` 200 `{"status":"ok",...,"implementation":"hermes-rs"}`; `GET /v1/models` (no key) 401; stopped 06:28:59 |
| t7 06:28:36 | same as t4b | live-like | exit 2 every ~3.2 s, NRestarts=7 at 06:28:59, still looping; stopped |
| t6 06:27:00 | `runuser -u nobody -- hermes-gateway --mock --check-config` (bad coding) | - | `coding=error`, rc 2 |

Delays measured (t2): 3.20, 5.50, 9.75, 17.00, 30.23 s. Model in
`crates/hermes-tools/tests/staged_dropin.rs` (systemd `service_restart_usec_next`):
3, 5.33, 9.49, 16.87, 30 s, refused at 64.7 s vs measured 65.7 s (each start adds
~0.2 s of process time + scheduling).

## Box tests

`cargo test -p hermes-tools --test staged_dropin`: 8 passed (4 new:
live_start_limit_never_trips, staged_start_limit_trips_within_its_window,
start_limit_keys_sit_in_the_right_sections, on_error_option_dropin_matches_the_code_value).
Gates 06:31 CET: `cargo fmt --all --check` ok; `cargo clippy -p hermes-tools -p hermes-gateway --all-targets -D warnings` ok; hermes-tools lib 3 + guard 6 + real_ticket 1 + staged_dropin 8 passed, 0 failed; hermes-gateway 3 passed (+0-test suites); hermes-rs 80 passed, 0 failed; `cargo check --tests -p hermes-tools` aarch64-musl + armv7-musleabihf ok; scripts/no-python.sh ok; scripts/spdx-scan.sh 0 missing. No gateway code changed (binary = d76b70df code).
