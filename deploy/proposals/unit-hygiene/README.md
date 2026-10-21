<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Unit-file hygiene proposals (vps-ops S6.3, wave-4 slot 183)

Written 2026-10-21 11:22-11:45 CEST (+02:00). **Proposals only. Nothing here is installed.** Every VPS step below needs Slavik's
approval, one item at a time. The VPS was read only: `systemctl cat/show/list-unit-files/list-units/--failed`, `systemd-analyze verify`
on the live files, `ls`/`stat`/`sha256sum`/`readlink`, `ss -ltn`, `journalctl` reads and loopback GETs (11:23:27, 11:24:22, 11:25:35 and
11:26 +02:00). No file was written there, nothing was reloaded or restarted. Evidence: `docs/real-tests/vps-unit-hygiene.md`.

Base: integration-4 42de672. Inputs: vps-ops plan S6.3, the S1.1 ledger (swarm/vps-baseline f403eeb, `docs/ops/vps-baseline.md`
§ findings 1, 2 and 4), and the CALL-RECORD lines for today's live changes (tuner 04:32, hermes mask 08:24, galion-one 08:39, nerve/memory/hermes
binaries 09:43-09:46).

## What is in this folder
| Path | What | Touches |
|---|---|---|
| `P1-galion-one/galion-one.service` | Proposed full `/etc/systemd/system/galion-one.service` | VPS (after approval) |
| `P1-galion-one/galion-one.service.patch` | Live (sha256 `cff8a7c1…cdeaf`) -> proposed, `patch -p1` from `/` | VPS (after approval) |
| `P6-dropin-comments/*.service.d/*.conf` + `dropin-comments.patch` | Comment-only refresh of three live drop-ins | VPS (after approval) |
| `P9-limits/{tuner,galion-desk}.service.d/20-limits.conf` | Optional memory/task caps | VPS (after approval) |
| `repo/repo-hygiene.patch` | Repo-side fixes (P1, P3, P3b, P4, P5): staged copies made to match live, superseded files marked | repo only |

The patches were tested on the box against **byte-exact copies of the live files** (sha256 identical to the VPS): forward apply gives the
proposed files exactly, reverse apply gives the live shas back, and `systemd-analyze verify` (box systemd 257; VPS runs 255) gives
0 warnings for all seven affected units with every proposed file and drop-in in place.

## Live unit inventory (2026-10-21 11:24 +02:00)
17 project units. All loaded, `NeedDaemonReload=no` everywhere, `systemd-analyze verify` rc 0 with no output for all 16 files in /etc (15 services + galion-backup.timer).

| Unit | State | Fragment / runs | Drop-ins | [Install] | Note |
|---|---|---|---|---|---|
| galion-one | enabled, active | fragment ExecStart in frozen working copy, **runs /opt/galion/rust/bin/galion-one** via drop-in | 50-rust-bin.conf | **missing** but enabled | P1 |
| galion-nerve | enabled, active | runs openmind-nerve-aa77864 via drop-in | 50-rust-bin.conf, 60-nerve-eventlog.conf | yes | P6 |
| hermes-rs | enabled, active | runs hermes-gateway-aa77864 via drop-in | 50-rust-bin.conf, 60-nerve-url.conf | yes | P6; Restart=on-failure |
| openmind-memory | enabled, active | runs openmind-memory-aa77864 via drop-in | 50-rust-bin.conf, 60-nerve-url.conf | yes | P6 |
| galion-backup (.timer) | static, timer enabled | /opt/galion/rust/bin/galion-backup.sh | offsite.conf | timer only | not in scope |
| hermes | **masked** (`-> /dev/null` since 2026-10-21 08:24 +02:00) | original unit in /var/backups/hermes-legacy-20261021 | - | - | P2 |
| link-watch | disabled, inactive | `/usr/bin/python3 …/link-watch.py` (file from 2026-10-08) | - | yes | P3 |
| link-watch-rs | enabled, active | /opt/galion/rust/bin/link-watch | - | yes | Conflicts=link-watch.service |
| tuner | enabled, active | /opt/galion/tuner/bin/tuner (sha256 `89c7fcbc…2b12`, integration-4 build since 04:32) | - | yes | P4, P9 |
| galion-desk | enabled, active | /opt/galion/bin/galion-desk (listens 127.0.0.1:8646) | - | yes | P9; Restart=on-failure, no limits |
| galion-nerve-shadow | **disabled**, active since 04:37 | /opt/galion/rust/bin/galion-nerve-shadow (8672) | - | yes | P7 |
| console | enabled, active | node vite dev, `--host 127.0.0.1 --port 8080`, 1.29 GB RSS | - | yes | P5 (repo comment) |
| cloudflared | enabled, active | config in /opt/galion/galion-src/… | - | yes | no limits; leave |
| tuner-quick | transient (/run) | cloudflared quick tunnel | - | - | SEC-5, vps-ops S5.3 |
| galion-health, tanga-proto, openmind-queue | enabled, active | - | - | yes | clean |

