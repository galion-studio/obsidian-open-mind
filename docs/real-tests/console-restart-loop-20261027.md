<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console.service "restart loop" (nerve inc-dc52e389): read-only diagnosis

Slot 271, `swarm/console-restart-loop-diag`. Wave-4, 2026-10-27, Europe/Zagreb CET (+01:00).
Probes ran on the live VPS from 18:23 to 18:27 +01:00. Every probe was read-only: `systemctl show/cat`,
`journalctl`, `ps`, `ss -ltnp`, `grep` of installed files and `curl` GETs to the nerve on 127.0.0.1:8670.
I changed no units, restarted nothing and wrote nothing on the VPS apart from short-lived /tmp copies of the nerve JSON, which were removed.
Live build at probe time: nerve `openmind-nerve-9d1a4f3` (started 18:15:00 +01:00) and cortex
`gaia-cortex-73583ab-llm` (started 18:17:08 +01:00).

## TL;DR

1. **There is no restart loop.** console.service has started 4 times in 7 days: at boot (2026-10-22 23:46:47 +02:00),
   and then 17:25:04, 17:25:06 and 17:28:29 +01:00 today. systemd `NRestarts=0`, so systemd's own
   `Restart=always` never fired. All three stops today were **systemd jobs** (`Stopping…` MESSAGE_ID
   `de5b426a…`, `JOB_TYPE=stop`, followed by `Stopped`, `JOB_TYPE=restart`). None was a crash. The nerve's
   "degrading: 2 restart(s) in the last hour" is a correct count over a 1-hour window. It aged out, and after the
   18:15 nerve restart `/state` shows console `healthy`, `restarts_1h 0`.
2. **Why each planned restart looks like a crash:** the unit starts srvx 0.11.21 **without `--prod`**. The srvx CLI wrapper
   then forks `node --watch …` and handles SIGTERM with `cleanup("SIGTERM", 143)` → `process.exit(143)`
   (`node_modules/srvx/dist/cli.mjs:402`). The main process therefore *exits* with code 143 (`code=exited`, not
   `code=killed, signal=TERM`). systemd counts that as `Result=exit-code`, the journal shows
   `Failed with result 'exit-code'`, and the nerve emits `unit.exit` and `unit.failed` (high).
3. **Why the cortex restarted it:** cortex R3-known-crash treats any `unit.failed` of an allow-listed unit as a crash.
   It does not check whether a stop or restart job was running. At 17:25:04 the operator ran a `systemctl restart`
   (cutover). The cortex saw the 143 "failure" and ran dec-874 `restart:console.service` at 17:25:06, a second,
   redundant restart. That restart also exited 143, and at 17:28:29 the planned cutover restart (after `daemon-reload`) exited 143 too.
   This produced 3 restarts in about 3.5 minutes, of which the cortex caused 1 and the operator 2.
4. **Side finding (more important than the "loop"):** the "production build" unit runs the console in **srvx dev mode**.
   The banner reads `srvx 0.11.21 · node 22.14.0 · dev`. `NODE_ENV=development` and `.env.local` are used instead of `.env.production`.
   There are 3 node processes: wrapper 138351, `node --watch` 138363 and listener 138369, using about 35 + 30 + 107 MB RSS.
   A file watcher runs on /opt/galion/console-dist, and it would restart the listener on any file change without
   systemd seeing it. Since 17:28, 0 watch restarts are logged. The service also runs as root (`User=` empty).

## Evidence (VPS, read-only)

`systemctl show console.service` at 18:23:42 +01:00:

```
ActiveState=active SubState=running MainPID=138351 NRestarts=0 Result=success
ExecMainStartTimestamp=2026-10-27 17:28:29 +01:00  Restart=always RestartUSec=3s
StartLimitBurst=5 StartLimitIntervalUSec=10s KillMode=control-group KillSignal=15 User=(root)
FragmentPath=/etc/systemd/system/console.service  DropInPaths=(none)
ExecStart=node …/srvx/bin/srvx.mjs --host=127.0.0.1 --port=8080 --static=…/static …/server/index.mjs   # no --prod
```

`ss -ltnp`: `127.0.0.1:8080 node pid=138369` is the grandchild, not MainPID.

Journal (`journalctl -u console.service`, Zagreb time), lifecycle lines only:

```
17:25:04 Stopping console.service            MESSAGE_ID=de5b426a… JOB_TYPE=stop      <- operator systemctl restart
17:25:04 Main process exited, status=143      MESSAGE_ID=98e32220… EXIT_STATUS=143
17:25:04 Failed with result 'exit-code'       MESSAGE_ID=d9b373ed… UNIT_RESULT=exit-code
17:25:04 Stopped                              MESSAGE_ID=9d1aaa27… JOB_TYPE=restart JOB_RESULT=done
17:25:04 Started                              JOB_TYPE=start
17:25:06 gaia-cortex dec-873 R9-process-exit proc:node -> none
17:25:06 Stopping console.service            JOB_TYPE=stop                           <- cortex dec-874
17:25:06 gaia-cortex dec-874 R3-known-crash unit:console.service -> restart:console.service (executed)
17:25:06 Main process exited, status=143 / Failed 'exit-code' / Stopped JOB_TYPE=restart / Started
17:25:07 gaia-cortex dec-876 R10-remediation -> none
17:25:10 galion-nerve ALERT … al-875 "console.service crashed (failure 1 in 900s); restart via nerve allowlist"
17:25:42 nerve escalation batch 36 -> cortex: unit.exit, unit.failed console.service, cortex.alert, incident.opened
17:28:29 systemd Reloading requested from client 'systemctl' (daemon-reload, cutover)
17:28:29 Stopping … exited 143 … Failed 'exit-code' … Started (new Description "production build … console-dist")
17:28:29 node[138351]: srvx 0.11.21 · node 22.14.0 · dev
```

