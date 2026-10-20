<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Tuner cutover: box rehearsal of docs/cutover/tuner.md

- Rehearsed: 2026-10-20 21:14–21:19 CEST (+02:00), swarm slot 33, branch `swarm/deploy-sim`. Run 5: **21/21 checks pass**. Full log: `docs/real-tests/deploy-sim/tuner-run5.log`.
- Re-run: `scripts/real/deploy-sim-build.sh && scripts/real/deploy-sim.sh tuner` (about 4 min, port 19330).
- Nothing touched the VPS. The box has no systemd (PID 1 is tini), so `simsup` played systemd. galion-deploy ran its real code with `GALION_DEPLOY_SM=direct`.

## Sim setup vs the VPS

| VPS (tuner.md §1) | sim |
|---|---|
| `/etc/systemd/system/tuner.service`: `ExecStart=/opt/galion/tuner/bin/tuner`, `TUNER_ADDR=127.0.0.1:8650`, `TUNER_ROOT=/opt/galion/tuner/www`, `Restart=always`, `RestartSec=2`, `Nice=10`, `NoNewPrivileges=true`, systemd default start limit (5 per 10 s) | the same text under `$R/units/tuner.service` with `$R/opt/...` paths and port 19330. `Nice`/`NoNewPrivileges` are parsed and not enforced. The start limit is modelled. |
| live binary `ecad72a4…dd25` (hand build of `/opt/galion/tuner/src/main.rs`) | `tuner-old` = main 13a296c `crates/tuner`, the same logic: ignores argv, `expect("bind")` |
| new `47a17df2…1370` | the same file (rebuilt from this branch, byte-identical to the artefact) |
| www root: the four files | the same four files from `crates/tuner/www` |
| galion-deploy 2,830 B | `deploy/foundation/galion-deploy` (systemd path proven identical, `scripts/real/galion-deploy-equiv.sh`). The stub `galion-backup.sh` sleeps 3 s like the real snapshot. |
| root shell env: no `TUNER_*` set, so a stray `tuner` binds the default 8650, the live port | the deploy gets `TUNER_ADDR=127.0.0.1:19330 TUNER_ROOT=…`, so a stray `tuner` binds the sim's live port, exactly as on the VPS |

## Step by step (tuner.md section → result)

| § | step | result |
|---|---|---|
| 3.1 | install -d 0700, copy artefact + SHA256SUMS, `sha256sum -c --ignore-missing`, chmod | `tuner-x86_64-unknown-linux-musl: OK` |
| 3.2 | installed sha, is-active, /health, `--version`, `--check` | `tuner 0.1.0` (no bind); `--check`: 4× ok, exit 0 |
| 3.3 | `galion-deploy tuner <new> <health>` | exit 0, 4.1 s total; **downtime 3 ms** (1 refused try at 2 ms spacing) |
| 3.4 | running exe sha, /health, `/` size, `/nope`, journal, full route diff | exe `47a17df2…`; `200 2356`; 404; **21/21 routes byte-identical to `crates/tuner/tests/fixtures/live/`** (the live VPS bytes) |
| 4 manual (L2) | `galion-deploy tuner <backup> <health>` | exit 0, old sha back, **2 ms** |
| 4 last resort (L3) | install .new + mv + restart + curl | old sha back, healthy, **2 ms** |
| 4 automatic (L1) | deploy `sim-broken` (exits 1 at start) | `health failed, rolling back` → `rolled back ok`, exit 1, 20.2 s of failed health |
| 4 automatic (L1) | deploy a truncated ELF (first 64 KiB of the new tuner, segfaults) | rolled back ok, exit 1, 20.2 s |
| 4 automatic (L1) | 3× truncated ELF with `GALION_DEPLOY_RESET_FAILED=1` | 3/3 rolled back ok |
| — | hang A: `sim-hang` (hangs on `--version`), script as shipped | the deploy never returns (killed by the outer timeout after 25 s). The service is untouched: no backup taken, nothing installed. |
| — | hang B: the same with `GALION_DEPLOY_VERSION_TIMEOUT=5` | the version step times out, install, health fails, automatic rollback, exit 1 |
| — | hang C: new tuner installed but **down**, then the L2 manual rollback | **the deploy hangs, and the OLD binary's `--version` serves 127.0.0.1:19330 (`GET /health` 200) while the unit is `inactive`** |
| — | hang C with `GALION_DEPLOY_VERSION_TIMEOUT=5` | completes, exit 0, old sha, healthy, no stray process |

## What this changes in tuner.md (apply before the go)

1. **Section 4, manual rollback (L2):** only while `systemctl is-active tuner` says `active`. If the unit is down or failed, use the **last-resort** command instead. The live galion-deploy has no timeout on `"$NEW" --version`, and the old tuner ignores the argument: with 8650 free it binds and serves forever, outside systemd, and the deploy never gets to the install (hang C).
2. **Last resort:** add `systemctl reset-failed tuner` before `systemctl restart tuner`. A crash-looping new binary can exhaust the default start limit (5 per 10 s with `RestartSec=2`). Then a plain restart is refused: `start of the service was attempted too often`.
3. **After any non-zero galion-deploy exit**, read `systemctl is-active tuner` and `sha256sum /opt/galion/tuner/bin/tuner`. Exit 1 means `rolled back ok` but also "died at a refused rollback restart" (start-limit trap, seen in 3 of 8 default L1 attempts in the sim). Recovery: `systemctl reset-failed tuner && systemctl restart tuner`.
4. **3.3:** run galion-deploy under `nohup … > /var/tmp/galion-cutover/deploy-tuner.log 2>&1` (or tmux). If it is interrupted after the install, there is no rollback.
5. **Section 5 (downtime):** confirmed. The binaries themselves account for 2–3 ms of refused connections. Add systemd's restart overhead (tens of ms). The full run takes about 4–5 s.
6. Cosmetic: during L2 the old binary's `--version` panics on the busy port (SIGABRT, `Aborted` in the deploy output; on the VPS, possibly a systemd-coredump entry). Harmless.
