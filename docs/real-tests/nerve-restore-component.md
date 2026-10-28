<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: nerve restore-test component (wave-4 slot 378)

Run on 2026-10-28 between 09:44:53 and 09:46:43 CET (+01:00) on the BOX by wave-4 slot 378,
branch `swarm/nerve-restore-component` (base nerve-merge-6 9baa450c). Release build.
Ports 22480-22489 (nerve API on 22480). No Python was run. Nothing was written, restarted,
installed or built on the VPS. **NO deploy.**

## The gap (slot 361 follow-up)

The live nerve (galion-nerve, 127.0.0.1:8670, `--mode event`) has no component that
watches the backup restore test. Read-only probe 2026-10-28 09:46:54 CET:

```
restore-in-state-count: 0          # GET /state: no restore-test component
"cycle":73  "version":"0.1.0"
galion-nerve: active   galion-backup.timer: active
Unit galion-restore-test.service could not be found.
ls: cannot access '/var/log/galion': No such file or directory
newest archive: galion-20261028-072141+0100.tar.gz
/etc/galion-nerve/components-live.json sha256 dc3434937fbd… (unchanged, = fixture)
```

So today a failed or never-run restore test is invisible: backups are unverified.

## What changed

- New `Kind::Restore` component (`crates/openmind-nerve/src/restore.rs`, config/collect/state):
  reads ONLY the tail (64 KiB) of the restore-test log (`log`, absolute) and its mtime.
  Never runs the test, never restarts anything (`restarts_per_hour` forced to 0).
- Verdicts: `never` (no log), `unreadable`, `ok`, `failed` (slot 361 strict
  `galion-backup: restore-test: … FAILED`, gzip trailer, `integrity=` not ok, and the
  live `.sh` `: FAILED` lines even when it still says `restore test ok` — F-326-1),
  `incomplete` (no verdict line; older than 15 min), `empty`, `running` (unfinished
  tail < 15 min keeps the previous verdict). `ok` with `MISSING:` paths warns; an ok
  older than `max_age_h` (default 26 h) warns as stale.
- All of these are **warn only** (Degrading). Never hard-down, never fed to the reflex:
  the oneshot's inactive/activating/failed unit states and its new PID/enter time are
  ignored for this kind (they are its normal lifecycle, not restarts).
- Staged components entry `restore-test` (DEMETER / survival, not critical) in
  `deploy/staged/nerve/components-live.json`, log `/var/log/galion/restore-test.log`.
- Staged drop-in `deploy/staged/nerve/galion-restore-test.service.d/50-nerve-log.conf`
  (STAGED / NOT INSTALLED): `StandardOutput=truncate:/var/log/galion/restore-test.log`,
  `StandardError=inherit`, `LogsDirectory=galion` (0750). VPS systemd 255 supports
  `truncate:`.

## Real test (GALION_REAL=1)

```
GALION_REAL=1 NERVE_RESTORE_BIN=<target>/r361/release/galion-backup \
  cargo-slot test -j 2 --release -p openmind-nerve --test real_restore_component -- --nocapture --test-threads=1
```

`NERVE_RESTORE_BIN` is the real slot-361 `galion-backup` (swarm/restore-checksum-fail
2b24e88b) built from source. Each case runs the REAL restore test (or the live `.sh`, embedded
verbatim, sha256 checked) on a real archive with its output going to the log, then waits for a
nerve cycle that started after the log change and reads `/state` from the real `openmind-nerve serve`
(staged entry, log path moved into the temp dir, systemd/journal/reflex off, dry-run).
Each case also checks the log bytes and mtime were not changed by the nerve, `consecutive_down` 0
and 0 restarts.

Caveat: `galion-backup run` backs up a fixed root `/`, so the test archive is built
by hand in the same layout (DBs from `galion-backup migrate-baseline`, `db/MAP`, `files.tar`,
`SHA256SUMS`, `tar -czf`). The tamper variants are the slot-361 ones.

Output (verbatim, timestamps dropped, tmp paths shown as `<tmp>`):