No `Scheduled restart job` (5eb03494…) line appears anywhere, so systemd never restarted it on its own. Starts per 7 days: 4.
Inner `node --watch` restarts since 17:28: 0. /opt/galion/console-dist files are dated 2026-10-18 and unchanged.

Nerve `/state` at 18:25:26 +01:00: console `status healthy`, `restarts_1h 0`, `reasons []`, `status_since 18:15:01`.
The message log still held `incident inc-dc52e389 [warn]: console: degrading: 2 restart(s) in the last hour` (ts 18:01:51).
`/incidents` (HTTP 200) had no console entry left at 18:26.

srvx source (MIT, 0.11.22 package.json in node_modules; the banner reports 0.11.21):

```
cli.mjs:283  if (!cliOpts.prod) runtimeArgs.push("--watch");
cli.mjs:402  process.on("SIGTERM", () => cleanup("SIGTERM", 143));
cli.mjs:12   NODE_ENV = cliOpts.prod ? "production" : "development"
```

## Root cause chain

`systemctl restart` → SIGTERM → srvx wrapper exits 143 → systemd Result=exit-code ("Failed")
→ nerve journal.rs maps `ID_FAILURE_RESULT` → `unit.failed` (high), with no stop-job context
→ cortex rules.rs R3 counts `s.failures()` → `R3-known-crash` → `restart:console.service` (executed live).
The nerve state.rs `restarts_1h` counts every MainPID/ActiveEnter change, whether planned or not → "degrading".

## Fix proposal (owner/deployer decisions; nothing applied)

Ordered by value. Each item is its own branch. None of them needs a console restart now; F1/F2 ride on the next planned console change.

- **F1 unit (console, needs a maintenance restart; D-271-1):** add `--prod` to ExecStart and `Environment=NODE_ENV=production`.
  This removes the `--watch` grandchild and dev mode, and leaves 2 processes instead of 3 (watch RSS about 30 MB saved).
  Also add `SuccessExitStatus=143` so a planned stop is `Result=success` (srvx exits 143 on SIGTERM in prod mode too, cli.mjs:402).
  Optional: `User=` a non-root account. Verify with a GET of `/` and `/console` after the restart.
  Rollback: the previous unit text. This is what removes the false "failed" at the source.
- **F2 nerve (`swarm/nerve-planned-stop`, base 9d1a4f3):** subscribe to `Stopping` MESSAGE_ID `de5b426a63be47a7b6ac3eaac82e2f6f`
  in evt/journal.rs `LIFECYCLE_IDS`. Remember `(unit, INVOCATION_ID)` of a stop/restart job. Tag the following `unit.exit` and
  `unit.failed` of the same invocation with `data.planned=true` and severity Info (event key `unit.planned_stop`).
  The Stopping line is always logged before the exit (seq 21467446 < 21467449), so no reordering window is needed.
  state.rs: count planned starts apart (`planned_restarts_1h`) so they do not raise `degrading`.
  Tests: replay today's 17:25:04–17:28:29 journal JSON, expecting 0 unit.failed high and 0 degrading. A real crash control
  (`kill -9` of a box test unit) must still produce unit.failed high.
- **F3 cortex (`swarm/cortex-planned-stop`, base 73583ab; D-271-2):** in rules.rs R3, before `Outcome::Restart`,
  return `R5b-planned-stop` (info, no action) when the subject's events include a stop or restart job for the unit
  (F2's `planned` flag, or `JOB_TYPE` in `data`), or when `exit_status` is 143 or 130 and the unit is already
  `active` again (`s.recovered()`). Also check `ActiveState` through the nerve just before executing, and drop the action if
  the unit is active and started less than 10 s ago. Test: today's sequence must give 0 restarts, and a crash-exit-1 control must give 1 restart.
- **F4 ops (D-271-3):** a cortex "maintenance hold" for a unit during cutovers (the nerve already publishes `hold` from
  nerve-publish-recoveries ae3eee0; the cortex should honour it). Runbook step: set the hold before `systemctl restart`
  and clear it after verify.

## Owner decisions

- **D-271-1:** apply F1 (`--prod`, `NODE_ENV=production`, `SuccessExitStatus=143`, optional non-root `User=`) at the next console change.
  The choice is A = with the Rust console V2 cutover (the srvx unit is replaced anyway) or B = now, as a separate short maintenance restart.
- **D-271-2:** cortex R3 behaviour on a planned stop. A = ignore (F3, recommended) or B = alert-only.
- **D-271-3:** add the maintenance-hold step to the cutover runbook (yes/no).

## Not done

- No fix code was written or built here, because this is a read-only diagnosis task. F2 and F3 are proposed as follow-up rows.
- I did not identify who issued the 17:25:04 `systemctl restart`. LIVE-FIX-WATCH.md attributes it to the cutover operator.
