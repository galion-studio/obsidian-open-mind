<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-rs R3 crash loop, 2026-10-27 19:45 CET: root cause and fix (wave-4 slot 355)

Written 2026-10-28 04:20 CET (+01:00). Branch `swarm/hermes-rs-crashloop-r3` off
`swarm/hermes-offhost-keyrot` 6845c350. VPS read-only for this work: nothing live was
restarted, changed or installed.

## What happened (VPS journal, read 2026-10-28 04:05 CET)

| time (CET +01:00) | event |
|---|---|
| 2026-10-27 19:07:29 | t291u deploy: `90-deploy-t291u.conf`, binary `hermes-gateway-b7fbbc78`, coding runtime OFF. Healthy. |
| 19:45:27 | Operator test (overnight-test-hermes t311u, "AUTO-ACCEPT") adds a coding drop-in with `HERMES_RS_CODING_WORKTREE=/var/lib/hermes-rs/coding-worktree` and restarts. |
| 19:45:27 to 19:46:10 | 13 starts, each: `coding: config error: worktree /var/lib/hermes-rs/coding-worktree overlaps the denied path /var/lib`, `status=2/INVALIDARGUMENT`. systemd restart counter reaches 13 (RestartSec=3). Cortex dec-35 restarted once (R3), dec-40 opened the circuit breaker (R2), handoff to Hermes failed (Hermes was the broken service). |
| 19:46:13 | Drop-in removed, started, listening. |
| 19:46:15 | Retry with worktree `/srv/hermes-coding-worktree`, state `/srv/hermes-coding-state`: `coding: refusing to start: mkdir /srv/hermes-coding-state/stage: Read-only file system (os error 30)`, exit 2 (ProtectSystem=strict, `/srv` not in ReadWritePaths). |
| 19:46:18 | Drop-in removed again; MainPID 255539, healthy since, NRestarts 0 (also at 2026-10-28 04:15 CET; nerve: healthy, restarts_1h 0). |

## Root cause

A **config** error, made likely by a **staging/code mismatch**, and not caught because
of a **check-config gap**:

1. `hermes-tools` refuses any worktree under `/var/lib` (`DENY_ROOTS`, since 8ccdfcc4).
   The unit only lets the gateway write in `/var/lib/hermes-rs` (ProtectSystem=strict,
   `ReadWritePaths=/var/lib/hermes-rs`). So there was **no valid place** for a worktree
   under the current unit.
2. The repo's own staged drop-in `deploy/staged/hermes-rs.service.d/coding-tools.conf`
   (slot 17, same commit 8ccdfcc4) set the worktree to `/var/lib/hermes-rs/work/openmind`:
   it could **never** have started. The 19:45 operator path followed the same pattern.
3. `hermes-gateway --check-config` returned before the coding block, so it reported ok for
   a config that exits 2 at start. Proven: the base 6845c350 build and the **live**
   `hermes-gateway-b7fbbc78` both exit 0 on `--check-config` with a `/var/lib` worktree
   (VPS, as nobody, 2026-10-28 04:15 CET).
4. Exit 2 is permanent for these errors, but the unit has `Restart=on-failure` and no
   `RestartPreventExitStatus`, so systemd looped (13 restarts in 46 s) and the cortex
   saw a crash loop.

Not a code crash: no panic, no credential or port problem (bind errors exit 1; the key
check passed; the port was free at 19:46:13).

## Fix (this branch)

Code (hermes-tools + hermes-gateway):
- `hermes-gateway --check-config` now also runs the coding env validation and a
  non-mutating write check of the coding paths (`state/stage`, `target`, audit dir:
  read-only mount via `statvfs` ST_RDONLY, else `access(W_OK)`). Prints `coding=off`,
  `coding=on worktree=... state=...`, or `coding=error` + exit 2. Creates nothing and
  runs no namespace/Landlock probe (start-up preflight still does).
- Deny-overlap and mkdir errors now carry an actionable hint (dedicated path outside
  the deny list + ReadWritePaths; this doc).
- New opt-in `HERMES_RS_CODING_ON_ERROR=disable`: a bad coding config or preflight
  refusal logs the reason and starts **without** coding tools (chat keeps serving;
  the tools fail closed). Default stays `exit` (old behaviour).
- New `worktree_path_ok` (path-only half of `worktree_ok`) for tests.

