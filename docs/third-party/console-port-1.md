<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: console-port-1

No new dependencies; console-ui stays std-only. No copied code.

- The civil-date conversion in `crates/console-ui/src/pages/cache.rs` (`civil`, `days_from_civil`) follows the
  algorithms described by Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms"
  (https://howardhinnant.github.io/date_algorithms.html, published as public domain); written from the description,
  no code copied.
- The JavaScript number formatting rules (`toFixed`, `Math.round`, `String(n)`) follow ECMA-262; written from the spec.
