<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: galion-os-admin-cli (galion-admin)

Checked 2026-10-21 08:35 CEST (+02:00) with `cargo metadata` (full dependency tree below).

| Name | Version | Licence (SPDX) | Source | Used for | Why not write it ourselves |
|---|---|---|---|---|---|
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | flock, kill, prctl(PR_SET_PDEATHSIG), getuid/geteuid, localtime_r (zone offset) | Same crate and version as galion-box (slot 144); per-arch constants and struct layouts for x86_64, aarch64, armv7 and Android are error-prone by hand. No transitive dependencies. |

Full tree: `galion-admin 0.1.0 (MIT)` → `libc 0.2.191 (MIT OR Apache-2.0)`. Nothing else.

Written here instead of borrowed: SHA-256 (from FIPS 180-4, checked against the NIST vectors
and coreutils `sha256sum`), the flat JSON escape/parse for the audit log, the civil-date
conversion (algorithm published by Howard Hinnant, public domain description; re-implemented,
no code copied), argument parsing. No code was copied. No GPL/LGPL/MPL code is linked.

`galion-box` (slot 144, MIT, this project) is called as a separate binary, not linked.

Test-only tools (not linked, not committed): `qemu-aarch64` and `qemu-arm` 10.0.13 (GPL-2.0,
Debian packages on the dev box) ran the cross-built unit tests on the box. Not copied to the VPS.
