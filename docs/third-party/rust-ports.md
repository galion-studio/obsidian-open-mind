<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: swarm/rust-ports (slot 47)

No code was copied. New crates in Cargo.lock: **miniz_oxide 0.8.9** and
**adler2 2.0.1** only.

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide | raw deflate (level 6) inside our own gzip framing for the backup .tar.gz | a correct, competitive DEFLATE encoder is thousands of lines; pure Rust, no C, no SIMD (`simd` feature off, default features off + `with-alloc`) |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 | transitive (miniz_oxide) | - |
| rusqlite | 0.32.1 | MIT | https://github.com/rusqlite/rusqlite | SQLite online backup API + integrity_check (as before) | already in the workspace (openmind-memory, same features) |
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | umask, statvfs, getpwuid_r/getgrgid_r, signal, major/minor (galion-backup); source-bound sockets (galion-guard dev-dep) | already in the workspace |

Written ourselves instead of adding crates: SHA-256 (`src/sha256.rs`, FIPS
180-4 vectors tested; `sha2` pulls `cpufeatures` with runtime SHA-NI/SIMD
paths), CRC-32 + gzip framing (`src/gzip.rs`; `flate2` pulls `crc32fast`
with a PCLMULQDQ path), GNU tar writer (`src/tarw.rs`; the `tar` crate
follows symlinks by default and pulls `filetime`/`xattr`; exact GNU header
parity - uname/gname, hard-link entries, shrink padding, 10240-byte record -
was needed to make files.tar byte-identical to GNU tar 1.35, ~300 lines).

Rejected: `flate2` (crc32fast SIMD), `sha2` (cpufeatures), `tar` (above),
`fs2`/`fs4` (std `File::try_lock` is stable).

## Test-only, not committed, not shipped

* Debian `nftables` 1.1.3 + libs (.debs via `apt-get download`, `dpkg -x`)
  for `crates/galion-guard/tests/real_netns.rs`, as before
  (docs/third-party/guard-rust.md).
* GNU tar 1.35 / coreutils / bash on the box, used by the tests as the
  reference implementation (the parity oracle is the original script read
  from git object `e43d7ab`, not a file in the tree).
* zig 0.13 `cc` (box-only, /workspace/oom-tools) as the cross C compiler for
  SQLite when verifying the aarch64-musl build and the armv7/android checks;
  pending owner exception (see common-migrate-2 report).

`cargo metadata` licences of the galion-backup dependency closure: 58 packages, every one MIT, Apache-2.0 or an
OR-choice including MIT (plus Zlib / 0BSD / BSD-2-Clause / Unicode-3.0
terms); checked 2026-10-20 23:15 +02:00.
