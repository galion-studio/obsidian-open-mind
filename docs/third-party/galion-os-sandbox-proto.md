<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: galion-os-sandbox-proto (galion-box)

Checked 2026-10-21 06:55 CEST (+02:00) with `cargo metadata` (full dependency tree below).

| Name | Version | Licence (SPDX) | Source | Used for | Why not write it ourselves |
|---|---|---|---|---|---|
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | Raw Linux/Android syscall bindings: clone, mount, mount_setattr, pivot_root, prctl, setrlimit, capget | The Rust project's own FFI bindings; hand-writing per-arch constants and struct layouts for x86_64, aarch64, armv7 and Android is error-prone. No transitive dependencies. |

No code was copied. No GPL/LGPL/MPL code is linked.

Test-only tool (not linked, not committed): `qemu-aarch64-static` 10.0.13 (GPL-2.0, Debian package
on the dev box) was used to run the aarch64 build; on the VPS it was copied into the throwaway
`/var/tmp/sandbox-test-<ts>` folder for the test and deleted with it. Already listed as an open
licence question for Slavik (GPL test-only tools).
