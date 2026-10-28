<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-test evidence: cortex-escalations-diag (wave-4 slot 417)

Branch `swarm/cortex-escalations-diag` off `cortex-handoff-exclude` 6e96ee54.
Code: 7e73796b (autoclose + max age, off by default), 32338392 (idle expiry sweep,
live corpus replay, GALION_REAL test, staged drop-in 93, lifecycle doc).
Times are Europe/Zagreb, CET +01:00. Design and owner decisions:
`docs/cortex/escalations-lifecycle.md`. Box only, loopback, slot ports 22870-22879.
VPS was read-only (HTTP GET, `systemctl show`, `ss -ltnp`, `sha256sum`). Nothing deployed.

## 1. Live VPS reading (read-only), 2026-10-28 19:48:07 +01:00

| Probe | Result |
|---|---|
| `GET 127.0.0.1:8674/health` | `escalations_open 0`, `incidents_open 2`, `mode live`, `nerve true`, `ok true` |
| `GET 127.0.0.1:8674/escalations` | open 0, total 0, no `closed_total` (live binary predates this branch) |
| `GET 127.0.0.1:8674/stats` (escalation keys) | `escalations 0`, `escalations_open 0` |
| `systemctl show gaia-cortex` | `gaia-cortex-58a12e6-llm`, PID 1249123, NRestarts 0, active since 2026-10-28 17:37:56 +01:00 |
| cortex drop-ins | 50, 85, 86, 87, 90, 91 (no 92 / 93: staged files are NOT installed) |
| live nerve on :8670 | `/opt/galion/rust/bin/openmind-nerve-9baa450`, sha256 `8c9d7a5f…f6593da` |
| `GET 127.0.0.1:8670/state` | 3 incidents |

The live 0 is the 17:37:56 restart emptying the in-memory queue (F-417-2), not a close.
With the live binary every new escalation will again stay open until the next restart.

## 2. Real end to end, `tests/real_escalation_autoclose.rs` (GALION_REAL=1)

```
GALION_REAL=1 GALION_NERVE_BIN=/workspace/swarm/artifacts/openmind-nerve-9baa450-x86_64-musl \
  nice -n 10 cargo test --release -j 4 -p gaia-cortex --test real_escalation_autoclose -- --nocapture --test-threads 1
```

Nerve binary sha256 `8c9d7a5f…f6593da` = the live VPS nerve binary (checked 19:48).
Real release cortex from this branch. Input: the real VPS journal of the slot-370 test-unit
crash loop (227 lines, 2026-10-28 06:24-06:31) at 8x, a control unit `p417ctl-20261028.service`
crashing 3x then recovering, and one unrelated burst after `escalation_max_age_s`.
Run 2026-10-28 19:59:28 -> 20:06:04 +01:00, `test result: ok. 1 passed; 0 failed`, 396.05 s.

| Phase | Config | After control recovery | End | Close lines | /stats |
|---|---|---|---|---|---|
| off | live (no autoclose, no max age) | open 13 (control 2), closed 0 | open 13, `/health escalations_open 13`, closed_total 0 | 0 | closed_incident 0, closed_expired 0 |
| on | `CORTEX_ESCALATION_AUTOCLOSE=1` + `CORTEX_ESCALATION_MAX_AGE_S` | open 10 (control 0), closed 3 | open 0, `/health escalations_open 0`, closed_total 13 | 13: `expired` 10, `incident-cleared` 2, `incident-quiet` 1 | closed_incident 3, closed_expired 10 |

Both phases: same 13 escalations (11 test-unit, 2 control) plus `nerve-incident:p417-dead`;
incidents_open 7; Hermes temp inbox 6 tickets (handoff unchanged by this branch).
Off reproduces the live growth; on closes all of them, with one close line each.
Earlier run r1 (18:42, same nerve 9baa450): off 13 open / 0 closed, on 0 open / 13 closed.

## 3. Live corpus replay (unit, no network)

`tests/escalation_live_corpus.rs` replays the real live `escalations.jsonl` lines of cortex
PID 186171 (`fixtures/escalations-live-20261028.jsonl`, subject/rule/incident/handoff only):
1 open at 05:47 and 11 open at 15:37, matching the live readings; 2 passed / 0 failed.
`tests/escalation_autoclose.rs`: 5 passed / 0 failed.

## 4. Gates (2026-10-28 19:47-20:07 +01:00, restart r3)

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets -- -D warnings` | ok |
| `cargo clippy -p gaia-cortex --features llm-advisor --all-targets -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect` | 1240 passed, 0 failed, 4 ignored |
| `scripts/no-python.sh` | ok |
| release `gaia-cortex` x86_64-unknown-linux-musl | ok, 1 164 064 B (also `--features llm-advisor` ok) |
| release `gaia-cortex` aarch64-unknown-linux-musl | ok, 940 968 B (linker `rust-lld`; default `cc` fails to link on this box) |
| `cargo check` armv7-unknown-linux-musleabihf, aarch64-linux-android | ok |

No sandbox tests (sandbox_vps_only): none were touched. No new dependencies, so no
`docs/third-party/` entry. Started processes were killed by the test; ports 22870-22879 free after.

## Not done

- No VPS run of the real test: this slot is READ-ONLY VPS (387 ran its test binary there as
  `nobody`; that needs an explicit allowance).
- `systemd-analyze verify` of drop-in 93 on the VPS not run (same reason). The file is env-only
  (two `Environment=` lines) in the same shape as the verified 92.
- Persistence across restarts and unique ids (F-417-2/3) are owner decision D-417-3.
