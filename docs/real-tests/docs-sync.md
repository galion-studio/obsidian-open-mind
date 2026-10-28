<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life tests: swarm/docs-sync

Box only, 2026-10-20 (Europe/Zagreb, +02:00). No server is started and no VPS
probe was needed: the real binaries here are the repo's own check tools.
`CARGO_TARGET_DIR=/workspace/swarm/target/docs-sync`.

## Before (main 13a296c, 2026-10-20T18:52:21+02:00)

```
repo-check 2026-10-20T18:52:21+02:00: 722 files, 0 error(s), 43 warning(s) -> PASS
```

All 43 were `utc` warnings: 22 `SystemTime::now()` reads and 21 RFC 3339
literals ending in `Z`.

## What changed for the 43

- 8 literals converted to Zagreb RFC 3339 with an offset: hermes-rs
  `time.rs` (4) and `spend.rs` (1) test inputs, openmind-schema doc comments
  (3). The hermes-rs test now also pins the unix values (1_792_350_000 and
  1_792_890_000), so the offset inputs are really checked.
- 13 literals keep the `Z` on purpose and carry a reviewed
  `repo-check: allow (<reason>)` marker: refused-input tests (galleon-arena 2,
  openmind-schema 1), the link-watch legacy `at` field (1), and
  openmind-memory, whose `iso_utc` reproduces the JS `toISOString` wire
  format of the live 8420 API (parity, B60) (9). Plus one new allow line in
  hermes-rs that keeps coverage of `Z` input on the wire.
- 22 `SystemTime::now()` reads are unix epoch numbers (ages, rate-limit
  windows, ids, temp names). None is printed as a timestamp; each carries a
  reviewed allow marker.

## After (this branch)

```
$ scripts/real/docs-sync.sh
real/docs-sync: time 2026-10-20T18:58:32+02:00
real/docs-sync: repo-check exit 0: repo-check 2026-10-20T18:58:32+02:00: 723 files, 0 error(s), 0 warning(s) -> PASS
real/docs-sync: spdx-scan: 175 scanned, 0 missing
real/docs-sync: self-check: self-check: ok
real/docs-sync: no-python: ok
status-percent: items 89 (D 54, P 12, N 2, O 21), weight 122
done: 60.7 % unweighted, 59.0 % weighted
done + half partial: 67.4 % unweighted, 67.2 % weighted
owner-blocked: 23.6 % of items
real/docs-sync: status-percent matches docs/STATUS.md
real/docs-sync: STATUS.md swarm table: 21 branches
real/docs-sync: PASS

$ SELF_CHECK_FULL=1 scripts/self-check.sh      (2026-10-20T19:11:16+02:00)
c6: ok (23 listed)
c7: ok
secret: ok
no-python: ok
spdx: ok
self-check: cargo fmt / cargo clippy / cargo test
size-budget: ok (19 binaries)
self-check: ok

$ cargo test -j 4 --workspace --no-fail-fast
121 test binaries, 497 passed, 0 failed
```

The status numbers are the same as before: this branch changes evidence text
only, and GAIA-1 was already P. The swarm table rows do not match the
status-row pattern, so they are not counted.

## Notes

- `openmind-sandbox` `group_kill_leaves_no_grandchild` failed once in a
  workspace run and passed 3 of 3 alone. It looks for a host-wide
  `sleep 3171`, so it collides when another worktree on the shared box runs
  the same test at once. Not touched here.
- The full self-check's size-budget step needs `file(1)`; it was missing on the
  box and was installed with apt (box only).
