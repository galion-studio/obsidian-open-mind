<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: swarm/nerve-wiring-rebase

**New external dependencies: none. No code was copied.**

This branch is integration-4 42de672 merged with nerve-wiring-2 d97dab9 (see
`docs/third-party/nerve-wiring.md` for that branch). The conflict resolution
adds no crates: `Cargo.lock` keeps both path edges (`galion-common` from
integration-4 and `nerve-client` from nerve-wiring) on `hermes-rs` and
`openmind-memory`. `galion-common` gains a std-only `BusyHook`.
