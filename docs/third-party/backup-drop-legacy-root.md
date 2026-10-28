<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: backup-drop-legacy-root (wave-4 slot 427)

No new dependencies and no copied code. The change uses what galion-backup
already had: `rusqlite` 0.32 (MIT, bundled SQLite, public domain) for the
read-only content count, std for the marker scan. Tests use the system
GNU `tar`/`sha256sum`/`bash` (already used by the existing tests) and the
workspace's own `openmind-memory` release binary. Hermes research: not used.
