<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party: hermes-rs-memory-client (slot 403)

Written 2026-10-28 17:40 (+01:00) Europe/Zagreb.

No new crates, no vendored code, Cargo.lock unchanged. The client is plain `std::net::TcpStream`
HTTP/1.0 plus the `serde_json` already in hermes-rs. Behaviour (fence text, system note,
capture max-4-in-flight, timeouts) was matched by reading the retired Python Hermes plugin
read-only on the VPS (`hermes-plugin` npm package and
`/var/backups/hermes-legacy-20261021/tooling-hermes-agent.tar`); no code was copied, only the
fence string and protocol fields, re-implemented in Rust. The real test drives our own
`openmind-memory` (MIT, this repo) as the throwaway server.
