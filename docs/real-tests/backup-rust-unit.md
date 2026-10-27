<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Slot 269 backup-rust-unit-prep — real tests (2026-10-27, Europe/Zagreb CET +01:00)

Branch `swarm/backup-rust-unit`, base main-staging 7c95348b fast-forwarded to
98466ab (the commit the live `galion-backup-98466ab` ELF was built from), so
the tests run against the code that is live.

## Context: already deployed

The task was "unit/drop-in proposal + box test; no live unit change". At
18:20 CET the t273u deploy had already switched the timer to the Rust ELF via
a drop-in (CALL-RECORD "DONE Mate t273u/t274u deploy-all"). This slot does
not touch any live unit. Instead it:

1. mirrors the live unit set as-built in `crates/galion-backup/deploy/`
   (base unit, `galion-backup.service.d/90-deploy-t273u.conf`, timer),
2. adds `tests/deploy_units.rs` (offline: ExecStart reset semantics, rollback,
   negative control) and `tests/real_unit_live.rs` (GALION_REAL=1, read-only
   drift check against the VPS),
3. re-runs the bash-vs-Rust parity suite on the **exact live binary**.

No VPS sandbox run was made by this slot: galion-backup is not a sandboxed
tool (no openmind-sandbox / cgroup-detect), and t273u already ran it in the VPS
sandbox (/var/tmp/galion-backup-sandbox-t273u, 4/4 DB verify) before going
live. This slot's VPS access was read-only only.

## 1. Live binary = tested binary

| what | sha256 |
|---|---|
| VPS `/opt/galion/rust/bin/galion-backup-98466ab` (static-pie x86_64, 1,959,376 B) | `fb26d673cfe6c615bf7272c81b0b076d92f4c05249bc5b8d40440b18fc1a2163` |
| box t273u artifact `target/backup-deploy-t273u/x86_64-unknown-linux-musl/release/galion-backup` | same `fb26d673…2163` |
| VPS `galion-backup.sh` | `0897efc0…80a4` = the parity oracle `e43d7ab:deploy/foundation/galion-backup.sh` |
| VPS `galion-backup-pre-t273u` (old helper, = `galion-backup`) | `3df3f88d…1d09` |

Rebuilding 98466ab in this slot's worktree with the same zig-cc env gave a
different file (1,963,728 B, `5ec7c802…633c`; same SQLite compile options).
The build is not bit-reproducible across target dirs/flags (likely
`CARGO_INCREMENTAL=0` / codegen-unit layout). So parity was run on the t273u
artifact itself, not on a rebuild.

## 2. Parity on the live binary (box, unshare -rm chroot, fake VPS tree)

`GALION_REAL=1 GALION_BACKUP_BIN=<t273u artifact> cargo test --release -p galion-backup --test real_parity -- --nocapture`
18:25:51–18:26:22 CET: **RESULT 26 passed, 0 failed.** S1 full backup
(files.tar byte-identical, 34 members, db/MAP + SHA256SUMS byte-identical,
extracted trees identical, gz 6911 B bash vs 6937 B rust), S2 rotation KEEP=3,
S3 lock held → "already running" exit 0, S4 low disk + MINFREE_KB=abc exit 1,
S5 missing DB warn+continue, S6 corrupt DB exit 1 + cleanup, S7 SIGTERM →
exit 143, only .lock left.

## 3. Offline unit tests

`cargo test -p galion-backup --test deploy_units`: 6/6 — effective ExecStart
with base + 90-deploy-t273u + offsite = `[galion-backup-98466ab run]` (single),
Type oneshot, UMask 0077, Nice 19, IO idle, offsite ExecStartPost/ExecStopPost
kept; without the drop-in = `[galion-backup.sh]` (rollback); negative control:
drop-in without the empty `ExecStart=` yields 2 ExecStart (bash then Rust);
drop-in touches only ExecStart, argv = 2 (`run`, main.rs needs argc == 2);
timer unchanged; SPDX headers.

## 4. Live drift check (VPS read-only)

`GALION_REAL=1 GALION_VPS_SSH=slavik-vps cargo test --release -p galion-backup --test real_unit_live -- --nocapture`
18:28:14–18:28:27 CET: **16 PASS, 0 FAIL**: live file set = base +
90-deploy-t273u.conf + offsite.conf + timer; base/drop-in/timer settings =
repo mirror; effective ExecStart single Rust; systemd's own `systemctl show`
ExecStart path + argv = `galion-backup-98466ab run`; Type oneshot; last
Result=success, ExecMainStatus=0; timer active+enabled; ELF sha = t273u
artifact; .sh still present with oracle sha; journal: 1 Rust ok run, 0 Rust
error lines.

Negative control 18:28:44 CET: repo mirror ExecStart changed to
`galion-backup-deadbee run` → `FAIL drop-in settings = repo mirror`, RESULT
1 failed (test fails). Mirror restored.

## 5. Live runs (journal, read-only, times converted to CET +01:00)

| run | implementation | log size_kb | real file size | .age size |
|---|---|---|---|---|
| 07:22:08 | bash | 1 | 18,642,497 B | 18,647,241 B |
| 13:17:13 | bash | 1 | 18,643,148 B | 18,647,892 B |
| 18:20:06 | **Rust 98466ab** (t273u manual start) | 2629 | 18,729,100 B | 18,733,860 B |

Archive layout (`tar -tzf | cut -d/ -f2 | uniq -c`) identical for the 13:17
bash and 18:20 Rust archives: SHA256SUMS 1, db/ 6 entries, files/ 2 entries.
Offsite hook after the Rust run: `msg=encrypted` + `msg=ok new=1 kept=12`.
Next timer run due 19:17 CET (+ up to 5 min random delay); `real_unit_live`
can be re-run after it to see `rust ok runs=2`.

B2 (known, docs/cutover/galion-backup.md): `size_kb` is `du -k` right after
rename; on ZFS the blocks are not yet allocated, so bash logged 1 and Rust
logged 2629. Faithful port; fixing it (use file length) is owner decision
D-269-2. Nothing reads `size_kb` (offsite script reads the file itself).

Disk: `/` 30 % used, 73,539,968 KiB free (MINFREE_KB default far below).

## Not done / owner

* D-269-1 fold drop-in into the base unit and retire `.sh` +
  `galion-backup-pre-t273u` after 12 clean timer runs (3-day retention window).
* D-269-2 B2 size_kb fix.
* D-269-3 reproducible release builds (two builds of 98466ab differ).
* aarch64/armv7/android checks not re-run: no `src/` change in this slot
  (tests + deploy mirrors + docs only).
