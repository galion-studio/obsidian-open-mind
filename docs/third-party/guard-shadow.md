<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: guard-shadow (swarm/guard-shadow)

No code was copied and **no new crate** entered the dependency graph.

`galion-guard` now names two crates directly that were already linked into
it through `zagreb-time` (same versions, same lock entries):

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| chrono | 0.4.45 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono | formatting/parsing fail2ban's offset-less `YYYY-MM-DD HH:MM:SS` stamps | already in the binary; hand-rolled calendar code would be a second, untested copy |
| chrono-tz | 0.10.4 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono-tz | Europe/Zagreb local time -> unix seconds (DST gap and overlap) for the shadow log and the comparison reader | the tz rules are the hard part; `zagreb-time` only goes unix -> Zagreb |

The fail2ban 1.0.2 log line layout (`%(asctime)s %(name)-24s[%(process)d]:
%(levelname)-7s %(message)s`) and message shapes (`Found`, `Ban`,
`Increase Ban`, `Restore Ban`, `Unban`) were taken from the live VPS
`/var/log/fail2ban.log` (read-only, 2026-10-21 04:40 +02:00), not from
fail2ban's GPL source.
