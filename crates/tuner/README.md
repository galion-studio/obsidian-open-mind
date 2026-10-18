<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->
# tuner

Ukulele tuner page (`www/`) and two std-only binaries:

- `tuner`: static server, byte-compatible with the live `/opt/galion/tuner/bin/tuner`. Env `TUNER_ADDR` (default 127.0.0.1:8650), `TUNER_ROOT` (default /opt/galion/tuner/www).
- `tuner-publish`: runs `cloudflared tunnel --no-autoupdate --url <local>` (a trycloudflare quick tunnel, no account), writes the public URL to `TUNER_URL_FILE`, restarts with 2..60 s backoff. `--dry-run` prints the command.

`js-test/pitch-test.js` is the original offline test of `www/pitch.js` (run with node; not part of `cargo test`).
Staged units: `deploy/staged/tuner.service`, `deploy/staged/tuner-publish.service`.
