<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-test record: vps-unit-hygiene (wave-4 slot 183)

2026-10-21, Europe/Zagreb (+02:00). Docs-only task: no build, no binary started. Proposals: `deploy/proposals/unit-hygiene/README.md`.
VPS clock reads UTC; every VPS time below is converted to +02:00. No secrets were read or printed: `systemctl cat` output was filtered
on the VPS so any `Environment=` value whose name contains KEY/TOKEN/SECRET/PASS/AUTH/CRED came back as `<redacted>` (1 line:
`OPENROUTER_KEY_FILE`, a file path; its value was then checked with `grep -c` against the repo's staged path and matched, count 1).
EnvironmentFile contents were never read.

## 1. VPS read-only probes (ssh slavik-vps)
| Time (+02:00) | Commands | Result |
|---|---|---|
| 11:23:27 | `systemctl list-unit-files`, `list-units --all`, `ls -la /etc/systemd/system`, `find … *.service.d/*.conf` | 17 project units; 8 drop-in files; hermes.service masked (`-> /dev/null`, 08:24); link-watch.service disabled |
| 11:24:22 | `systemctl cat` + `systemctl show` (19 props) for 17 units, `systemd-analyze verify` on 16 files, `stat` of 6 binary paths | all verify rc 0, 0 lines; `/opt/galion/openmind/dist/bin-ws/tuner`: **No such file** |
| 11:24:22 | `ls multi-user.target.wants`, `readlink -f /workspace…`, `sha256sum` 7 binaries, `/proc/<pid>/exe` for 7 units, `ls /var/backups`, `journalctl` 08:20-08:30 | galion-one link from 2026-10-18 22:13; `/workspace -> /opt/galion`; running exes = drop-in paths (galion-one, openmind-nerve-aa77864, hermes-gateway-aa77864, openmind-memory-aa77864); systemd 255.4-1ubuntu8 |
| 11:25:35 | `systemctl show -p MemoryCurrent,MemoryPeak,TasksCurrent` for 13 units; loopback GETs | tuner peak 1,105,920 B / 1 task; galion-desk 10,706,944 B / 1; nerve-shadow 10,051,584 B; console 1,288,105,984 B current |
| 11:25:35 | `systemctl --failed` | 3: run-u11093/4/5.service (`systemctl is-active ssh.service`, failed 2026-10-18 22:13:59) |
| 11:26 | `sha256sum` of 7 live unit/drop-in files; GET `/events?limit=1`; `/state` sources | see §2; `/events` 200 (156 B); sources hermes-rs 6, openmind-memory 6 |

Live loopback health at 11:25:35: 8660 200, 8091/health 200, 8650/health 200, 8642/health 200, 8420/health 200, 8671/tanga/ 200, 8672/state 200.
Listeners (`ss -ltn`): 22, 25, 53, 8002, 8080, 8091, 8420, 8642, 8646, 8650, 8660, 8670, 8671, 8672, 19120, 20241, 20242 (8431-8433/8652/8653 gone).

### Nerve (real-time observation; cortex not a separate live service, nerve /state carries the cortex keys)
11:25:24 +02:00: openmind-nerve 0.1.0, started 09:43:54, **cycle 204**, cycle_ms 561, mode live, status degraded, **score 61 / D**,
healthy 11, degrading 1, failing-soon 1, absent 2; top incident: galion-one-mouth 503 "stuck loading" (known, slot 181). Unit hygiene
does not move this score; no item here touches the mouth.

## 2. Byte-exact live copies on the box
The live files were rebuilt from the `systemctl cat` output (the one redacted path restored after the `grep -c` match) and hashed. All equal the VPS hashes:

| File | sha256 (VPS = box copy) |
|---|---|
| galion-one.service | cff8a7c151cbfb89ce150c7d1599e9bd2018b3afe726b99462b6fd006e5cdeaf |
| link-watch.service | a283c5a1c6051f1f15c24b02e85639a8c8e58c914f3279cccfe9b0fdb9191a75 |
| tuner.service | 8d47534506f874b6ccce913c6e633eaf3a4925fe5a87caf438b325ac5bb2ada4 |
| galion-nerve.service.d/60-nerve-eventlog.conf | c663ab5cbe25c3fb28d95f2889db2726ecb216289952fb15b454797dcbbbe98f |
| hermes-rs.service.d/60-nerve-url.conf | 833acdfbb2a68367e95b706f29478d13bc6065a950d6a508f35ab404a96d9850 |
| openmind-memory.service.d/60-nerve-url.conf | fdd150c7377f0f492b52f12e8e3045812fd1e755642aaea99c919a2187bc2642 |
| galion-one.service.d/50-rust-bin.conf | 467d894b661cfe8e4fb0fb6ecc0f9f15c77dd2b60a55f702ac81692d0f85ff8b |

These copies stay in the box scratch dir only (not committed: link-watch-rs carries a host address).

## 3. Box tests of the proposals (systemd 257.13, offline, `--root` trees)
Two trees: `live-root` (live units + live drop-ins, stub executables at every ExecStart path, the box's /usr/lib/systemd/system for targets)
and `prop-root` (same plus P1, P6, P9 and the five survival drop-ins from the repo patch).

| Test | Result |
|---|---|
| `patch -p1 --dry-run` P1 and P6 on live copies | OK, 4 files |
| forward apply == proposed files (`cmp`) | P1 match; P6 3/3 match |
| reverse apply (`-R`) gives live sha256 back | 4/4 (cff8a7c1, c663ab5c, 833acdfb, fdd150c7) |
| P6 diff touches no `Environment=` line | 0 such lines |
| `systemd-analyze verify --root` live-root, 7 units | rc 0, 0 warnings each |
| same on prop-root before the P3b fix | link-watch-rs: `10-restart.conf:5: Unknown key 'StartLimitIntervalSec' in section [Service], ignoring.` |
| same on prop-root after P3b (all survival drop-ins attached to galion-desk, galion-one, link-watch-rs, tuner) | 7/7 rc 0, 0 warnings |
| `systemctl --root enable galion-one` on live-root (no link) | "The unit files have no installation config …", no link made |
| `systemctl --root disable galion-one` on live-root (with link) | link removed |
| `systemctl --root enable galion-one` on prop-root | `Created symlink …/multi-user.target.wants/galion-one.service` |
| `git apply --check repo/repo-hygiene.patch` on integration-4 42de672 | OK (11 files, +64/-27) |

## 4. Repo checks (11:31 +02:00)
- `scripts/no-python.sh`: `no-python: ok`.
- `repo-check --root .` (release build of crates/repo-check on this branch): 1393 files, **0 errors**, 47 warnings (all pre-existing `utc`/timestamp warnings in other crates), PASS. No finding in the new files.
- No cargo build or test is needed for a docs-only change; nothing in `crates/` changed.
- The three P6 `.conf` files carry no SPDX header on purpose: they are byte-for-byte replacements for live drop-ins (comment lines only differ), like the live 50-rust-bin.conf files. Every other new file has the header.
