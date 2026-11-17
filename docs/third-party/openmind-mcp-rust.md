<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: swarm/openmind-mcp-rust (2026-10-29 +01:00)

No new third-party dependency. `openmind-mcp-stack` uses:

| crate | version | licence | why |
|---|---|---|---|
| serde_json | workspace (1.0.154) | MIT OR Apache-2.0 | JSON-RPC framing (already used) |
| zagreb-time | path (ours, MIT) | MIT | Zagreb mtimes in `vault_list` |
| hermes-rs | path (ours, MIT), dev only | MIT | round-trip test with the real Hermes MCP client |

HTTP to openmind-memory is a hand-written HTTP/1.1 client over `std::net::TcpStream`
(loopback only), so no HTTP crate was added.
