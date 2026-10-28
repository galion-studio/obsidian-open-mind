<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Slot 357 cortex-nerve-pair-runbook: real-test record

Branch `swarm/cortex-nerve-pair-runbook` off cortex-live-merge-4 `58a12e61`. All
times Europe/Zagreb CET (+01:00), 2026-10-28. Box loopback ports 22270-22279; VPS
throwaway `/var/tmp/pair-357-20261028052100` as `nobody` on 127.0.0.1:22270/22271.
**No deploy**: no live file, unit, permission or service changed.
Runbook: `docs/cutover/cortex-nerve-pair.md`.

## Artifacts

| role | build | bytes | sha256 | source |
|---|---|---|---|---|
| candidate cortex | 58a12e61 llm-advisor x86_64 musl | 1,147,680 | `7ca6512b1ecb1d66eea6446498c92812796fd2d98374a6b5c24ce2e00edd49a2` | slot 341 artifact |
| candidate nerve | 9baa450c x86_64 musl | 1,479,456 | `8c9d7a5fa4f84d6d56d6185dfd5b71171c8ba5b17bf49c0bde7fb5503f6593da` | rebuilt here 05:10 from `git archive 9baa450c`, byte-identical to slots 333/352 |
| rollback cortex | 73583ab llm | 1,136,200 | `3cdab111…6af5` | = VPS `/opt/galion/rust/bin/gaia-cortex-73583ab-llm` (sha256sum 05:09) |
| rollback nerve | 9d1a4f3 | 1,458,760 | `d0c70b87…1d45` | = VPS `/opt/galion/rust/bin/openmind-nerve-9d1a4f3` (sha256sum 05:09) |

12956b45 is an ancestor of 9baa450c (`git merge-base --is-ancestor`), so the merge-6
build carries the planned-stop tag (binary marker `unit.planned_stop` 2 hits; live
9d1a4f3 0 hits) and the deep probe (`deep route` 1 hit; 9d1a4f3 / 12956b4 0 hits).

## Live state read (VPS, read-only, 05:09-05:29)

- `gaia-cortex` active, MainPID 186171, NRestarts 0, drop-ins 50/85/86/87/90, effective
  `ExecStart=/opt/galion/rust/bin/gaia-cortex-73583ab-llm serve --config /etc/gaia-cortex/config.json`.
  `/health`: ok, mode live, llm_enabled, hermes_handoff, busy false, incidents_open 2, events_in 118-119.
- `galion-nerve` active, MainPID 184014, NRestarts 0, drop-ins 50/60/70/75/80/85/90, effective
  `openmind-nerve-9d1a4f3 serve --mode event`. `/health` cycle 52-53, score 64, degraded.
- `/etc/galion-nerve/components-live.json` 12 entries, 2,669 B, sha256 `dc343493…e05c`,
  mtime 2026-10-21 21:56 CET; `jq -S` diff against the staged file = exactly the two
  D-333-1 changes (`deep` on console, new console-rust-primary).
- systemd 255 (255.4-1ubuntu8), kernel 6.8.12-43-pve, `/` 30 % used.
- Same MainPIDs and NRestarts 0 for both units after all tests (05:29).

## Box gates

| gate | time | result |
|---|---|---|
| `cargo fmt -p gaia-cortex -p openmind-nerve -- --check` | 05:30 | ok |
| `clippy -p gaia-cortex --all-targets -D warnings` (default and `--features llm-advisor`) | 05:30 | ok (0 warnings) |
| `cargo test -p gaia-cortex --all-targets` default / `llm-advisor` | 05:30 | **106/0** each (101 of merge-4 + 5 new; real suites skip-pass without GALION_REAL) |
| `scripts/no-python.sh`, `scripts/spdx-scan.sh` | 05:25 | ok; 433 scanned, 0 missing |

## Real tests (box, release, `GALION_REAL=1`)

`pair_preflight` 5/5, three runs (05:16:48-05:18:07, 05:19:01-05:20:20,
05:30:40-05:31:59; 79.8 s each; same numbers each run). Final run:

- `artifacts_match_manifest`: all 4 binaries size + sha256 = `pair.json`, ELF64 x86_64,
  no PT_INTERP, `--version` ok (`gaia-cortex 0.1.0` / `openmind-nerve 0.1.0`), markers
  present; negative controls: rollback nerve has no `unit.planned_stop`, rollback cortex no R5b.
- `deploy_order_rehearsal` (persistent data dirs across swaps, dry-run nerve, FIFO journal replay):

