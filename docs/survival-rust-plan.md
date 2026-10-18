---
tags: [survival, guardian, rust, study]
project: galion
status: draft
---
<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Survival: Guardian/healer study and Rust port plan (ticket B17, feeds B18)

Written: 2026-10-18T20:09:00+02:00 (Europe/Zagreb). Read-only study. No script was run.
All times below are Zagreb (+02:00). The sources print `...Z` stamps; I converted them. The Rust port must emit +02:00 only.
Base dir: `/opt/galion/galion-light` (abbrev `GL`). The scripts hardcode `/workspace/galion-light`, which is the same tree.

## 1. Probes

| name | target | interval | pass condition | source file:line |
|---|---|---|---|---|
| console | http://127.0.0.1:8080/api/v1/health | 60 s | HTTP 200 or 401, 2.5 s timeout | GL/scripts/guardian-tick.py:19 (timeout :39) |
| hermes | http://127.0.0.1:8642/health | 60 s | HTTP 200 | guardian-tick.py:20 |
| whisper | http://127.0.0.1:8643/health | 60 s | HTTP 200 | guardian-tick.py:21 |
| vision | http://127.0.0.1:8645/health | 60 s | HTTP 200 | guardian-tick.py:22 |
| voice | http://127.0.0.1:8002/health | 60 s | HTTP 200 | guardian-tick.py:23 |
| origin | http://127.0.0.1:80/ | 60 s | HTTP 200 | guardian-tick.py:24 |
| tunnel | process `cloudflared` exists (`pgrep -x`) | 60 s | process found, else critical | guardian-tick.py:51-57 |
| tool-loop warn | newest `[Tool loop warning:` row in hermes state.db `messages` (read) | 60 s | informational; writes `state/eval/tool-warn.txt` once per message id | guardian-tick.py:61-89 |
| hermes (desk) | http://127.0.0.1:8642/health | 30 s | HTTP 200; 503 body containing `loading` also counts as pass | GL/guardian/desk_heal.py:169, :202-203, :363 |
| one (desk) | http://127.0.0.1:8091/health | 30 s | HTTP 200 (or 503 loading) | desk_heal.py:170 |
| whisper (desk) | http://127.0.0.1:8643/health | 30 s | HTTP 200 (or 503 loading) | desk_heal.py:171 |
| vision (desk) | http://127.0.0.1:8645/health | 30 s | HTTP 200 (or 503 loading) | desk_heal.py:172 |
| voice (desk) | http://127.0.0.1:8002/health | 30 s | HTTP 200 (or 503 loading) | desk_heal.py:173 |
| follow-up recheck | same URL as the case, 60 s after a fault | once per case | result held / returned | desk_heal.py:175, :292-304 |
| health-tick | scripts/health-tick.py (not in B17 scope, not read) | 900 s | logs `health ok` | GL/keep.sh:194-197 |
| keep.sh service probes | 8080, 8091, 8093, 8094, 4020, 8642, 8645, 8643, 8002 `/health` via curl | 20 s loop | 200 (8080: 200 or 401) | GL/keep.sh:82-164 |
| upstream: memory | psutil virtual_memory | 30 s (health_check_interval) | warn over 85 %, critical over 95 % | GL/guardian/upstream/diagnostics.py:544-557; guardian.py:63 |
| upstream: disk `/` | psutil disk_usage | 30 s | warn over 85 %, critical over 95 % | diagnostics.py:566-578 |
| upstream: cpu | psutil cpu_percent | 30 s | critical over 95 % | diagnostics.py:587-588 |
| upstream: log patterns | compiled regex set on logs | 300 s deep scan | no pattern match | diagnostics.py:140; guardian.py:64 |

Upstream guardian.py/diagnostics.py/auto_healer.py is a Docker-era library (`/app`, `/tmp/guardian_logs`). Nothing in keep.sh starts it; only guardian-tick.py and desk_heal.py ran live. Treat upstream as a reference for thresholds only.

## 2. Actions

