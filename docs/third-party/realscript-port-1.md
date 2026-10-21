<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: swarm/realscript-port-1

No new dependencies and no copied code. The Rust ports use only `std`,
`serde_json` (already a galion-realtest dependency, MIT OR Apache-2.0) and the
in-repo `zagreb-time` crate and `galion_realtest::{http, sha256}` modules.
External programs the tests call (as the shell scripts did): `git`, `nice`,
`kill`, `ss`, `readelf`, `pgrep`, `unshare`, `strace`, `rustc`, `uname` and
`qemu-aarch64` (optional). `curl` and `jq` are no longer needed by these tests.
