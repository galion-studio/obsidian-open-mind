<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-test record: vps-live-table (slot 232), 2026-10-22 CEST (+02:00)

All VPS access was READ-ONLY (systemctl show, readlink, stat, sha256sum, grep -a, ss, curl GET with body discarded). No file was written on the VPS. The VPS address is masked (`<vps-ip>`) in every output below; ssh stderr is not recorded.

## Runs
| When (+02:00) | What | Result |
|---|---|---|
| 00:09:22–00:09:24 | probe batch 1 (units, ExecStart path, file+exe sha, drop-ins, ss, 19 GETs, nerve/cortex flags) | raw output §A |
| 00:09:51–00:09:52 | probe batch 2 (build stamps via `grep -a -o`, PID/NRestarts/exe-sha re-check, path unit, timers) | raw output §B; 8/8 re-checked units unchanged |
| 00:10:44 | batch 3: directive names of the 5 new drop-ins + 2 handoff units (`Environment=` values, ExecStart args, EnvironmentFile paths withheld on the VPS) | summarised in docs/ops/live-binary-table.md §1 |
| 00:12:45–00:12:48 | `GALION_REAL=1 cargo test -p galion-realtest --test real_live_table -- --nocapture` | 3/3 pass, **checked 15 rows, drift 0** |
| 00:12:57 | negative: same test on a tampered copy (nerve sha set to the old aa77864 sha, cortex size +1) via `GALION_LIVE_TABLE_DOC` | **FAILED as expected**: `drift: ["galion-nerve", "gaia-cortex"]` |
| ~00:13:10 | negative offline: copy with a TEST-NET IPv4 and a `…Z` stamp appended | **FAILED as expected**: `public IPv4 in doc` |
| 00:13:31 | live test again after fmt/clippy edits | 3/3 pass, checked 15 rows, drift 0 |

Live test output (00:12:45):
```
galion-nerve         state=active   pid=938448   exe=c15be071f8263175 file=c15be071f8263175 size=1415176 OK
gaia-cortex          state=active   pid=969619   exe=1b01c3fce5e94c0d file=1b01c3fce5e94c0d size=1041248 OK
hermes-rs            state=active   pid=718450   exe=eb691b2967eb0375 file=eb691b2967eb0375 size=1841904 OK
openmind-memory      state=active   pid=88565    exe=18e35ab49a76702b file=18e35ab49a76702b size=1702640 OK
galion-one           state=active   pid=715767   exe=24cb4976b1325ae0 file=24cb4976b1325ae0 size=963904 OK
openmind-queue       state=active   pid=17979    exe=8a7f7faaaa864e0a file=8a7f7faaaa864e0a size=889632 OK
tuner                state=active   pid=4004553  exe=89c7fcbcb54c5108 file=89c7fcbcb54c5108 size=480032 OK
galion-nerve-shadow  state=active   pid=4008991  exe=3a566c56dfea79cf file=3a566c56dfea79cf size=1438496 OK
galion-health        state=active   pid=1374810  exe=6afe668c33630310 file=6afe668c33630310 size=503512 OK
link-watch-rs        state=active   pid=1359406  exe=2f5874f72243d24f file=2f5874f72243d24f size=508248 OK
galion-desk          state=active   pid=231      exe=555cc44314aa3d6a file=555cc44314aa3d6a size=4735088 OK
tanga-proto          state=active   pid=1467846  exe=9c540eeb054dd120 file=9c540eeb054dd120 size=447264 OK
cloudflared          state=active   pid=611846   exe=03f1f25d1cc93b9a file=03f1f25d1cc93b9a size=39838488 OK
tuner-quick          state=active   pid=1198838  exe=03f1f25d1cc93b9a file=03f1f25d1cc93b9a size=39838488 OK
console              state=active   pid=2501864  exe=1abce2374a485bdd file=1abce2374a485bdd size=120177224 OK
checked 15 rows, drift 0
```

Gates: `cargo fmt --check -p galion-realtest` ok; `cargo clippy -p galion-realtest --all-targets -- -D warnings` ok; `cargo test -p galion-realtest` 15/0; `scripts/no-python.sh` ok;
repo-check 10 errors / 48 warnings = the known integration-5 blocker 0 (vendored candle-core `core::arch`, fixed on int5-candle-repocheck 58a89bc), none from this branch.

