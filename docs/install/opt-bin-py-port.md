<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Install plan: retire the last .py helpers in /opt/galion/bin (PLAN ONLY, nothing done)

Status 2026-10-27 18:40 +01:00 (CET): nothing below was run on the VPS. Owner (Slavik) decides; an agent may do
steps 1-4 only with an explicit go (they are installs on a live host, outside slot 270's read-only scope).

## Facts

- Neither `pw.py` nor `replicate-picture.py` is started by a unit, timer, cron or Hermes config (0 hits). They are
  manual helpers, so replacing them has no service impact and needs no restart.
- `desk-hermes.py` IS used: live `galion-desk` (8646) forks `python3 /opt/galion/bin/desk-hermes.py` per `/desk`.
  Its Rust replacement already exists; switching it is the galion-desk cutover in `docs/cutover/desk.md`
  (needs the newer galion-desk with `DESK_HERMES_BIN`). Do not delete desk-hermes.py before that cutover.

## Steps

1. Build on the box (never on the VPS):
   `cargo build --release --target x86_64-unknown-linux-musl -p pw-check -p replicate-picture`
   (static, ~0.6 MB and ~0.5 MB).
2. Copy to the VPS as new files, nothing replaced:
   `/opt/galion/rust/bin/pw-check` and `/opt/galion/rust/bin/replicate-picture` (0755 root:root).
3. Read-only smoke on the VPS: `pw-check --help`; `replicate-picture --help`;
   `pw-check /console --out-dir /var/tmp/pw-check-smoke` (Chromium is found automatically in
   `/root/.cache/ms-playwright`; the admin cookie is read from `/root/galion-secrets/pw-cookie` by the binary itself,
   never printed). Compare with the 2026-10-10 Python report (same keys/order). Remove `/var/tmp/pw-check-smoke`.
   `replicate-picture` costs money per call: run it only with Slavik's spend approval (rule 5).
4. Keep the old names working for anyone who types them:
   symlink `/opt/galion/bin/pw` -> `/opt/galion/rust/bin/pw-check` (new name, pw.py untouched) and
   `/opt/galion/bin/replicate-picture` -> `/opt/galion/rust/bin/replicate-picture`.
5. After one week with no use of the .py files (atime / shell history check), and only with Slavik's approval:
   move `pw.py`, `replicate-picture.py`, `__pycache__/` to `/opt/galion/archive/py-<date>/` (B64 archive list),
   never delete. desk-hermes.py follows after the galion-desk cutover.
6. Then `/opt/galion/venv` is only needed by whatever else still uses Playwright/Python (not checked by this slot).

## Rollback

The .py files stay until step 5, so rollback is: remove the two symlinks and the two files in /opt/galion/rust/bin.

## Differences from the Python helpers (owner should know)

- pw-check: base/cookie URL follow `--base` (Python hardcoded 8080 for both, same default). Extra flags
  `--base --out-dir --cookie-file --chrome` (+ env PW_CHROME). A navigation error or networkidle timeout ends the run
  with exit 1 and no report, like the Python traceback. Page-error text is the exception message without the
  `Name: ` prefix (matches Playwright's `str(err)` for Error objects; non-Error throws keep their value).
- replicate-picture: HTTPS via curl (no TLS crate). Extra flags `--key-file --api --curl --poll-ms` for tests.
  `--help` exits 0; missing args exit 2 (argparse also 2). Transport errors read `URLError <curl message>` instead
  of the bare class name.
