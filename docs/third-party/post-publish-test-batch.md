<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: post-publish-test-batch (slot 415)

No new dependency and no copied code. The only Cargo change is the workspace path crate
`galion-realtest` as a `[dev-dependencies]` entry, brought in by slots 363/375/383 (their own
third-party files: real-test-port-env.md, real-ports-batch-2.md, real-ports-batch-3.md).
Cargo.lock adds no registry package versus 83e573e2.
