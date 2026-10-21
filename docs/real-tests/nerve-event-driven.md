<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: nerve-event-driven (event mode)

Base `aa77864` (live nerve build = `nerve-t36-consumer` 334beaa + DST fix).
Box ports 20760-20763, VPS sandbox port 8673. Times Europe/Zagreb (+02:00).
No paid call anywhere; the only "LLM" contacted is an in-test loopback stub.

## 1. VPS read-only inventory (2026-10-21 18:13-18:20 +02:00)

`systemctl list-units --type=service`, `ss -tlnp`, `ps -eo ...`, journal reads.
No docker. LXC (Proxmox kernel 6.8.12-43-pve, systemd 255): no `/dev/kmsg`,
no kernel lines in the journal, OOM arrives as systemd `fe6faa94...`.

Journal volume (last hour, read 18:15): 3,738 lines/h unfiltered (cloudflared
stdout 1,900, ssh 574, PID 1 547, of which session scopes ~350). With the
event-mode failure matches: 108 lines in 24 h, all sandbox (`sbx*`) or
transient (`run-u*`) units -> severity low, no escalation.

Live nerve polling rate (read-only `/state` cycle counter): cycle 44 at
18:36:28 -> 75 at 18:51:58 = 31 full cycles in 15.5 min = **120 wakes/h**
(`NERVE_INTERVAL=30`). Live nerve restarted 18:14:58 by another worker
(`75-no-openrouter.conf`), not by this task; untouched here.

## 2. Box tests

- `cargo test -p openmind-nerve`: lib 98 (26 new in `evt::*`), `tests/evt_mode.rs` 3/3:
  - `event_mode_end_to_end`: real binary `serve --mode event`; journal lines
    through a FIFO (`NERVE_EVT_JOURNALCTL=replay:`), inotify dir + record file,
    pidfd on a `sleep`, `POST /event`, `POST /actions`; asserts no wakes while
    idle, warm-up incident not escalated, crash-loop burst of 13 journal lines
    -> exactly 1 `escalation.batch` (critical, delegated) on the NDJSON stream,
    dedupe >= 8, inotify `task.failed` + `slot.done`, pidfd `proc.exit`,
    actions dry-run / 403 / 400, evt events mirrored into `/events`.
    Run: `wakes_total=7 by={debounce:1,http:3,inotify:1,journal:1,pidfd:1} escalations=2 deduped=10`.
  - `openrouter_endpoint_refused_at_start`: `https://openrouter.ai/...` and
    `http://127.0.0.1:8642/...` -> exit 2, "refused".
  - `paid_slot_against_stub_respects_caps`: OpenAI-compatible stub on
    127.0.0.1:20762; key from the NAMED env var arrives only in the
    Authorization header (never in the body, never in `/stats`); two bursts ->
    `called` then `refused` (per-hour cap 1); spend 800 in / 60 out tokens =
    $0.00049, persisted in `evt-llm-spend.json`.
- Workspace (excl. openmind-sandbox, cgroup-detect, hermes-tools): 266 suites,
  **1279 passed / 0 failed / 6 ignored** (base 1241/0/6 at 334beaa).
- fmt, clippy `-D warnings` (all targets) clean; static x86_64 musl release
  1,418,016 bytes, sha256 `4b8b9720...6ad2` (commit 86c0e36).

## 3. VPS sandbox run (18:36:47 -> 18:52:22 +02:00, 15.6 min)

User `sbxevt1836` (temp, group systemd-journal for read access), folder
`/var/tmp/sandbox-test-20261021-183618`, `127.0.0.1:8673`, `NERVE_MODE=event`,
`NERVE_DRY_RUN=1`, `NERVE_REFLEX=0`, escalation `delegate`, watch = sandbox
`tasks/` + `record.md`, `NERVE_EVT_UNITS=nerve-evt-test.service`, default
component table (read-only GETs like the live nerve). Binary 86c0e36.

Inventory recognised at start (153 service units, 29 running, 41 processes, 21 listeners):

