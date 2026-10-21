<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/hermes-bench-e2e (wave 4, slot 162)

Written 2026-10-21 08:30 CEST (+02:00).

No new dependency and no copied code. `crates/hermes-bench/src/e2e.rs` uses
std only (TcpListener/TcpStream HTTP/1.1, std::process) plus crates already in
the workspace (`serde_json`, `hermes-rs`, the bench crates). `Cargo.lock` is
unchanged by this branch's own commit.
