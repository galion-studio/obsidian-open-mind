<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code: room (branch swarm/room)

- Written: 2026-10-20T19:04:51+02:00 (Europe/Zagreb)

## New Rust dependencies

None. `crates/room/Cargo.toml` still has an empty `[dependencies]` table. The
error type, JSON writer, percent-decoder and HTTP responder are all written
here using std only. Cargo.lock does not change for room.

## Copied code

None.

## Host tools used by the tests (not linked, not shipped)

| tool | version on the box | licence | used for |
|---|---|---|---|
| strace | 6.13+ds-1 (Debian, installed with `apt-get install strace` on 2026-10-20) | LGPL-2.1-or-later | `tests/no_bind.rs` traces network syscalls |
| util-linux `unshare`, `nsenter` | box default | GPL-2.0-or-later | network namespaces in tests and `crates/galion-realtest/tests/real_sh_room.rs` |
| curl | box default | curl (MIT-style) | probes in `crates/galion-realtest/tests/real_sh_room.rs` |

These are external programs that the tests run as subprocesses. Nothing from
them is linked into or shipped with room, so the copyleft licences of strace
and util-linux do not apply to the room binary.
