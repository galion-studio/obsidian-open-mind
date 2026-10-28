<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: deploy-sim (swarm slot 33), cutover rehearsals on the box

- Branch `swarm/deploy-sim`, built on `swarm/tuner-cutover` 5fd1611 (base main 13a296c).
- Times are Europe/Zagreb, CEST (+02:00), 2026-10-20.
- **Nothing was deployed, copied, written, restarted or POSTed on the VPS.** The only VPS access was one batch of read-only HTTP GETs over ssh (section 6).
- Ports used on the box: 19330-19339 only. Every process the rehearsals started was stopped. `ss` at 21:20 showed no listener in 19330-19339, and no process under `/workspace/swarm/sim/deploy-sim` was left.

## 1. What was built

| piece | where | what it does |
|---|---|---|
| pluggable galion-deploy | `deploy/foundation/galion-deploy` | `GALION_DEPLOY_SM=systemd` (the default) runs exactly the original `systemctl`/`journalctl` calls. `GALION_DEPLOY_SM=direct` sends the same five operations (known, exec-path, restart, is-active, journal) to `$GALION_DEPLOY_CTL <verb> <svc>`. Optional overrides (none set means the original behaviour): `GALION_DEPLOY_BACKUP_DIR`, `GALION_DEPLOY_DATA_BACKUP`, `GALION_DEPLOY_VERSION_TIMEOUT`, `GALION_DEPLOY_RESET_FAILED`. |
| `simsup` | `sim/deploy-sim/` (std only), binaries in `/workspace/swarm/sim/deploy-sim/bin` | A tiny supervisor that plays systemd for the subset the cutover units use: `ExecStart`, `Environment`, `Restart=always/on-failure/no`, `RestartSec`, `TimeoutStopSec` (SIGTERM, then SIGKILL to the process group), `StartLimitIntervalSec`/`StartLimitBurst` (systemd's fixed-window rate limit, which also refuses manual restarts until `reset-failed`), Type=simple exec-failure semantics (203/EXEC, and the job still "succeeds"), a per-unit journal, `daemon-reload`, and a control socket. It also contains `simsup probe`, a downtime probe (one GET every 2 or 5 ms, which sleeps between tries). |
| `sim-broken`, `sim-hang` | `sim/deploy-sim/src/bin` | A valid ELF that answers `--version` but exits 1 as a service. A valid ELF that hangs on `--version` and never binds as a service. |
| equivalence harness | `scripts/real/galion-deploy-equiv.sh` | Runs the original 2,830 B script and the new one side by side against shimmed `systemctl`/`journalctl`/`curl`/`sleep`, inside an unprivileged user+mount namespace (`/var/backups` is a bind-mounted scratch dir). |
| rehearsals | `scripts/real/deploy-sim.sh [tuner\|nerve\|room\|desk\|all]` | One fresh sim root per service, the live unit text with sim paths and ports, and the real old and new binaries. |
| builds | `scripts/real/deploy-sim-build.sh` | Static x86_64 musl builds of every old and new binary, from git worktrees under `/workspace/swarm/sim/deploy-sim/src`. |

### Rehearsal binaries (`/workspace/swarm/sim/deploy-sim/bin/SHA256SUMS`)

| name | source | bytes | sha256 |
|---|---|---|---|
| tuner-old | main 13a296c `crates/tuner` (same logic as the live hand-built binary: it ignores argv and `expect("bind")`s) | 450,568 | `067c56be…3090` |
| tuner-new | this branch, which is the swarm/tuner-cutover 5fd1611 crate | 459,176 | `47a17df2234fdd5c741f13e9403228e1d4f4dc5f73ac7b41e7996d5e3c511370`, **byte-identical to `/workspace/swarm/artifacts/tuner/tuner-x86_64-unknown-linux-musl`** (reproducible) |
| nerve-old | ws/nerve a63b730 `crates/galion-nerve` (the line the live galion-nerve was built from; the live sha was not read, see nerve.md) | 816,456 | `3b150dea…6d3d` |
| nerve-new | swarm/nerve 5802d0e `crates/openmind-nerve` | 1,110,120 | `ad9a5b29d8ce71337c9945e10e8117700da6c4b3dd3d5aeb1f2f96e183bd106a`, reproducible (a second build from another worktree and target dir gave the same sha) |
| room-old / room-new | main 13a296c / swarm/room 5bb27e1 | 469,832 / 494,472 | `d0600109…c1` / `05075415…0949` |
| desk-old / desk-new | main 13a296c (stand-in for the Oct 10 glibc build, whose source is not in git) / swarm/desk-linkwatch a54c792 | 753,448 / 755,784 | `869b60fc…a76d` / `b46b148f…fce5` |

simsup itself: 10 unit tests (unit parser, Zagreb DST edges, start-limit refusal of a manual restart, process swap, exec failure, probe). `cargo fmt --check` and `clippy --all-targets -D warnings` are clean. It builds for x86_64 and aarch64 musl, and `cargo check` passes for aarch64-linux-android and armv7-unknown-linux-musleabihf.

## 2. "No behaviour change for systemd": equivalence run

```
$ scripts/real/galion-deploy-equiv.sh
SAME  healthy: exit=0, 6 shim calls
SAME  healthy-unit-suffix: exit=0, 6 shim calls
SAME  broken-rolls-back: exit=1, 68 shim calls
SAME  rollback-also-bad: exit=1, 125 shim calls
SAME  restart-refused: exit=1, 3 shim calls
SAME  unit-only-health: exit=0, 14 shim calls
SAME  unknown-unit: exit=1, 1 shim calls
SAME  not-elf: exit=1, 2 shim calls
SAME  missing-new: exit=1, 2 shim calls
SAME  usage: exit=2, 0 shim calls
galion-deploy systemd equivalence: 10 same, 0 different
```

For every scenario it compares the output, the exit code, the ordered list of every `systemctl`/`journalctl`/`curl`/`sleep` call with its arguments, the installed binary and the backup files. As a mutation check, changing `-n 20` to `-n 21` in the new script's journal call made the harness report `DIFF` for the two rollback scenarios. The harness is therefore sensitive to a one-character change. Re-run after the opt-in flags were added: still 10/10 SAME.

## 3. Results (run 5, 21:14–21:19 CEST, all four in parallel)

| rehearsal | checks | full log |
|---|---|---|
| tuner (docs/cutover/tuner.md, every step) | **21 pass, 0 fail** | `docs/real-tests/deploy-sim/tuner-run5.log` |
| nerve (docs/cutover/nerve.md) | **18 pass, 0 fail** | `docs/real-tests/deploy-sim/nerve-run5.log` |
| room (docs/cutover/room.md) | **5 pass, 0 fail** | `docs/real-tests/deploy-sim/room-run5.log` |
| desk (docs/cutover/desk.md) | **8 pass, 0 fail** | `docs/real-tests/deploy-sim/desk-run5.log` |

Runs 1–4 are what found and fixed the problems in section 5. Their logs stay on the box in `/workspace/swarm/sim/deploy-sim/runs/`.

### Measured downtime (probe: one GET every 2 ms, or 5 ms for the long scenarios; value = first failed try to first OK try, so ±2 ms)

| scenario | tuner | nerve | room | desk |
|---|---|---|---|---|
| planned swap via galion-deploy | **3 ms** (1 refused try) | **2 ms** | 5 ms (redeploy) | **2 ms** |
| manual swap (desk.md steps 8–11) | | | | 4 ms |
| rollback L2 (galion-deploy with the backup) | 2 ms | 4 ms | | 2 ms |
| rollback L3 / desk step 13 (install + mv + restart) | 2 ms | 2 ms | | 2 ms |
| rollback L1 (automatic, broken binary) | 20,163 ms | 20,186 ms | 20,158 ms | 20,159 ms |
| rollback L1 (truncated ELF, segfaults) | 20,188 ms | | | |
| rollback L1 (sim-hang + `GALION_DEPLOY_VERSION_TIMEOUT=5`) | 20,496 ms | | | |
| first install (room) | | | 316 ms from `install` to the first 200 (includes the ctl round trip) | |

A whole planned `galion-deploy` run took about 4.1 s: 3 s of (stub) data backup, the restart, then the first health poll after 1 s. The old process serves for most of that. These numbers come from simsup, not systemd. systemd's own restart adds tens of ms (fork, cgroup setup, D-Bus). docs/cutover/tuner.md's "well under 1 s" therefore holds, and the binaries themselves contribute only a few ms.

## 4. Tuner: steps of docs/cutover/tuner.md, as rehearsed

1. **3.1 copy**: the real artefact and `SHA256SUMS` went into a 0700 stage dir, `sha256sum -c --ignore-missing` printed OK, then chmod 0755.
2. **3.2 preflight**: `--version` printed `tuner 0.1.0` without binding. `TUNER_ROOT=… --check` printed 4× ok, exit 0.
3. **3.3 swap**: `galion-deploy tuner <new> <health>` exited 0 in 4.1 s, with 1 refused probe (3 ms).
4. **3.4 health**: the running exe's sha was `47a17df2…`, `/` gave `200 2356`, `/nope` gave 404, and all **21/21 routes were byte-identical to `crates/tuner/tests/fixtures/live/`**, the bytes the live VPS tuner served.
5. **Rollback L2** (galion-deploy with the backup) exited 0 with the old sha back (2 ms). The old binary's `--version` panics on the busy port, which is harmless but leaves an `Aborted` line in the deploy output. **L3** (install + mv + restart) brought the old sha back (2 ms). **L1** with a broken binary and with a truncated ELF: health failed for 20 s, galion-deploy restored the backup, and it reported `rolled back ok`.
6. **Hangs**: see F1 below.

## 5. Findings (the reason for the rehearsal)

**F1. galion-deploy waits forever for `"$NEW" --version`.** There is no timeout, and the step runs before the backup and the install. Two of the binaries in these cutovers ignore their arguments: the **old/live tuner**, and **galion-desk** (both the live build and the new swarm/desk-linkwatch build, which handles only `--help`). Such a binary binds its default address. While the unit holds that port, the bind fails and the step returns (old tuner and old desk panic and abort; the new desk prints the error and exits 1 after 4 ms). **When the service is down, the bind succeeds, the deploy hangs, and the `--version` process serves the port outside the unit.** Rehearsed:
- tuner, "hang C": the new tuner is installed but down, and the operator does the documented L2 manual rollback. galion-deploy hung, and `GET /health` returned 200 from `tuner-<stamp>.bin --version` (pid shown in the log) while the unit was `inactive`.
- desk: desk down, forward deploy of the new desk. It hung the same way.
- sim-hang (hangs on `--version` and never serves): the deploy hung while the service stayed untouched. No backup was taken and nothing was installed.
- With the opt-in `GALION_DEPLOY_VERSION_TIMEOUT=5` all three complete: hang C and desk exited 0 with the right sha, and sim-hang was installed, failed health, and was rolled back automatically.
- On the VPS today the live script has no timeout. **Plan rule: run L2 (galion-deploy) only while the unit is active. If the unit is down, use L3.** For the desk, deploy only while the desk is up, or add `--version` to galion-desk (one line; separate change on that branch).

**F2. Start-limit trap: the rollback restart can be refused, and galion-deploy then dies without restoring service.** systemd refuses a start, including a manual `systemctl restart`, once a unit has used `StartLimitBurst` starts inside `StartLimitIntervalSec`, until `systemctl reset-failed`. galion-deploy runs with `set -e`, so a refused rollback restart ends the script right there: the old binary is back on disk, the unit is `failed`, and the service is **down**.
- **nerve** (`StartLimitBurst=5`, `StartLimitIntervalSec=600`): deterministic. Swap, L2, re-deploy and L3 used 4 starts. A broken deploy used the 5th, and the broken binary's auto-restart and then the rollback restart were refused (`Job for galion-nerve.service failed because start of the service was attempted too often`). Recovery: `reset-failed` + `restart`, 54 ms.
- **tuner** (systemd defaults 5 per 10 s, `RestartSec=2`): a race between the crash-loop cadence and galion-deploy's 20 s health loop. The trap fired in 3 of 8 default L1 attempts across runs 2–5 (run 3: 1/2, run 4: 2/2, runs 2 and 5: 0/2). On real systemd, whose restart latency is lower than simsup's 20 ms reaper tick, it is probably rarer, but it is not impossible.
- With the opt-in **`GALION_DEPLOY_RESET_FAILED=1`** (it runs `reset-failed` before each restart) it never fired: tuner 3/3 runs and nerve 1/1 rolled back ok with a full window.

**F3. Exit code 1 is ambiguous.** `rolled back ok`, "died at a refused restart" (F2) and `restart-refused` in the equivalence run all exit 1. **After any non-zero exit, check `systemctl is-active <unit>` and the installed sha. Do not trust the exit code alone.**

**F4. Nerve health URL.** galion-deploy's table has no `galion-nerve` entry, so without a URL it only checks `is-active` for 5 s. The live nerve answers **`/health` with 404** (read-only GET, 21:03 CEST). Only the new one has `/health`. Rehearsed: a broken deploy judged on `/health` rolled back to the old nerve and then reported `ROLLBACK ALSO UNHEALTHY`, although the old nerve was serving `/healthz` 200. **Always pass `http://127.0.0.1:8670/healthz`.**

**F5. galion-deploy cannot do a first install** (room). It dies with `cannot resolve installed binary for room` (exit 1, 40 ms) because there is nothing to back up. Use the manual first install from room.md. Later upgrades through galion-deploy work, as the redeploy and the broken-binary rollback show.

**F6. Interrupting galion-deploy after the install leaves the new binary in place with no rollback.** This means Ctrl-C, a dropped ssh session, or an outer timeout. It was seen by accident in run 4: a parallel rehearsal killed nerve's `sim-hang --version`, the deploy went on, installed sim-hang, and was then killed in its health loop. The next swap had to recover from a never-binding binary (3.4 s outage). Fixed in the script (per-rehearsal copies of the bad binaries), but the lesson stands: **run galion-deploy under `nohup`/`systemd-run` or in tmux, never in a fragile ssh session.**

**F7. State compatibility.** The new nerve started on the old nerve's data dir (`alerts.json`, `self-eval`, `state.json`), and the old nerve started on the new nerve's dir after L2. Both served `/healthz` 200 and logged normal cycles. The reflex module is byte-identical between the two (the diff is one copyright line). The new engine, however, sees more: it reads cloudflared errors and correlates incidents, so in live mode it can choose reflexes the old one never chose. See nerve.md §3.0, which recommends dry-run mode first.

**F8. Box-side slip (mine, fixed).** In my first tuner run the old tuner's `--version` inherited no `TUNER_ADDR`, bound its default **127.0.0.1:8650 on the box** (outside my range), and hung the deploy for 60 s until the outer timeout killed it. It was GETs from my own probe only, and nothing on the VPS was involved. The script now gives the deploy the same defaults the VPS has (`DENV`), so a stray `--version` binds the sim's port, and later runs stayed inside 19330-19339.

Smaller notes: the new nerve flagged the 2 ms probe as a request flood (`ddos:nerve-api`), so its DDoS detection works, and the real cutover polls at 1/s. `daemon-reload` does not start a new unit, matching systemd, which is why room needs the explicit `start`.

### Gated Rust test (`sim/deploy-sim/tests/real_deploy.rs`)

`GALION_REAL=1 cargo test --test real_deploy` (port 19335) starts simsup with tuner-old, deploys
tuner-new and asserts a gap under 1000 ms, then deploys `sim-broken` and expects exit 1 and the old
binary back. Without `GALION_REAL` it skips. Run at about 21:22 +02:00: **1 passed, 22.89 s**. In that run the
start limit (F2) fired during the health window (`Start request repeated too quickly`), and the
rollback restart came after the 10 s window had expired, so the test also exercised the trap-recovery path.

## 6. Read-only VPS probe (GET only, nothing written)

```
probed at 2026-10-20T21:03:01+02:00
http://127.0.0.1:8670/healthz 200
http://127.0.0.1:8670/health 404
http://127.0.0.1:8670/ 200
http://127.0.0.1:8650/health 200
http://127.0.0.1:8646/health 200
http://127.0.0.1:4020/health 000
```

Command: `ssh -o BatchMode=yes slavik-vps 'for u in …; do curl -s -m 3 -o /dev/null -w "%{http_code}" "$u"; done'`. The live galion-deploy text was **not** re-read (that would not be a GET). Its content is taken from ws/foundation e43d7ab, whose size (2,830 B) matches what docs/cutover/tuner.md recorded from the VPS. **Preflight must compare `sha256sum /opt/galion/rust/bin/galion-deploy` with `86207af5617720098c820dee4f6d994e4b4dc57d5a46903b76ef9d85e83aea0b`** (e43d7ab).

## 7. Re-run

```
export PATH=/workspace/swarm/bin:$PATH
scripts/real/deploy-sim-build.sh          # builds into /workspace/swarm/sim/deploy-sim/bin
scripts/real/galion-deploy-equiv.sh       # 10/10 SAME
scripts/real/deploy-sim.sh all            # or tuner | nerve | room | desk (about 4 min each)
```
