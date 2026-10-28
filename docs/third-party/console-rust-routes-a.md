<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: console-rust-routes-a (slot 395)

No new dependencies. `console_static::owners` and the `--owner-routes`
forward reuse the existing in-crate `proxy::forward` (std only).
`galion_realtest::console_routes` additions (`ApiTarget`, `LocalApi`) use
only `std`. Cargo.lock unchanged versus ab0b7d04 (including the merged
`swarm/console-deploy-candidate` 4f763ade).

Written 2026-10-28 16:34 CET (+01:00, Europe/Zagreb).
