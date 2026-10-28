<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cortex escalations: why `escalations_open` only grew, and the staged fix

Wave-4 slot 417 `cortex-escalations-diag`, 2026-10-28 (times Europe/Zagreb, CET +01:00).
Branch `swarm/cortex-escalations-diag` off `cortex-handoff-exclude` 6e96ee54.
VPS work was read-only (cortex API, journal, `escalations.jsonl` summary via jq; no content
beyond subject/rule/incident/handoff copied). No live config change, nothing deployed.

## What the live numbers were

| When (CET) | Live `escalations_open` | Source |
|---|---|---|
| 2026-10-28 05:47 | 1 | live watch (row 417) |
| 2026-10-28 15:37 | 11 | slot 392 publish-path check |
| 2026-10-28 18:02 | 0 | this slot, `GET 127.0.0.1:8674/health` |

The 0 at 18:02 is **not** a fix: the 17:37:56 cutover restarted gaia-cortex
(73583ab-llm pid 186171 -> 58a12e6-llm pid 1249123) and the open queue lives only
in memory, so the restart silently dropped all 11. `escalations.jsonl` has no record of that.

## The 11, classified

All 11 were written by cortex pid 186171 (started 2026-10-27 18:17:08, stopped
2026-10-28 17:37:56). Reproduced exactly from the real file by
`tests/escalation_live_corpus.rs` (1 open at 05:47, 11 at 15:37).

| # | id | opened (CET) | subject | rule | handoff | class |
|---|---|---|---|---|---|---|
| 1 | esc-42 | 10-27 19:46:00 | unit:hermes-rs.service | R2-circuit-open | handed-to-hermes | **real incident, recovered, never closed**: hermes-rs running since 10-27 19:46:18, NRestarts 0 (read 18:02) |
| 2 | esc-103 | 10-28 06:25:10 | unit:noloop370-t1-… | R0-stack-wide | handed-to-hermes | test unit (slot 370) |
| 3 | esc-116 | 06:25:40 | unit:noloop370-t2-… | R2-circuit-open | handed-to-hermes | test unit |
| 4 | esc-118 | 06:25:40 | unit:noloop370-t3-… | R2-circuit-open | handed-to-hermes | test unit |
| 5 | esc-122 | 06:27:01 | unit:noloop370-t4-… | R4-unknown-unit | handed-to-hermes | test unit |
| 6 | esc-124 | 06:27:01 | unit:noloop370-t5-… | R4-unknown-unit | handed-to-hermes | test unit |
| 7 | esc-132 | 06:27:11 | unit:noloop370-t4-… | R2-circuit-open | duplicate | test unit |
| 8 | esc-134 | 06:27:11 | unit:noloop370-t5-… | R2-circuit-open | duplicate | test unit |
| 9 | esc-138 | 06:28:38 | unit:noloop370-t4b-… | R4-unknown-unit | handed-to-hermes | test unit |
| 10 | esc-140 | 06:28:38 | unit:noloop370-t7-… | R4-unknown-unit | rate-limited | test unit |
| 11 | esc-145 | 06:29:11 | unit:noloop370-t7-… | R2-circuit-open | rate-limited | test unit |

- **Test units 10/11.** The noloop370 units no longer exist on the VPS
  (`systemctl list-units --all 'noloop370*'` = 0 at 18:02), so their incidents never got a
  "cleared" signal either.
- **Real incidents 1/11** (hermes-rs). It recovered 18 s after the escalation.
- **Stale / never closed: 11/11.** No item was ever closed.
- **387 `exclude_units` would NOT have lowered the count.** It skips only the Hermes inbox
  file; the escalation record and `escalations_open` are unchanged (`handoff:"excluded"`).
  It is also not live (no 92-hermes-exclude.conf drop-in on the VPS).

## Close / expiry logic before this branch (F-417-1..4)

- **F-417-1 (root cause).** An item left `open` only through `POST /escalations/ack?id=`.
  Nothing in the repo calls it (no console button, no Hermes client), and the 37-line live
  `escalations.jsonl` (2026-10-21 19:06 -> 2026-10-28 06:29) has **0 ack lines**. Incident
  resolution (cleared / expired / stale / quiet) never touched escalations. So the count only
  grows, up to the 200-item ring cap.
