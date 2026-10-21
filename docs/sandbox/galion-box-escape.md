<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-box escape suite (wave-4 slot 164)

Written 2026-10-21 08:30 CEST (+02:00). Extends the L3 launcher from slot 144
(`swarm/galion-os-sandbox-proto` 4374605) with one real test per escape family.

## What was added

| Piece | Role |
|---|---|
| `crates/galion-box/src/escape.rs` | Payload probes: `galion-box escape <family\|all>` |
| `crates/galion-box/tests/real_escape.rs` | VPS-only suite (GALION_REAL=1 GALION_BOX_ON_VPS=1) |
| `docs/real-tests/galion-os-box-escape.md` | Numbers from the VPS run |

Families: **ptrace**, **mount/pivot_root**, **userns nesting**, **bpf/perf**, **io_uring**,
**keyctl**, **socket families** (AF_PACKET, AF_NETLINK, AF_VSOCK, AF_BLUETOOTH, AF_XDP,
abstract AF_UNIX, TCP internet).

## Containment model (what "CONTAINED" means here)

A family is CONTAINED when the payload cannot reach **host** state:

- host processes (ptrace/kill) → ESRCH (other pid ns) or EPERM
- host mounts / pivot_root / remount → EPERM/EACCES; `/.oldroot` absent
- host uid 0 via nested userns → cannot write a host-root uid_map
- host keys → keyctl unavailable or cannot read foreign key id 1
- host network / abstract UNIX → blocked; host listener got 0 connections
- host paths via io_uring → still hidden after pivot_root

MEASURED (not a host escape by themselves, but inputs to D-S3):

- `io_uring_setup=ALLOWED` (unprivileged)
- `socket(AF_VSOCK)=ALLOWED` and `socket(AF_NETLINK)=ALLOWED`
- `unshare(CLONE_NEWUSER)=ALLOWED` (nested; host-root map still denied)

## How to re-run (VPS only)

Build static musl on the box, copy into a throwaway `/var/tmp/sandbox-test-<ts>` as a
non-root temp user, set `GALION_REAL=1 GALION_BOX_ON_VPS=1`, run `real_escape`, then
remove the user and folder. Never set those env vars on the dev box.
