<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# galion-backup: drop-in fold + .sh retirement (D-269-1 prep, slot 311)

Prepared 2026-10-27 ~22:00 CET (+01:00) on `swarm/backup-dropin-fold` (off
`swarm/backup-rust-unit` eada2741). **Nothing here has been installed. No live
unit was changed.** The fold and the retirement are live unit changes and
need the gate below to be open plus an explicit go.

## What is staged

| File | Role |
|---|---|
| `crates/galion-backup/deploy/staged/galion-backup.service` | Folded base unit: the live base with `ExecStart=/opt/galion/rust/bin/galion-backup-98466ab run` instead of the `.sh`. Every other key is unchanged. |
| `crates/galion-backup/deploy/galion-backup.service` | As-built live base (still names the `.sh`). After the fold this is the **rollback** file. |
| `crates/galion-backup/deploy/galion-backup.service.d/90-deploy-t273u.conf` | As-built live drop-in. It is removed by the fold. |
| `crates/galion-backup/tests/fold_units.rs` | Offline checks (always run, 11 tests) |
| `crates/galion-backup/tests/real_fold_gate.rs` | GALION_REAL=1 read-only checks against the VPS: whether the fold is neutral, the gate, and what still blocks retirement |
| `crates/galion-backup/tests/units/gate.rs` | Journal → runs → gate logic |

## The gate: 12 consecutive clean timer runs

Twelve runs is the 3-day retention window (`kept=12`). A **clean** run meets all of these:

* systemd logged `Starting` and then `Finished galion-backup.service`.
* There was no `Failed with result`, no `Failed to start`, and no non-zero `Main process exited`.
* The Rust ELF logged `msg="ok"`.
* Neither the ELF nor `galion-offsite-encrypt` logged `level=error` or `level=warn`.

Only **timer** runs count. A timer run starts at hour%6==0, minute 17–23, in the
VPS clock (Etc/UTC). That window is `OnCalendar=00/6:17` plus up to 5 min of
random delay. Manual starts are listed but never counted. Any unclean timer run
resets the streak to 0.

Status at 2026-10-27 21:56 CET: **CLOSED, streak 1/12**. That covers the
19:18:58 CET timer run. The 18:20:06 CET deploy smoke was a manual start and
does not count. There have been 0 unclean runs.
Earliest opening, if every run is clean: run #12 is the 2026-10-30 12:17 UTC
tick, which is **2026-10-30 ~13:17–13:23 CET (+01:00)**.

To re-check (read-only):
`GALION_REAL=1 GALION_VPS_SSH=slavik-vps cargo test --release -p galion-backup --test real_fold_gate -- --nocapture`
→ `GATE OPEN|CLOSED: streak N/12`.

## Step 1: fold (live unit change; only when the gate is OPEN and approved)

Pre-checks:
1. `real_fold_gate` shows GATE OPEN, `unclean_timer=0`, and PASS on "folded +
   live offsite == live effective unit (all keys)". If a newer deploy drop-in
   has replaced 90-deploy-t273u (for example the D-269-2 size_kb fix from slot
   304), this check FAILS by design. In that case re-stage the folded
   ExecStart for the new ELF and restart the 12-run gate for that ELF.
2. `real_unit_live` is 16/16.
3. Check the time. Do not fold within 30 min of a tick (00/06/12/18:17 UTC =
   01/07/13/19:17 CET).

Order A is the recommended order. Every intermediate state keeps a single
ExecStart; `fold_units::every_install_order_keeps_one_rust_or_sh_execstart`
checks this.
```
install -m 0644 -o root -g root <staged galion-backup.service> /etc/systemd/system/galion-backup.service
# intermediate: folded base + old drop-in -> still exactly the Rust ELF (drop-in resets ExecStart)
rm /etc/systemd/system/galion-backup.service.d/90-deploy-t273u.conf
systemctl daemon-reload
systemd-analyze verify /etc/systemd/system/galion-backup.service
systemctl show galion-backup.service -p ExecStart -p DropInPaths   # Rust ELF run; only offsite.conf
```
Don't start the unit by hand. Let the next timer tick run it, then check its
journal for `msg="ok"` and `Finished`.
The repo follow-up goes in one commit: move `deploy/staged/galion-backup.service`
to `deploy/galion-backup.service`, delete the drop-in mirror, and update
`deploy_units.rs` and `real_unit_live.rs`. After this, the "live still has the
t273u drop-in" check in `real_fold_gate` fails as intended, so retire that test.

**Rollback after the fold:** reinstall the as-built
`deploy/galion-backup.service` (the `.sh` one) and run `systemctl daemon-reload`.
Another way is to restore the drop-in. This works only while the `.sh` still
exists, which is why Step 2 comes later.

## Step 2: retire the .sh and the pre-t273u helper ELF (checklist)

Gate: Step 1 is done, then there are at least 4 more clean timer runs (1 day)
on the folded unit, and the owner says go.
Read-only findings from `real_fold_gate` (2026-10-27 21:57 CET):

- [ ] **Blocker:** `/opt/galion/rust/bin/galion-restore-test.sh` line 11 calls
      `/opt/galion/rust/bin/galion-backup verify`. That is the pre-t273u helper
      ELF (`galion-backup`, the same file as `galion-backup-pre-t273u`, sha256
      `3df3f88d…1d09`). Repoint it to `galion-backup-98466ab verify` (the Rust
      main.rs supports `verify <db>`). Then run the restore drill once more and
      expect the 43/43 shape (checksums ok, files restored, no MISSING) before
      retiring the helper.
- [ ] Repoint `/opt/galion/offsite/README.md` (lines 12, 46). It names
      `galion-backup.sh` and `galion-backup verify`. This is docs only.
- [ ] Repoint the `/opt/galion/offsite/galion-offsite-encrypt.sh` comment
      (line 17). It says "galion-backup.sh resets $SRC to 0700". This is only a
      comment: the Rust ELF does the same `chmod 0700` on DEST
      (`src/backup.rs:371`), so the offsite 0710 re-grant still works. Live now:
      `/var/backups/galion` is `710 root:galion-offsite`.
- [ ] Re-run `real_fold_gate`. The "refs to galion-backup.sh" and "refs to
      helper ELF" lists should list only the files being retired.
- [ ] **Archive, don't delete.** Move `galion-backup.sh` (sha256 `0897efc0…80a4`,
      the parity oracle), `galion-backup` and `galion-backup-pre-t273u` to
      `/opt/galion/rust/bin/retired-<date>/` (mode 0700). Record their sha256
      values in the report. After this, rollback means restoring from that
      folder, then reinstalling the as-built base.
- [ ] Keep `real_parity.rs` runnable: it needs the `.sh` as its oracle. Point
      `GALION_BACKUP_SH` (or an equivalent) at the archived copy.
- [ ] Optional: use a stable binary name. Point
      `/opt/galion/rust/bin/galion-backup` (now free) at the current ELF as a
      symlink, so later upgrades don't need a unit edit. This needs a separate
      decision.

## Observability gap found

The nerve (`127.0.0.1:8670/state`, read 2026-10-27 ~21:58 CET) has **no
galion-backup component**. A failed backup timer run would raise no nerve
alert today. The gate test is the only automated check. Suggestion for the
backlog: a nerve probe on `systemctl show galion-backup.service -p Result`
plus the timer's `LastTriggerUSec` age (alert at >6 h 10 min).
