<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: memory-db-stale-diag (slot 388)

No new dependencies and no copied code. `freshness.rs` uses the crate's
existing `rusqlite` 0.32 (MIT, bundled SQLite, public domain) and the
in-repo `zagreb-time`. The days-from-civil formula in `rfc3339_ms` is
Howard Hinnant's public-domain algorithm
(https://howardhinnant.github.io/date_algorithms.html), rewritten here, no
code copied. Cargo.lock unchanged. Hermes research not used.
