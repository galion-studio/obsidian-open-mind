<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: sandbox-workdir-quota (wave-4 slot 151)

No new dependencies and no copied code. The tmpfs mount, `open_tree` and the
tree copy use raw Linux syscalls through `libc` 0.2 (MIT OR Apache-2.0), which
openmind-sandbox already depends on. Cargo.lock is unchanged.