| action | trigger | source file:line | port to Rust? |
|---|---|---|---|
| insert health_checks row per probe | every beat | guardian-tick.py:110-114 | yes |
| open incident (severity high if critical, else low) | probe not healthy and no open incident for service | guardian-tick.py:120-129 | yes |
| resolve incident (`beat healthy`) | probe healthy | guardian-tick.py:115-119 | yes |
| prune health_checks to newest ~4000 | every beat | guardian-tick.py:130 | yes (on the copy) |
| write beat file | every beat | guardian-tick.py:131 | yes (own beat file) |
| write tool-warn.txt | new tool-loop warning id | guardian-tick.py:87-89 | yes (read-only source) |
| write case to desk.jsonl + inbox/<id>.json | new fault signature | desk_heal.py:225-230, :334 | yes |
| nerve line `heal sent` / `heal held|returned` | after ask / after follow-up | desk_heal.py:219-222, :338-341, :299 | yes |
| write open.json and pulse.json | every 30 s cycle | desk_heal.py:347-349 | yes |
| call OpenRouter Flash for one sentence (max_tokens 120) | new fault, not hermes itself | desk_heal.py:243-289 | yes, later and opt-in; needs a key, name only `/workspace/hermes/.env`, never read by the port |
| read API key from env file | desk_heal.py `key()` / `openrouter_key()` | desk_heal.py:209-216, :233-240 | EXCLUDED: secret sealing (port reads no secrets) |
| seal secrets to `/root/galion-secrets`, write `SEALED` stubs, chmod 400 | keep.sh start | GL/keep.sh:20-25; GL/guard/lock-secrets.sh:16,30 | EXCLUDED: secret sealing |
| source sealed env files for children | keep.sh start | keep.sh:32-45, :125-130 | EXCLUDED: secret sealing |
| restart console / one / llama / hermes / vision / whisper / voice when probe fails | keep.sh loop | keep.sh:82-164 | EXCLUDED: restart |
| respawn desk_heal.py if absent | keep.sh loop | keep.sh:206-207 | EXCLUDED: restart |
| `pkill` / `kill` of hermes, origin, keep.sh, cloudflared, vite | blocked by command guard (shows what not to do) | GL/guard/dangerous-patterns.txt:77-82 | EXCLUDED: process kill |
| upstream RESTART_SERVICE | healing plan | upstream/auto_healer.py:42 | EXCLUDED: restart |
| upstream run_command allow-list incl. kill, pkill, systemctl | healing plan | auto_healer.py:721-770 (list :738-742) | EXCLUDED: process kill |
| upstream INSTALL_PACKAGE (pip), CREATE/EDIT/DELETE file, FIX_PERMISSIONS, CLEAR_CACHE, ROLLBACK | healing plan | auto_healer.py:605, :636, :675, :704, :800, :832 | EXCLUDED: restart (mutating repairs; the port observes and records only) |

## 3. guardian.db

Path: `/opt/galion/galion-light/guardian/guardian.db` (356352 bytes, mtime 2026-10-12T06:57:24+02:00). Two older copies exist: `/opt/galion/from-grok-build/workspace/galion-light/guardian/guardian.db` and `/opt/galion/hermes/archive/grok-files/galion-light/guardian/guardian.db`.

Schema was read from the file text (`strings`), because `sqlite3` is not installed on this box and Python is banned. Row counts could NOT be taken: no sqlite3 binary, so no `SELECT count(*)`. Open item for B18 (the Rust crate can count on a copy).

| table | columns | row count |
|---|---|---|
| health_checks | id integer pk, service text, status text, response_time_ms real, error text, checked_at text | unavailable (code prunes to about 4001 max, guardian-tick.py:130) |
| incidents | id text pk, title text, severity text, status text, service text, description text, detected_at text, resolved_at text, resolution text | unavailable |
| snapshots | id text pk, service text, payload text, created_at text | unavailable (nothing in the two scripts writes it) |
| healing | id text pk, service text, action text, status text, started_at text, completed_at text | unavailable (nothing in the two scripts writes it) |

Stored timestamps are in a suffix-Z style (first rows are from 2026-09-24, about 16:30 Zagreb time). B18 must not rewrite them; its own output uses +02:00.

## 4. Why they stopped

Evidence is file mtimes (`stat`) and log tails. Nothing was executed.