## §A raw batch 1 (masked)
```
PROBE_START 2026-10-22 00:09:22 +0200
WC_HEAD 2f5a730
LOAD 4.50 5.51 5.47 MEMAVAIL_MB 14678
UNIT cloudflared MainPID=611846|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 17:08:54 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/cloudflared.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/local/bin/cloudflared
  FILE 34 2026-10-08 11:00:20.933728918 +0200 777 SHA 03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc
  EXE /opt/galion/tooling/bin/cloudflared SHA 03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc
UNIT console MainPID=2501864|NRestarts=0|ExecMainStartTimestamp=Thu 2026-10-15 08:02:09 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/console.service|DropInPaths=|UnitFileState=enabled| EXEC=/opt/galion/tooling/node/bin/node
  FILE 120177224 2025-02-11 01:08:18.000000000 +0100 775 SHA 1abce2374a485bddae3c27b17a3e3143e2780232026e627c4fe74ddde3f380a1
  EXE /opt/galion/tooling/node/bin/node SHA 1abce2374a485bddae3c27b17a3e3143e2780232026e627c4fe74ddde3f380a1
UNIT galion-desk MainPID=231|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/galion-desk.service|DropInPaths=|UnitFileState=enabled| EXEC=/opt/galion/bin/galion-desk
  FILE 4735088 2026-10-10 11:02:43.800637531 +0200 755 SHA 555cc44314aa3d6a2ffe2ca4273cee20c566fb1296ac43c0df7e1ca0a9fd11e6
  EXE /opt/galion/bin/galion-desk SHA 555cc44314aa3d6a2ffe2ca4273cee20c566fb1296ac43c0df7e1ca0a9fd11e6
UNIT galion-health MainPID=1374810|NRestarts=0|ExecMainStartTimestamp=Sun 2026-10-18 22:14:38 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/galion-health.service|DropInPaths=|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/galion-health
  FILE 503512 2026-10-18 22:14:38.854368487 +0200 755 SHA 6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a
  EXE /opt/galion/rust/bin/galion-health SHA 6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a
UNIT galion-nerve MainPID=938448|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 22:56:55 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/galion-nerve.service|DropInPaths=/etc/systemd/system/galion-nerve.service.d/50-rust-bin.conf /etc/systemd/system/galion-nerve.service.d/60-nerve-eventlog.conf /etc/systemd/system/galion-nerve.service.d/70-nerve-dryrun.conf /etc/systemd/system/galion-nerve.service.d/75-no-openrouter.conf /etc/systemd/system/galion-nerve.service.d/80-event-mode.conf /etc/systemd/system/galion-nerve.service.d/85-live-actions.conf|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/openmind-nerve-bb287cc
  FILE 1415176 2026-10-21 19:13:51.254620974 +0200 755 SHA c15be071f8263175949465d9848c7f0d6e2643cb6362ef2ae613c0346eae5673
  EXE /opt/galion/rust/bin/openmind-nerve-bb287cc SHA c15be071f8263175949465d9848c7f0d6e2643cb6362ef2ae613c0346eae5673
  DROPIN 50-rust-bin.conf 2026-10-21 09:43:54 98d5cec1f2a305de
  DROPIN 60-nerve-eventlog.conf 2026-10-21 09:31:06 c663ab5cbe25c3fb
  DROPIN 70-nerve-dryrun.conf 2026-10-21 17:08:27 3fe865c9fa1f1bc3
  DROPIN 75-no-openrouter.conf 2026-10-21 18:18:23 354e5b44dba06086
  DROPIN 80-event-mode.conf 2026-10-21 19:16:56 053b29920ac10b67
  DROPIN 85-live-actions.conf 2026-10-21 22:56:55 9bf55307c5149632
UNIT galion-nerve-shadow MainPID=4008991|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 04:37:18 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/galion-nerve-shadow.service|DropInPaths=|UnitFileState=disabled| EXEC=/opt/galion/rust/bin/galion-nerve-shadow
  FILE 1438496 2026-10-21 04:37:18.445308595 +0200 755 SHA 3a566c56dfea79cf8cb28bc413c2f7e3f4004969380c9be1ce7dca61382256f1
  EXE /opt/galion/rust/bin/galion-nerve-shadow SHA 3a566c56dfea79cf8cb28bc413c2f7e3f4004969380c9be1ce7dca61382256f1
UNIT galion-one MainPID=715767|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 18:10:21 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/galion-one.service|DropInPaths=/etc/systemd/system/galion-one.service.d/50-rust-bin.conf /etc/systemd/system/galion-one.service.d/60-no-warm.conf|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/galion-one
  FILE 963904 2026-10-21 08:39:10.527526224 +0200 755 SHA 24cb4976b1325ae0f5a100a7165a1e2e0bbd29f3f2c0e584dbd3a1cdd03b7ca3
  EXE /opt/galion/rust/bin/galion-one SHA 24cb4976b1325ae0f5a100a7165a1e2e0bbd29f3f2c0e584dbd3a1cdd03b7ca3
  DROPIN 50-rust-bin.conf 2026-10-21 08:39:24 467d894b661cfe8e
  DROPIN 60-no-warm.conf 2026-10-21 18:10:21 5c52f92160e21bd1
UNIT hermes-rs MainPID=718450|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 18:12:42 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/hermes-rs.service|DropInPaths=/etc/systemd/system/hermes-rs.service.d/50-rust-bin.conf /etc/systemd/system/hermes-rs.service.d/60-nerve-url.conf /etc/systemd/system/hermes-rs.service.d/70-research-budget.conf|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/hermes-gateway-aa77864
  FILE 1841904 2026-10-21 09:41:31.464884383 +0200 755 SHA eb691b2967eb03758dcc6b2b6f46fe0a9d45d779b4d4bf937a28f37b19633e2f
  EXE /opt/galion/rust/bin/hermes-gateway-aa77864 SHA eb691b2967eb03758dcc6b2b6f46fe0a9d45d779b4d4bf937a28f37b19633e2f
  DROPIN 50-rust-bin.conf 2026-10-21 09:46:21 910cddb2eb9fc187
  DROPIN 60-nerve-url.conf 2026-10-21 09:32:33 833acdfbb2a68367
  DROPIN 70-research-budget.conf 2026-10-21 18:12:30 6c8731018b9b9e1f
UNIT link-watch-rs MainPID=1359406|NRestarts=0|ExecMainStartTimestamp=Sun 2026-10-18 21:57:48 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/link-watch-rs.service|DropInPaths=|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/link-watch
  FILE 508248 2026-10-18 21:57:47.934376698 +0200 755 SHA 2f5874f72243d24f52eb767f660d57ab14959c205679547b51366fa865f61116
  EXE /opt/galion/rust/bin/link-watch SHA 2f5874f72243d24f52eb767f660d57ab14959c205679547b51366fa865f61116
UNIT openmind-memory MainPID=88565|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 09:45:26 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/openmind-memory.service|DropInPaths=/etc/systemd/system/openmind-memory.service.d/50-rust-bin.conf /etc/systemd/system/openmind-memory.service.d/60-nerve-url.conf|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/openmind-memory-aa77864
  FILE 1702640 2026-10-21 09:41:26.175899772 +0200 755 SHA 18e35ab49a76702b47742b8c5d4f95f29c3956839979e7df7a6ede671f1d2bb6
  EXE /opt/galion/rust/bin/openmind-memory-aa77864 SHA 18e35ab49a76702b47742b8c5d4f95f29c3956839979e7df7a6ede671f1d2bb6
  DROPIN 50-rust-bin.conf 2026-10-21 09:45:26 8ea8565d02589918
  DROPIN 60-nerve-url.conf 2026-10-21 09:32:33 fdd150c7377f0f49
UNIT openmind-queue MainPID=17979|NRestarts=1|ExecMainStartTimestamp=Wed 2026-10-21 08:27:54 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/openmind-queue.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/local/bin/openmind-queue
  FILE 889632 2026-10-21 08:27:27.235532621 +0200 755 SHA 8a7f7faaaa864e0a6e5f628f723081a48e2179a21130062f8569aa1685f8ec8d
  EXE /usr/local/bin/openmind-queue SHA 8a7f7faaaa864e0a6e5f628f723081a48e2179a21130062f8569aa1685f8ec8d
UNIT tanga-proto MainPID=1467846|NRestarts=3|ExecMainStartTimestamp=Sun 2026-10-18 22:35:49 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/tanga-proto.service|DropInPaths=|UnitFileState=enabled| EXEC=/opt/galion/tanga-proto/bin/tanga-serve
  FILE 447264 2026-10-18 22:35:48.769911286 +0200 755 SHA 9c540eeb054dd1205666f116167b9ad8ba7edf08f900ce4228a79d210a1de7f7
  EXE /opt/galion/tanga-proto/bin/tanga-serve SHA 9c540eeb054dd1205666f116167b9ad8ba7edf08f900ce4228a79d210a1de7f7
UNIT tuner MainPID=4004553|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 04:32:32 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/tuner.service|DropInPaths=|UnitFileState=enabled| EXEC=/opt/galion/tuner/bin/tuner
  FILE 480032 2026-10-21 04:32:32.689247807 +0200 755 SHA 89c7fcbcb54c510887dafd8c67b255f05d00595af598fe65f022e204c0ef2b12
  EXE /opt/galion/tuner/bin/tuner SHA 89c7fcbcb54c510887dafd8c67b255f05d00595af598fe65f022e204c0ef2b12
UNIT tuner-quick MainPID=1198838|NRestarts=0|ExecMainStartTimestamp=Sun 2026-10-18 19:19:30 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/run/systemd/transient/tuner-quick.service|DropInPaths=|UnitFileState=transient| EXEC=/usr/local/bin/cloudflared
  FILE 34 2026-10-08 11:00:20.933728918 +0200 777 SHA 03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc
  EXE /opt/galion/tooling/bin/cloudflared SHA 03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc
UNIT gaia-cortex MainPID=969619|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 23:36:53 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/etc/systemd/system/gaia-cortex.service|DropInPaths=/etc/systemd/system/gaia-cortex.service.d/50-safety.conf /etc/systemd/system/gaia-cortex.service.d/85-live-actions.conf /etc/systemd/system/gaia-cortex.service.d/86-hermes-boss.conf /etc/systemd/system/gaia-cortex.service.d/87-live-fixes.conf|UnitFileState=enabled| EXEC=/opt/galion/rust/bin/gaia-cortex-bd6fbe6
  FILE 1041248 2026-10-21 23:36:36.041817350 +0200 755 SHA 1b01c3fce5e94c0d74049abaf13b8a325acf05342a2e25a17e9e49f17cce5cce
  EXE /opt/galion/rust/bin/gaia-cortex-bd6fbe6 SHA 1b01c3fce5e94c0d74049abaf13b8a325acf05342a2e25a17e9e49f17cce5cce
  DROPIN 50-safety.conf 2026-10-21 19:06:15 1d89f47bffba3faa
  DROPIN 85-live-actions.conf 2026-10-21 22:56:55 e48661cb45c57b95
  DROPIN 86-hermes-boss.conf 2026-10-21 23:08:11 f398922cf3f68f83
  DROPIN 87-live-fixes.conf 2026-10-21 23:36:42 76090e8b80acd5a3
UNIT gaia-hermes-handoff MainPID=0|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 23:08:38 CEST|LoadState=loaded|ActiveState=inactive|SubState=dead|FragmentPath=/etc/systemd/system/gaia-hermes-handoff.service|DropInPaths=|UnitFileState=static| EXEC=/opt/galion/rust/bin/gaia-cortex-c5cae88
  FILE 1037088 2026-10-21 23:08:11.016770007 +0200 755 SHA dec978c3f715a4390ddd12cfcf75794bdefd49049c227da296f60bee3fe405a7
UNIT galion-backup MainPID=0|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 20:17:37 CEST|LoadState=loaded|ActiveState=inactive|SubState=dead|FragmentPath=/etc/systemd/system/galion-backup.service|DropInPaths=/etc/systemd/system/galion-backup.service.d/offsite.conf|UnitFileState=static| EXEC=/opt/galion/rust/bin/galion-backup.sh
  FILE 3026 2026-10-18 22:13:33.001563357 +0200 755 SHA 0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4
  DROPIN offsite.conf 2026-10-18 22:22:07 e370c03716b738ee
UNIT console-getty MainPID=243|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/console-getty.service|DropInPaths=|UnitFileState=enabled-runtime| EXEC=/sbin/agetty
  FILE 60992 2024-04-09 16:02:37.000000000 +0200 755 SHA 96593eba53a29514897b1029794672ea41ac372fd1566c5c90859d5e8f2ddda4
  EXE /usr/sbin/agetty SHA 96593eba53a29514897b1029794672ea41ac372fd1566c5c90859d5e8f2ddda4
UNIT container-getty@1 MainPID=245|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/container-getty@.service|DropInPaths=|UnitFileState=enabled| EXEC=/sbin/agetty
  FILE 60992 2024-04-09 16:02:37.000000000 +0200 755 SHA 96593eba53a29514897b1029794672ea41ac372fd1566c5c90859d5e8f2ddda4
  EXE /usr/sbin/agetty SHA 96593eba53a29514897b1029794672ea41ac372fd1566c5c90859d5e8f2ddda4
UNIT container-getty@2 MainPID=246|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/container-getty@.service|DropInPaths=|UnitFileState=enabled| EXEC=/sbin/agetty
  FILE 60992 2024-04-09 16:02:37.000000000 +0200 755 SHA 96593eba53a29514897b1029794672ea41ac372fd1566c5c90859d5e8f2ddda4
  EXE /usr/sbin/agetty SHA 96593eba53a29514897b1029794672ea41ac372fd1566c5c90859d5e8f2ddda4
UNIT cron MainPID=227|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/cron.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/sbin/cron
  FILE 60080 2024-03-31 01:06:27.000000000 +0100 755 SHA 6bd8593640af2413bce259fa0affc18dbf149892756ebe805bf316624f8b590f
  EXE /usr/sbin/cron SHA 6bd8593640af2413bce259fa0affc18dbf149892756ebe805bf316624f8b590f
UNIT dbus MainPID=228|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/dbus.service|DropInPaths=|UnitFileState=static| EXEC=/usr/bin/dbus-daemon
  FILE 236176 2024-08-09 04:33:22.000000000 +0200 755 SHA 8c479f1fcddfd6693c736ce541da0955f6752834bdeeef4b928e27f5e974c247
  EXE /usr/bin/dbus-daemon SHA 8c479f1fcddfd6693c736ce541da0955f6752834bdeeef4b928e27f5e974c247
UNIT fail2ban MainPID=1268562|NRestarts=0|ExecMainStartTimestamp=Sun 2026-10-18 20:10:04 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/fail2ban.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/bin/fail2ban-server
  FILE 1417 2024-06-10 23:27:45.000000000 +0200 755 SHA 22f0ec1a16f006a8d921e563d4576dd15cbefe1d7f05bae2867ff3c8ae51d1b1
  EXE /usr/bin/python3.12 SHA 359ca926ba4596147d7d558c1e0cbf6a5784f5475d7e8bf0a55d9d7c4fa7106c
UNIT networkd-dispatcher MainPID=232|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/networkd-dispatcher.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/bin/networkd-dispatcher
  FILE 20362 2023-04-24 17:32:27.000000000 +0200 755 SHA aad2823762b60d03df6762ef18eb747a30d20ff7882116f96f4adf745800d697
  EXE /usr/bin/python3.12 SHA 359ca926ba4596147d7d558c1e0cbf6a5784f5475d7e8bf0a55d9d7c4fa7106c
UNIT postfix@- MainPID=0|NRestarts=0|ExecMainStartTimestamp=|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/postfix@.service|DropInPaths=|UnitFileState=enabled-runtime| EXEC=/usr/sbin/postmulti
  FILE 31424 2024-04-08 16:48:02.000000000 +0200 755 SHA 90e5d758ebd00e32829ecfda65fc9dd7138d05834e4f609f053295092804eea7
UNIT rsyslog MainPID=257|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/rsyslog.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/sbin/rsyslogd
  FILE 790192 2024-04-08 16:49:24.000000000 +0200 755 SHA f18c5c55dceff1dead60fc0d81fbd833f86667eea35b69337556497899a8f9bc
  EXE /usr/sbin/rsyslogd SHA f18c5c55dceff1dead60fc0d81fbd833f86667eea35b69337556497899a8f9bc
UNIT ssh MainPID=248|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:36 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/ssh.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/sbin/sshd
  FILE 921416 2024-04-05 21:30:31.000000000 +0200 755 SHA 4e73d1ca689e66a85ff1f6d78f204d0433f6bf24b44a377e00935d22876ebcac
  EXE /usr/sbin/sshd SHA 4e73d1ca689e66a85ff1f6d78f204d0433f6bf24b44a377e00935d22876ebcac
UNIT systemd-journald MainPID=1373084|NRestarts=0|ExecMainStartTimestamp=Sun 2026-10-18 22:13:33 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/systemd-journald.service|DropInPaths=|UnitFileState=static| EXEC=/usr/lib/systemd/systemd-journald
  FILE 193664 2024-04-19 16:24:36.000000000 +0200 755 SHA f271c2d87cd0cde6442c964da9d386cb2b33fa79488d9e477144068729964fb6
  EXE /usr/lib/systemd/systemd-journald SHA f271c2d87cd0cde6442c964da9d386cb2b33fa79488d9e477144068729964fb6
UNIT systemd-logind MainPID=226|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:35 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/systemd-logind.service|DropInPaths=/usr/lib/systemd/system/systemd-logind.service.d/dbus.conf /usr/lib/systemd/system/systemd-logind.service.d/nice.conf|UnitFileState=static| EXEC=/usr/lib/systemd/systemd-logind
  FILE 285264 2024-04-19 16:24:36.000000000 +0200 755 SHA 1c9b4c4aa67592bbdb86a74d76747553f962734ee3dfbd400a4efa1307c1fb39
  EXE /usr/lib/systemd/systemd-logind SHA 1c9b4c4aa67592bbdb86a74d76747553f962734ee3dfbd400a4efa1307c1fb39
UNIT systemd-networkd MainPID=106|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:34 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/systemd-networkd.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/lib/systemd/systemd-networkd
  FILE 1657576 2024-04-19 16:24:36.000000000 +0200 755 SHA c96a8cc55b0292b5247dafae400e99294053f45d46c5662e996dd28d0fa93ebf
  EXE /usr/lib/systemd/systemd-networkd SHA c96a8cc55b0292b5247dafae400e99294053f45d46c5662e996dd28d0fa93ebf
UNIT systemd-resolved MainPID=222|NRestarts=0|ExecMainStartTimestamp=Mon 2026-10-12 07:01:35 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/systemd-resolved.service|DropInPaths=|UnitFileState=enabled| EXEC=/usr/lib/systemd/systemd-resolved
  FILE 535048 2024-04-19 16:24:36.000000000 +0200 755 SHA bc23030c1dc41a6f15bb4d9a4de39bc99c0fc37e0bfa66d8c54290bd657bdaeb
  EXE /usr/lib/systemd/systemd-resolved SHA bc23030c1dc41a6f15bb4d9a4de39bc99c0fc37e0bfa66d8c54290bd657bdaeb
UNIT user@0 MainPID=680694|NRestarts=0|ExecMainStartTimestamp=Wed 2026-10-21 17:26:20 CEST|LoadState=loaded|ActiveState=active|SubState=running|FragmentPath=/usr/lib/systemd/system/user@.service|DropInPaths=/usr/lib/systemd/system/user@0.service.d/10-login-barrier.conf /usr/lib/systemd/system/user@.service.d/timeout.conf|UnitFileState=static| EXEC=/usr/lib/systemd/systemd
  FILE 100816 2024-04-19 16:24:36.000000000 +0200 755 SHA 58d70c49e2f801eb405b98a35a6268a96065665f2e11fd673c7262e44d601124
  EXE /usr/lib/systemd/systemd SHA 58d70c49e2f801eb405b98a35a6268a96065665f2e11fd673c7262e44d601124
LISTEN
*:22 users:(("sshd",),("systemd",))
127.0.0.1:18800 users:(("sshd",))
127.0.0.1:19120 users:(("openmind-queue",))
127.0.0.1:20241 users:(("cloudflared",))
127.0.0.1:20242 users:(("cloudflared",))
127.0.0.1:25 users:(("master",))
127.0.0.1:8002 users:(("galion-one",))
127.0.0.1:8080 users:(("node",))
127.0.0.1:8091 users:(("galion-one",))
127.0.0.1:8420 users:(("openmind-memory",))
127.0.0.1:8642 users:(("hermes-gateway-",))
127.0.0.1:8646 users:(("galion-desk",))
127.0.0.1:8650 users:(("tuner",))
127.0.0.1:8660 users:(("galion-health",))
127.0.0.1:8670 users:(("openmind-nerve-",))
127.0.0.1:8671 users:(("tanga-serve",))
127.0.0.1:8672 users:(("galion-nerve-sh",))
127.0.0.1:8674 users:(("gaia-cortex-bd6",))
127.0.0.53%lo:53 users:(("systemd-resolve",))
127.0.0.54:53 users:(("systemd-resolve",))
[::1]:25 users:(("master",))
HTTP
GET 127.0.0.1:20241/ready 200 88
GET 127.0.0.1:8080/health 404 2928
GET 127.0.0.1:8646/health 200 11
GET 127.0.0.1:8660/ 200 1950
GET 127.0.0.1:8670/health 200 149
GET 127.0.0.1:8670/state 200 38082
GET 127.0.0.1:8670/events 200 29538
GET 127.0.0.1:8672/health 200 150
GET 127.0.0.1:8091/health 200 3
GET 127.0.0.1:8002/health 503 86
GET 127.0.0.1:8642/health 200 122
GET 127.0.0.1:8420/health 200 277
GET 127.0.0.1:19120/healthz 200 117
GET 127.0.0.1:8671/tanga/ 200 41547
GET 127.0.0.1:8650/health 200 12
GET 127.0.0.1:20242/ready 200 88
GET 127.0.0.1:8674/health 200 234
GET 127.0.0.1:8674/incidents 200 634
GET 127.0.0.1:18800/ 200 3805
NERVE "cycle":10 "mode":"live" "grade":"D" "score":61 "grade":"D" "score":61 
CORTEX "busy":false "events_in":5 "hermes_handoff":true "llm_enabled":true "mode":"live" "version":"0.1.0" 
RUSTBIN
  gaia-cortex-bd6fbe6 1041248 2026-10-21 23:36:36.04 1b01c3fce5e94c0d
  gaia-cortex-c5cae88 1037088 2026-10-21 23:08:11.01 dec978c3f715a439
  gaia-cortex-c7f56b8 942880 2026-10-21 19:06:15.408 fbe83e136789b37b
  gaia-cortex-e57b946 1037088 2026-10-21 23:05:52.81 d5b7f0b97916cd9c
  galion-backup 1599472 2026-10-18 22:13:33.00 3df3f88d9087c34e
  galion-backup.sh 3026 2026-10-18 22:13:33.00156 0897efc0c8314c02
  galion-deploy 639664 2026-10-21 04:31:47.455 2a60e1ac632e27d9
  galion-health 503512 2026-10-18 22:14:38.854 6afe668c33630310
  galion-nerve 816456 2026-10-18 22:35:03.938 3b150dea692ffac0
  galion-nerve-shadow 1438496 2026-10-21 04:37:18.44 3a566c56dfea79cf
  galion-one 963904 2026-10-21 08:39:10.527 24cb4976b1325ae0
  galion-restore-test.sh 840 2026-10-18 22:13:33.001563 264397750a310832
  hermes-gateway 2173696 2026-10-18 22:00:49.53 cd344b167dc65425
  hermes-gateway-aa77864 1841904 2026-10-21 09:41:31.46 eb691b2967eb0375
  link-watch 508248 2026-10-18 21:57:47.934 2f5874f72243d24f
  openmind-memory 1980096 2026-10-18 21:41:22.24 473ae4a3b13ceabb
  openmind-memory-aa77864 1702640 2026-10-21 09:41:26.17 18e35ab49a76702b
  openmind-nerve-aa77864 1213216 2026-10-21 09:41:20.95 f0994b6db1fd8ed8
  openmind-nerve-bb287cc 1415176 2026-10-21 19:13:51.25 c15be071f8263175
GDEPLOY 639664 2026-10-21 04:31:47.455396572 +0200 2a60e1ac632e27d9d918102d5fb02e1bafe35d4a48ead734f403f650de07464d
TIMER Thu 2026-10-22 02:20:59 CEST 2h 11min Wed 2026-10-21 20:17:37 CEST 3h 51min ago galion-backup.timer galion-backup.service
PROBE_END 2026-10-22 00:09:24 +0200
```

