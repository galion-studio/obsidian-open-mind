# Rollback investigation — /workspace/swarm, 2026-10-29 ~12:12 CET (Europe/Zagreb)

Read-only on the VPS and GitHub. No git commands were run in /opt/galion/openmind (only `ls`/`find`).
The only box writes were the new guard (/workspace/swarm-guard, /workspace/swarm-snapshots, done later
and authorized under t390u) and this report. All times are CET (UTC+1) unless marked UTC.
Correction to the brief: the box clock is set to CET, not UTC (`date` -> CET; /etc/localtime -> Europe/Zagreb).

## Verdict
This was **not a rollback to an older copy**. The box VM was recreated at 12:12:19 CET (platform host
upgrade a9c79d5 -> ce1748b) and /workspace was **re-copied from the box's persistent store**. The copy took
503 s and ended at 12:20:47 CET. Every file got a new mtime between 12:12 and 12:20 (that is when it was
copied). Today's reports were among the last files copied (12:20:41-43). Anyone who looked before 12:20:47
saw a half-copied folder: today's reports missing, wt/hermes-sandbox-gate with no `.git` (its `.git` file is
stamped 12:20:43), and refs missing. Once the copy finished, the content was current. The newest commit in
base.git is e2b68411 at 12:11:25, about 1 minute before the reset.
**Code and deployment were not affected.** The VPS has not rebooted since 2026-10-22 and nothing on it reverted.
Confidence: high.

## Evidence (box)
- `uptime` 1:27 at 13:43; `who -b` 12:15; PID 1 = `/tini -- /pod-daemon`, started 12:15:52. /pod-daemon, /tini, /dev, /proc and /etc/hostname all stamped 12:12:19.
- `/tmp/sand-box-telemetry.log`: `host_boot_fetch outcome=applied fromVersion=a9c79d5 toVersion=ce1748b`; `desktop_up durationMs=508992`.
- `/tmp/sand-copy-in.log`: `STILL copying after 300009ms (files=277610/428759 ...)` and then `result outcome=hydrated store_entries=428759 files=428759 bytes=18530030905 verified=428759 failures=0 duration_ms=503507`. `/tmp/sand-copy-in-status.json` mtime is 12:20:47.
- `/tmp/sand-host.log`: `hibernate hook installed ... exec runs a store flush`; `box-store sync enabled (snapshot-out only)`; later sync cycles are ok with 0 failures.
- There is no restore script or timer: there is no crontab binary, PID 1 is tini (not systemd), and `journalctl` says "No journal files were found". wtmp and lastlog are empty (0 bytes). dpkg.log ends 2026-10-28 17:25 (base image). reports/t314u-box-restore.md describes a manual agent restore on 10-27; there is no script for it in swarm/bin or swarm/etc, and it did not re-run.
- `/workspace/swarm/backups/grokbot-box-20261027-1823-cet*` is the 10-27 snapshot. It was not extracted over the folder (contents are current, see below).
- The disk-cleanup-20261029 deletions held: none of the 123 removed worktrees came back. The dirty worktrees (e.g. main-staging) were already dirty before the reset (the cleanup report lists them as "Kept: uncommitted changes").
- `state/openrouter-spend-monitor.json` was rewritten by the routine (latest 13:40:27). That is normal.

## 1) Did code survive?
Box (now):
- 25 `*-20261029*` files are in reports/, including hermes-model-picker, galion-stt-install, process-inventory, openmind-memory-status, openmind-vault-state, openmind-integration-ci, sandbox-1/3/4, hermes-cpu-sandbox-t376u, disk-cleanup and deploy-430-435. Today's scratch dirs are also present: scratch/hermes-sandbox-gate-20261029 and scratch/sandbox-1-20261029.
- **Missing: reports/sandbox-2-wiring-20261029.md.** It is nowhere on the box (`find /`) or the VPS, and no file references it. UNVERIFIED whether it was ever written, or whether it was written after the last store sync before 12:12.
- base.git: `swarm/hermes-sandbox-gate` = **e2b68411** (2026-10-29 12:11:25). wt/hermes-sandbox-gate is a valid worktree at e2b68411. The commit is also in the new bundle /workspace/swarm-snapshots/20261029-135004+0100/base-all.bundle. It is NOT on GitHub (404) and NOT on the VPS.
- No local clone has main 7ba85b60, ccca5545, 8887a0cb, 4828997d or a98a75bc. These only exist on GitHub (below). Copies of the other commits are in local clones:
  - e1dafdb1: wt/galion-stt
  - dff00cbe: wt/hermes-ui-rebuild
  - 1d7164b4: wt/galion-stt, wt/hub-incident-redact, wt/staging-rehearsal-1028, tmp407/recon
  - 1010eb7c: wt/hub-incident-redact
  - b7fbbc78: many clones
