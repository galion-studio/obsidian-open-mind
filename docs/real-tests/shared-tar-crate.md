<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: shared-tar-crate (slot 122, ports 19920-19929 unused)

All on the box, 2026-10-21 (+02:00). No VPS access was needed or made: this
is a library move; the live services do not use these binaries yet.
Rust toolchain 1.99.1, GNU tar 1.35 (same version as the VPS).

## Unit + integration tests (debug)

`cargo test -p galion-tar -p galion-backup -p galion-deploy`

| suite | before (merge a658fb9) | after |
|---|---|---|
| galion-backup lib | 8 | 7 (2 tar tests moved to galion-tar, 1 new `LibcNames` vs `EtcNames` test) |
| galion-backup fake_tree / real_parity (ungated) | 3 / 2 | 3 / 2 |
| galion-deploy lib | 33 | 31 (2 ustar tests moved to galion-tar) |
| galion-deploy real_backup / real_build / real_rehearsal / systemd_backend | 1 / 1 / 4 / 1 | 1 / 1 / 4 / 1 |
| galion-tar lib | — | 9 |
| galion-tar gnu_oracle | — | 2 |

No test file under `crates/galion-backup/tests` or `crates/galion-deploy/tests`
was changed.

## GALION_REAL=1 runs (release)

04:08:00-04:08:34 — `GALION_REAL=1 cargo test --release -p galion-backup --test real_parity`
(bash galion-backup.sh vs Rust in an unprivileged namespace):

```
PASS files.tar byte-identical
PASS files.tar listing equal
   files.tar: 34 members, 235520 bytes
PASS extracted files.tar trees identical
RESULT: 26 passed, 0 failed
```

04:14:45 — same with `GALION_BACKUP_BIN=<x86_64-musl static build>`:
`PASS files.tar byte-identical`, `RESULT: 26 passed, 0 failed`.

`GALION_REAL=1 cargo test -p galion-tar --test gnu_oracle -- --nocapture`:

```
oracle: tar (GNU tar) 1.35
gnu tar 30720 bytes, galion-tar 30720 bytes, identical: true
reader: 13 members from GNU tar's archive
tar -tvf ustar:
drwxr-x--- 0/0               0 2025-10-09 10:53 opt/galion/rust/bin/
-rwxr-xr-x 0/0               9 2025-10-09 10:53 opt/galion/rust/bin/galion-nerve
lrwxr-x--- 0/0               0 2025-10-09 10:53 opt/galion/current -> rust
```

04:08:41-04:12:30 — `GALION_REAL=1 cargo test --release -p galion-deploy`:
lib 31, real_backup 1, real_build 1 passed; real_rehearsal 3/4 — `rehearsal_room`
failed at `git show 5bb27e1:deploy/staged/room.service` because that commit
(swarm/room) was not in this fresh clone (environment, not tar). After
`git fetch ../room` (object only, branch unchanged) it passed:
`rehearsal_room ... 1 passed` (F5 first install, redeploy outage 2 ms, L1
rollback 20196 ms). The rehearsal tests use their own fixed ports 19390-19393.

## Builds

* x86_64-unknown-linux-musl release: galion-deploy 639664 B, galion-backup 1588992 B.
* aarch64-unknown-linux-musl release (rust-lld): galion-deploy 565408 B.
* armv7-unknown-linux-musleabihf and aarch64-linux-android `cargo check`:
  galion-tar + galion-deploy OK.
* galion-backup for aarch64: fails in libsqlite3-sys (no aarch64-linux-musl-gcc),
  pre-existing (docs/galion-backup.md, zig-cc owner decision), not touched here.
* `cargo clippy --all-targets -D warnings` clean for all three crates;
  `cargo fmt --check` clean; scripts/no-python.sh ok; scripts/spdx-scan.sh missing 0.