```
test real_restore_component_judges_real_logs ... [2026-10-28T09:44:53+01:00] archives: clean 1120 B; tampered mismatch/dropped/unlisted/gztrailer; missing-env
PASS S0 no log yet: restore-test rc None -> nerve degrading state never | degrading: restore test never recorded (no log at <tmp>/log/restore-test.log); backups are unverified
PASS S1 clean archive: restore-test rc Some(0) -> nerve healthy state ok | 
PASS S2 digest mismatch: restore-test rc Some(1) -> nerve degrading state failed | degrading: restore test FAILED: galion-backup: restore-test: <tmp>/a-mismatch/galion-20261028-093300+0100.tar.gz: checksum check FAILED: 1 mismatched [./db/MAP], 0 unreadable, 0 malformed line(s), 0 unsafe name…
PASS S3 dropped SHA256SUMS line: restore-test rc Some(1) -> nerve degrading state failed | degrading: restore test FAILED: galion-backup: restore-test: <tmp>/a-dropped/galion-20261028-093300+0100.tar.gz: checksum check FAILED: 0 mismatched, 0 unreadable, 0 malformed line(s), 0 unsafe name(s), 1 unlis…
PASS S4 unlisted file: restore-test rc Some(1) -> nerve degrading state failed | degrading: restore test FAILED: galion-backup: restore-test: <tmp>/a-unlisted/galion-20261028-093300+0100.tar.gz: checksum check FAILED: 0 mismatched, 0 unreadable, 0 malformed line(s), 0 unsafe name(s), 1 unli…
PASS S5 gzip CRC trailer flip: restore-test rc Some(1) -> nerve degrading state failed | degrading: restore test FAILED: galion-backup: restore-test: <tmp>/a-gztrailer/galion-20261028-093300+0100.tar.gz: gzip check FAILED: gzip: invalid compressed data--crc error (stored 8580c437, computed 8580c436…
PASS S6 MISSING presence path: restore-test rc Some(0) -> nerve degrading state ok | degrading: restore test ok but 1 path(s) MISSING: /etc/openmind-memory.env
PASS S7 stale ok log (30 h old): restore-test rc Some(0) -> nerve degrading state ok | degrading: restore test stale: last run 30 h ago at 2026-10-27T03:46:03+01:00 (max 26 h)
PASS S8 interrupted run (20 min old): restore-test rc None -> nerve degrading state incomplete | degrading: restore test log has no verdict (interrupted run?); last line: verify db/hermes-rs-state.db integrity=ok tables=1
PASS S9 live .sh on mismatch (F-326-1: exit 0 + 'restore test ok'): restore-test rc Some(0) -> nerve degrading state failed | degrading: restore test FAILED: ./db/MAP: FAILED
PASS S10 recovery: clean run again: restore-test rc Some(0) -> nerve healthy state ok | 
PASS event mode: nerve "degrading" | degrading: restore test FAILED: galion-backup: restore-test: <tmp>/a-mismatch/galion-20261028-093300+0100.tar.gz: checksum check FAILED: 1 mismatched [./db/MAP], 0 unreadable, 0 malformed line(s), 0 unsafe name…
test result: ok. 1 passed; 0 failed; finished in 110.75s
```

## Owner decisions

- **D-378-1** (Mate / Slavik): to make the warning real, install together:
  (a) the slot-361 restore-test unit + timer (D-361-1 step 4, daily 03:47),
  (b) the drop-in `50-nerve-log.conf`, and (c) the staged `restore-test` components entry
  into `/etc/galion-nerve/components-live.json` + nerve restart. If (c) goes in before
  (a)+(b), the nerve warns "restore test never recorded" until the first run — that is the
  intended signal. Rollback: remove the entry and the drop-in, `daemon-reload`.
- **D-378-2**: in event mode a `unit.failed` event for galion-restore-test.service may still
  reach cortex/Hermes escalation. Decide whether to add it to the cortex/Hermes handoff
  exclusion list (as the wave-4 cortex-handoff-exclude row does for swarm test units) or keep it as a real alert.

## Gates (2026-10-28 ~09:55 CET, box)

- `cargo fmt --all --check` ok; clippy `--workspace --all-targets -D warnings`
  (excl. openmind-sandbox, cgroup-detect) exit 0.
- Workspace tests (excl. openmind-sandbox, cgroup-detect, hermes-tools): 1355 passed,
  0 failed, 6 ignored (281 suites). `openmind-nerve --lib` 127 passed; `staged_components` 3/3.
- `scripts/no-python.sh` ok. No new dependencies.
- musl release `openmind-nerve` size vs base 9baa450c:
  x86_64 1,479,456 -> 1,495,840 B (+16,384); aarch64 (rust-lld) 1,221,440 -> 1,234,408 B (+12,968).
