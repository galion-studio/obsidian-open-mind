<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Who still runs /opt/galion/bin/*.py on the VPS? (wave-4 slot 280, desk-py-callers-audit)

Written 2026-10-27 19:25 +01:00 (CET). Branch `swarm/desk-py-callers-audit`, based on slot 270
`swarm/opt-bin-py-port` 60b692b0 (which ports pw.py and replicate-picture.py to Rust). All VPS access read-only;
nothing on the VPS was installed, restarted, reconfigured or deleted. Evidence: `docs/real-tests/desk-py-callers-audit.md`.

## Answer (inventory item 3, "Unverified" -> verified)

| File | Live caller? | Proof | Rust replacement |
|---|---|---|---|
| `desk-hermes.py` | **YES, automatic.** Live `galion-desk` (pid 283, unit `galion-desk.service`, Oct 10 binary sha256 `555cc443…11e6`) forks `python3 /opt/galion/bin/desk-hermes.py` on every `GET`/`POST /desk`. `/desk` is fetched by the live console (`console-dist` SSR `desk-foundation-*.mjs`: `fetch("http://127.0.0.1:8646/desk")`), so each console desk/foundation page view = one Python process. | Caught live: 19:16:02 +01:00 one `GET /desk` -> child `230761 ppid 283 python3 /opt/galion/bin/desk-hermes.py`; 35 s passive watch before it: 0 children. Source `/workspace/galion-light/desk-rs/main.rs` (sha `2e44456b…ab76`) line 217 `Command::new("python3").arg("/opt/galion/bin/desk-hermes.py")`; same string in the live binary. | `crates/desk-hermes` (exists, mindpack 13 keys); workspace `crates/galion-desk` forks `DESK_HERMES_BIN` only, proven by the new real test (below). |
| `pw.py` | **No automatic caller.** Only the V1 Hermes skill `skills/playwright-console/SKILL.md` ("Run /opt/galion/venv/bin/python /opt/galion/bin/pw.py") and old docs/notes. | Python Hermes `hermes.service` is **masked**; live Hermes is `hermes-rs` (`hermes-gateway-b7fbbc78`), whose binary has 0 hits for `SKILL.md`, `skills_prompt`, `python3`, `pw.py`, `replicate`. 0 hits in units, cron, timers, journals (7 d). No process. | `crates/pw-check` (slot 270). |
| `replicate-picture.py` | **No automatic caller.** Only the V1 skill `skills/replicate-picture/SKILL.md` and old Hermes sessions/delegation manifests (last 2026-10-18). | same as above | `crates/replicate-picture` (slot 270). Paid API: no live call without spend approval. |
| `__pycache__/pw.cpython-312.pyc` | No | Bytecode only; the atime (18:13 +01:00 today) comes from today's read-only scans/backup reads, not imports (relatime; all three .py share one 17:34 +01:00 atime). | archive with the .py files |

Other live Python on the VPS (not ours, out of scope): `fail2ban-server`, `networkd-dispatcher` (Ubuntu system packages).
`/opt/galion/venv` exists only for pw.py/Playwright.

## What this changes in the plan

- The **only** live Python in the Galion stack is the per-request `desk-hermes.py` fork from the Oct 10 galion-desk.
  Removing it = the galion-desk cutover (`docs/cutover/desk.md`), not a file delete. Deleting `desk-hermes.py` before
  that cutover would silently empty the desk `mindpack` (`{}`), not crash anything (the Oct 10 code returns `{}` on
  failure), so it would be easy to miss: **do not delete it first**.
- `pw.py` and `replicate-picture.py` can be retired as soon as the Rust binaries are installed (slot 270 install plan),
  with no service impact. The two V1 SKILL.md files should be updated to the Rust names in the same change, so any
  future skill loader does not point at removed files.

## Recommendation (for the owner; nothing done)

1. **galion-desk cutover** (removes live Python): install the box-built workspace `galion-desk` + `desk-hermes`
   (static musl) as new files in `/opt/galion/rust/bin/`, add `Environment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`
   (the crate default `/opt/galion/openmind/dist/bin/desk-hermes` lives in the frozen openmind working copy, an
   Oct 18 build; better not to depend on it), switch `ExecStart`, restart `galion-desk`, then compare `GET /desk`
   mindpack values with the Python output captured just before (checklist step 4 in docs/cutover/desk.md).
   Rollback: old ExecStart, `/opt/galion/bin/galion-desk` stays untouched.
2. **pw.py / replicate-picture.py**: slot 270 install plan steps 1-4 (new files + symlinks, .py untouched), update the
   two SKILL.md files, then after 7 days archive (never delete) the .py files and `__pycache__`.
3. **desk-hermes.py**: archive only after (1) has run a week with `/desk` mindpack non-empty.
4. After 1-3, `/opt/galion/venv` (Playwright) and `/usr/bin/python3` are needed by no Galion service; venv can be
   archived, the system python stays for fail2ban/networkd-dispatcher (OS packages).

## Owner decisions (Slavik)

- **D-280-1** galion-desk cutover now (option A, recommended: removes the last live Python fork, ~1 restart of a
  loopback-only service, rollback = old ExecStart) / B: wait for the Rust console V2 cutover and do both together /
  C: leave as is.
- **D-280-2** `DESK_HERMES_BIN` location: A `/opt/galion/rust/bin/desk-hermes` (recommended, new install) /
  B keep the crate default in `/opt/galion/openmind/dist/bin` (frozen working copy).
- **D-280-3** pw.py + replicate-picture.py: install Rust replacements + symlinks + SKILL.md update (slot 270 plan)
  yes/no; archive window (7 days proposed).
- **D-280-4** `/opt/galion/venv` archive after D-280-3 (yes/no).
