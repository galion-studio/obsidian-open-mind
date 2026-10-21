<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: swarm/nerve-wiring

**New external dependencies: none. No code was copied.**

- `nerve-client` stays std-only (bounded queue = `std::sync::mpsc::sync_channel`).
- `hermes-rs` and `openmind-memory` gain one path dependency on the
  workspace crate `nerve-client` (MIT, this repo). The `Cargo.lock` change is
  just those two path edges.
- `openmind-nerve::owners` reads `/proc/net/tcp*`, `/proc/<pid>/fd`,
  `/proc/<pid>/comm` and `/proc/<pid>/cgroup` with std only. The formats come
  from the Linux `proc(5)` man page. The `ss -ltnp` `users:(("name",pid=N,fd=M))`
  format was read from real output. No iproute2 code is used.

The transitive closure is the same as for swarm/nerve, as listed in
[`docs/third-party/nerve.md`](nerve.md). That file has all permissive licences and no copyleft.
