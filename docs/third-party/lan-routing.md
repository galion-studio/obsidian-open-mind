<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: lan-routing (swarm slot 31)

- Written: 2026-10-20T20:48:01+02:00 (Europe/Zagreb)
- Branch: `swarm/lan-routing` (merges `swarm/lan-discovery` fe5f0bd and `swarm/poseidon` 7d0c478)

## New dependencies

None. The router (`crates/openmind-discovery/src/routing.rs`), the mock
OpenAI server (`examples/mock_openai.rs`) and the registry/VRAM changes use
only `std`, `serde` and `serde_json` (already workspace dependencies) and
in-workspace crates (`openmind-provider`, `zagreb-time`). No code was copied
from elsewhere. `Cargo.lock` is unchanged by this branch beyond what the two
merged wave-1 branches already brought (socket2 0.6, MIT OR Apache-2.0, see
`docs/third-party/lan-discovery.md`).

Why no HTTP crate: the router speaks plain HTTP/1.1 with `Connection: close`
to loopback/LAN model servers, non-streaming only. A minimal client
(Content-Length and chunked bodies, 4 MiB cap) is ~150 lines of std code;
hyper/reqwest/ureq would pull in a much larger tree (and TLS, which is not
needed on the LAN path and openssl is not allowed).

Test tools used by `scripts/real/lan-routing.sh`: `curl` and `jq` from the
box (not linked, not shipped).
