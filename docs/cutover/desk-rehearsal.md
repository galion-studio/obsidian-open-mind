<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Desk cutover: box rehearsal of docs/cutover/desk.md (swarm/desk-linkwatch a54c792)

- Rehearsed: 2026-10-20 21:14:56–21:15:45 CEST (+02:00), swarm slot 33, branch `swarm/deploy-sim`. **8/8 checks pass.** Full log: `docs/real-tests/deploy-sim/desk-run5.log`.
- Re-run: `scripts/real/deploy-sim-build.sh && scripts/real/deploy-sim.sh desk` (port 19333; all desk probe targets point at the dead port 19339).
- The plan (desk.md §5) lives on `swarm/desk-linkwatch`. Nothing touched the VPS. A read-only GET at 21:03 CEST: `127.0.0.1:8646/health` → 200.

## Sim setup and its limits

| VPS (desk.md §1) | sim |
|---|---|
| `galion-desk.service`, `/opt/galion/bin/galion-desk`, no `Environment=` (defaults = production paths) | `ExecStart=$R/opt/galion/bin/galion-desk` with `DESK_BIND=127.0.0.1:19333`, `DESK_STATE_DIR`/`DESK_ONE_DIR`/`DESK_HERMES_DIR` under `$R`, `DESK_OLLAMA` and every `DESK_PROBE_*` set to 127.0.0.1:19339 (dead), `OPENMIND_CONFIG` set to a missing file. `Restart=always`, `RestartSec=2` (**assumed**: the live unit's restart lines were not read in this task) |
| live binary: the Oct 10 glibc build of `/workspace/galion-light/desk-rs/main.rs` (not in git), sha `555cc443…11e6` | stand-in `desk-old` = main 13a296c `crates/galion-desk` (it also ignores argv and `expect("bind")`s) |
| new: swarm/desk-linkwatch a54c792, x86_64 musl | the same source, sha `b46b148f…fce5` (desk.md records `b35e2e7b…` for its build; a different toolchain or path, so **rebuild from merged main and record the sha**) |
| mindpack via desk-hermes | no desk-hermes in the sim (`mindpack` = `{}`). The value-level mindpack parity (desk.md step 5) can only be checked on the VPS shadow run. |

## Step by step

| desk.md step | result in the sim |
|---|---|
| before | old desk `{"ok":true}`, `/desk` has 22 top-level keys |
| preflight: `galion-desk.new --help` | prints usage, exit 0, no bind (safe) |
| preflight: `galion-desk.new --version` while the desk holds the port | **not handled**: it resolves settings and tries to bind. It exits 1 after 4 ms only because the port is busy. |
| 8. `cp -p galion-desk galion-desk.oct10` | ok |
| 9–10. install the new binary + `systemctl restart` | **downtime 4 ms** |
| 11. verify | new sha running, `/health` `{"ok":true}`, `/desk` keys `foundation,at,line,model,vram,ctx,hermes,one,tunnel,voice,heard,said,turns,waking,resting,hold,services,board,events,mindpack,focus,notes` (22, same order as live) |
| 13. rollback: install `.oct10` + restart | old sha back, healthy, **2 ms** |
| alternative: `galion-deploy galion-desk <new> <health>` | exit 0, **2 ms** |
| galion-deploy with a broken binary | automatic rollback to the new desk, 20.2 s of failed health |
| galion-deploy L2 with the backup | exit 0, old sha back, 2 ms (the old desk's `--version` aborts on the busy port: `Aborted` in the output, harmless) |
| **trap:** desk **down**, then `galion-deploy galion-desk <new>` | **the deploy hangs, and `galion-desk.new --version` serves `/health` 200 on the desk port, outside the unit.** It hung until the outer timeout (20 s) killed it. |
| the same with `GALION_DEPLOY_VERSION_TIMEOUT=5` | completes, exit 0, new sha, healthy |

## What this changes in desk.md

1. **Never run galion-deploy for galion-desk while the desk is down.** Neither the live nor the new galion-desk handles `--version`, and the live galion-deploy waits for it without a timeout. If the desk is down, use the manual steps 9–10 (install + restart) instead. Better: add `--version` to galion-desk (a 3-line change on swarm/desk-linkwatch, next to the existing `--help`), so both routes are safe.
2. Steps 8–13 work as written. The manual swap and the manual rollback each cost 2–4 ms of refused connections in the sim (add systemd's restart overhead on the VPS).
3. If galion-deploy is used (it has a `[galion-desk]=http://127.0.0.1:8646/health` entry), add `systemctl reset-failed galion-desk` before it, and check `is-active` after any non-zero exit (start-limit trap and exit-code ambiguity, `docs/real-tests/deploy-sim.md` F2/F3).
4. Before the go: read the live unit's `Restart=`/`RestartSec=`/`StartLimit*` lines (`systemctl cat galion-desk`, read-only). The sim assumed `Restart=always`, `RestartSec=2`.
