<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Route dry

- DESK-ID: openmind.ROUTE-DRY
- Stamp: 2026-10-20T10:30:04+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Crate: crates/openmind-route (not edited)
- Result: ALREADY

ALREADY route_never_returns_a_pending_seed_row_or_remote_and_opens_no_socket

The test `route_never_returns_a_pending_seed_row_or_remote_and_opens_no_socket` in `crates/openmind-route/tests/route.rs` already returns the approved CPU-local row when the caller slice has no GPU row and no approved LAN row. That slice is a remote row, one pending seed, and the approved CPU-local row. The returned id is `cpu-local`. The pending seed is not returned. The remote row is not returned. The test reads `/proc/self/fd` before and after `route` and asserts the socket targets are unchanged.

`cargo fmt -p openmind-route -- --check`, `cargo clippy -j 1 --offline -p openmind-route --all-targets -- -D warnings`, and `cargo test -j 1 --offline -p openmind-route` exited 0. Target dir was `/tmp/om-routedry`. `Cargo.lock` matched the snapshot `/tmp/om-lock-ROUTEDRY-before` and was not copied back. `git checkout` was not used.

The stamp offset is numeric. It is not a trailing UTC designator.

Nothing was committed. Nothing was pushed. systemctl was not used. Python was not used. crates/galion-one was not edited. No endpoint was dialed.
