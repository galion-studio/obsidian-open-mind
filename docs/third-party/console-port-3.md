<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: console-port-3

No new dependencies and no copied code. console-ui and console-static stay std-only (`Cargo.lock` unchanged).
The Zagreb clock (EU summer-time rule: last Sunday of March / October, 01:00 UTC) and the civil-date conversion in
`crates/console-ui/src/pages/p3.rs` are written here from the public-domain description of the days-from-civil algorithm
(Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms", https://howardhinnant.github.io/date_algorithms.html);
no code was copied.
