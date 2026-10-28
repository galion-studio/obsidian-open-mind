<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# VPS baseline ledger 2026-10-20 (the "before" picture; superseded by vps-baseline.md 2026-10-21)

- Plan task: `vps-ops` S1.1 (/workspace/swarm/plans/vps-ops.md). Branch `swarm/vps-baseline` off main 13a296c.
- Host: <vps-public-ip> (`ip50`). It is an LXC container on a ZFS subvolume: 8 vCPU, 16 GB, no swap.
- Probe windows: session 1 ran from **2026-10-20 22:52:32 to 22:52:35 CEST (+02:00)**. Session 2 ran at **22:53:09 CEST (+02:00)**.
  The remote shell ran with `TZ=Europe/Zagreb`, so every time below is CEST (+02:00) unless it says otherwise.
- Method: **read-only**. Two batched ssh sessions under `nice -n 10`. Commands used: `systemctl show -p … / cat / is-enabled / list-units / list-unit-files / list-timers`,
  `ss -ltnp`/`-lunp`, `readlink /proc/<pid>/{exe,cwd}`, `/proc/<pid>/cgroup`, `ps`, `stat`, `sha256sum` (on the file and on `/proc/<pid>/exe`),
  `journalctl … | wc -l`, cloudflared journal refusal counts, one `curl -s -o /dev/null -w '%{http_code}'` GET per URL (loopback, 5 s timeout),
  and `git --no-optional-locks rev-parse` in /opt/galion/openmind.
  Nothing was started, stopped, restarted, written or POSTed.
- Secrets: no secrets directory was read and no EnvironmentFile was opened. Every `Environment=` value was withheld; only the variable **names** are listed.
  EnvironmentFile **paths** are listed. Argv was passed through a key/token/secret/password redaction filter. Nothing matched it.
- Working copy /opt/galion/openmind: HEAD **2f5a730**, unchanged.

## 1. Host
| Item | Value |
|---|---|
| Uptime | 8 days 15:50 (booted Mon Oct 12 07:01 CEST) |
| Load (1/5/15 min) | **4.02 / 3.56 / 3.25** on 8 cores. The plan's 20:42 reading was 7.89 / 7.51 / 6.61 |
| RAM (`free -m`, container view) | total 16384, used 1694, free 12927, buff/cache 1762, **available 14689 MB**. No swap |
| Disk | `/` = `rpool/subvol-1022217-disk-0` (zfs): 100G size, 26G used, 75G available (26%). /opt, /var and /root are on the same fs |
| Timers | galion-backup.timer: last run Tue 20:22:05, **next Wed Oct 21 02:20:50 CEST**. dpkg-db-backup: next Wed 02:00:00 |
| galion-deploy | /opt/galion/rust/bin/galion-deploy, 2830 B, mtime 2026-10-18 22:13:33, sha256 `86207af5617720098c820dee4f6d994e4b4dc57d5a46903b76ef9d85e83aea0b`. Unchanged from the plan. /var/backups/galion/deploy still holds only the 2 galion-health `.bin` backups (Oct 18 22:13 and 22:14) |

## 2. Per-unit ledger
Common to every unit unless the row says otherwise:
- Fragment: `/etc/systemd/system/<unit>.service`.
- **DropInPaths is empty.** No unit has a drop-in except galion-backup.
- StartLimitIntervalSec=10s, StartLimitBurst=5, StartLimitAction=none (the systemd defaults).
- Logs: StandardOutput=journal, StandardError=inherit. Read them with `journalctl -u <unit>`.
- NRestarts=0.