| group | running units | processes | ports |
|---|---|---|---|
| nerve | galion-nerve, galion-nerve-shadow | openmind-nerve (live), galion-nerve-shadow, sandbox nerve | 8670, 8672, 8673 |
| gaia | openmind-memory (APOLLO), openmind-queue (POSEIDON), galion-one (ELEUTHIA), link-watch-rs, galion-health, tuner, tanga-proto | 7 | 19120, 8420, 8091, 8002, 8650, 8660, 8671 |
| hermes | hermes-rs | hermes-gateway | 8642 |
| console | console (vite node), galion-desk | 3 | 8080, 8646 |
| infra | cloudflared, tuner-quick, ssh, cron, dbus, fail2ban, postfix, rsyslog, journald, logind, networkd, resolved, networkd-dispatcher, gettys (16) | 23 | 22, 25 (x2), 53 (x2), 20241, 20242 |
| sandbox | - | 3 (probe shell, runuser) | - |
| transient | user@0 | - | - |
| other | 44 loaded, 0 running | - | 18800 (sshd forward) |
| cortex, galion-os, swarm | none on the VPS | | |

Journal lifecycle-tracked units: 16 (incl. galion-backup, cloudflared,
tuner-quick, nerve-evt-test). pidfd watching 14 service main processes.

Timeline (all from `/evt`, `/stats`, `/escalations`):

| time | wake | what |
|---|---|---|
| 18:36:47 | start | warm-up cycle: incidents `edge` (high) + `galion-one-mouth 503` (medium) published, not escalated |
| 18:39:52 | journal | REAL event: another worker's `cortex-sbx-crash-20261021-183907.service` exit 3 + failed -> deferred cycle 18:39:58 -> batch 1 delegated 18:40:22 (1 escalation) |
| 18:41:47 | pidfd | that worker's `runuser` exited |
| 18:42:05 | journal | its crash unit restarted 3x (deduped, count 3) |
| 18:42:15 | journal | SYNTHETIC `systemd-run --unit=nerve-evt-test sleep 5` -> `unit.started` |
| 18:42:20 | journal | `unit.finished` (info) |
| 18:42:25 | journal | SYNTHETIC `nerve-evt-fail` (`sh -c 'sleep 2; exit 3'`) -> `unit.exit` + `unit.failed` (high) |
| 18:42:55 | debounce | batch 2 delegated (1 escalation) |
| 18:43:03 - 18:51:46 | none | 8.7 min silent |
| 18:51:47 | heartbeat | liveness cycle + inventory + thresholds, `nerve.heartbeat` |

Totals at 18:52:22: wakes 16 (journal 9, pidfd 1, deferred-cycle 3,
debounce 2, heartbeat 1), escalations 2 (both delegated, $0), engine cycles 5,
deduped 8, LLM calls 0. `/health` 200 throughout. NDJSON
`/events/stream?since=12&min=high` returned the 3 high lines. RSS: nerve
2.3 MB, journalctl 38 MB; CPU time 0:00 for both.

| | polling nerve (live) | event nerve (sandbox) |
|---|---|---|
| wakes/h | 120 (every 30 s, each a full probe cycle) | idle floor 4/h (heartbeat); measured 61/h avg over this window **with** 9 synthetic wakes and a neighbour's crash loop; 7 wakes in 15.6 min without the synthetic ones (= 27/h, all caused by real events) |
| full probe cycles/h | 120 | 5 in 15.6 min (start, heartbeat, 3 event-triggered) |
| escalations/h | n/a (no LLM) | 2 in 15.6 min: 1 real (cortex sandbox crash unit; after fix 5e... classified sandbox = low = 0), 1 synthetic; idle = 0 |

Fix from this run: units/processes named `*-sbx-*`, run by `sbx*` users or
`runuser` are group `sandbox` (low severity, not lifecycle-tracked, not pidfd
targets). Before the fix the cortex worker's sandbox crash unit counted as
`cortex` and escalated once.

Cleanup 18:52:49: processes killed (0 left), `userdel sbxevt1836`, folder
removed, port 8673 free, transient units `nerve-evt-*` gone
(`CollectMode=inactive-or-failed`), `/tmp/ned-nerve.log` copy removed. Live
`galion-nerve` MainPID 721130, NRestarts 0, drop-ins unchanged
(50/60/70/75) before and after.
