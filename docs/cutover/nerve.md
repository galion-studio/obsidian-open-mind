<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve cutover plan: galion-nerve → openmind-nerve (8670)

- Status (refreshed 2026-10-21 17:30 CEST (+02:00), wave-4 slot 203): **EXECUTED, but not as planned below.** galion-nerve has run openmind-nerve since **2026-10-21 09:43:54 CEST (+02:00)**.
  The swap used an ExecStart drop-in `/etc/systemd/system/galion-nerve.service.d/50-rust-bin.conf` → `/opt/galion/rust/bin/openmind-nerve-aa77864 serve` (sha256 `f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68`, source aa77864c2744fad56d11e84476d5bfab1fd25bf7), not the galion-deploy swap of §3.3.
  The old binary `/opt/galion/rust/bin/galion-nerve` (`3b150dea…`) is untouched. Approved by Slavik ~09:35 (t175), relayed by Mate. Record: `swarm/nerve-eventlog-deploy@0ddce9bafe29771ca5dd1e16d2c965a5ba95155a:docs/ops/nerve-eventlog-nerveurl-20261021.md`; index [README.md](README.md) §2 C10a/C10c.
  Reflex dry-run (§3.0) went live at **17:08:27** by drop-in `70-nerve-dryrun.conf` (`NERVE_DRY_RUN=1`, Mate 17:04 t193, D-T32-R1; CALL-RECORD 17:15:45; record `swarm/nerve-dryrun-live@80974b951d6a0fc38a43b172387d65da46f54840:docs/cutover/nerve-dryrun-live.md`). At 17:22 `/state` showed `mode dry-run`.
  Rollback now: `rm /etc/systemd/system/galion-nerve.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart galion-nerve` (dry-run only: remove `70-nerve-dryrun.conf` the same way).
  Sections 1–6 below are the 2026-10-20 plan, kept as written for the record.
- Original status (2026-10-20): PREPARED AND REHEARSED ON THE BOX, NOT EXECUTED. Needs Slavik's approval. Nothing has been copied to the VPS, and no unit was restarted or edited.
- Prepared: 2026-10-20 21:25 CEST (+02:00) by swarm slot 33, branch `swarm/deploy-sim`.
- Rehearsal: `scripts/real/deploy-sim.sh nerve` (bash, removed on swarm/deploy-rust; now `GALION_REAL=1 cargo test --release -p galion-deploy --test real_rehearsal rehearsal_nerve`, see docs/real-tests/deploy-rust.md), 18/18 checks (`docs/real-tests/deploy-sim.md`, full log `docs/real-tests/deploy-sim/nerve-run5.log`).
- Do it **after** the tuner cutover, never in the same session (one service at a time, docs/next.md).
- If the cutover runs on or after 2026-10-25 03:00, timestamps are CET (+01:00).

## 1. What runs today