- **F-417-2.** The open queue is in memory only; every restart empties it without a log line.
  Restarts are the only thing that ever "closed" escalations live.
- **F-417-3.** Escalation ids restart per process (`esc-3` appears 4 times in the file, from
  4 processes), so ids in `escalations.jsonl` are not unique and an ack after a restart can
  never target an old item.
- **F-417-4.** Live `incident_sticky_max_s` is 0 (default), so unit incidents whose unit is
  deleted (test units) stay open forever as well; the max-age close below covers them.

## The staged fix (code + tests, OFF by default)

| Config (JSON) | Env | Default | Effect |
|---|---|---|---|
| `escalation_autoclose` | `CORTEX_ESCALATION_AUTOCLOSE=1` | false | when a cortex incident resolves, every open escalation with that `incident` closes, `why` = `incident-cleared` / `-expired` / `-stale` / `-quiet` |
| `escalation_max_age_s` | `CORTEX_ESCALATION_MAX_AGE_S=N` | 0 (never) | an open escalation older than N s closes as `expired`; checked at the start of every burst and by an idle sweep every N/4 s (5 s .. 15 min); with 0 the decider has no idle wake-ups at all (unchanged zero-idle behaviour) |

- Every close appends `{"close":id,"incident":…,"why":…,"ts":…}` to `escalations.jsonl`
  (like `{"ack":…}`), moves the item to a `closed` ring (200) with `closed_why`/`closed_ts`.
- `GET /escalations` adds `closed_total` and `closed_recent` (newest 20); `open` and `total`
  are unchanged. `/stats` adds `escalations_closed_incident`, `escalations_closed_expired`.
  `/health` `escalations_open` now goes down.
- `POST /escalations/ack` unchanged (not counted as an auto-close).
- Staged drop-in `deploy/staged/cortex-escalations-diag/gaia-cortex.service.d/93-escalation-autoclose.conf`
  (autoclose on, max age 6 h). **Not installed.** An older binary ignores the env vars. Do not
  put the keys into `/etc/gaia-cortex/config.json` for an older binary (`deny_unknown_fields`).

Not changed (owner decisions below): persistence across restarts (F-417-2) and unique ids (F-417-3).

## Owner decisions

- **D-417-1** Deploy the autoclose build + drop-in 93 with the next cortex deploy?
  Rec: **yes** (off by default in code; drop-in is env-only; rollback = remove the file + restart).
- **D-417-2** Max age. A: **6 h (rec)**, which leaves 0 of the 11 at the 15:37 reading. B: 24 h
  would have left all 11 at 15:37 (oldest was 19 h 51 min) and cleared them by 06:30 the next day.
  C: 0 = incident-only (test-unit escalations whose units are deleted would stay until restart).
  Closed items stay in `escalations.jsonl` and `closed_recent`, so nothing is lost.
- **D-417-3** Persist open escalations across restarts (reload `open` from `escalations.jsonl`
  minus ack/close lines at start), plus process-unique ids (e.g. `esc-<start-unix>-N`)? Rec:
  yes, as a follow-up slot; small, but it changes the id format the console/Hermes may read.
- **D-417-4** Install 387's 92-hermes-exclude.conf too? It stops test-unit Hermes tickets
  (cost), but it does not lower `escalations_open`; 417 is what does. Rec: both, same deploy.

## Slot 431: open escalations survive a restart, ids are unique (D-417-3)

Wave-4 slot 431 `cortex-escalations-persist`, 2026-10-28 (Europe/Zagreb, CET +01:00).
Branch `swarm/cortex-escalations-persist` off `cortex-escalations-diag` 9ece3df4. Fixes
F-417-2 and F-417-3. Nothing deployed, no live config change.

### Reload at start (F-417-2)

`Escalations::load` (crates/gaia-cortex/src/sink.rs) replays `escalations.jsonl` once when the
cortex starts:

- a record line (has `id`) opens the item; its open time for `escalation_max_age_s` comes from the
  record's own `ts` (`zagreb_time::to_unix`), so the age keeps counting across restarts;
