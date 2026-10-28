<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: shared-tar-mergefix (wave-4 slot 242, ports 21120-21129 unused)

Why: the main merge skipped shared-tar-crate 820740e because
`gaia-core/tests/workspace_ownership.rs` failed on the new, unmapped
`galion-tar` crate. Fix: merge swarm/main-staging into 820740e on this branch
(main-staging itself untouched), map `galion-tar` (survival / DEMETER, pending
Slavik, next to `galion-backup`), bump the counts (68 crates, 56 system crates)
in `gaia-core` and `gaia/tests/commands.rs`, and add a row to
`docs/gaia-ownership.md`. All times Europe/Zagreb (CEST +02:00).

## Merge

- e4f1f17 (01:10): merge main-staging 7e2a12e1. One conflict,
  `crates/galion-backup/src/backup.rs`: kept the shared-tar side
  (`tarw::writer(gz)`, i.e. `galion_tar::gnu::TarWriter` with libc names) and
  main-staging's `GzWriter::new(.., o.gzip_level as i32)` (backup-perf 5c3a8ed).
- 1fa4127 (01:31): merge main-staging 7c95348b (CI fixes for PR #12), clean.
  The branch head is a descendant of main-staging 7c95348b, so it merges
  fast-forward.

## Gate (same steps as scratch/main-merge-20261021/gate.sh, own target dir, -j 4)

| head | tests (excl. sandbox, cgroup-detect, hermes-tools) | repo-check | cargo-deny | no-python |
|---|---|---|---|---|
| e4f1f17 | 370 suites, 1957 passed, 2 failed then passed on retry (hermes-rs lib, room lib: known flakies), 18 ignored | 2088 files, 0 errors, 79 warnings, PASS | ok | ok |
| 1fa4127 | 370 suites, 1958 passed, 0 failed, 18 ignored | 2094 files, 0 errors, 79 warnings, PASS | ok | ok |

`cargo fmt --check` ok; `cargo clippy --workspace (same excludes) --all-targets -D warnings` ok
(only warnings are inside vendored candle-core, not denied).

## GALION_REAL=1 (release, box)

01:44:32 `GALION_REAL=1 cargo test --release -p galion-tar --test gnu_oracle -- --nocapture`

```
oracle: tar (GNU tar) 1.35
gnu tar 30720 bytes, galion-tar 30720 bytes, identical: true
reader: 13 members from GNU tar's archive
test result: ok. 2 passed; 0 failed
```

01:44:39-01:45:10 `GALION_REAL=1 cargo test --release -p galion-backup --test real_parity -- --nocapture`
(the live bash galion-backup.sh vs the Rust binary, merged tree):

```
PASS files.tar byte-identical
   files.tar: 34 members, 235520 bytes
PASS SHA256SUMS byte-identical
PASS extracted files.tar trees identical
PASS SIGTERM: exit 143, only .lock left (no .work-*, no .part, no archive)
RESULT: 26 passed, 0 failed
```

galion-deploy real_backup / real_build / real_failfast / real_rehearsal /
systemd_backend (ungated, they use galion-tar's ustar writer/reader) pass in
the gate run above.

## Targets

`cargo check` for galion-tar and galion-deploy: x86_64-musl, aarch64-musl,
aarch64-linux-android, armv7-musleabihf all ok. galion-backup on
armv7/android fails in `libsqlite3-sys` (bundled SQLite C needs
`arm-linux-musleabihf-gcc`, not on the box). Pre-existing since rust-ports
(rusqlite "bundled"), not caused by galion-tar.

## VPS (read-only, 01:31 CEST)

- nerve `GET 127.0.0.1:8670/state`: 200, cycle 18, score 61, grade D
  (open edge incident: tunnel routes 12 hostnames to 7 closed origin ports).
- cortex `GET 127.0.0.1:8674/health`: 200.
- GNU tar on the VPS: 1.35 (same as the box oracle).
- galion-backup.timer active (last trigger 2026-10-21 20:17 CEST; live backup
  is still the bash script; this branch changes nothing live).
- Slip: one probe wrote the /state body to /tmp/.n242 on the VPS and removed
  it in the same command; no secrets in it.
