<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: rust-ports (slot 47, ports 19470-19479)

All times Europe/Zagreb, CEST (+02:00). Box: Debian 13 (trixie), unprivileged
user, `unshare -rn` / `unshare -rm` permitted. Nothing written on the VPS.

## 1. VPS read-only probes (2026-10-20 22:56-23:00 +02:00)

`ssh ... root@51.38.137.193` with `systemctl cat galion-backup*`,
`systemctl list-timers`, `sha256sum`/`cat` of the script, `stat`, `df`,
`tar -tv` (names/modes only) of the newest archive, `journalctl -u
galion-backup -n 4`. No secret values printed (no file contents other than
the script, MAP and SHA256SUMS file-name column).

* Script sha256 `0897efc0...80a4` = git `e43d7ab:deploy/foundation/galion-backup.sh` (diff: identical).
* Timer: last run 2026-10-20 20:22:05 +02:00, next 2026-10-21 02:20:50 +02:00.
* `/var/backups/galion` is `710 root:galion-offsite` between runs (offsite
  ExecStopPost); the script sets 0700 at each run start. Archives 0600 root:root,
  ~18.5 MB each.
* Service locale `LANG=C`; GNU tar 1.35; gzip 1.12.
* Newest archive: outer members in readdir order (SHA256SUMS, files/, files/files.tar, db/, ...);
  SHA256SUMS names `./db/MAP ./db/hermes-legacy-state.db ./db/hermes-rs-state.db
  ./db/memory-root-vectors.db ./db/memory-vectors.db ./files/files.tar` (C order);
  files.tar 1845 members, first `var/lib/memory-tencentdb/` (unsorted readdir order);
  MAP = the 4 DB lines in script order.

## 2. galion-backup: bash vs Rust parity (GALION_REAL=1)

`GALION_REAL=1 GALION_BACKUP_BIN=$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/galion-backup cargo test --release -p galion-backup --test real_parity -- --nocapture`

The original script (from git, sha256 checked) and the static musl release
binary run against the same fake VPS tree at the real absolute paths inside
`unshare -rm` + chroot (bind-mounted /usr, /dev, /proc, /etc/alternatives).
Fake tree: 4 WAL-mode SQLite DBs with live -wal files, nested dirs, `.backup`
dir, `*.db`-named dir, `keep.db.bak`, 100+-byte path, symlink, dangling
symlink, hard link, fifo, empty file, 200 KB binary, all 5 unit files, one
missing env file, dns-backup glob with mixed case + broken symlink. Result
**26 passed, 0 failed**:

