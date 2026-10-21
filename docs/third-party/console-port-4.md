<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: console-port-4

No new dependencies. `console-ui` stays std-only.

One algorithm (not code) is used: Howard Hinnant's `civil_from_days` / `days_from_civil` date conversion, described as public
domain at https://howardhinnant.github.io/date_algorithms.html; written from the description in
`crates/console-ui/src/pages/p4.rs`. The workspace already has `zagreb-time`, but it pulls in `chrono` + `chrono-tz`, which
`console-ui` (std only, no dependencies) does not take.
