<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code: swarm/one-probe

No new dependencies and no copied code. `crates/galion-one` still has an empty
`[dependencies]` table (std only); `Cargo.lock` is unchanged.

The two ws/ui changes ported here (15cdb53 fail-closed pairing, f69a916
`/api/v1/services`) are this project's own code (galion-studio, MIT), not third
party. The `systemctl` shell-out of f69a916 was replaced with a std-only
`/proc/<pid>/comm` scan.