Also: `systemctl --failed` lists 3 leftover transient units `run-u11093/4/5.service` (`systemctl is-active ssh.service`, failed
2026-10-18 22:13:59 +02:00). P8.

### Drop-in inventory
| Drop-in | Added (+02:00) | Purpose | Comment state |
|---|---|---|---|
| galion-one.service.d/50-rust-bin.conf | 2026-10-21 08:39 | run one-no-shellout 40454f9 | current |
| galion-nerve.service.d/50-rust-bin.conf | 2026-10-21 09:43 | run openmind-nerve-aa77864 | current |
| galion-nerve.service.d/60-nerve-eventlog.conf | ~2026-10-21 09:31 (backup stamp) | NERVE_EVENT_LOG | **stale**: says the live binary ignores it; it is read since 09:43 (GET /events 200) |
| hermes-rs.service.d/50-rust-bin.conf | 2026-10-21 09:46 | run hermes-gateway-aa77864 | current |
| hermes-rs.service.d/60-nerve-url.conf | ~2026-10-21 09:32 (backup stamp) | NERVE_URL | **stale**: says inert/not restarted; nerve /state shows source `hermes-rs` |
| openmind-memory.service.d/50-rust-bin.conf | 2026-10-21 09:45 | run openmind-memory-aa77864 | current |
| openmind-memory.service.d/60-nerve-url.conf | ~2026-10-21 09:32 (backup stamp) | NERVE_URL | **stale**: as above, source `openmind-memory` present |
| galion-backup.service.d/offsite.conf | 2026-10-18 22:22 | offsite encrypt hooks | current |

Backups of every unit edited today exist in /var/backups (`*.service-20261021-*`, `galion-one-20261021/`, `hermes-legacy-20261021/`).

## Items
Each item: problem, evidence, proposal, runtime effect, apply, rollback. "Daemon-reload only" means no service restarts.