- /workspace/swarm/all.bundle (34 heads) has none of the target SHAs. /workspace/oom-rust has none of them (newest commit 10-27).
GitHub galion-studio/obsidian-open-mind (get_branch, read-only), all heads as expected:
main 7ba85b60, swarm/galion-stt e1dafdb1, swarm/hermes-model-picker dff00cbe, swarm/hermes-memory-session-scope ccca5545,
swarm/memory-db-stale-diag 1d7164b4, swarm/hermes-rs-memory-client 8887a0cb, swarm/memory-freshness-sensor 1010eb7c,
swarm/hermes-sandboxed-require 4828997d, swarm/realtest-real-env-guard a98a75bc. swarm/hermes-sandbox-gate: 404 (local only).
VPS: /opt/galion/rust/src does not exist. None of the VPS clones (/opt/galion/galion-src/*, /srv/box-archive/2026-10-21/*, /srv/hermes-coding-worktree*) contain any target SHA. /opt/galion/openmind (ls only) has newest mtimes 10-27/10-28 and no file newer than 10:30 UTC today.
Binaries: `sha256sum -c` OK for hermes-gateway-b7fbbc78 and galion-console-serve-dff00cbe.

## 2) VPS units / ports (checked 12:44 UTC = 13:44 CET; VPS up 6 d 14 h, boot 2026-10-22 21:46 UTC)
| unit | state | binary | since (UTC) | drop-ins (latest) |
|---|---|---|---|---|
| hermes-rs | active, enabled | hermes-gateway-b7fbbc78 :8642 | **2026-10-29 12:42:11** (NRestarts=1) | 50,60,70,90-deploy-t291u, **95-cpu-always.conf (written 12:41:36 UTC)** |
| console-rust-primary | active | galion-console-serve-dff00cbe :18080 | 10-29 10:01:32 | 60,62,63-hermes-ui-rebuild-a69e0e35,64-hermes-model-picker-dff00cbe |
| console | active | galion-console-serve-dff00cbe :8080 | 10-29 10:02:54 | same 60/62/63/64 |
| galion-stt | active, enabled | /opt/galion/stt-moonshine/bin/galion-stt :8651 | 10-29 08:17:41 | none |
| galion-nerve | active | openmind-nerve-f66974c :8670 | 10-29 07:34:41 | ...92-merge9-f66974c |
| gaia-cortex | active | gaia-cortex-4b036bc-llm :8674 | 10-29 07:21:09 | ...92-escpersist-4b036bc |
| openmind-memory | active | openmind-memory-aa77864 :8420 | 10-22 21:46:47 | 50,60 |
| galion-desk / galion-one / galion-health / openmind-queue | active | :8646 / :8091+:8002 / :8660 / :19120 | 10-27 / 10-22 | |
| hermes.service | masked | | | |
| galion-backup.timer | every 6 h, last run 12:18:13 UTC, next 18:19:46 UTC | | | |
Health: 8642 /health 200 (hermes-rs); 18080 and 8080 /hermes 200; 8420, 8670 (commit f66974c3), 8674, 8091 and 8651 all 200.
Nothing reverted. One change since the brief: **hermes-rs was restarted 12:41:46-12:42:11 UTC (13:41-13:42 CET)**. It was a systemctl call from SSH session-131260, after drop-in 95-cpu-always.conf (Restart=always, PrivateDevices, InaccessiblePaths=/opt/galion/openmind,/opt/galion/rust/src, unsets HERMES_RS_CODING_* no-landlock vars) was written. The binary is still b7fbbc78, but it is no longer "up since Oct 27". The actor is UNVERIFIED (likely a parallel agent's CPU-sandbox work). It is unrelated to the box reset.
VPS journal 10:55-11:30 UTC shows only routine activity: guardian-tick, sysstat, cron debian-sa1, cloudflared origin errors for :3011/:4001/:4020 (already present), a root `crontab -l` LIST at 10:56:32, 16 SSH logins and scanner probes. No unit changes and nothing touching box sync.

## 3) Microphone model
galion-stt (Moonshine Tiny Streaming, engine `moonshine-tiny-streaming-en`, 16 kHz pcm16le) is active and enabled on 127.0.0.1:8651. /health returns 200 with sessions_active 0 of 2. The model files in /opt/galion/stt-moonshine/models are dated 08:17 UTC. nftables `table inet galion_stt_egress` is present: skuid 994, non-lo traffic is dropped, counter at 13 packets (the install report said 9, all from egress tests). Both consoles run with `--stt 127.0.0.1:8651` (set in drop-ins 63 and 64), and `/api/stt/health` returns 200 on :18080 and :8080. /api/stt/stream was not exercised here (UNVERIFIED now; the install report tested it end to end).

## Surviving copies (exact paths)
- e2b68411: /workspace/swarm/base.git (refs/heads/swarm/hermes-sandbox-gate), /workspace/swarm/wt/hermes-sandbox-gate, /workspace/swarm-snapshots/20261029-135004+0100/base-all.bundle
- Today's reports: /workspace/swarm/reports/*-20261029* (25 files + hermes-model-picker-20261029/), plus copies in /workspace/swarm-snapshots/20261029-135004+0100/swarm.tar.gz
- Other target commits: GitHub branches as above. Local clones: e1dafdb1 wt/galion-stt; dff00cbe wt/hermes-ui-rebuild; 1d7164b4 wt/galion-stt, wt/hub-incident-redact, wt/staging-rehearsal-1028, tmp407/recon; 1010eb7c wt/hub-incident-redact
- Not found anywhere: reports/sandbox-2-wiring-20261029.md

## Guard installed (t390u)
/workspace/swarm-guard/{manifest.py,check.py,snapshot.sh,README.md}, with state/ and alerts/. Snapshots go in /workspace/swarm-snapshots/.
The first baseline and snapshot were taken at 13:50 CET: 945 MB in about 2.5 min (swarm.tar.gz 279 MB, wt-patches 666 MB, base-all.bundle 13 MB).
check.py exits 3 while a box copy-in is running, which covers the failure mode seen today. It exits 2 with an alert file on a real rollback; this was tested on a simulated tree in /tmp/guardtest.
