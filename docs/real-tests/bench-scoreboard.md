<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: bench-scoreboard

Run 2026-10-20T23:10:00+02:00 (Europe/Zagreb) on the box. No server, no network; CLI only (slot 54
range 19540-19549 not needed).

`GALION_REAL=1 nice -n 10 cargo test -p bench-scoreboard -p galion-bench --release -j 2 --offline`
- bench-scoreboard unit: 4 passed (all PENDING; fold + failing axes; lying `pct` and score >100 rejected; empty card stays PENDING).
- `tests/real_cli.rs`: 1 passed. The release binary folded a B3 card `{"pct":90,...}` as
  `| 90% | swarm/bench-vending @ deadbee |` with `pricing (80)` failing, kept B1/B2 PENDING, stamped `+02:00`;
  a card claiming `pct 100` with axis 0 exited 1; an unknown flag exited 2.
- galion-bench (merged from swarm/bench-harness ea8315c): 11 unit + 2 real_mock + 4 server passed.
  Note: galion-bench's own real_mock test uses that crate's port choice, not slot 54's range.

`bench-scoreboard --out docs/bench/SCOREBOARD.md` with no `--score` → exit 0, three PENDING rows.