### 2a. Units and binaries
| Unit | Enabled / state | ExecStart (argv) | Restart / RestartSec | StartLimit | Main PID, started |
|---|---|---|---|---|---|
| cloudflared | enabled / active | `/usr/local/bin/cloudflared tunnel --no-autoupdate --config /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml run` | always / 5s | default | 1468472, Sun Oct 18 22:36:14 |
| console | enabled / active | `/opt/galion/tooling/node/bin/node scripts/with-app-env.mjs /opt/galion/console-ide/node_modules/vite/bin/vite.js dev --host 127.0.0.1 --port 8080` (WorkingDirectory /opt/galion/console-ide) | always / 3s | default | 2501864 (listener is child 2501884), Thu Oct 15 08:02:09 |
| galion-desk | enabled / active | `/opt/galion/bin/galion-desk` | **on-failure** / 2s | default | 231, Mon Oct 12 07:01:36 (since boot) |
| galion-health | enabled / active | `/opt/galion/rust/bin/galion-health` (User=root) | always / 5s | default | 1374810, Sun Oct 18 22:14:38 |
| galion-nerve | enabled / active | `/opt/galion/rust/bin/galion-nerve serve` (User=root, Nice 10, SyslogIdentifier galion-nerve, StateDirectory galion-nerve) | always / 10s | **600s / 5** | 1466797, Sun Oct 18 22:35:04 |
| galion-one | enabled / active | `/opt/galion/openmind/dist/bin-ws/galion-one` (WorkingDirectory /opt/galion/galion-light, Nice 5) | always / 5s | **300s / 10** | 1373612, Sun Oct 18 22:13:56 |
| hermes-rs | enabled / active | `/opt/galion/rust/bin/hermes-gateway` (no `--mock`; EnvironmentFile `/opt/galion/hermes/.env`; Conflicts=hermes.service) | **on-failure** / 3s | default | 1361728, Sun Oct 18 22:00:49 |
| link-watch-rs | enabled / active | `/opt/galion/rust/bin/link-watch` (Conflicts=link-watch.service) | always / 3s | default | 1359406, Sun Oct 18 21:57:48 |
| openmind-memory | enabled / active | `/opt/galion/rust/bin/openmind-memory` (User=root; EnvironmentFile `/etc/openmind-memory.env`; Conflicts=memory-gateway.service) | always / 5s | default | 1374875, Sun Oct 18 22:14:40 |
| tanga-proto | enabled / active | `/opt/galion/tanga-proto/bin/tanga-serve` (DynamicUser, Nice 10) | **on-failure** / 5s | default | 1467846, Sun Oct 18 22:35:49. **NRestarts=3** |
| tuner | enabled / active | `/opt/galion/tuner/bin/tuner` (Nice 10) | always / 2s | default | 1196273, Sun Oct 18 19:18:07 |
| tuner-quick | **transient** / active (`/run/systemd/transient/tuner-quick.service`) | `/usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650` | on-failure / 100ms | default | 1198838, Sun Oct 18 19:19:30. **Will not survive a reboot** |
| link-watch (old) | disabled / inactive | `/usr/bin/python3 /opt/galion/galion-light/scripts/link-watch.py` | always / 3s | default | - |
| hermes (old) | disabled / inactive | `/usr/local/bin/hermes gateway run --accept-hooks` (WorkingDirectory /opt/galion; EnvironmentFile `/etc/memory-gateway.env`) | always / 5s | default | - (last start Sun Oct 18 19:36:46) |
| memory-gateway (old) | disabled / inactive | not probed in detail | - | - | - |
| galion-backup | static (timer enabled) | `/opt/galion/rust/bin/galion-backup.sh`, Type=oneshot, Nice 19. **Drop-in** `galion-backup.service.d/offsite.conf`: ExecStartPost/ExecStopPost `/opt/galion/offsite/galion-offsite-encrypt.sh [--perms-only]` | no | default | last run 20:22:05-20:22:08, exit 0 |
| room, guardian-tick, galion-voice-serve | **not-found** (no unit file) | - | - | - | - |

Resource limits set on the live units:

| Unit | Limits |
|---|---|
| galion-health | MemoryMax 16M, CPUQuota 10%, TasksMax 8 |
| galion-nerve | MemoryMax 64M, CPUQuota 15%, TasksMax 64 |
| galion-one | MemoryMax 128M, TasksMax 512 |
| hermes-rs | MemoryMax 64M, TasksMax 64 |
| openmind-memory | MemoryMax 64M, TasksMax 64 |
| link-watch-rs | MemoryMax 32M, TasksMax 16 |
| tanga-proto | MemoryMax 32M, CPUQuota 10% |
| cloudflared, console, galion-desk, tuner, tuner-quick | **none** (MemoryMax infinity). console MemoryCurrent is **1.24 GiB** (vite RSS 1,297,204 kB) |

Environment variable **names** set in the units (all values withheld):

| Unit | Variable names |
|---|---|
| console | PATH |
| galion-health | GALION_HEALTH_ADDR, GALION_LOG_FORMAT |
| galion-nerve | NERVE_ADDR, NERVE_INTERVAL, NERVE_DATA_DIR, NERVE_DRY_RUN, NERVE_FAIL_CYCLES, NERVE_REPORT_EVERY |
| galion-one | GALION_HOME, ONE_BIND, ONE_MOUTH_BIND, ONE_DEVICE_STORE, ONE_HERMES_DIR, ONE_HERMES_ENV, OPENROUTER_KEY_FILE |
| hermes-rs | HERMES_HOME, HERMES_RS_HOST, HERMES_RS_PORT, HERMES_RS_STATE_DB, HERMES_RS_LEGACY_STATE_DB, HERMES_RS_SPEND_FILE, HERMES_RS_CRON_STATE, HERMES_RS_CRON_OUTPUT, HERMES_RS_SESSION_USD_CAP, HERMES_RS_DAILY_USD_CAP, HERMES_RS_DAILY_CALL_CAP, HERMES_RS_MCP |
| link-watch-rs | LINK_WATCH_FRANCE_URL |
| tanga-proto | TANGA_ADDR, TANGA_ROOT, TANGA_PREFIX |
| tuner | TUNER_ADDR, TUNER_ROOT |
| galion-desk, cloudflared, openmind-memory | none inline (openmind-memory uses only its EnvironmentFile) |

### 2b. Bytes, listener, health, logs
The sha256 was taken from the file on disk and from `/proc/<pid>/exe`. **The two match for every unit**, so every service runs the bytes that are on disk.
All times are CEST (+02:00) unless marked.