### P1 galion-one fragment: wrong header, missing [Install], ordering on a masked unit
- **Problem.** The live fragment still says "STAGED, NOT INSTALLED … No [Install] section on purpose", but the unit is installed and
  **enabled** (link `multi-user.target.wants/galion-one.service`, made 2026-10-18 22:13 +02:00 by hand). Without `[Install]`,
  `systemctl disable galion-one` removes that link but `systemctl enable galion-one` cannot make it again ("The unit files have no
  installation config"; tested offline with `systemctl --root` on a byte-exact copy). One disable/enable cycle and One no longer starts at boot.
  With the P1 file, enable recreates the link (tested the same way). `After=hermes.service` orders against a unit that has been masked since 08:24; the Hermes that One talks to is
  `hermes-rs.service`. The SPDX line is `2026 Slavik / Galion Studio` instead of the project line.
- **Proposal.** `P1-galion-one/galion-one.service`: new header (installed, enabled, drop-in owns the binary, base ExecStart kept as the
  rollback target), `After=network-online.target hermes-rs.service`, `[Install] WantedBy=multi-user.target`. **Everything in `[Service]` is
  byte-identical.**
- **Runtime effect.** None on the running process. `After=` matters only at the next start; `[Install]` matches the link that already exists.
- **Apply.** `cd / && patch -p1 --dry-run < galion-one.service.patch`, then without `--dry-run`; `systemctl daemon-reload`;
  check `systemctl show galion-one -p After,UnitFileState,NeedDaemonReload` (expect hermes-rs.service listed, enabled, no).
  Backup first: `cp -a /etc/systemd/system/galion-one.service /var/backups/galion-one.service-<ts>`.
- **Rollback.** `patch -p1 -R < galion-one.service.patch` (tested: gives sha256 `cff8a7c1…` back) or restore the backup; daemon-reload.
- **Not proposed now (owner choice, P1b/P1c).** P1b: fold `ExecStart=/opt/galion/rust/bin/galion-one` into the fragment and delete
  50-rust-bin.conf, once 40454f9 is accepted as the permanent build (the rollback then becomes galion-deploy, not "rm the drop-in").
  P1c: `GALION_HOME=/workspace/galion-light` resolves through the VPS symlink `/workspace -> /opt/galion`; writing `/opt/galion/galion-light`
  is clearer, same target, but needs a restart to take effect. Recommendation: P1 now, P1b after the voice/mouth decision (slot 181), P1c only together with P1b.

### P2 hermes.service: leftover is the mask itself
- **State.** Already handled by t172: unit file moved to /var/backups/hermes-legacy-20261021/ and masked 08:24 +02:00. hermes-rs keeps
  `Conflicts=hermes.service`; link-watch-rs and galion-one (fixed in P1) referenced it.
- **Proposal.** Keep the mask at least until the 7-day archive window ends (2026-10-28) and through launch. It is a 9-byte symlink, it blocks a
  stray `systemctl start hermes`, and the `Conflicts=` line then costs nothing. Later option: remove the mask and the `Conflicts=` line together.
- **Decision (Slavik):** D-U2 keep the mask permanently (recommended) or remove it after 2026-10-28.

### P3 link-watch.service: old Python unit still installed (disabled)
- **Problem.** `/etc/systemd/system/link-watch.service` (2026-10-08, `ExecStart=/usr/bin/python3 /opt/galion/galion-light/scripts/link-watch.py`,
  `[Install] WantedBy=multi-user.target`) is disabled and inactive, but one `systemctl enable --now link-watch` would bring the Python
  watcher back. link-watch-rs has `Conflicts=link-watch.service`, so starting the old one **stops the Rust one**.
- **Proposal (VPS, same pattern as hermes).**
  ```
  install -d -m 0700 /var/backups/link-watch-legacy-<ts>
  cp -a /etc/systemd/system/link-watch.service /var/backups/link-watch-legacy-<ts>/
  sha256sum /var/backups/link-watch-legacy-<ts>/link-watch.service   # expect a283c5a1…1a75
  rm /etc/systemd/system/link-watch.service && systemctl daemon-reload
  systemctl mask link-watch.service
  systemctl is-active link-watch-rs   # expect active, unchanged
  ```
  The script `/opt/galion/galion-light/scripts/link-watch.py` stays (galion-light is not ours to prune here).
- **Runtime effect.** None: the unit is inactive; link-watch-rs is untouched (no restart).
- **Rollback.** `systemctl unmask link-watch.service; cp -a /var/backups/link-watch-legacy-<ts>/link-watch.service /etc/systemd/system/; systemctl daemon-reload` (stays disabled, like today).
- **Repo side (in `repo/repo-hygiene.patch`).** `deploy/link-watch.service` (a third copy, expecting `dist/bin/link-watch` in the working
  copy) gets a SUPERSEDED header. `deploy/survival/link-watch.d/` becomes `link-watch-rs.d/`: as staged it targeted the disabled Python unit
  and would never have applied (S1.1 finding 1).
- **Decision (Slavik):** D-U3 retire link-watch.service as above (recommended), or keep it as a documented fallback.

### P3b survival drop-ins: StartLimit settings in the wrong section (repo bug)
- **Problem.** All five `deploy/survival/*/10-restart.conf` put `StartLimitIntervalSec=60` under `[Service]`. systemd ignores it there:
  `Unknown key 'StartLimitIntervalSec' in section [Service], ignoring` (systemd-analyze verify, box systemd 257). The crash-loop limit the
  survival plan expects (60 s / 5) would silently not apply.
- **Proposal.** Move `StartLimitIntervalSec` and `StartLimitBurst` to `[Unit]` in all five files (repo patch). Re-verified: 0 warnings.
  None of these drop-ins is installed (DropInPaths empty for those units), so nothing live changes.

### P4 deploy/staged/tuner.service points at a path that does not exist
- **Problem.** The staged file has `ExecStart=/opt/galion/openmind/dist/bin-ws/tuner`. That file **does not exist** on the VPS (stat:
  No such file, 11:24). The tuner cutover happened differently: galion-deploy replaced `/opt/galion/tuner/bin/tuner` in place at 04:32
  (sha256 `89c7fcbc…2b12`), and the unit was not changed. `docs/ws-one-desk-tuner.md` step 8 still says to install the staged file and restart:
  following it today would take tuner down.
- **Proposal (repo only).** Replace `deploy/staged/tuner.service` with the live unit (sha256 `8d475345…bada4`) plus a header saying
  so. After that, step 8 is a no-op instead of an outage. No VPS change.

### P5 deploy/console-prod.service stale comment
- **Problem.** The comment says the live console runs vite on `0.0.0.0:8080`. Live (11:24) is `--host 127.0.0.1 --port 8080` in
  /opt/galion/console-ide. The staged unit also points into the frozen working copy (`/opt/galion/openmind/stage/console-dist`).
- **Proposal (repo only).** Correct the comment and add the note that the bundle must be copied out of the working copy before any install
  (RULES hard rule 3). The console port work (slot 169, swarm/console-port-merge 508401d) may replace this unit entirely.

### P6 drop-in comments that are no longer true
- **Problem.** Three live drop-ins say they are inert ("the binary live … ignores this variable", "inert until a nerve-client build is
  deployed. Not restarted for it"). Since 09:43-09:46 +02:00 the aa77864 binaries read them: `GET /events` 200, and nerve `/state` carries
  signals with source `hermes-rs` (6) and `openmind-memory` (6) at 11:26. A future operator reading the comment could delete a live setting
  thinking it does nothing.
- **Proposal.** `P6-dropin-comments/`: comment lines rewritten (ACTIVE since …; inert again if 50-rust-bin.conf is rolled back). The
  `[Service]` and `Environment=` lines are unchanged (checked: no `Environment` line in the diff).
- **Runtime effect.** None. Daemon-reload only (to clear NeedDaemonReload after the mtime change).
- **Apply / rollback.** `patch -p1 [--dry-run] < dropin-comments.patch` from `/`; `-R` to undo (tested, gives the live shas back).

### P7 galion-nerve-shadow: running but disabled
- **State.** Active since 2026-10-21 04:37 +02:00, `UnitFileState=disabled` although the file has `[Install]`. It will not come back after
  a reboot. MemoryPeak 9.6 MiB, Restart=on-failure.
- **Decision (Slavik):** D-U7 (a) leave as is: a temporary observer that vanishes on reboot (recommended until the cortex/nerve-swap verdict);
  (b) `systemctl enable` it so it survives reboots; (c) stop and remove it after the nerve swap (vps-ops S4.6). Nothing to change in the file.

### P8 three failed transient units
- `run-u11093.service`, `run-u11094.service`, `run-u11095.service` (`systemctl is-active ssh.service` via systemd-run, failed 2026-10-18
  22:13:59 +02:00) still show in `systemctl --failed` (3 lines). Any check that counts failed units sees them.
- **Proposal.** `systemctl reset-failed run-u11093.service run-u11094.service run-u11095.service`. Clears the record only; no rollback needed.

### P9 optional resource caps for tuner and galion-desk
- tuner and galion-desk run with `MemoryMax=infinity` and the default TasksMax (154243). Measured 11:25: tuner peak 1.1 MiB / 1 task;
  galion-desk peak 10.2 MiB / 1 task.
- **Proposal.** `P9-limits/*/20-limits.conf`: tuner `MemoryMax=32M TasksMax=16`, galion-desk `MemoryMax=64M TasksMax=32`. Cgroup limits apply at
  daemon-reload, no restart. Risk: an OOM kill if a future build needs much more memory; both units restart themselves (tuner always/2 s,
  desk on-failure/2 s). Rollback: rm the file, daemon-reload.
- Not proposed: caps for cloudflared (public tunnel; an OOM kill is an outage) and console (vite dev, 1.29 GB; replaced by the console port).
- **Decision (Slavik):** D-U9 install P9 (low value, low risk) or skip.

## Order, if approved
P8 and P6 first (no effect at all), then P1, then P3, then P9. One item per approval, daemon-reload after each, then
`systemctl --failed`, `systemctl show <unit> -p ActiveState,NRestarts,NeedDaemonReload` and the nerve `/state` check. Repo patch (P1, P3, P3b,
P4, P5) can land on its own at any time: `git apply --index repo/repo-hygiene.patch` (checked against integration-4 42de672).

## Owner decisions (Slavik)
| ID | Question | Recommendation |
|---|---|---|
| D-U1 | Apply P1 (galion-one header + `[Install]` + `After=hermes-rs.service`)? P1b/P1c later? | P1 yes; P1b after the slot-181 voice decision |
| D-U2 | hermes.service mask: keep permanently, or remove after 2026-10-28? | keep |
| D-U3 | Retire link-watch.service (backup, remove, mask)? | yes |
| D-U4 | Land the repo patch (P1, P3, P3b, P4, P5)? | yes |
| D-U6 | Apply the P6 comment refresh? | yes |
| D-U7 | galion-nerve-shadow: leave / enable / remove after nerve swap? | leave until the nerve-swap verdict |
| D-U8 | `reset-failed` the 3 run-u units? | yes |
| D-U9 | P9 caps for tuner and galion-desk? | optional |
