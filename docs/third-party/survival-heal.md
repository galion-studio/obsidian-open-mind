<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code and dependencies: survival-heal

Recorded 2026-10-20 19:15 CEST (+02:00).

**No new external crate and no copied third-party code.**

- `guardian-tick` gained one dependency, `openmind-nerve` (workspace crate, MIT, this repo).
  Its own deps (`serde`, `serde_json`, `zagreb-time`) were already in guardian-tick's tree.
  `Cargo.lock` changes by that one in-workspace edge only.
- Code ported from this repository's history (our own MIT code, same copyright):
  `crates/galion-healer` and `crates/galion-guardian` at commit 317d1d4 (protected-unit list,
  `[[restart]]` allow-list, streak rule, actions/incident logging), removed by the 13a296c merge.
- `dummy_service` (test example) and the shell script are new, written here.