| Unit | Binary (resolved exe) | Size B | mtime | sha256 | Listener | Health GET → code | Logs (journal lines in the last 1 h) |
|---|---|---|---|---|---|---|---|
| cloudflared | /opt/galion/tooling/bin/cloudflared (via /usr/local/bin symlink) | 39,838,488 | 2026-09-21 23:16:58 | `03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc` | 127.0.0.1:20241 (metrics) + UDP/QUIC | `/ready` 200 | journal, **1163** (1162 "connection refused", §4) |
| console | /opt/galion/tooling/node/bin/node | 120,177,224 | 2025-02-11 01:08:18 CET (+01:00) | `1abce2374a485bddae3c27b17a3e3143e2780232026e627c4fe74ddde3f380a1` | **127.0.0.1:8080** | `/health` 404 (2928 B), `/` 302, **`/package.json` 200 (3505 B)** | journal, 0 |
| galion-desk | /opt/galion/bin/galion-desk | 4,735,088 | **2026-10-10 11:02:43** | `555cc44314aa3d6a2ffe2ca4273cee20c566fb1296ac43c0df7e1ca0a9fd11e6` | 127.0.0.1:8646 | `/health` 200 (11 B), `/` 404 | journal, 0 |
| galion-health | /opt/galion/rust/bin/galion-health | 503,512 | 2026-10-18 22:14:38 | `6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a` | 127.0.0.1:8660 | `/health` **404**, `/` 200 (1950 B; took 0.19 s) | journal, 0 |
| galion-nerve | /opt/galion/rust/bin/galion-nerve | 816,456 | 2026-10-18 22:35:03 | `3b150dea692ffac098945e9413c4fe1caf95ee3a50d3f6c937e09df3598d6d3d` | 127.0.0.1:8670 | `/health` **404**, `/` 200 (3901 B), `/state` 200 (13089 B) | journal (id galion-nerve), 18 |
| galion-one | /opt/galion/openmind/dist/bin-ws/galion-one (**inside the frozen working copy**) | 828,640 | 2026-10-18 22:13:56 | `090c15d0fbb32d372edd87ea3cb154a5be40cbadbb3ea3d8bb4a7a5880e50191` | 127.0.0.1:8091 + 127.0.0.1:8002 | 8091 `/health` 200 (3 B); **8002 `/health` 503** (86 B) | **append:/var/log/galion-one.log** (1,908,335 B, mtime 22:53:01, actively written); journal 0 |
| hermes-rs | /opt/galion/rust/bin/hermes-gateway | 2,173,696 | 2026-10-18 22:00:49 | `cd344b167dc65425b63bcb3765f439ee70a0bf47f355126cac8a75f3192ac0f6` | 127.0.0.1:8642 | `/health` 200 (122 B) | journal, 0 |
| link-watch-rs | /opt/galion/rust/bin/link-watch | 508,248 | 2026-10-18 21:57:47 | `2f5874f72243d24f52eb767f660d57ab14959c205679547b51366fa865f61116` | none (writes /opt/galion/galion-light/state) | n/a | journal, 0 |
| openmind-memory | /opt/galion/rust/bin/openmind-memory | 1,980,096 | 2026-10-18 21:41:22 | `473ae4a3b13ceabba007a89f7e3389df1c91fb6e7986bf2efd5890304ff12e37` | 127.0.0.1:8420 | `/health` 200 (278 B) | journal, 0 |
| tanga-proto | /opt/galion/tanga-proto/bin/tanga-serve | 447,264 | 2026-10-18 22:35:48 | `9c540eeb054dd1205666f116167b9ad8ba7edf08f900ce4228a79d210a1de7f7` | 127.0.0.1:8671 | `/health` 404, `/` 404, **`/tanga/` 200** (41547 B) | journal, 0 |
| tuner | /opt/galion/tuner/bin/tuner | 508,680 | 2026-10-18 19:18:00 | `ecad72a4368a3c9155a676b772ffb7c0caf863884c8614d9fdfcb5604866dd25` (= plan) | 127.0.0.1:8650 | `/health` 200 (12 B), `/` 200 (2356 B) | journal, 0 |
| tuner-quick | /opt/galion/tooling/bin/cloudflared (same bytes as cloudflared) | 39,838,488 | 2026-09-21 23:16:58 | `03f1f25d…dcd68cc` | 127.0.0.1:20242 (metrics) | `/ready` 200 | journal, 0 |

