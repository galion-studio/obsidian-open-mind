<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-tar: the one tar implementation (swarm/shared-tar-crate, slot 122)

2026-10-21 04:20 CEST (+02:00). Base: merge of swarm/rust-ports 26a2a8e and
swarm/cutover-prep a5ac49b (clean merge, no conflicts).

## Why

Two tools had two tar implementations:

| tool | file (before) | format | job |
|---|---|---|---|
| galion-backup (periodic 6-hourly backup) | `src/tarw.rs` | GNU (`ustar  \0`), tree walker | `files.tar` + outer `.tar.gz`, **byte-identical to GNU tar 1.35** |
| galion-deploy (pre-cutover snapshot) | `src/tar.rs` | POSIX ustar (`ustar\0` `00`), in-memory entries | `backup.tar` writer + `backup-verify` reader |

cutover-prep logged this as a decision item ("after rust-ports merges, move
tarw plus a reader into a std-only shared crate and switch both tools to it").
This branch does that.

## What

`crates/galion-tar`, **std only, zero dependencies** (not even libc), so
galion-deploy stays dependency-free:

* `gnu::TarWriter` — galion-backup's writer moved verbatim. Two changes only:
  uname/gname come from a pluggable `names::Names` (default `EtcNames`:
  pure-std `/etc/passwd` / `/etc/group`), and device major/minor are split in
  std (`num::dev_split`, the glibc/musl Linux encoding) instead of
  `libc::major/minor`.
* `ustar` — galion-deploy's writer/reader moved verbatim (`Entry`, `Kind`,
  `header`, `write_entry`, `finish`, `read_all`).
* `read::Reader` — **new** streaming reader for both formats: GNU `L`/`K` long
  names, base-256 numbers, hard links, char/block devices, fifos, ustar
  `prefix` (POSIX magic only), minimal pax `x` (path, linkpath, size, uid,
  gid, uname, gname, mtime) and skipped pax `g`; unsigned or signed checksum;
  truncation is an error. It reads GNU tar's own archives member for member.
* `num` — octal / base-256 fields, checksums, `dev_split`.

Consumers:

* galion-backup `src/tarw.rs` is now policy only: `backup_exclude`, a
  `LibcNames` (`getpwuid_r`/`getgrgid_r`, what GNU tar calls, so non-`files`
  NSS sources still match) and `tarw::writer(out)`. `backup.rs` changed in
  two call sites (`TarWriter::new` -> `tarw::writer`).
* galion-deploy `src/tar.rs` is `pub use galion_tar::ustar::*;` so every
  `galion_deploy::tar::...` path is unchanged.

## Byte parity

* galion-backup real parity test (bash script vs Rust, unchanged test):
  26/26, `files.tar byte-identical` — glibc release build and the static
  x86_64-musl build.
* `crates/galion-tar/tests/gnu_oracle.rs` (GALION_REAL=1): real GNU tar 1.35
  and galion-tar archive the same scratch tree (dirs, empty/512/10241-byte
  files, 0600 file, short and 150-byte symlink, hard link, fifo, 120+ byte
  path): 30720 bytes each, identical. GNU tar lists and extracts the ustar
  output.
* galion-deploy: ustar code moved without edits (only the `octal` helper now
  lives in `num::octal_strict`, same body), so `backup.tar` bytes cannot change;
  its real backup/verify and rehearsal tests pass unchanged.

## Not done / follow-ups

* galion-deploy's `backup.tar` stays POSIX ustar (owner decision D-T1 below).
* `galion-deploy backup-verify` could also check galion-backup's
  `files.tar` with `read::Reader`; the outer `.tar.gz` needs an inflater,
  which galion-deploy (zero deps) does not have. Not started.
* galion-backup still needs zig-cc (or a pure-Rust SQLite) for aarch64/armv7
  (pre-existing, libsqlite3-sys C build); galion-tar itself builds for every
  target.

## Owner decisions

* **D-T1 deploy archive format**: (a) keep `backup.tar` POSIX ustar
  (recommended: universally readable, verify already proven, no change), or
  (b) switch to the GNU writer for one format (long names/targets without the
  155+100 limit, but the archive then reads file bytes itself, so the
  manifest sha must be taken from the archive member, not the collected
  bytes).
* **Merge order**: this branch is rust-ports 26a2a8e + cutover-prep a5ac49b +
  one commit; it should merge after both (or onto integration-3, slot 110,
  which carries both). Expected conflict surface: `Cargo.lock` only.
