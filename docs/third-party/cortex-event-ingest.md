<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: swarm/cortex-event-ingest

2026-10-21 01:25 CEST (+02:00). No new dependency (Cargo.lock unchanged) and no copied code.

- The RFC 3339 parser in `crates/openmind-nerve/src/sensors/event_feed.rs` (`parse_rfc3339`) is written here. Its
  days-from-civil arithmetic follows the algorithm Howard Hinnant describes in "chrono-Compatible Low-Level Date
  Algorithms" (http://howardhinnant.github.io/date_algorithms.html, explained there as public domain); no code was
  copied. Chosen over `chrono` / `time` to keep the nerve at std + serde + serde_json + zagreb-time.
- Test-only: the GALION_REAL test can drive the in-repo `gaia` binary (MIT, this repository) via `GAIA_BIN`.
