<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: hermes-tools (slot 17)

**No new third-party dependency and no copied code.**

| crate | used by | already in the workspace via | licence |
|---|---|---|---|
| `serde_json` 1.x | hermes-tools, hermes-mock-model | workspace dependency (hermes-rs) | MIT OR Apache-2.0 |
| `libc` 0.2 | indirectly, through openmind-sandbox | openmind-sandbox | MIT OR Apache-2.0 |
| `hermes-rs`, `openmind-sandbox` | hermes-tools | workspace crates | MIT |

The Cargo.lock change is only the new workspace member `hermes-tools` and its edges, with no new
registry package (`git diff Cargo.lock`). Tools called at run time (`git`, `dd`, `install`, `nice`
and the Rust toolchain) are system programs. They are not linked or vendored.

Why no crates for HTTP, sandboxing or JSON-RPC: the mock server is about 200 lines of std
`TcpListener`, sandboxing reuses openmind-sandbox, and the agent loop and provider are hermes-rs.
Adding hyper/tokio or a landlock crate would only grow the binary
(hermes-gateway musl release: 1.07 MB on x86_64, 0.91 MB on aarch64).
