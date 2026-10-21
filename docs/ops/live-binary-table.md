<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# LIVE binary table — VPS (2026-10-22 00:09 CEST +02:00)

- Plan task: Ops **S7.3** (docs: LIVE binary table), wave-4 slot 232 `vps-live-table`. Branch `swarm/vps-live-table` off integration-5 `0d68f46`.
- Why now: tonight's cortex deploy (`gaia-cortex-bd6fbe6`, 23:36:53, `87-live-fixes.conf`) and the nerve/cortex live-action switch (`85-live-actions.conf`, 22:56:55).
- Probe windows (remote `TZ=Europe/Zagreb`): batch 1 **2026-10-22 00:09:22–00:09:24**, batch 2 (build stamps + PID/sha re-check) **00:09:51–00:09:52**, batch 3 (new drop-in directive names) **00:10:44**, all CEST (+02:00).
- Method, **read-only only**: one `ssh … 'nice -n 10 bash -s'` batch each with `systemctl show -p <fixed props>`, `systemctl show -p ExecStart --value` reduced on the VPS to `path=` (arguments never leave the host),
  `readlink`/`sha256sum /proc/<pid>/exe`, `stat`, `sha256sum` of the file, `grep -a -o` for build stamps, `ss -ltnpH`, and one `curl -s -m 5 -o /dev/null -w '%{http_code} %{size_download}'` GET per URL (bodies discarded; only the nerve score/mode and cortex mode flags were extracted).
  Nothing was started, stopped, restarted, written or POSTed; no file was created on the VPS. Binaries were **not executed** (`--version` not run), so the build commit comes from the embedded stamp or, failing that, the file name / deploy record.
- Secrets: no secrets directory or EnvironmentFile was opened; for drop-ins only directive names and `Environment=` variable **names** left the VPS. The VPS address is masked in every saved output and is not in this file.
- Working copy /opt/galion/openmind: HEAD **2f5a730**, unchanged. Load 1/5/15 **4.50 / 5.51 / 5.47** (was 8.88 at 11:23), MemAvailable 14,678 MB.
- Reference: the previous ledger is `docs/ops/vps-baseline.md` on `swarm/vps-baseline-2` **f761f49** (2026-10-21 11:23 CEST); it is not on integration-5, so §3 diffs against that commit. Change records R1–R22: `docs/ops/rollback-table.md` on `swarm/vps-rollback-table` 7a1990e.

## 1. LIVE binary table (Gaia / galion units)
File sha256 = `/proc/<MainPID>/exe` sha256 for every running row (re-checked in batch 2: same PID, same sha, same NRestarts). The test `crates/galion-realtest/tests/real_live_table.rs` parses the rows between the markers.