- `{"ack":id}` removes the item; `{"close":id,...}` removes it and puts it in the `closed` ring
  (`closed_why` / `closed_ts` from the close line), so `closed_recent` survives too;
- an ack/close removes the **most recently opened** item with that id (legacy logs hold one
  `esc-<N>` per process);
- a line that is not a JSON object (a torn last line after a crash mid-append) is skipped and
  counted; if the file does not end in `\n`, one `\n` is appended so the next record starts on its
  own line (the only write `load` ever makes; it never creates the file);
- the 200-item ring cap applies as before.

Counters stay "since this start": `/escalations` `total` and `closed_total` start at 0.
New in `/stats`: `escalations_reloaded` (open items rebuilt), `escalations_reload_skipped`,
`escalation_epoch`. One stderr line per start when the file has lines:
`gaia-cortex: escalations reloaded: Reload { lines, opened, acked, closed, unmatched, skipped, repaired_newline, open, max_epoch }, id epoch N`.

Reload is always on (no switch). An older binary ignores the file content at start, so rollback
is the previous binary.

### Ids (F-417-3): read this if you parse escalation ids (console, Hermes)

| Shape | Written by | Unique? |
|---|---|---|
| `esc-<N>` | builds up to slot 417 (live 58a12e6 included) | no: `N` restarts at 1 per process (`esc-3` x4 in the live file) |
| `esc-<epoch>-<seq>` | slot 431 on | yes: `<epoch>` = unix second the process started, bumped to `max(epoch in log) + 1` when needed; `<seq>` = the process-wide decision/alert/escalation counter (the same counter as `dec-<seq>`, so ids grow 3, 6, 9 ...) |

- `gaia_cortex::sink::parse_esc_id` reads both shapes (`EscId::Legacy(N)` / `EscId::Unique{start,n}`);
  anything else is `None`.
- Treat ids as opaque strings. `POST /escalations/ack?id=` matches the full string, either shape.
- The guarantee is "never equal to an id already in the log": a process that writes no escalation
  may share its epoch with the next start (seen in the real test: run 3 wrote none, run 4 got the
  same second), which cannot collide.
- `dec-<N>`, `al-<N>` and Hermes `hoff-<unix>-<N>` are unchanged. No repo code outside gaia-cortex
  parses escalation ids (searched: console worktrees, hermes-rs, openmind-nerve; the nerve only
  lists `/escalations` as a handoff path). The Hermes ticket embeds the escalation object as-is.
- Legacy duplicate ids share the newest open time when reloaded (they never expire early).

### First deploy effect (live file measured 2026-10-28 23:34 CET, read-only)

The live `/var/lib/gaia-cortex/escalations.jsonl` is 37 lines, 0 ack / 0 close, last write
2026-10-28 06:29 CET, sha256 prefix 33c21973a75e0e8b = byte-identical to
`tests/fixtures/escalations-live-20261028.jsonl`. Live cortex 58a12e6 `/health`
`escalations_open` 0 (in-memory, emptied by the 17:37:56 restart).

- Default config: the first start of this build reopens **all 37** (oldest 2026-10-21) -
  `escalations_open` 0 -> 37.
- With drop-in 93 (`CORTEX_ESCALATION_AUTOCLOSE=1`, `CORTEX_ESCALATION_MAX_AGE_S=21600`): still 37
  at start, then the first burst (any event) closes all 37 as `expired` with 37 close lines;
  the next restart stays at 0. Measured with the real release binary (real test phase B).

### Owner decisions (slot 431)

- **D-431-1** Ship reload + new ids with the next cortex deploy? Rec: **yes**, together with
  drop-in 93 (D-417-1/2), so the 37-item legacy backlog closes on the first burst instead of
  showing up as 37 open.
- **D-431-2** Without drop-in 93: accept 37 reopened legacy items until acked, or clear them first?
  Options: A ship with 93 (rec); B ack them after deploy (37 `POST /escalations/ack`, a live
  state change that needs approval); C archive the old file before the first start (a VPS write
  in /var/lib/gaia-cortex; needs approval). Rec: A.
- **D-431-3** No off switch for reload was added (fewer knobs; rollback = old binary). Add
  `CORTEX_ESCALATION_RELOAD=0` later if wanted? Rec: no.