## §B raw batch 2 (masked)
```
PROBE2_START 2026-10-22 00:09:51 +0200
STAMP openmind-nerve-bb287cc want=bb287cc found=[]
STAMP gaia-cortex-bd6fbe6 want=bd6fbe6 found=[]
STAMP gaia-cortex-bd6fbe6 want=c5cae88 found=[]
STAMP gaia-cortex-bd6fbe6 want=1c5dec4 found=[]
STAMP gaia-cortex-bd6fbe6 want=e982210 found=[]
STAMP hermes-gateway-aa77864 want=aa77864 found=[]
STAMP openmind-memory-aa77864 want=aa77864 found=[aa778642026 ]
STAMP galion-one want=40454f9 found=[]
STAMP tuner want=42de672 found=[]
STAMP galion-deploy want=42de672 found=[]
STAMP openmind-queue want=eec2d99 found=[]
STAMP galion-nerve-shadow want=42de672 found=[]
BUILT openmind-nerve-bb287cc unknown_count=11 builtstr=[]
BUILT gaia-cortex-bd6fbe6 unknown_count=8 builtstr=[]
BUILT hermes-gateway-aa77864 unknown_count=8 builtstr=[]
BUILT openmind-memory-aa77864 unknown_count=8 builtstr=[2026-10-21T09:39:35+02:00 ]
BUILT galion-one unknown_count=5 builtstr=[]
BUILT tuner unknown_count=8 builtstr=[]
BUILT galion-deploy unknown_count=9 builtstr=[]
BUILT openmind-queue unknown_count=12 builtstr=[]
BUILT galion-nerve-shadow unknown_count=9 builtstr=[2026-10-20T21:24:19+02:00 ]
BUILT galion-health unknown_count=7 builtstr=[]
BUILT link-watch unknown_count=6 builtstr=[]
RECHECK galion-nerve MainPID=938448 NRestarts=0 ActiveState=active  c15be071f8263175
RECHECK gaia-cortex MainPID=969619 NRestarts=0 ActiveState=active  1b01c3fce5e94c0d
RECHECK hermes-rs MainPID=718450 NRestarts=0 ActiveState=active  eb691b2967eb0375
RECHECK openmind-memory MainPID=88565 NRestarts=0 ActiveState=active  18e35ab49a76702b
RECHECK galion-one MainPID=715767 NRestarts=0 ActiveState=active  24cb4976b1325ae0
RECHECK tuner MainPID=4004553 NRestarts=0 ActiveState=active  89c7fcbcb54c5108
RECHECK openmind-queue MainPID=17979 NRestarts=1 ActiveState=active  8a7f7faaaa864e0a
RECHECK cloudflared MainPID=611846 NRestarts=0 ActiveState=active  03f1f25d1cc93b9a
PATHUNIT ActiveState=active UnitFileState=enabled 
TIMERS apt-daily-upgrade.service apt-daily.service dpkg-db-backup.service e2scrub_all.service fstrim.service galion-backup.service logrotate.service man-db.service motd-news.service sysstat-collect.service sysstat-summary.service systemd-sysupdate-reboot.service systemd-sysupdate.service systemd-tmpfiles-clean.service ua-timer.service 
NERVE "cycle":10 "mode":"live" "grade":"D" "score":61 
CORTEX "busy":false "events_in":5 "hermes_handoff":true "llm_enabled":true "mode":"live" 
PROBE2_END 2026-10-22 00:09:52 +0200
```
