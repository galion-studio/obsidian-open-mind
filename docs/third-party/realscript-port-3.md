<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party: realscript-port-3 (wave 4 slot 114)

**No new crate dependencies and no copied code.** The new tests use only std plus
workspace crates already in `galion-realtest` (`zagreb-time`, `serde_json`, and
the crate's own `http` / `sha256` modules). `Cargo.lock` is unchanged.

External programs the real tests call as separate processes (never linked,
vendored or redistributed), the same set the deleted shell scripts used:

| Tool | Licence | Used by | Why |
|---|---|---|---|
| util-linux `unshare`, `nsenter`, `setsid` | GPL-2.0-or-later (tool only) | real_sh_room, real_sh_survival_heal | user+net namespaces; detached restart of the dummy service without a shell |
| `strace` | LGPL-2.1-or-later (tool only) | real_sh_room | proves the no-flag page makes zero network syscalls |
| `ip` (iproute2) | GPL-2.0-or-later (tool only) | real_sh_room | bring `lo` up inside the namespace |
| `curl` | curl (MIT-style) | real_sh_room (optional) | probe from inside the namespace |
| `sudo` | ISC-style (tool only) | real_sh_sandbox | setuid-root helper + delegated cgroup v2 subtree on the box; skipped without `sudo -n` |
| `qemu-aarch64-static`, `qemu-arm-static` | GPL-2.0-only (tool only) | real_sh_ucpu | run aarch64/armv7 binaries on x86_64 (owner note in docs/third-party/ucpu.md applies) |
| `zig` (`zig cc`, `zig ar`) | MIT | real_sh_ucpu via `galion-ccwrap` | cross C compiler for bundled SQLite |
| OpenSSH `ssh` | BSD-style | real_sh_tuner_cutover, real_stack (`GALION_REAL_VPS=1` only) | read-only GETs to the live VPS |
| `nice`, `kill`, `id`, `cp`, `chown`, `chmod`, `tee`, `rmdir`, `rm` (coreutils/procps) | GPL (tools only) | various | CPU politeness and box-side setup/teardown |

`galion-ccwrap` (new, `crates/galion-realtest/src/bin/galion-ccwrap.rs`, MIT, ours)
replaces the generated `cc-<triple>` shell wrappers: it drops cc-rs's
`--target=<rust triple>`, maps cargo's `$TARGET` to the zig triple and rewrites
`-march=armv7-a` to `-mcpu=generic+v7a`.
