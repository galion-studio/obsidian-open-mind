<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Remaining Python / Node files on the VPS: inventory and archive plan (wave-4 slot 308)

Generated 2026-10-27 21:52–22:05 CET (+01:00), Europe/Zagreb. Read-only: every probe was
`find`, `ls`, `stat`, `strings`, `grep`, `systemctl status/show/cat`, `ss -ltnp`, `journalctl` reads and
HTTP GET. Nothing was moved, deleted, chmod-ed, started or stopped. Secrets dirs
(`/opt/galion/galion-light/secrets`, `/root/galion-secrets`) were pruned from every scan.
Raw probe outputs: `docs/real-tests/py-archive-plan.md`.

Note: the VPS clock is UTC; all times below are converted to CET (+01:00).

## 1. Headline

- **Nothing on the live path runs Python or Node.** All 16 TCP listeners are Rust/ELF binaries
  (or OS daemons: sshd, postfix, systemd-resolved, cloudflared). The only Python processes are
  OS-owned `fail2ban-server` and `networkd-dispatcher` (Ubuntu packages, out of scope, keep).
- No systemd unit `ExecStart`, timer, or cron line references `python`, `.py`, `node` or `.mjs`.
  The three units whose text mentions Python only do so in comments (see §3).
- 45 528 `.py`/`.mjs` files remain on disk (28 582 inside venvs/site-packages). They are dead
  weight, rollback copies or history, not running code.

## 2. Named files (row 308)

| File | Size / mtime (CET) | Status | Live references | Replacement |
|---|---|---|---|---|
| `/opt/galion/bin/desk-hermes.py` | 7 889 B, 2026-10-10 10:28 | **Rollback copy** since desk cutover | Old ELF `/opt/galion/bin/galion-desk` hardcodes `python3 /opt/galion/bin/desk-hermes.py`, but that ELF is no longer the ExecStart. Drop-in comment in `galion-desk.service.d/rust-desk-hermes.conf` | `/opt/galion/rust/bin/galion-desk` + `DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes` (drop-in written 2026-10-27 19:56:49 CET; service active since 19:56:50 CET) |
| `/opt/galion/bin/galion-desk` (old ELF, Python caller) | ELF, not stripped | Rollback copy | Only the drop-in comment | same as above |
| `/opt/galion/bin/pw.py` (+ `__pycache__/pw.cpython-312.pyc`) | 2 499 B, 2026-10-10 12:22 | Dead; shebang `/opt/galion/venv/bin/python` (Playwright) | `/opt/galion/bin/pw` is now a symlink to `/opt/galion/rust/bin/pw-check` (2026-10-27 20:02 CET). Text mentions only in old `/root/.hermes/sessions/request_dump_one-warm_*.json` (history, Oct 12–18) | `rust/bin/pw-check` |
| `/opt/galion/bin/replicate-picture.py` | 2 969 B, 2026-10-10 18:50 | Dead | `/opt/galion/bin/replicate-picture` symlink → `/opt/galion/rust/bin/replicate-picture` (2026-10-27 20:02 CET) | `rust/bin/replicate-picture` |
| `/opt/galion/hermes/archive/grok-files/artifacts/replicate-picture.py` | 2 969 B (identical size) | Archive duplicate | none | — |
| `/opt/galion/archive/py-units-20261027/` | `link-watch.service` (228 B), `memory-gateway.service` (665 B); dir created 2026-10-27 20:03 CET | Archived unit files; both units are **masked** and inactive | none | `link-watch-rs.service` (active since 2026-10-18), `openmind-memory.service` (active) |

## 3. Units that still mention Python (comments only, verified)

- `galion-one.service`: ExecStart is Rust (`openmind/dist/bin-ws/galion-one`, overridden by
  `50-rust-bin.conf` → `/opt/galion/rust/bin/galion-one`, PID exe confirmed). The legacy Chatterbox
  `tts/worker.py` spawn is gated by `ONE_LEGACY_PY=1` and is off (see `docs/legacy-python.md`).
  Live nerve shows galion-one mouth stuck `loading` for 3.2 h (incident inc-865945dd, 21:30 CET) —
  unrelated to Python, the remote mouth :8002 is the live path.
- `galion-desk.service.d/rust-desk-hermes.conf`: comment "old galion-desk + desk-hermes.py left untouched".
- `link-watch-rs.service`: comment "Rust port of …/link-watch.py".
- `hermes.service`, `link-watch.service`, `memory-gateway.service`: **masked**.

## 4. Bulk trees (no live references found)

