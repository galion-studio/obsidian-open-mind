<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# drift fixture (T2.5, synthetic)

Synthesised 2026-10-20 23:35 CEST (+02:00) with `printf` + `sha256sum` (no Python, no host data).

- `SHA256SUMS`: the "release manifest" (SHA256SUMS format, as `dist/bin/SHA256SUMS`), relative names.
- `deployed/`: what is "on the host" (`NERVE_DRIFT_ROOT`).

| manifest line | deployed file | expected finding |
|---|---|---|
| galion-one | same bytes | none |
| galion-desk | hand-patched bytes (manifest has the release bytes' hash) | `drift-galion-desk` error |
| tuner.service | same bytes | none |
| room | absent | `drift-missing-room` warn |

`drift::tests::replay_fixture` also checks the B41 rule: one `drift`/`vulnerability` event, severity `high`, 2 findings.
