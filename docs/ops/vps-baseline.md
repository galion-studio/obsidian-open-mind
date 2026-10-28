<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# VPS baseline ledger (refreshed 2026-10-21 11:23 CEST +02:00)

- Plan task: wave-4 slot 182 `vps-baseline-2` (READ-ONLY refresh of S1.1). Branch `swarm/vps-baseline-2` off `swarm/vps-baseline` f403eeb.
- The 2026-10-20 22:52 CEST (+02:00) ledger is kept unchanged as [vps-baseline-20261020.md](vps-baseline-20261020.md). §5 below is the diff against it.
- Host: <vps-public-ip> (`ip50`). LXC container on ZFS: 8 vCPU, 16 GB, no swap.
- Probe windows (remote shell `TZ=Europe/Zagreb`): batch 1 **2026-10-21 11:23:08**, batch 2 **11:23:38**, batch 3 **11:23:59**, batch 4 **11:24:22**, all CEST (+02:00).
  A stability re-check ran at 11:25:46 (all 14 PIDs, exe sha and NRestarts unchanged; see docs/real-tests/vps-baseline-2.md).
- Method: **read-only**, same as the 20 Oct run: `ssh … 'nice -n 10 bash -s'` heredoc batches with `systemctl show/cat/list-units/list-timers`, `ss -ltnp`,
  `readlink /proc/<pid>/exe`, `stat`, `sha256sum` (file and `/proc/<pid>/exe`), `journalctl … | wc -l`, cloudflared refusal counts by upstream,
  and one `curl -s -m 5 -o /dev/null -w '%{http_code}'` GET per URL. Nothing was started, stopped, restarted, written or POSTed.
- Secrets: no secrets directory and no EnvironmentFile was opened. In unit files and drop-ins every `Environment=` value was replaced by the variable **name** before it left the VPS.
- Working copy /opt/galion/openmind: HEAD **2f5a730**, unchanged.

## 1. Host
| Item | 2026-10-21 11:23 | 2026-10-20 22:52 |
|---|---|---|
| Uptime | 9 days 4:21 (same boot, Mon Oct 12 07:01) | 8 days 15:50 |
| Load 1/5/15 | **8.88 / 8.53 / 8.29** on 8 cores | 4.02 / 3.56 / 3.25 |
| RAM (MB) | used 1591, buff/cache 2129, **available 14792** | used 1694, available 14689 |
| Disk `/` (zfs) | 100G, 26G used, 75G avail (26%) | same |
| galion-backup.timer | last Wed 08:18:18, **next Wed 14:17:48** | last Tue 20:22:05 |
| galion-deploy | 639,664 B, mtime **2026-10-21 04:31:47**, sha256 `2a60e1ac632e27d9d918102d5fb02e1bafe35d4a48ead734f403f650de07464d` | 2830 B shell script `86207af5…` |

## 2. Per-unit ledger
Defaults unless a row says otherwise: fragment `/etc/systemd/system/<unit>.service`; StartLimit 10s/5; logs to the journal; NRestarts 0.
**File sha256 = `/proc/<pid>/exe` sha256 for every running unit** (every service runs the bytes on disk; no `(deleted)` exe).

