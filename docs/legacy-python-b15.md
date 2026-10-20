---
tags: [galion, openmind, audit, rust-port]
project: galion
status: draft
---
<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# set-model.py and trace.py (B15)

Desk: openmind.B15. Checked 2026-10-19 (Europe/Zagreb). Nothing was executed from the helpers. No live modes file was opened or written. `crates/galion-one` was not edited.

B10 (`docs/legacy-python.md`) already listed both helpers as having no caller. This note rechecks that. They are not shipped. A small crate still covers the modes-file check so a caller can validate a lane without writing the live file.

## What was searched

- `/opt/galion/galion-light/one/set-model.py` is present (mtime 2026-09-24). A bytecode cache sits beside it (`one/__pycache__/set-model.cpython-311.pyc`). That only shows the file was loaded sometime, not that a current unit or crate calls it.
- `/opt/galion/galion-light/one/trace.py` is present (mtime 2026-09-24, 131 lines). It was read in full.
- Call sites under `/opt/galion/openmind/crates`, `/opt/galion/galion-light` (excluding `shadow/` and `__pycache__`), `/opt/galion/bin`, `/etc/systemd`, and cron.
- No crate, shell helper, bin, unit, or cron entry we ship runs either script.

Unrelated names, not these files:

- Vendored Hermes kanban `set-model` under `galion-light/shadow/hermes-agent` (a task-id override, not `one/set-model.py`).
- `fire/trace.py` inside virtualenvs (the `fire` library).

`crates/galion-one` only reads `/workspace/hermes/modes.json`. Reads: `lane_json` (around line 1824), `pool_json` (around line 3087), `cortex_body` (around line 3135). It does not spawn `set-model.py`.

## set-model.py

Not shipped. Manual operator helper only. No caller in crates or galion-light scripts.

If someone ran it: argv is lane, then model. Lane must be `plan` or `action`. Model must contain `/`. Otherwise it exits 1 before touching the file. On success it loads a modes JSON file, keeps other top-level keys, rebuilds `modes` to only `plan` and `action` (a missing lane becomes `""`), sets the chosen lane, and writes that object back with indent 2 and a trailing newline. The path in the script is `/workspace/hermes/modes.json` (the host file is `/opt/galion/hermes/modes.json`). That write would change live lane config. This ticket does not do it.

## trace.py

Not shipped. Manual helper only. No caller in crates, galion-light scripts, skill docs, bins, or units.

If someone ran it: argv is a source image and a dest SVG. It POSTs the image to `http://127.0.0.1:8645/see` for a caption, walks dark pixels, simplifies the contour, and writes one filled SVG. It needs Pillow. It is not ported here: nothing we ship calls it, and this ticket does not add an image stack or dial that port.

## Crate `crates/one-set-model`

Library plus a print-only bin. `validate` checks lane (`plan` or `action`) and that the model contains `/` (lane first). `apply` returns the JSON object `set-model.py` would write. `apply_file` only reads the path it is given. `render` pretty-prints that object with a trailing newline. Nothing in the crate creates or writes a modes file.

Tests build a fixture under the system temp directory, assert the source bytes stay unchanged, and write the returned object to a second temp file in that same directory. They never open `/opt/galion/hermes/modes.json` or `/workspace/hermes/modes.json`.

`trace.py` stays not shipped. No Rust port of it in this ticket.