| Tree | Files (.py/.mjs, non-venv) | Size | Class |
|---|---|---|---|
| `/srv/box-archive/2026-10-21/` | 2 537 | 11 G | box snapshot archive (keep as archive; separate retention decision) |
| `/opt/galion/galion-light/shadow/hermes-agent` | 7 233 | 114 M | Python Hermes V1 shadow copy |
| `/opt/galion/tooling/hermes-agent` (+ venv 3 483) | 2 121 | 258 M | Python Hermes V1 (hermes.service masked; hermes-rs live) |
| `/opt/galion/from-grok-build/workspace` | 1 143 | 661 M | build history |
| `/opt/galion/hermes/archive` | 1 071 | 267 M | already an archive |
| `/opt/galion/galion-src/*` | ~1 030 | 366 M | old source trees |
| `/opt/galion/crew-skills/*` | ~800 | 24 M | Hermes skill scripts (V1) |
| `/opt/galion/galion-light/tulu` (llama.cpp conversion) | 222 | 4.6 G | model tooling |
| `/opt/galion/arena/homes` | 204 | 96 M | arena agent homes |
| `/opt/galion/openmind/stage` | 148 | — | **inside the protected working copy: never touch (RULES §3)** |
| `/opt/galion/console-dist/server/*.mjs` + `node_modules` | 60 | 1.5 M | old TanStack SSR bundle; console-serve runs `--ui rust` and only reads `--dist` statics |
| `/opt/galion/console-ide` (+ `.vercel`, scripts) | 74 | 340 M | old Node IDE |
| `/opt/galion/galion-light/{scripts,one,guardian,*.py}` | 28 named scripts | — | all ported or dead per `docs/legacy-python.md`; `guardian-tick.timer` runs Rust `rust/bin/guardian-tick` (60 s, last ok 21:54:54 CET) |
| venvs `/opt/galion/{venv,stt/venv,tts/venv,vision/venv}` | 16 621 | 787 M | no unit/process uses them |
| Node runtime `/opt/galion/tooling/node` (`/usr/local/bin/node` symlink) | — | — | no process uses it |

## 5. Archive / removal plan (after 7 clean days, docs only — NO deletes in this slot)

"Clean day" = 24 h in which: the replacing Rust service stays `active`, nerve `/state` raises no
incident for it, no journal line mentions the old file, and `ps` shows no python/node except the
OS daemons. Clock per item starts at the cutover time.

| Phase | Earliest date (CET) | Items | Action (needs owner OK; see `docs/swarm/plans/vps-ops.md` — "Slavik approves each kill; the archive move; each unit-file edit") |
|---|---|---|---|
| A | 2026-11-03 20:05 | `bin/desk-hermes.py`, old `bin/galion-desk` ELF, `bin/pw.py` + `__pycache__`, `bin/replicate-picture.py` (cutovers 2026-10-27 19:56–20:03) | `mv` into `/opt/galion/archive/py-bin-20261103/` with `sha256sum` manifest; then remove the comment line in the desk drop-in only if folding it into the unit (separate unit-edit approval). Rollback = `mv` back. |
| B | 2026-11-03 20:05 | `archive/py-units-20261027/*` | keep the two masked unit files 30 more days, then delete together with `systemctl unmask`-free cleanup (units stay masked until files are gone; then `daemon-reload`). |
| C | any time (already >7 days: hermes-rs, link-watch-rs live since ≤ 2026-10-18) | `tooling/hermes-agent` + venv, `galion-light/shadow/hermes-agent`, `crew-skills`, `galion-light/{scripts,one,guardian}/*.py`, 4 venvs, `console-dist/server` + `node_modules`, `console-ide`, `tooling/node` | Pack each tree to `tar --zstd` under `/srv/box-archive/2026-11-py/` with manifest + sha256, verify by listing the tarball, then remove the source tree. Order: venvs → Hermes V1 → console Node → scripts. One tree per change, nerve watched 15 min after each. Frees ≈ 1.6 G. |
| D | after C | `from-grok-build`, `galion-src`, `hermes/archive`, `arena/homes`, `galion-light/tulu` | Retention decision for Slavik (history/model tooling, not code paths). |
| — | never (this plan) | `/opt/galion/openmind/stage` (RULES §3), OS Python (fail2ban, networkd-dispatcher), `/srv/box-archive/2026-10-21` | out of scope |

Pre-check before every phase (read-only, repeat of §1): listeners → all Rust; `grep` units/timers/cron
for `python|\.py|node|\.mjs` → comments only; `ps` → only OS daemons; nerve `/state` clean for the
replacing service.

## 6. Open points

- Phase A/B/C deletes are **not** done and need owner approval (outside this row's read-only scope).
- `galion-one` binary contains the string `node_modules`: confirmed in source as a directory-walk
  skip filter (`crates/galion-one/src/main.rs:2436,2474,2541`), not a Node call.
- Console Node bundle not served: GET 127.0.0.1:8080 `/server/index.mjs` 404, `/node_modules/` 404, `/` 302 (22:04 CET). Still re-check the console-serve source before Phase C.
- `/srv/box-archive/2026-10-21` (11 G) retention is a separate owner decision.