| Item | Value | Source |
|---|---|---|
| Unit | `galion-nerve.service`. Expected text = `deploy/nerve/galion-nerve.service` of ws/nerve a63b730 (identical to swarm/nerve 5802d0e's): `ExecStart=/opt/galion/rust/bin/galion-nerve serve`, `NERVE_ADDR=127.0.0.1:8670`, `NERVE_DATA_DIR=/var/lib/galion-nerve`, `NERVE_DRY_RUN=0`, `Restart=always`, `RestartSec=10`, **`StartLimitIntervalSec=600`, `StartLimitBurst=5`**, `MemoryMax=64M`, `CPUQuota=15%`, `ProtectSystem=strict`, `ReadWritePaths=/var/lib/galion-nerve` | repo. **Not read from the VPS in this task (GET-only rule). Preflight 3.2 reads it.** |
| Listener | 127.0.0.1:8670 | earlier read-only probes (docs/real-tests/nerve.md on swarm/nerve) |
| HTTP | `/healthz` → 200, **`/health` → 404**, `/` → 200 | read-only GET from the box over ssh, 2026-10-20 21:03:01 CEST |
| Reflex mode | `live` (self-eval at 19:24:35 CEST: "cycle: 5380, mode: live") | swarm/nerve docs/real-tests/nerve.md |
| Binary | `/opt/galion/rust/bin/galion-nerve`, built from the ws/nerve `crates/galion-nerve` line. **The live sha256 is not recorded anywhere. Preflight records it.** | |
| Deploy tool | `/opt/galion/rust/bin/galion-deploy`, expected sha256 `86207af5617720098c820dee4f6d994e4b4dc57d5a46903b76ef9d85e83aea0b` (ws/foundation e43d7ab, 2,830 B). **Its health table has no galion-nerve entry.** | repo + tuner.md |
| Dependants | galion-desk does not read 8670. The nerve is the hub other things POST signals to (nerve-client in hermes-rs/openmind-memory: no-op unless `NERVE_URL` is set). Nothing user-facing. | ws/nerve |

## 2. What replaces it

`openmind-nerve` from swarm/nerve 5802d0e (`crates/openmind-nerve`), installed under the **same path and unit**. The unit is not edited: `ExecStart=… serve`, and every env var the unit sets means the same to the new binary.

| artefact (box only) | bytes | sha256 |
|---|---|---|
| `/workspace/swarm/artifacts/nerve/openmind-nerve-x86_64-unknown-linux-musl` | 1,110,120 | `ad9a5b29d8ce71337c9945e10e8117700da6c4b3dd3d5aeb1f2f96e183bd106a` |

Static-pie, stripped, cargo 1.99.1. It is **reproducible**: two builds from different worktrees and target dirs gave the same sha. SOURCE-OF-TRUTH rule 6 says to rebuild from the merged main commit and deploy only if the sha matches, or else re-run the rehearsal with the new sha.

What changes for the operator:
- New API: `/health` (JSON with `service: openmind-nerve`, version, score), `/api/v1/*`, `/incidents`, `/cortex`, `/ddos`, `/signals`. Old routes (`/`, `/healthz`, `/state`, `/self-eval(.md)`, `/alerts`, `/audit`, `POST /signal`) stay. `POST /event` is off unless `NERVE_EVENT_LOG` is set (it is not set in the unit).
- A new engine: incident correlation, cortex (vulnerabilities, bleeding, DDoS), score, journald priority parsing of zerolog `ERR` lines. On the 19:23 capture it says **FAIL 61/100**, where the old one said PASS. The dashboard and self-eval will look worse from the first cycle. That is expected and correct (swarm/nerve docs/real-tests/nerve.md §2).
- The reflex module (`reflex.rs`) is **byte-identical** apart from one copyright line: the same allowlist, deny list and rate limits. The *inputs* differ, though. The new engine sees cloudflared errors and refused upstreams that the old one missed, so in live mode it may pick allowlisted restarts that the old nerve never picked (for example `cloudflared.service`, 1/h). See §3.0.
- Data dir: the same three files (`alerts.json`, `self-eval`, `state.json`). The rehearsal started each binary on the other's data dir. Both came up and served `/healthz` 200.

## 3. The cutover (2026-10-20 plan; the live cutover on 2026-10-21 differed, see Status at the top)

### 3.0 Reflexes to dry-run first (recommended; a state change, so part of the approval)

```
ssh slavik-vps 'cat /var/lib/galion-nerve/mode 2>/dev/null; echo dry-run > /var/lib/galion-nerve/mode'
```
The old nerve honours the same switch. Keep dry-run for 24 h after the swap, read `/audit` for the decisions the new engine *would* have taken, then decide on `echo live > …`. Skipping this step means the new engine acts on day one.

### 3.1 Copy (needs approval)

```
ssh slavik-vps 'install -d -m 0700 /var/tmp/galion-cutover'
scp /workspace/swarm/artifacts/nerve/openmind-nerve-x86_64-unknown-linux-musl /workspace/swarm/artifacts/nerve/SHA256SUMS slavik-vps:/var/tmp/galion-cutover/
ssh slavik-vps 'cd /var/tmp/galion-cutover && sha256sum -c --ignore-missing SHA256SUMS && chmod 0755 openmind-nerve-x86_64-unknown-linux-musl'
```

### 3.2 Preflight (read-only)

```
ssh slavik-vps '
  sha256sum /opt/galion/rust/bin/galion-nerve           # RECORD it: this is the rollback target
  sha256sum /opt/galion/rust/bin/galion-deploy          # must be 86207af5…aea0b (else re-read the script and re-check F1-F3)
  systemctl cat galion-nerve                            # compare with section 1; any drift is a no-go until understood
  systemctl is-active galion-nerve                      # active
  systemctl show galion-nerve -p NRestarts -p ActiveEnterTimestamp
  curl -s -m 3 http://127.0.0.1:8670/healthz            # ok
  /var/tmp/galion-cutover/openmind-nerve-x86_64-unknown-linux-musl --version   # openmind-nerve 0.1.0 (exits, no bind)
'
```

### 3.3 The cutover command (exactly this)

```
ssh slavik-vps 'systemctl reset-failed galion-nerve; nohup /opt/galion/rust/bin/galion-deploy galion-nerve /var/tmp/galion-cutover/openmind-nerve-x86_64-unknown-linux-musl http://127.0.0.1:8670/healthz > /var/tmp/galion-cutover/deploy-nerve.log 2>&1; echo exit=$?; cat /var/tmp/galion-cutover/deploy-nerve.log'
```
- **The health URL is mandatory and must be `/healthz`** (F4). Without it, galion-deploy only checks `is-active` for 5 s. With `/health`, a rollback to the old nerve is judged by a route the old nerve does not have (404), and galion-deploy reports `ROLLBACK ALSO UNHEALTHY` although the old nerve serves. Rehearsed.
- **`reset-failed` first** (F2): the unit allows 5 starts per 600 s. Every galion-deploy run uses at least one start, and a crash-looping new binary uses more. If the window fills, systemd refuses the rollback restart, and galion-deploy dies under `set -e` with the old binary on disk and the unit **failed**. In the rehearsal that happened deterministically on the 6th start (swap, L2, forward, L3, broken deploy). `reset-failed` on an active unit only clears the counter.
- `nohup` plus a log file (F6): an interrupted galion-deploy after the install leaves the new binary in place with no rollback.
- `--version` returns at once (openmind-nerve handles it before reading the config), so F1 (the version hang) does not apply to this binary.

### 3.4 Health after the switch (one read each)

```
ssh slavik-vps '
  systemctl is-active galion-nerve; systemctl show galion-nerve -p MainPID -p NRestarts
  sha256sum /opt/galion/rust/bin/galion-nerve                         # ad9a5b29…106a
  curl -s -m 3 http://127.0.0.1:8670/healthz                           # ok
  curl -s -m 3 http://127.0.0.1:8670/health                            # {"service":"openmind-nerve","status":"ok",...}
  curl -s -m 3 -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8670/state        # 200
  journalctl -u galion-nerve -n 5 --no-pager -o cat                    # "<6>openmind-nerve 0.1.0 listening on 127.0.0.1:8670 ... mode=dry-run"
  cat /var/lib/galion-nerve/mode                                       # dry-run (if 3.0 was done)
'
```
Expect `ALERT` lines and a lower score from the first cycle (§2). That is the new engine reporting what it sees, not a failure.

## 4. Rollback (all three rehearsed)

**Always check the unit after any non-zero exit** (F3): `rolled back ok`, "died at a refused restart" and other failures all exit 1.
```
systemctl is-active galion-nerve; sha256sum /opt/galion/rust/bin/galion-nerve
```

- **L1 automatic:** galion-deploy restores the backup if `/healthz` is not 2xx within 20 s. Rehearsed with a broken binary: rolled back ok, 20.2 s of failed health. If the unit ends up `failed` (start-limit trap), recover with `systemctl reset-failed galion-nerve && systemctl restart galion-nerve` (54 ms in the rehearsal).
- **L2 manual (new nerve healthy but wrong):**
  `systemctl reset-failed galion-nerve; /opt/galion/rust/bin/galion-deploy galion-nerve /var/backups/galion/deploy/galion-nerve-<STAMP>.bin http://127.0.0.1:8670/healthz`
  `<STAMP>` is in the `backup=` field of the deploy log. Rehearsed: exit 0 and 4 ms of downtime, and the old nerve came up on the new nerve's data dir.
- **L3 last resort:** `install -m 0755 /var/backups/galion/deploy/galion-nerve-<STAMP>.bin /opt/galion/rust/bin/galion-nerve.new && mv -f /opt/galion/rust/bin/galion-nerve.new /opt/galion/rust/bin/galion-nerve && systemctl reset-failed galion-nerve && systemctl restart galion-nerve && curl -s http://127.0.0.1:8670/healthz`. Rehearsed: 2 ms.
- Check after any rollback: the sha equals the one recorded in 3.2, and `/healthz` is ok.
- The data dir needs no restore (compatible both ways). Restore the mode file to its 3.2 value if 3.0 was done and you are abandoning the cutover.

## 5. Expected downtime

- **Planned: a few ms of refused connections on 127.0.0.1:8670** (rehearsal: 2 ms swap, 4 ms L2, 2 ms L3), plus systemd's restart overhead (tens of ms). The whole galion-deploy run takes about 5 s (3 s data backup, the first health poll after 1 s), and the old nerve serves for most of it.
- Effect of the gap: signal POSTs from nerve-clients during those ms fail (clients are best-effort and the next heartbeat arrives within 60 s). One probe cycle may be lost.
- **Worst case:** about 20 s of failed health and then the automatic rollback. With the start-limit trap, the nerve stays down until `reset-failed` + `restart` (manual).

## 6. Go / no-go

Go only when every box is ticked:
- [ ] Slavik approves *this one* service (galion-nerve, 8670) and the 3.0 dry-run step, recorded in `docs/cutover-approval.md`.
- [ ] swarm/nerve (and this branch, for the rehearsal tooling) are merged through a PR with the gate green.
- [ ] Rebuilt from merged main: the sha matches `ad9a5b29…106a`, or the rehearsal was re-run with the new sha (`GALION_REAL=1 GALION_REAL_BUILD=1 cargo test --release -p galion-deploy --test real_build`, then `DEPLOY_SIM_BIN=/workspace/swarm/sim/deploy-rust/bin GALION_REAL=1 cargo test --release -p galion-deploy --test real_rehearsal rehearsal_nerve`).
- [ ] Copied and `sha256sum -c` OK (3.1).
- [ ] Preflight (3.2): the live unit matches section 1, the galion-deploy sha matches, the unit is active, `/healthz` is ok, `--version` prints `openmind-nerve 0.1.0`, and the live binary sha is recorded.
- [ ] No other cutover in the same session. The tuner cutover was done (or abandoned) in an earlier session.
- [ ] Not within 5 min of `galion-backup.timer`.
- [ ] The rollback commands of section 4 are at hand, with `/healthz`.

**No-go if:** the unit text drifted, the galion-deploy sha differs, `/healthz` is not ok before the swap, the merged-main build differs and was not re-rehearsed, or the 3.0 decision was not made.

## 7. Open points (separate approvals)

- Adding `[galion-nerve]=http://127.0.0.1:8670/healthz` to galion-deploy's health table, and installing the pluggable galion-deploy of this branch on the VPS with `GALION_DEPLOY_VERSION_TIMEOUT` / `GALION_DEPLOY_RESET_FAILED` turned on. Both are changes to a live tool.
- The live galion-nerve's own rollback reflex runs galion-deploy through `systemd-run`. It inherits F2/F3 (a refused restart looks like `rolled back ok` by exit code). Worth a look once the new nerve is live.
- swarm/deploy-rust: a Rust `galion-deploy` (crates/galion-deploy) fixes F1-F6 (bounded `--version`, reset-failed by default, exit codes 0/1/2/3/4/5/6, galion-nerve -> `/healthz` in the table and rollback judged on the unit's own path, first install, signal-safe with a rollback journal). Not installed anywhere. **Owner decision:** whether and when it replaces `/opt/galion/rust/bin/galion-deploy` (see docs/real-tests/deploy-rust.md). Until then this plan stays as written for the live bash script.
- Moving reflex mode from `dry-run` back to `live` after 24 h, based on `/audit`.