| step | swap → up | re-subscribed | planned restart (unit) | crash (unit) |
|---|---|---|---|---|
| 0 live pair 9d1a4f3+73583ab | 101 / 100 ms | 1 ms | console: R3-known-crash, **1 restart request** | hermes-rs: R3, 1 |
| 1 cortex → 58a12e6 | 101 ms | 1414 ms | galion-desk: **R5b, 0** (no nerve tag) | galion-voice: R3, 1 |
| 2 nerve → 9baa450 | 101 ms | 908 ms | tuner: **R5b, 0** (nerve tag true) | link-watch-rs: R3, 1 |
| 3 rollback nerve → 9d1a4f3 | 101 ms | 910 ms | galion-one: R5b, 0 | — |
| 4 rollback cortex → 73583ab | 101 ms | 1415 ms | openmind-memory: R3, 1 (bug back) | galion-health: R3, 1 |

  Cortex incidents_open carried across every swap (4 → 5 → 4 → 4 → 5): the
  cortex state dir is readable forwards and backwards. Ports 22270/22271 closed,
  temp dir removed after.
- `real_planned_stop_pair` with the **merge-6 nerve** (was only run with 12956b4
  before), `REAL_CORTEX_PORT_BASE=22270`, `--features llm-advisor`, 05:20:30-05:22:46,
  **1/1 ok** (126.3 s): pair R5b, R5b, R5-recovered, R5b, 0 requests; with live cortex
  R5-recovered, R99, R99, 0 requests; split tagged R99 + R5b, 0; split base nerve R3, 1;
  crash / kill -9 / restart-then-crash R3, 1 each. Same as slot 341's run with 12956b4.

Logs: `/workspace/swarm/logs/cortex-nerve-pair-runbook-{real1,real2,real3,real-pair,gates,nerve-build}.log`.

## VPS unit verify (05:22, throwaway copies, no reload)

Copies of the live `gaia-cortex.service` / `galion-nerve.service` + all live drop-ins
in `/var/tmp/pair-357-…/units/{A,B,C}`, `SYSTEMD_UNIT_PATH=<dir>: systemd-analyze verify --man=no`:

| set | content | gaia-cortex | galion-nerve |
|---|---|---|---|
| A | live only (= rollback state) | rc 0 | rc 0 |
| B | live + staged 91 drop-in (`/opt/...` path, binary not installed) | rc 1 `Command /opt/galion/rust/bin/gaia-cortex-58a12e6-llm is not executable` | rc 1 same for `openmind-nerve-9baa450` |
| C | live + staged 91 pointed at the throwaway copy of the binary | rc 0 | rc 0 |

B proves the 91 drop-in's `ExecStart` is the effective one and that verify catches
a missing binary; C proves the unit set is valid once the binary exists.

## VPS shadow pair (05:24:06-05:28:54, as `nobody`)

Candidate nerve 9baa450 (staged table, `NERVE_REFLEX=0 NERVE_DRY_RUN=1 NERVE_ROLLBACK=0
NERVE_EVT_ACTIONS=dry-run NERVE_EVT_ESCALATE=off NERVE_EVT_HEARTBEAT=60`) on :22270 and
candidate cortex 58a12e6 (`CORTEX_ACTIONS=dry-run CORTEX_LLM_ENABLED=0 CORTEX_HERMES_HANDOFF=0`,
no env file, minimal config) on :22271 subscribed to it, against the real live services.
10 samples every 28 s:

- Shadow cortex `/health` ok, `nerve:true`, mode dry-run, llm off, busy false in all 10;
  events_in 3 → 8; incidents_open 3. Shadow nerve `/health` ok, cycles 1-5, score 70 → 68.
- Shadow cortex decisions: R11-nerve-incident ×3 (`console`, `edge`, `galion-one`),
  R99-default ×5 (nerve/inventory heartbeat). **0 nerve actions**.
- Components: same status as live for 12 of 15; differences are the expected ones of
  slot 352: `console` degrading (deep 404 ×2, live healthy = the F-313-2 blind spot),
  `console-rust-primary` degrading (new), `cloudflared` degrading vs live failing-soon
  (`probe_errors: ["journald: journalctl exit 1"]`, `nobody` cannot read the journal;
  the live nerve runs as root).
- `unmanaged_ports`: shadow `[]`, live `[18080]`.
- Live nerve stayed at cycle 53 for the whole run (no live cycle overlapped), so the
  shadow ports were never seen as stray listeners. Live cortex unchanged (events_in 119).
- Cleanup 05:28:54-05:29:39: both processes ended, 0 `nobody` processes left, ports
  22270-22279 closed, folder removed. Raw captures kept on the box only
  (`/workspace/swarm/logs/cortex-nerve-pair-runbook/`, contain hostnames/incident text).

## Findings

- F-357-1: `--version` of both binaries prints only the crate version (0.1.0), so the
  sha256 is the only build identity; the preflight and runbook check it. Suggest a
  build-info commit in `--version` (separate task).
- F-357-2: nerve-merge-6 doc says the live table had 13 entries; the live file has 12.
- F-357-3: the rehearsal shows the rollback brings back the console planned-restart
  false positive (R3 + restart request) by design; worth knowing before choosing rollback.

## Not done

- No deploy, no install, no reload. VPS writes were only the throwaway `/var/tmp`
  folder (removed). Hermes not used.