### 2a. ExecStart, drop-ins, bytes
| Unit | State | Effective ExecStart | Drop-ins | Binary size B / mtime | sha256 | Main PID, started |
|---|---|---|---|---|---|---|
| cloudflared | enabled/active | `/usr/local/bin/cloudflared tunnel --no-autoupdate --config …/cloudflared/config.yml run` | none | 39,838,488 / 2026-09-21 23:16:58 | `03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc` | 22835, **Wed 08:32:32** |
| console | enabled/active | node … `vite.js dev --host 127.0.0.1 --port 8080` | none | 120,177,224 / 2025-02-11 01:08:18 +01:00 | `1abce237…f380a1` | 2501864, Thu Oct 15 08:02:09 |
| galion-desk | enabled/active | `/opt/galion/bin/galion-desk` (on-failure/2s) | none | 4,735,088 / 2026-10-10 11:02:43 | `555cc443…11e6` | 231, Mon Oct 12 07:01:36 |
| galion-health | enabled/active | `/opt/galion/rust/bin/galion-health` | none | 503,512 / 2026-10-18 22:14:38 | `6afe668c…e01a` | 1374810, Sun Oct 18 22:14:38 |
| galion-nerve | enabled/active | **`/opt/galion/rust/bin/openmind-nerve-aa77864 serve`** (StartLimit 600s/5, Nice 10) | **`50-rust-bin.conf`** (ExecStart override, 09:43:54), **`60-nerve-eventlog.conf`** (env name `NERVE_EVENT_LOG`, 09:31:06) | 1,213,216 / **2026-10-21 09:41:20** | `f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68` | 87026, **Wed 09:43:54** |
| galion-nerve-shadow | **disabled**/active (new) | `/opt/galion/rust/bin/galion-nerve-shadow serve` (on-failure/30s, Nice 15, MemoryMax 256M) | none | 1,438,496 / 2026-10-21 04:37:18 | `3a566c56dfea79cf8cb28bc413c2f7e3f4004969380c9be1ce7dca61382256f1` | 4008991, **Wed 04:37:18** |
| galion-one | enabled/active | **`/opt/galion/rust/bin/galion-one`** (StartLimit 300s/10, Nice 5) | **`50-rust-bin.conf`** (ExecStart override, 08:39:24, from one-no-shellout 40454f9) | 963,904 / **2026-10-21 08:39:10** | `24cb4976b1325ae0f5a100a7165a1e2e0bbd29f3f2c0e584dbd3a1cdd03b7ca3` | 29782, **Wed 08:39:24** |
| hermes-rs | enabled/active | **`/opt/galion/rust/bin/hermes-gateway-aa77864`** (on-failure/3s; EnvironmentFile path `/opt/galion/hermes/.env`) | **`50-rust-bin.conf`** (ExecStart + SyslogIdentifier=hermes-gateway, 09:46:21), **`60-nerve-url.conf`** (env name `NERVE_URL`, 09:32:33) | 1,841,904 / **2026-10-21 09:41:31** | `eb691b2967eb03758dcc6b2b6f46fe0a9d45d779b4d4bf937a28f37b19633e2f` | 89484, **Wed 09:46:22** |
| link-watch-rs | enabled/active | `/opt/galion/rust/bin/link-watch` | none | 508,248 / 2026-10-18 21:57:47 | `2f5874f7…1116` | 1359406, Sun Oct 18 21:57:48 |
| openmind-memory | enabled/active | **`/opt/galion/rust/bin/openmind-memory-aa77864`** (EnvironmentFile path `/etc/openmind-memory.env`) | **`50-rust-bin.conf`** (ExecStart + SyslogIdentifier=openmind-memory, 09:45:26), **`60-nerve-url.conf`** (env name `NERVE_URL`, 09:32:33) | 1,702,640 / **2026-10-21 09:41:26** | `18e35ab49a76702b47742b8c5d4f95f29c3956839979e7df7a6ede671f1d2bb6` | 88565, **Wed 09:45:26** |
| openmind-queue (POSEIDON) | **enabled/active (new)** | `/usr/local/bin/openmind-queue serve --bind 127.0.0.1:19120 --data /var/lib/openmind-queue` (User openmind-queue, on-failure/2s, MemoryMax 128M, ProtectSystem=strict, no caps) | none | 889,632 / 2026-10-21 08:27:27 | `8a7f7faaaa864e0a6e5f628f723081a48e2179a21130062f8569aa1685f8ec8d` | 17979, **Wed 08:27:54**, **NRestarts=1** |
| tanga-proto | enabled/active | `/opt/galion/tanga-proto/bin/tanga-serve` (DynamicUser tanga-proto) | none | 447,264 / 2026-10-18 22:35:48 | `9c540eeb…7f7` | 1467846, Sun Oct 18 22:35:49, NRestarts=3 |
| tuner | enabled/active | `/opt/galion/tuner/bin/tuner` | none | **480,032 / 2026-10-21 04:32:32** | **`89c7fcbcb54c510887dafd8c67b255f05d00595af598fe65f022e204c0ef2b12`** | 4004553, **Wed 04:32:32** |
| tuner-quick | **transient**/active | `/usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650` | none | same bytes as cloudflared | `03f1f25d…` | 1198838, Sun Oct 18 19:19:30. Still will not survive a reboot |
| galion-backup | static (timer) | `/opt/galion/rust/bin/galion-backup.sh` oneshot | `offsite.conf` (unchanged, 2026-10-18 22:22:07) | 3026 B script | `0897efc0…` (16-hex prefix) | last run Wed 08:18:18 |
| hermes (old) | **masked** (`/etc/systemd/system/hermes.service` → /dev/null since Wed 08:24:00) | - | - | - | - | - |
| link-watch, memory-gateway (old) | loaded, inactive | - | - | - | - | - |
| poseidon, galion-cortex, cortex | **not-found** (POSEIDON runs as openmind-queue; there is **no cortex unit**, the cortex runs inside the nerve's /state) | - | - | - | - | - |

Old binaries are still on disk next to the new ones (rollback = delete the `50-rust-bin.conf`): `galion-nerve` `3b150dea…` (816,456 B), `hermes-gateway` `cd344b16…` (2,173,696 B), `openmind-memory` `473ae4a3…` (1,980,096 B), and `/opt/galion/openmind/dist/bin-ws/galion-one` (828,640 B, Oct 18 22:13).

Resource limits (changes in bold): galion-health MemoryMax 16M CPUQuota 10%; galion-nerve 64M / 15%; galion-one 128M; hermes-rs 64M; openmind-memory 64M; link-watch-rs 32M; tanga-proto 32M / 10%;
**openmind-queue 128M**; **galion-nerve-shadow 256M**; console, cloudflared, galion-desk, tuner, tuner-quick none (console MemoryCurrent **1.20 GiB**).

Environment variable **names** (values withheld): galion-nerve now `NERVE_ADDR NERVE_INTERVAL NERVE_DATA_DIR NERVE_DRY_RUN NERVE_FAIL_CYCLES NERVE_REPORT_EVERY` **`NERVE_EVENT_LOG`**;
hermes-rs and openmind-memory **+ `NERVE_URL`**; galion-nerve-shadow `NERVE_ADDR NERVE_DATA_DIR NERVE_DRY_RUN NERVE_REFLEX NERVE_ROLLBACK NERVE_INTERVAL NERVE_FAIL_CYCLES NERVE_REPORT_EVERY NERVE_KNOWN_PORTS`;
openmind-queue none. All other units as on 20 Oct.

Drop-in file sha256 (16-hex prefix): galion-nerve 50 `98d5cec1f2a305de`, 60 `c663ab5cbe25c3fb`; galion-one 50 `467d894b661cfe8e`; hermes-rs 50 `910cddb2eb9fc187`, 60 `833acdfbb2a68367`;
openmind-memory 50 `8ea8565d02589918`, 60 `fdd150c7377f0f49`; galion-backup offsite `e370c03716b738ee`.
Note: the 60-* drop-ins still carry the comment "inert until a nerve-client build is deployed"; that is now stale, because the 50-rust-bin.conf builds (aa77864) read those variables.

### 2b. Listener, health code, logs
| Unit | Listener | GET → code (bytes) | Journal lines, last 1 h |
|---|---|---|---|
| cloudflared | 127.0.0.1:20241 | `/ready` 200 | **7990** (7994 "connection refused", §4) |
| console | 127.0.0.1:8080 | `/health` 404 (2928), `/` 302, `/package.json` **200** (3505): still serves source | 0 |
| galion-desk | 127.0.0.1:8646 | `/health` 200 (11) | 0 |
| galion-health | 127.0.0.1:8660 | `/health` 404 (0.43 s), `/` 200 (1951) | 0 |
| galion-nerve (openmind-nerve) | 127.0.0.1:8670 | **`/health` 200 (149)** (was 404), `/` 200 (13851), `/state` 200 (**43972**, was 13089), **`/events` 200** (156, new) | 38 (20 with error/warn) |
| galion-nerve-shadow | 127.0.0.1:8672 | `/health` 200 (149), `/` 200 (13508), `/state` 200 (49144) | 35 |
| galion-one | 127.0.0.1:8091 + 127.0.0.1:8002 | 8091 `/health` 200 (3); **8002 `/health` 503** (86), body `status loading, model chatterbox` | journal 0; `/var/log/galion-one.log` 1,986,977 B, still written; last lines `warm rust skipped mouth=0 mind=402` |
| hermes-rs | 127.0.0.1:8642 | `/health` 200 (122) | 0 (unit and `-t hermes-gateway`) |
| link-watch-rs | none | n/a | 0 |
| openmind-memory | 127.0.0.1:8420 | `/health` 200 (276) | 0 (unit and `-t openmind-memory`) |
| openmind-queue | **127.0.0.1:19120** | `/healthz` **200** (`function POSEIDON, ok true`), `/stats` 200; `/health` 404, `/` 404 | 0 |
| tanga-proto | 127.0.0.1:8671 | `/tanga/` 200 (41547) | 0 |
| tuner | 127.0.0.1:8650 | `/health` 200 (12), `/` 200 (2356) | 0 |
| tuner-quick | 127.0.0.1:20242 | `/ready` 200 | 0 |

Closed (000): 4020, 8003, and the former strays **8431, 8652, 8653**. Other listeners: sshd *:22, postfix 127.0.0.1:25 / [::1]:25, resolved 53.
Public, GET from the box at 11:24: `galion.app`, `www.galion.app`, `one.galion.app` **200**; `tuner.galion.app` **000** (still does not resolve).
The same GETs sent from the VPS itself return 403 (likely Cloudflare blocking requests from the origin's own IP; the box gets 200, so users are not affected).

## 3. Stray listeners
**None left.** 8431, 8432, 8433, 8652 and 8653 are gone, `ps` shows no `--mock` or `.new` process, and `/root/growth-20261018/bin` no longer exists.
`/var/backups/growth-20261018-archive-20261021.tar.gz` (8,487,748 B, Wed 04:34) is the archive of that folder. The console Vite child (pid 2501884) is unchanged.

## 4. Edge (cloudflared)
- cloudflared was **restarted Wed 08:32:32** with config.yml **sha256 `e3825ce4344906bd5feb6bd8041efc76ae861ca354ad03deea0a145b6263d976`** (3257 B, mtime 08:32:15).
  Backup of the old config: `/var/backups/cloudflared-config-20261021-063215.yml` (3319 B) + `.sha256`.
- Ingress (hostname → origin) for galion: `one.galion.app` → :8671 (tanga path) and :8091; `galion.app`, `www.galion.app` → **127.0.0.1:8091**; `console`/`obsidian` → :8080; `tuner.galion.app` → :8650; catch-all `http_status:404`.
  37 hostname rules in total; the full list is in the real-test record.
- Connection-refused lines in the last hour: **7994** (was 1162). By upstream:

| Upstream | 21 Oct 10:24-11:24 | 20 Oct 21:53-22:53 |
|---|---|---|
| [::1]:4042 (docs.galion.app) | **2544** | 1 |
| [::1]:4444 (downloader.galion.app) | **2526** | 3 |
| [::1]:4060 (slaviknomad.com) | 660 | 6 |
| [::1]:3020 (maivirtue.com) | 558 | 2 |
| [::1]:4004 (tactical-belts) | **548** | 0 |
| [::1]:4001 (studio/galion.studio) | 478 | 237 |
| [::1]:3011 (merlinelement.com) | 478 | 239 |
| [::1]:4020 (ws3d/workspace3d) | 94 | 1 |
| [::1]:3004 (machine) | 50 | 84 |
| [::1]:3005 (torchbearer) | 40 | 0 |
| [::1]:4003 (voice) | 18 | 1 |
| 127.0.0.1:3010 (adriaticpadel) | 4 | 2 |

`localhost` now resolves to `[::1]` in the log lines. The nerve reports the same thing: "tunnel routes 18 public hostnames to 11 closed origin ports … 108.6 failed requests/min".
The rise on 4042 and 4444 (about 42/min each) is new since the 20 Oct baseline; the cause is not determined by this read-only run (see §5 item 9).

## 5. Diff vs the 2026-10-20 22:52 baseline
1. **Drop-ins now exist.** 20 Oct: "DropInPaths is empty on every unit except galion-backup". Now galion-one (1), galion-nerve (2), hermes-rs (2), openmind-memory (2) have drop-ins; all carry a rollback comment and a dated backup in /var/backups.
2. **galion-one** no longer runs from the frozen working copy: `dist/bin-ws/galion-one` `090c15d0…` → `/opt/galion/rust/bin/galion-one` `24cb4976…` (one-no-shellout 40454f9). 8091 still 200, 8002 still 503 "loading".
3. **galion-nerve** runs openmind-nerve aa77864 `f0994b6d…` instead of galion-nerve `3b150dea…`. `/health` changed 404 → 200; `/state` grew 13 KB → 44 KB and now carries `cortex` and `cortex_events`; `/events` is new and holds 1 event (09:43:55 `nerve-cutover`, source galion-deploy).
   Live nerve at 11:23:24: cycle 200, interval 30 s, mode live, **score 61/D**, 3 open incidents (galion-one mouth 503 for 1.7 h of nerve uptime; edge refusals; hermes-rs spend cap hit).
4. **hermes-rs** `cd344b16…` → `hermes-gateway-aa77864` `eb691b29…`; **openmind-memory** `473ae4a3…` → `openmind-memory-aa77864` `18e35ab4…`. Both have NERVE_URL. Same ports, health 200.
5. **New units:** `openmind-queue` (POSEIDON, 127.0.0.1:19120, enabled, from poseidon-refusal-persist eec2d99, NRestarts=1) and `galion-nerve-shadow` (127.0.0.1:8672, **disabled** but active, so it will **not** come back after a reboot).
6. **tuner** binary replaced at 04:32:32: `ecad72a4…` (508,680 B) → `89c7fcbc…` (480,032 B). Three tuner `.bin` backups are in /var/backups.
7. **galion-deploy** replaced at 04:31:47: 2830 B shell script `86207af5…` → 639,664 B Rust binary `2a60e1ac…`.
8. **hermes.service masked** (Wed 08:24:00); `/var/backups/hermes-legacy-20261021/` exists (Wed 08:25).
9. **cloudflared** restarted 08:32:32 on config `e3825ce4…`; refusals 1162/h → 7994/h, now led by 4042 and 4444. tuner.galion.app is still unresolved. Public galion.app/www/one return 200.
10. **Strays gone** (8431, 8432, 8433, 8652, 8653), growth folder archived.
11. **Load** 4.02 → 8.88 (1-min), which is the 8-core limit; RAM and disk unchanged. The cause is not visible from the unit ledger (every unit has a small RSS); it was not investigated in this read-only row.
12. **Unchanged:** working copy 2f5a730, cloudflared/console/galion-desk/galion-health/link-watch-rs/tanga-proto/tuner-quick bytes and PIDs, console still Vite dev with `/package.json` 200, transient tuner-quick, no survival drop-ins (`deploy/survival/*` still not installed), backup offsite.conf.

## 6. Follow-ups for the owner (no change was made)
- **galion-nerve-shadow is disabled but running.** After a reboot it stays down. Decide: enable it, or stop it once the live nerve (now the same code line) has proven itself. (D-B2-1)
- **The 60-*.conf comments** ("inert until a nerve-client build is deployed") are stale; refresh the comments at the next planned change. (D-B2-2, cosmetic)
- **Edge 4042/4444 refusals at ~42/min each** (docs.galion.app, downloader.galion.app). Either remove those ingress rules or start the origins; until then they dominate the nerve's edge incident. A config change needs Slavik's approval and is outside this read-only row. (D-B2-3)
- **Load average ~8.9 on 8 cores**: worth a dedicated read-only look (top/pidstat on the host) if it persists. (D-B2-4)
- The `deploy/*.service` files in the repo still do not describe the live drop-in layout (50-rust-bin / 60-nerve-*). (D-B2-5: check them into deploy/ as the source of truth.)

## 7. Re-running this baseline
Same as the 20 Oct method (§6 of vps-baseline-20261020.md), plus: `systemctl show -p DropInPaths` and `cat` every `/etc/systemd/system/*.service.d/*.conf` with `Environment=` values replaced by names;
probe 8672, 19120 `/healthz` and `/stats`, 8670 `/events`; group cloudflared refusals with `grep -oE 'dial tcp (\[::1\]|127\.0\.0\.1|localhost):[0-9]+'` (the 20 Oct pattern no longer matches the `[::1]` form).
Compare against §2. Any sha, mtime, PID, port or code change is drift.