```
[2026-10-20T23:12:45+02:00] rust binary under test: /workspace/swarm/target/rust-ports/x86_64-unknown-linux-musl/release/galion-backup
[2026-10-20T23:12:45+02:00]    bash: ts=2026-10-20T23:12:45+02:00 svc=galion-backup level=info msg="ok" archive=galion-20261020-231245+0200.tar.gz size_kb=8 kept=1
[2026-10-20T23:12:45+02:00]    rust: ts=2026-10-20T23:12:45+02:00 svc=galion-backup level=info msg="ok" archive=galion-20261020-231245+0200.tar.gz size_kb=8 kept=1
[2026-10-20T23:12:45+02:00] PASS exit codes equal (0)
[2026-10-20T23:12:45+02:00] PASS log lines equal (ts/stamp/size blanked)
[2026-10-20T23:12:45+02:00] PASS DEST mode 0700 both
[2026-10-20T23:12:45+02:00] PASS .lock mode equal (0600)
[2026-10-20T23:12:45+02:00] PASS one archive each
[2026-10-20T23:12:45+02:00] PASS archive mode 0600 both
[2026-10-20T23:12:45+02:00] PASS DEST contents equal (no .work-*, no .part)
[2026-10-20T23:12:45+02:00] PASS outer listing equal (order, modes, owners, sizes)
[2026-10-20T23:12:45+02:00] PASS files.tar byte-identical
[2026-10-20T23:12:45+02:00] PASS files.tar listing equal
[2026-10-20T23:12:45+02:00]    files.tar: 34 members, 235520 bytes
[2026-10-20T23:12:45+02:00] PASS db/MAP byte-identical
[2026-10-20T23:12:45+02:00] PASS SHA256SUMS byte-identical
[2026-10-20T23:12:45+02:00] PASS extracted snapshot dirs identical (bytes + modes)
[2026-10-20T23:12:45+02:00] PASS extracted files.tar trees identical
[2026-10-20T23:12:45+02:00]    gz size bash 6904 B, rust 6960 B
[2026-10-20T23:12:45+02:00] == S2 rotation KEEP=3 with 14 older archives + a non-matching file
[2026-10-20T23:12:45+02:00] PASS rotation exit 0 both
[2026-10-20T23:12:45+02:00] PASS rotation log equal (kept=3)
[2026-10-20T23:12:45+02:00]    survivors bash ["galion-20260104-000013+0100.tar.gz", "galion-20260106-000006+0100.tar.gz"]
[2026-10-20T23:12:45+02:00] PASS same survivors (mtime order, name tie-break)
[2026-10-20T23:12:45+02:00] == S3 another run holds DEST/.lock
[2026-10-20T23:12:45+02:00] PASS locked: exit 0 both, 'already running', no archive
[2026-10-20T23:12:45+02:00] == S4 low disk (MINFREE_KB above free space) and non-integer MINFREE_KB
[2026-10-20T23:12:45+02:00]    rust: ts=2026-10-20T23:12:45+02:00 svc=galion-backup level=error msg="low disk 43708204KB"
[2026-10-20T23:12:45+02:00] PASS low disk: exit 1 both, same log
[2026-10-20T23:12:45+02:00] PASS low disk: DEST created 0700, nothing else
[2026-10-20T23:12:45+02:00] PASS MINFREE_KB=abc: exit 1 both, same log
[2026-10-20T23:12:45+02:00] == S5 one DB missing (warn, continue)
[2026-10-20T23:12:45+02:00]    rust: ts=2026-10-20T23:12:45+02:00 svc=galion-backup level=warn msg="missing db /opt/galion/hermes/state.db" | ts=2026-10-20T23:12:45+02:00 svc=galion-backup level=info msg="ok" archive=galion-20261020-231245+0200.tar.gz size_kb=8 kept=1
[2026-10-20T23:12:45+02:00] PASS missing db: exit 0 both, same warn + ok lines
[2026-10-20T23:12:45+02:00] PASS missing db: neither archive has it
[2026-10-20T23:12:45+02:00] == S6 corrupt DB (snapshot fails: abort, exit 1, cleanup)
[2026-10-20T23:12:45+02:00]    rust stdout: ts=2026-10-20T23:12:45+02:00 svc=galion-backup level=error msg="snapshot failed /var/lib/hermes-rs/rs-state.db"
[2026-10-20T23:12:45+02:00]    rust stderr: galion-backup: backup: not an error
[2026-10-20T23:12:45+02:00] PASS corrupt db: exit 1 both, same stdout and stderr
[2026-10-20T23:12:45+02:00] PASS corrupt db: no archive, no .work-* left
[2026-10-20T23:12:45+02:00] == S7 SIGTERM mid-run (rust): work dir removed, exit 143
[2026-10-20T23:12:45+02:00]    exit=Some(143) signal=None left=[".lock"]
[2026-10-20T23:12:45+02:00] PASS SIGTERM: exit 143, only .lock left (no .work-*, no .part, no archive)
[2026-10-20T23:12:45+02:00] RESULT: 26 passed, 0 failed
ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.59s
```

## 3. galion-guard namespace test, Rust port of scripts/real/guard-rust.sh

`GALION_REAL=1 cargo test --release -p galion-guard --test real_netns`
(release binary, `unshare -rn`, real nft 1.1.3). **38 passed, 0 failed
(backend=nft)**, same 38 checks as the bash run of 2026-10-20 21:26 +02:00.
Full output in docs/real-tests/guard-rust.md section A2.

## 4. Unit / integration tests

* `cargo test --workspace`: 136 suites, 632 passed, 0 failed (gated real
  tests print "skipped").
* galion-backup: sha256 FIPS vectors + split points, CRC-32 vector, gzip
  round-trip via miniz inflate, tar exclude semantics / octal / base-256 /
  checksum, stamp format; `tests/fake_tree.rs` (plain cargo test, no
  namespace): contents + modes + manifest checked with system GNU tar and
  `sha256sum -c`, rotation, low disk, lock held, missing DB, corrupt DB.
* Builds: x86_64-musl and aarch64-musl release (static, 1.58 / 1.61 MB),
  armv7-musleabihf and aarch64-android `cargo check` (SQLite C via zig cc
  stopgap, see third-party doc).