Ports with nothing listening: 4020 (room's default bind) and 8003 (planned voice-serve). Both returned 000 (connection refused).

## 3. Stray listeners (no unit; started by hand in root ssh sessions)
Every stray has ppid 1, cwd `/root/growth-20261018`, and a cgroup under `/user.slice/user-0.slice/session-*.scope`. All bind loopback only.

| Port | PID | Started (CEST) | RSS kB | Argv | Running bytes sha256 (`/proc/pid/exe`) | On-disk file now | `/health` |
|---|---|---|---|---|---|---|---|
| 8431 | 1375169 | Sun Oct 18 22:14:52 | 4040 | `/opt/galion/rust/bin/openmind-memory` (session-48893) | `473ae4a3…ff12e37`: **same bytes as the live 8420 unit** | same file as the unit | 200 (278 B) |
| 8432 | 1412682 | Sun Oct 18 22:23:51 | 6728 | `/root/growth-20261018/bin/openmind-memory.new` **(deleted)** (session-49009) | `e75ddab7ce8c7bbbd84c8452d76b4c5de43abd2e79ddffebe501c1bda4f1b304` | the path now holds **different** bytes `1c35a0d65cc15d65de2f8564c2454a9301ba3acab3658a0ed9b67dd8784df8f0` (2,026,368 B, mtime Oct 18 22:32:24) | 200 (278 B) |
| 8433 | 1428048 | Sun Oct 18 22:24:14 | 3304 | same `.new` (deleted) (session-49012) | `e75ddab7…` (same as 8432) | as above | 200 (277 B) |
| 8652 | 1395619 | Sun Oct 18 22:15:53 | 53172 | `/opt/galion/rust/bin/hermes-gateway --mock` (session-48941) | `cd344b16…` = **same bytes as the hermes-rs unit** | same file as the unit | 200 (122 B) |
| 8653 | 1443105 | Sun Oct 18 22:32:28 | 29820 | `/root/growth-20261018/bin/hermes-gateway.new --mock` (session-49065) | `a68271dadf7c7b9271c6a411075b79adea3a7ca237ebabb48da7ffea518db510` | **still on disk, same sha** (2,195,392 B, mtime Oct 18 22:32:24) | 200 (122 B) |
| 8080 (console) | 2501884 | Thu Oct 15 08:02:09 | 1,297,204 | Vite **dev** server (`vite.js dev`). It sits **in** console.service's cgroup, so it is not a stray; it is listed here because the plan names it one | node `1abce237…` | - | `/package.json` 200 = it serves source |

Relaunch facts for S6.1:
- 8431 and 8652 can be relaunched from the recorded argv; the binary is the unit's own file.
- **8653 can also be relaunched**: its binary is still on disk with the same sha. The plan (§S6.1) says it cannot.
- **8432/8433 cannot be relaunched with the same bytes.** The path now holds a newer build.

## 4. Edge (cloudflared journal, last 1 h before 22:53 CEST)
There were 1162 "connect: connection refused" lines in the last hour.

| Upstream | Refusals |
|---|---|
| localhost:3011 | 239 |
| localhost:4001 | 237 |
| localhost:3004 | 84 |
| localhost:4060 | 6 |
| localhost:4000 | 5 |
| **localhost:4444** | 3 |
| localhost:3020 | 2 |
| 127.0.0.1:3010 | 2 |
| **localhost:4042** | 1 |
| localhost:4020 | 1 |
| localhost:4003 | 1 |

Ports **4444 and 4042** are not in the plan's list of 10 failing ports. Of the plan's 10 ports, only :3005 was not seen in this hour.
The 4020 hit confirms the room-on-4020 collision risk (plan §2).

## 5. What differs from what the repo expects
The repo baseline is main 13a296c: `deploy/*.service`, `deploy/staged/*`, `deploy/survival/*` and the plan's §2.

1. **No survival drop-ins are installed.** `deploy/survival/{console,galion-desk,galion-one,link-watch,tuner}.d/10-restart.conf` expect Restart=always/5s and StartLimit 60s/5. Live: DropInPaths is empty on all of them. galion-desk is still `Restart=on-failure`/2s. console, tuner and galion-one keep their own RestartSec (3s, 2s, 5s) and StartLimit (10s/5, 10s/5, 300s/10). **guardian-tick is not-found.**
   The survival `link-watch.d` targets **link-watch.service**, which is the old, disabled Python unit. The live Rust unit is **link-watch-rs.service**. As staged, that drop-in would never apply.
2. **galion-one.** The live unit is byte-for-byte the staged `deploy/staged/galion-one.service`: same ExecStart, the same 7 env names, StartLimit 300/10 and MemoryMax 128M. Its header still says "STAGED, NOT INSTALLED" and it has no [Install] section, yet it is **enabled**. `deploy/galion-one.service` (top level) expects `dist/bin/galion-one` with RestartSec 3; live runs `dist/bin-ws/galion-one` with RestartSec 5, from inside the frozen working copy. The mouth still answers 503.
3. **galion-desk.** `deploy/galion-desk.service` expects `/opt/galion/openmind/dist/bin/galion-desk` with DESK_BIND and DESK_HERMES_BIN. Live runs `/opt/galion/bin/galion-desk` (built Oct 10, sha `555cc443…`) with no Environment and Restart=on-failure.
4. **tuner.** `deploy/staged/tuner.service` expects `/opt/galion/openmind/dist/bin-ws/tuner`, a path inside the working copy. Live runs `/opt/galion/tuner/bin/tuner` with sha `ecad72a4…dd25`, which matches the plan.
5. **hermes-rs.** `deploy/staged/hermes-rs.service` expects `/opt/galion/hermes-rs/hermes-gateway --mock` on **8652**. Live is `/opt/galion/rust/bin/hermes-gateway` **without --mock** on **8642**.
   The `--mock` instance on 8652 exists, but only as a hand-started stray using the unit's own binary.
6. **openmind-memory.** The staged unit expects `/opt/galion/openmind/dist/openmind-memory` and ReadWritePaths covering the shadow dir and tencentdb. Live is `/opt/galion/rust/bin/openmind-memory` with ReadWritePaths `/var/lib/memory-tencentdb` only.
7. **console** listens on **127.0.0.1:8080**, not 0.0.0.0:8080 as the `deploy/console-prod.service` comment says. It is still the Vite dev server: `/package.json` returns 200, and its RSS is **1.27 GB** (the plan said about 505 MB) with no MemoryMax.
   No unit file in the repo covers live console, cloudflared, galion-health, galion-nerve, tanga-proto or tuner-quick.
8. **Health paths.** `/health` returns 404 on galion-nerve 8670, galion-health 8660 and tanga 8671. Probes for those must use `/`, `/state` and `/tanga/`. Only tanga-proto has restarted since start (NRestarts=3); every other unit is at 0.
9. **Strays.** 8653's binary is still on disk (the plan says it cannot be relaunched). 8432/8433 run deleted bytes `e75ddab7…`, while the same path now holds `1c35a0d6…`. 8431 and 8652 are duplicates of live unit binaries.
10. **Load and edge.** Load is 4.02 against the plan's 7.89, with 14.7 GB RAM available and disk at 26%. cloudflared adds refusals on **4444 and 4042**, beyond the plan's 10 ports.
    These match the plan: galion-deploy sha `86207af5…`, working copy 2f5a730, the tuner sha, the transient tuner-quick, 12 project units active, and 2 deploy backups.

## 6. Re-running this baseline (read-only)
The probe was two heredoc batches sent as `ssh -i ~/.ssh/id_ed25519_vps -o IdentitiesOnly=yes root@<vps-public-ip> 'nice -n 10 bash -s'`. No script was added to the repo (Rust-only rule).
For each unit it ran:
- `systemctl show <u>.service -p Id,LoadState,ActiveState,SubState,UnitFileState,FragmentPath,DropInPaths,Restart,RestartUSec,StartLimitIntervalUSec,StartLimitBurst,StartLimitAction,NRestarts,MainPID,ExecMainStartTimestamp,WorkingDirectory,User,StandardOutput,StandardError,EnvironmentFiles,MemoryCurrent,MemoryMax,CPUQuotaPerSecUSec,TasksMax,Nice,ExecStart,Transient`. The `Environment` property is deliberately absent.
- `systemctl cat`, with every `Environment=` line replaced by its variable names.
- `readlink /proc/$MAINPID/exe`, `stat -c '%s %y'`, `sha256sum <file> /proc/$MAINPID/exe`.
- `journalctl -u <u> --since -1h | wc -l`.

Then, once for the whole host:
- `ss -ltnp`, plus pid, ppid, lstart, rss, cgroup, exe sha and cwd for every listener pid.
- One `curl -s -m 5 -o /dev/null -w '%{http_code}'` GET per URL in §2b and §3.

Compare a re-run against §2b and §3. Any sha, mtime, port or code change is drift.
