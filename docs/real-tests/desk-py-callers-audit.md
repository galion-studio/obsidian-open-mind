<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# desk-py-callers-audit (wave-4 slot 280): evidence

Europe/Zagreb times (CET +01:00; dates before Oct 25 in CEST +02:00). VPS UTC stamps converted. Box ports 21500-21503
used (21509 = closed probe target). No secret file was opened; `/proc/283/environ` was read as key names only.

## Live VPS, read-only (2026-10-27)

- 19:14:43 `ls -la /opt/galion/bin`: desk-hermes.py 7,889 B (sha `28a393f0…7e0e`), pw.py 2,499 B (`f4ab1ddd…1355`),
  replicate-picture.py 2,969 B (`a27a684e…a8219`), galion-desk 4,735,088 B (`555cc443…11e6`), `__pycache__/`.
- `galion-desk.service`: `ExecStart=/opt/galion/bin/galion-desk`, no Environment; pid 283 since 2026-10-22 23:46:46 +02:00,
  NRestarts 0, listens 127.0.0.1:8646. Env keys: INVOCATION_ID JOURNAL_STREAM LANG MEMORY_PRESSURE_* PATH SYSTEMD_EXEC_PID USER.
- Strings in the live binary: `python3/opt/galion/bin/desk-hermes.py` (1). Source on VPS `/workspace/galion-light/desk-rs/main.rs`
  sha `2e44456b…ab76`, line 217-219 `Command::new("python3").arg("/opt/galion/bin/desk-hermes.py").output()`.
  desk-hermes.py opens state.db `mode=ro` and only reads files (so a GET /desk is a safe probe).
- 19:15:26-19:16:01 passive watch of `ps --ppid 283` every 0.1 s: 0 children (no automatic poller in that window).
- 19:16:02 one `GET http://127.0.0.1:8646/desk` -> `200 10292B 0.170s`; watcher saw
  `child: 230761 283 python3 /opt/galion/bin/desk-hermes.py`; response mindpack populated
  (`"context":{"cached_sessions":35,"cached_turns":153,"cache_bytes":47118,...`). (Body went to `/tmp/.d280` on the VPS
  for the grep and was removed in the same command; confirmed absent 19:22:49.)
- Callers of `/desk`: `console-dist/server/_ssr/desk-foundation-*.mjs` `fetch("http://127.0.0.1:8646/desk")` (live
  console = node srvx on :8080). `galion-health` probes `galion-desk|8646|/health` only (no fork). nerve
  (openmind-nerve-9d1a4f3, galion-nerve), cortex 73583ab-llm, hermes-gateway-b7fbbc78, link-watch: 0 hits for 8646 / `/desk`.
- 19:16:18 recursive grep (`desk-hermes\.py|pw\.py|replicate-picture`) over /opt/galion (excl. openmind, secrets, venv,
  backups, archive, node_modules), /etc, /workspace/galion-light, /root/.hermes, /usr/local/bin, /srv:
  live config hits only `galion-light/desk-rs/main.rs`, `galion-light/skills/playwright-console/SKILL.md`,
  `galion-light/skills/replicate-picture/SKILL.md`, `/opt/galion/bin/pw.py` (self), Hermes V1 caches
  (`cache/pw/REVISION.md`, `cache/WHATS_WRONG.md`, 2 delegation manifests 2026-10-18 19:28/19:36 +02:00,
  `.skills_prompt_snapshot.json` 2026-10-18 20:06 +02:00), session-handoff notes; 1494 of 2010 Hermes V1 request dumps
  (last 2026-10-18 17:33 VPS time) list the skills. /srv hits = box-archive copies of swarm docs. 0 in systemd, cron,
  crontab, timers (19 listed, none matching). `journalctl -u hermes-rs --since -7d`: 0 hits.
- Hermes: `hermes.service` masked; `hermes-rs.service` = `/opt/galion/rust/bin/hermes-gateway-b7fbbc78` (since
  19:07:29 +01:00); binary hits: SKILL.md 0, skills_prompt 0, python3 0, pw.py 0, replicate 0, desk-hermes 0.
- Live python processes: only fail2ban-server, networkd-dispatcher (OS). No desk-hermes/pw/replicate process.
- 19:22:49 nerve /state score 58 grade D (open: hermes-rs, console, galion-health degrading; tuner-quick failing-soon;
  voice-worker failing; galion-one-mouth loading); cortex /health ok mode live incidents_open 5 escalations 0.
  galion-desk not flagged; nothing in this slot changed the VPS.

## Box real test: `crates/galion-desk/tests/real_mindpack_exec.rs` (harness = false, GALION_REAL=1)

Release galion-desk started on 127.0.0.1:21500-21503 with env cleared, all probes to a closed port, own scratch state.
The test binary doubles as the `DESK_HERMES_BIN` stub (records ppid/argc/stdin per call).

| Scenario | Result |
|---|---|
| S1 stub: /health, GET, GET, POST /desk, /health | 3 forks exactly (0 for /health), each `ppid=<desk pid> args=0 stdin=/dev/null`; mindpack == stub JSON |
| S2 `DESK_HERMES_BIN=/bin/false` | mindpack `{}`, desk keeps serving |
| S3 missing binary | mindpack `{}`, no python fallback |
| S4 strings in built galion-desk | no `python3`, `desk-hermes.py`, `pw.py`, `replicate-picture` |
| S5 real release desk-hermes, empty HERMES_HOME | mindpack object with 13 keys (live mindpack also has 13) |

Runs: 19:22:00 (via `GALION_REAL=1 GALION_REAL_DESK_HERMES=… cargo-slot test --release -p galion-desk --test real_mindpack_exec`),
19:22:19, 19:22:20, 19:22:20: 5 passed / 0 failed each. Without GALION_REAL: `SKIP`. No process left, no port left open.

## Gates

`cargo fmt --check -p galion-desk` ok; `cargo clippy -p galion-desk --all-targets -D warnings` ok;
`cargo test -p galion-desk -p desk-hermes`: desk-hermes 4+9, galion-desk 28+9+1 passed, real_wired 3 ignored;
`scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 743 scanned, 0 missing. No new dependencies (serde_json already a
dev-dependency). Sandbox crates not touched, nothing sandboxed run.

## Rule slip (honest note)

At ~19:20 +01:00 one VPS probe line included `python3 -c 0` (a no-op existence check, output discarded). It ran nothing
else and changed nothing, but rule 1 forbids even probes; reported to the parent. All other evidence uses ls/stat/grep/ps.