<!-- live-table:begin -->
| Unit | State | ExecStart binary | Size B | mtime (+02:00) | sha256 | Build commit | MainPID, started (+02:00) | Health GET → code (bytes) |
|---|---|---|---|---|---|---|---|---|
| galion-nerve | enabled/active | /opt/galion/rust/bin/openmind-nerve-bb287cc | 1415176 | 2026-10-21 19:13:51 | `c15be071f8263175949465d9848c7f0d6e2643cb6362ef2ae613c0346eae5673` | bb287cc (name; no stamp) | 938448, 2026-10-21 22:56:55 | :8670 `/health` 200 (149), `/state` 200 (38082), `/events` 200 (29538) |
| gaia-cortex | enabled/active | /opt/galion/rust/bin/gaia-cortex-bd6fbe6 | 1041248 | 2026-10-21 23:36:36 | `1b01c3fce5e94c0d74049abaf13b8a325acf05342a2e25a17e9e49f17cce5cce` | bd6fbe6 (name; no stamp) | 969619, 2026-10-21 23:36:53 | :8674 `/health` 200 (234), `/incidents` 200 (634) |
| hermes-rs | enabled/active | /opt/galion/rust/bin/hermes-gateway-aa77864 | 1841904 | 2026-10-21 09:41:31 | `eb691b2967eb03758dcc6b2b6f46fe0a9d45d779b4d4bf937a28f37b19633e2f` | aa77864 (name; no stamp) | 718450, 2026-10-21 18:12:42 | :8642 `/health` 200 (122) |
| openmind-memory | enabled/active | /opt/galion/rust/bin/openmind-memory-aa77864 | 1702640 | 2026-10-21 09:41:26 | `18e35ab49a76702b47742b8c5d4f95f29c3956839979e7df7a6ede671f1d2bb6` | aa77864 (stamped, built 2026-10-21T09:39:35+02:00) | 88565, 2026-10-21 09:45:26 | :8420 `/health` 200 (277) |
| galion-one | enabled/active | /opt/galion/rust/bin/galion-one | 963904 | 2026-10-21 08:39:10 | `24cb4976b1325ae0f5a100a7165a1e2e0bbd29f3f2c0e584dbd3a1cdd03b7ca3` | 40454f9 (record; no stamp) | 715767, 2026-10-21 18:10:21 | :8091 `/health` 200 (3); :8002 `/health` **503** (86) |
| openmind-queue | enabled/active | /usr/local/bin/openmind-queue | 889632 | 2026-10-21 08:27:27 | `8a7f7faaaa864e0a6e5f628f723081a48e2179a21130062f8569aa1685f8ec8d` | eec2d99 (record; no stamp) | 17979, 2026-10-21 08:27:54 (NRestarts 1) | :19120 `/healthz` 200 (117) |
| tuner | enabled/active | /opt/galion/tuner/bin/tuner | 480032 | 2026-10-21 04:32:32 | `89c7fcbcb54c510887dafd8c67b255f05d00595af598fe65f022e204c0ef2b12` | 42de672 (record; no stamp) | 4004553, 2026-10-21 04:32:32 | :8650 `/health` 200 (12) |
| galion-nerve-shadow | **disabled**/active | /opt/galion/rust/bin/galion-nerve-shadow | 1438496 | 2026-10-21 04:37:18 | `3a566c56dfea79cf8cb28bc413c2f7e3f4004969380c9be1ce7dca61382256f1` | unknown (stamp has only built 2026-10-20T21:24:19+02:00) | 4008991, 2026-10-21 04:37:18 | :8672 `/health` 200 (150) |
| galion-health | enabled/active | /opt/galion/rust/bin/galion-health | 503512 | 2026-10-18 22:14:38 | `6afe668c3363031014a5fe3eee54afccd5886db7d71e0995077a94d7ecd8e01a` | unknown | 1374810, 2026-10-18 22:14:38 | :8660 `/` 200 (1950) |
| link-watch-rs | enabled/active | /opt/galion/rust/bin/link-watch | 508248 | 2026-10-18 21:57:47 | `2f5874f72243d24f52eb767f660d57ab14959c205679547b51366fa865f61116` | unknown | 1359406, 2026-10-18 21:57:48 | no listener |
| galion-desk | enabled/active | /opt/galion/bin/galion-desk | 4735088 | 2026-10-10 11:02:43 | `555cc44314aa3d6a2ffe2ca4273cee20c566fb1296ac43c0df7e1ca0a9fd11e6` | unknown | 231, 2026-10-12 07:01:36 | :8646 `/health` 200 (11) |
| tanga-proto | enabled/active | /opt/galion/tanga-proto/bin/tanga-serve | 447264 | 2026-10-18 22:35:48 | `9c540eeb054dd1205666f116167b9ad8ba7edf08f900ce4228a79d210a1de7f7` | unknown | 1467846, 2026-10-18 22:35:49 (NRestarts 3) | :8671 `/tanga/` 200 (41547) |
| cloudflared | enabled/active | /usr/local/bin/cloudflared (symlink → /opt/galion/tooling/bin/cloudflared) | 39838488 | 2026-09-21 23:16:58 | `03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc` | upstream | 611846, 2026-10-21 17:08:54 | :20241 `/ready` 200 (88) |
| tuner-quick | transient/active | /usr/local/bin/cloudflared (same bytes) | 39838488 | 2026-09-21 23:16:58 | `03f1f25d1cc93b9ad6c60569d44060bc4f17ed97075760ed8cfca4b12dcd68cc` | upstream | 1198838, 2026-10-18 19:19:30 | :20242 `/ready` 200 (88) |
| console | enabled/active | /opt/galion/tooling/node/bin/node (Vite dev) | 120177224 | 2025-02-11 01:08:18 +01:00 | `1abce2374a485bddae3c27b17a3e3143e2780232026e627c4fe74ddde3f380a1` | upstream | 2501864, 2026-10-15 08:02:09 | :8080 `/health` 404 (2928) |
<!-- live-table:end -->

Not running (oneshot / timer), same probe:

| Unit | State | ExecStart binary | sha256 | Last run (+02:00) |
|---|---|---|---|---|
| gaia-hermes-handoff.service (via `gaia-hermes-handoff.path`, enabled/active) | static, inactive | /opt/galion/rust/bin/gaia-cortex-**c5cae88** (1,037,088 B, 23:08:11) | `dec978c3f715a4390ddd12cfcf75794bdefd49049c227da296f60bee3fe405a7` | started 2026-10-21 23:08:38 |
| galion-backup.service (timer) | static, inactive | /opt/galion/rust/bin/galion-backup.sh (3026 B) | `0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4` | last 2026-10-21 20:17:37, **next 2026-10-22 02:20:59** |
| galion-deploy (tool, no unit) | – | /opt/galion/rust/bin/galion-deploy (639,664 B, 2026-10-21 04:31:47) | `2a60e1ac632e27d9d918102d5fb02e1bafe35d4a48ead734f403f650de07464d` | – |

Live drop-ins (file mtime, sha256 16-hex prefix):
- **galion-nerve**: 50-rust-bin 09:43:54 `98d5cec1f2a305de`; 60-nerve-eventlog 09:31:06 `c663ab5cbe25c3fb`; 70-nerve-dryrun 17:08:27 `3fe865c9fa1f1bc3`; 75-no-openrouter 18:18:23 `354e5b44dba06086`; 80-event-mode 19:16:56 `053b29920ac10b67`; **85-live-actions 22:56:55 `9bf55307c5149632`** (env names `NERVE_EVT_ACTIONS NERVE_DRY_RUN NERVE_ROLLBACK NERVE_COMPONENTS`).
- **gaia-cortex**: 50-safety 19:06:15 `1d89f47bffba3faa` (env names `CORTEX_ACTIONS CORTEX_LLM_ENABLED CORTEX_NERVE_URL CORTEX_NERVE_STREAM CORTEX_PULL_EVERY_S CORTEX_LISTEN CORTEX_DATA_DIR`, one EnvironmentFile, UnsetEnvironment of OpenRouter/OpenAI/Anthropic names); **85-live-actions 22:56:55 `e48661cb45c57b95`** (`CORTEX_ACTIONS`); **86-hermes-boss 23:08:11 `f398922cf3f68f83`** (ExecStart → gaia-cortex-c5cae88, `CORTEX_HERMES_HANDOFF CORTEX_HERMES_PER_HOUR`); **87-live-fixes 23:36:42 `76090e8b80acd5a3`** (ExecStart → gaia-cortex-bd6fbe6, Description "live actions"; last ExecStart wins).
- galion-one: 50-rust-bin 08:39:24 `467d894b661cfe8e`; 60-no-warm 18:10:21 `5c52f92160e21bd1`.
- hermes-rs: 50-rust-bin 09:46:21 `910cddb2eb9fc187`; 60-nerve-url 09:32:33 `833acdfbb2a68367`; 70-research-budget 18:12:30 `6c8731018b9b9e1f`.
- openmind-memory: 50-rust-bin 09:45:26 `8ea8565d02589918`; 60-nerve-url 09:32:33 `fdd150c7377f0f49`. galion-backup: offsite 2026-10-18 22:22:07 `e370c03716b738ee`.
- New unit files: gaia-hermes-handoff.service `a847fe04a4a1ecb6`, gaia-hermes-handoff.path `12f2156fdad873d4` (PathExistsGlob on the cortex hermes-inbox, TriggerLimit 12/600 s).

Rollback / older binaries still in /opt/galion/rust/bin (sha256 16-hex): openmind-nerve-aa77864 `f0994b6db1fd8ed8`, galion-nerve `3b150dea692ffac0`, gaia-cortex-c7f56b8 `fbe83e136789b37b` (942,880 B, 19:06:15), gaia-cortex-e57b946 `d5b7f0b97916cd9c` (1,037,088 B, 23:05:52), gaia-cortex-c5cae88 `dec978c3f715a439`, hermes-gateway `cd344b167dc65425`, openmind-memory `473ae4a3b13ceabb`, galion-backup (Rust, unused by the unit) `3df3f88d9087c34e`.

Live state seen through the nerve and cortex at 00:09:22 and 00:09:51: nerve mode live, **score 61/D**, cycle 10 (event mode since 22:56:55); cortex `/health` mode **live**, llm_enabled true, hermes_handoff true, busy false, events_in 5 (unchanged between batches).

## 2. Listeners (ss -ltn, 00:09:23)
127.0.0.1: 8002, 8080, 8091, 8420, 8642, 8646, 8650, 8660, 8670, 8671, 8672, **8674 (gaia-cortex, new)**, **18800 (sshd, forge reverse tunnel, new; `/` 200 3805)**, 19120, 20241, 20242, 25; [::1]:25; *:22; resolved 53. No stray listener.

