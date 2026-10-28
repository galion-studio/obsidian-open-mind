<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: swarm/omnode-enroll

**No new crates.** Cargo.lock dependency set unchanged from integration-5 0d68f46 +
swarm/hermes-enroll-2 (ed25519-dalek, sha2 and the hand-written store codec are documented in
`docs/third-party/hermes-enroll.md`). edge/openmind-node: no new crates.

Runtime tool (not linked): the system `curl` binary, used by `omnode` for `https://` and by
`--via-curl` (Termux: `pkg install curl`). Arguments carry only the URL and fixed flags;
headers, bearer and body go through `curl -K -` on stdin.

Test-only tools on the box: `qemu-aarch64` (user-mode emulation of the phone build) and `ss`
(listening-socket check).