| when (Zagreb) | evidence |
|---|---|
| 2026-09-24T16:24:19+02:00 | guardian.log first line, `watch`. 22413 lines total, 18110 are `watch`. |
| 2026-10-07T22:14:58+02:00 | heal.log last readable line (`clear api 0`, line is cut off). File mtime 2026-10-08T00:19:11+02:00, 22623 lines. desk_heal stdout stopped being logged here. |
| 2026-10-07T16:41:20+02:00 | revive.log last entry (`cleared dead lock`). 4 lines. |
| 2026-10-08T01:48:58+02:00 | last `watch` line in guardian.log (all probes green). After this every beat is `down ...`. |
| 2026-10-08T02:10:40+02:00 | guardian.log: `down console,whisper,vision,voice`; by 02:17:50 `whisper,vision,voice`. |
| 2026-10-08T02:11:04+02:00 | desk.jsonl and nerve.jsonl last write (state/heal). No further nerve lines. |
| 2026-10-08T11:00:32+02:00 | gateway-deaths.log last write (350 lines). hermes-restart.log (2026-10-07T16:38:50+02:00) tail says another gateway holds the slot (`hermes gateway run --replace`). |
| 2026-10-08T11:00:20+02:00 | heal-vps.log shows `down api 0` every 30 s; it ends 2026-10-11T11:24:27+02:00 (8192 lines). |
| 2026-10-09T17:43:20+02:00 | persist.log last write (403 lines). |
| 2026-10-10T17:27:37+02:00 | keep.log last write, 22541 lines. Line 18 and the tail: `keep.sh: curl: command not found`. loop.log stops at 17:27:39 with the same error (`loop-daemon.sh: line 4: curl: command not found`). keep.sh itself was edited 2026-10-10T19:04:55+02:00. |
| 2026-10-12T06:56:43+02:00 | health.log last write (`health ok`). |
| 2026-10-12T06:57:10+02:00 | state/heal/open.json and pulse.json last write (desk_heal was still cycling). |
| 2026-10-12T06:57:24+02:00 | guardian.log, guardian.db, `beat` and `/workspace/hermes/last-guardian.ts` all written once more. Last line: `down whisper,voice` (the 2121 count of that state is the most common down state). Nothing after this. |

Reading of the evidence:
1. No `guardian`/`heal` systemd unit exists (`systemctl list-unit-files | rg -i 'guard|heal'` returned nothing). They lived only as children of `keep.sh` (keep.sh:200-207). No process matches `keep.sh`, `desk_heal` or `guardian` now, apart from the search command itself.
2. They were recorders, not fixers: guardian-tick says "It records. It does not restart anything" (line 2); desk_heal forbids restarts in its model prompt (line 258-259). The 5 days of `down whisper,voice` were never repaired by them.
3. The loop that called them broke on 2026-10-10: `curl` is absent in the box, so every keep.sh health probe failed and the supervisor spun (22541 log lines of the same error). After the 2026-10-12 06:57 write, the pod was moved to `/opt/galion` and keep.sh was not restarted. Hypothesis, not proven: the move is the final cause; journals have no guardian unit to confirm.
4. The `heal.log` stop on 2026-10-08 was a log-path/restart artifact; pulse.json kept updating to 2026-10-12.

## 5. Proposed crate `openmind-survival` (for B18)

- Std-only Rust, no dependencies, no async, no threads beyond one sleeping loop with `sleep >= 30 s`. Binary `survival-beat`, plus `--once`.
- Probes: plain `std::net::TcpStream` HTTP/1.0 GET with 2.5 s timeouts, no TLS (all targets are 127.0.0.1). Table from section 1, rows 1-7 plus the 5 desk targets, loaded from a small JSON-lines config file (not hardcoded); pass codes as in the table. Tunnel check: scan `/proc/*/comm` for `cloudflared` (read-only, no pgrep, no kill).
- Host stats: parse `/proc/meminfo` and `/proc/loadavg` with thresholds 85/95 % from upstream. Std has no statvfs, so disk is skipped in v1 (or read `df -P /` output).
- Output, never the live DB: (a) `state/survival/beat.json` and `state/survival/probes.jsonl` (JSON lines, +02:00 stamps from the system offset, never Z); (b) optional later `survival-copy.db` written by a one-way export from a copy of guardian.db. No SQLite writes in v1 (std-only has no sqlite); reading the old db is a B18 stretch and runs only on `cp` of the file.
- Incident logic ported as pure functions: open on first non-pass, resolve on pass, one open per service (guardian-tick.py:115-129), with unit tests on a fake clock.
- Desk logic ported: fault signature `name:detail`, one case per signature, follow-up after 60 s, nerve lines `heal sent` and `heal held|returned` (desk_heal.py:306-350). The Flash call is left out of v1.
- Explicitly absent: secret reading or sealing, kill/pkill, any restart, systemctl, package installs, file edits to other trees.
- Test ports only: 18091, 18002, 18646, 14020, 18080 against a mock listener in the crate tests.
- Every file carries the two SPDX lines. Build: `nice -n 19 cargo build -j 2`.

## Open items
- Row counts of the four tables (no sqlite3 binary; needs a Rust reader or an approved tool).
- Whether the 2026-10-12 06:57 stop was the pod move or a manual stop; no log says.
- scripts/health-tick.py and other upstream modules (boot_manager, db_integration, file_editor) were out of scope and are not read.