## 3. Diff vs `docs/ops/vps-baseline.md` @ f761f49 (2026-10-21 11:23 CEST)
1. **galion-nerve binary**: `openmind-nerve-aa77864` `f0994b6d…` (1,213,216 B) → **`openmind-nerve-bb287cc` `c15be071…`** (1,415,176 B, mtime 19:13:51), event mode (R22, 19:16:57). PID 87026 → **938448, started 22:56:55** (the 85-live-actions restart). Drop-ins 2 → **6** (+70, 75, 80, 85). `/state` 43,972 → 38,082 B; `/events` 156 → 29,538 B. Score 61/D (same grade as 11:23).
2. **gaia-cortex is new.** 11:23: "there is no cortex unit". Now enabled/active on 127.0.0.1:8674, `gaia-cortex-bd6fbe6` `1b01c3fc…`, 4 drop-ins, mode live, LLM advisor on, Hermes handoff on. Three earlier cortex builds stay on disk (c7f56b8, e57b946, c5cae88).
3. **gaia-hermes-handoff.service + .path are new**; the service runs **c5cae88**, not the live cortex build bd6fbe6 (see §4 D-232-2).
4. **galion-one** restarted 18:10:21 for 60-no-warm (R18); PID 29782 → 715767; same bytes `24cb4976…`. :8002 still 503 (86 B).
5. **hermes-rs** restarted 18:12:42 for 70-research-budget (R19); PID 89484 → 718450; same bytes `eb691b29…`.
6. **cloudflared** restarted 17:08:54 for the forge ingress (R17); PID 22835 → 611846; same bytes `03f1f25d…`. New listener 127.0.0.1:18800 (sshd) serves forge.
7. **galion-backup.timer**: last 08:18:18 → 20:17:37; next 2026-10-22 02:20:59.
8. **Load** 8.88 → 4.50 (1-min).
9. **Unchanged** (same sha, PID, NRestarts): openmind-memory, openmind-queue (NRestarts 1), tuner, galion-nerve-shadow (still disabled/active), galion-health, link-watch-rs, galion-desk, tanga-proto (NRestarts 3), tuner-quick (transient), console (Vite dev), galion-deploy, galion-backup.sh + offsite.conf, working copy 2f5a730. galion-one 8091, hermes 8642, memory 8420, queue 19120 health codes unchanged.
10. Every change in 1–7 has a record: R17–R22 (rollback-table 7a1990e) and CALL-RECORD 22:56 / 23:08 / 23:36:53 (cortex-live-fixes bd6fbe6). **No unrecorded drift found.** Gap: the 22:56:55 `85-live-actions.conf` (nerve + cortex), 23:08 `86-hermes-boss.conf` and the gaia-hermes-handoff units are **not yet rows in the rollback table** (it stops at R22, 20:28).

## 4. Owner decisions / follow-ups (nothing changed)
- **D-232-1 Build stamps.** Only `openmind-memory-aa77864` carries a commit stamp (`aa77864`, built 09:39:35); `galion-nerve-shadow` has only a build time. Every other live Rust binary would print `commit unknown` — the commit is known only from the file name or the deploy record. Option: make `galion-deploy` refuse an unstamped binary (or build every deploy with `GALION_BUILD_COMMIT`/`GALION_BUILD_TIME`). Owner choice.
- **D-232-2 Handoff build skew.** `gaia-hermes-handoff.service` runs `gaia-cortex-c5cae88` while the cortex runs `bd6fbe6` (which merged cortex-hermes-boss 1c5dec4 + t51 chaos fixes). Options: point the handoff unit at bd6fbe6 at the next approved change, or keep c5cae88 pinned on purpose and say so in its runbook.
- **D-232-3 Rollback table rows R23–R26** for nerve 85-live-actions, cortex 85-live-actions, cortex 86-hermes-boss + handoff units, cortex 87-live-fixes. Docs-only; needs the deploy owner's backup paths (not read here).
- **D-232-4 Old cortex builds** c7f56b8 and e57b946 (plus aa77864 nerve, legacy galion-nerve/hermes-gateway/openmind-memory) stay on disk as rollback targets; decide a retention rule (e.g. keep the last 2 per service).
- Still open from f761f49: galion-nerve-shadow disabled but running (D-B2-1); :8002 503 "loading"; console Vite dev on :8080 (D-B2-5 unit layout not in deploy/).

## 5. Re-running
`GALION_REAL=1 cargo test -p galion-realtest --test real_live_table -- --nocapture` (VPS host alias from `GALION_VPS_SSH`, default `slavik-vps`): one read-only ssh batch (`systemctl show -p MainPID,ActiveState`, `sha256sum /proc/<pid>/exe`) per row of §1; fails on any sha/state drift. Without `GALION_REAL=1` only the offline table checks run.