Staged config (NOT installed):
- `deploy/staged/hermes-rs.service.d/coding-tools.conf`: worktree
  `/srv/hermes-coding/worktree`, state `/srv/hermes-coding/state`,
  `ReadWritePaths=-/srv/hermes-coding`, `HERMES_RS_CODING_ON_ERROR=disable`.
- New `deploy/staged/hermes-rs.service.d/40-config-no-loop.conf`:
  `RestartPreventExitStatus=2` (stop after the first permanent config error).
- Regression test `crates/hermes-tools/tests/staged_dropin.rs`: the staged drop-in must
  pass the code's deny list and its paths must be inside the unit's ReadWritePaths.

## Runbook for the next coding enable (owner-approved only)

1. (Slot 397: superseded by `docs/ops/hermes-coding-dir-stage.md` runbook 1a-1c, which
   creates the folders via the staged tmpfiles.d file.) `install -d -m 0700 /srv/hermes-coding /srv/hermes-coding/state`; clone the dedicated
   worktree to `/srv/hermes-coding/worktree` (never `/opt/galion/openmind`).
2. Check with the exact env, **before** any restart (a fixed binary is needed; the live
   b7fbbc78 cannot see coding errors in `--check-config`):
   `systemd-run --wait --pipe --collect -p ProtectSystem=strict -p ReadWritePaths=/var/lib/hermes-rs -p ReadWritePaths=-/srv/hermes-coding -p EnvironmentFile=/opt/galion/hermes/.env -p Environment=HERMES_RS_CODING_WORKTREE=/srv/hermes-coding/worktree -p Environment=HERMES_RS_CODING_BRANCH=hermes/ticket -p Environment=HERMES_RS_CODING_STATE=/srv/hermes-coding/state <binary> --check-config`
   It must print `coding=on ...` and exit 0. (Running it inside the same ProtectSystem
   sandbox is what makes the read-only check meaningful; plain shell has no RO view.)
3. Install the drop-in(s), `systemctl daemon-reload`, restart at an approved time, watch
   `journalctl -u hermes-rs -n 20` and nerve `/state` (hermes-rs healthy, restarts_1h 0).
4. Rollback: remove the coding drop-in, `systemctl daemon-reload`, restart.

Note: on this PVE kernel Landlock is absent (`/sys/kernel/security/lsm` =
lockdown,capability,yama,apparmor), so the preflight still refuses coding unless
`HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` is set. With `ON_ERROR=disable` that refusal
leaves chat up and coding off, which is the safe outcome.

## Owner decisions

- **D-355-1** Install `40-config-no-loop.conf` (`RestartPreventExitStatus=2`)? Yes = a
  permanent config error stops the unit once (failed, reason in journal) instead of
  13 restarts + cortex breaker; trade-off: a self-clearing session-store error (also
  exit 2) is no longer retried. Recommended: yes.
- **D-355-2** Use `HERMES_RS_CODING_ON_ERROR=disable` for the live unit? Yes = chat stays
  up when the coding config is bad (coding silently off except the log line). No =
  old exit-2 behaviour. Recommended: yes, with D-355-1.
- **D-355-3** Where the coding worktree lives: (a) `/srv/hermes-coding` + one extra
  ReadWritePaths (staged here, no code security change); (b) narrow the `/var/lib`
  deny to allow only a subdir of the gateway's own StateDirectory (keeps the unit as is,
  but the worktree would sit next to `rs-state.db`/`rs-spend.json`, which the sandbox
  must then hide; not implemented). Recommended: (a).
- **D-355-4** Leftovers of the 19:45 test on the VPS (not touched, VPS read-only):
  `/srv/hermes-coding-worktree`, `/srv/hermes-coding-worktree.tmpmove`,
  `/srv/hermes-coding-state`, `/srv/hermes-coding-audit.jsonl`,
  `/var/lib/hermes-rs/coding-state`, `/var/lib/hermes-rs/coding-audit.jsonl` (all root,
  empty or a README clone). Remove or keep.
- Land order: this branch is off 6845c350, which does not have the survival-s47
  hermes-tools changes in the live b7fbbc78 (workdir quota, hide /opt/galion). The fix
  touches `config.rs` and gateway `main.rs` only in small hunks, but `git merge-tree`
  b7fbbc78 + this branch already conflicts in gateway `main.rs`, `hermes-rs/src/api.rs`
  and `docs/hermes-tools.md` (mostly the existing offhost vs survival-s47 divergence).
  Port these hunks onto the b7fbbc78 line before any deploy.
