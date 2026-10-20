<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: cortex-s2-d

No new dependency and no copied code (2026-10-20 23:25 CEST +02:00). openmind-nerve still uses std + serde +
serde_json + zagreb-time; the tests use the existing dev-dependency openmind-schema. The days-from-civil formula in
`sensors/hades.rs::parse_ts` is the public-domain algorithm by Howard Hinnant, re-typed (no code copied).
The real test runs the workspace's own `hades-gate` binary (MIT, this repo); openmind-nerve does not depend on it.
