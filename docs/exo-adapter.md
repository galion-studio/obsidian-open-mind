<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Exo adapter

- DESK-ID: openmind.B37
- Written: 2026-10-19T16:27:31+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: `crates/openmind-exo`

Exo GPU on Linux is not used here. `ExoProvider` in `crates/openmind-exo` only parses fixture bodies for an OpenAI-compatible `GET /v1/models` (the `/models` alias is the same list) and `GET /health`. It stores an endpoint string and never dials it, so there is no socket, no process, and no GPU. A Mac may run exo for Apple Silicon; this adapter does not. Linux GPU support is outside this crate, and the CUDA fork is not linked and is not a kind here (see `TEACHER/A-foundation.md` section 5).
