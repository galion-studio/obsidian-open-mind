<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: backup-dropin-fold-prep (slot 311, D-269-1 prep)

All times are Europe/Zagreb CET (+01:00). The VPS journal is in Etc/UTC and
its times are converted here. Every VPS access was read-only: `systemctl cat`,
`systemctl show -p`, `journalctl`, `grep -l`, `stat`, and a GET on the nerve.
**No live unit change.** No ports were needed, because nothing serves HTTP
(the assigned range is 21810-21819 and none of it was used).

## Offline (box), 2026-10-27 ~21:55 CET
`cargo test -p galion-backup --test fold_units --test deploy_units`
→ fold_units 11/11 and deploy_units 6/6 pass.
* `systemd_analyze_verify_folded`: real `systemd-analyze --user verify` (systemd
  257) accepts the staged unit, with the ELF path swapped for /bin/true. The
  negative control (no Type, 2× ExecStart) is rejected with "more than one ExecStart".
* Mutation check: changing `Nice=19` to `18` in the staged unit makes 2 tests
  fail (`folded_is_behaviour_neutral_vs_live_set` and
  `folded_differs_from_base_only_in_execstart`). The change was then reverted.
* `gate_reads_real_t273u_journal` uses the real journal captured from the VPS
  (`tests/fixtures/journal-t273u-20261027.txt`). It finds 2 runs: manual
  18:20:06 CET and timer 19:18:58 CET, both clean, streak 1.

## Real, read-only against the VPS, 2026-10-27 21:56:52–21:57:04 CET
`GALION_REAL=1 GALION_VPS_SSH=slavik-vps cargo test --release -p galion-backup --test real_fold_gate -- --nocapture`
```
live unit files: [/etc/systemd/system/galion-backup.service, …/90-deploy-t273u.conf, …/offsite.conf]
PASS live still has the t273u drop-in (fold not yet done)
PASS live offsite.conf present
compared 8 effective keys
PASS folded + live offsite == live effective unit (all keys)
PASS systemd's own ExecStart = folded ExecStart
run start=2026-10-27T17:20:06+00:00 kind=manual clean=true (finished=true failed=false rust_ok=true bad_lines=0)
run start=2026-10-27T18:18:58+00:00 kind=timer clean=true (finished=true failed=false rust_ok=true bad_lines=0)
GATE CLOSED: streak 1/12 clean timer runs (timer=1 manual=1 unclean_timer=0)
PASS no unclean timer run since t273u
PASS at least one clean timer run
timer: NextElapseUSecRealtime=Wed 2026-10-28 00:20:27 UTC (= 01:20:27 CET) | LastTriggerUSec=Tue 2026-10-27 18:18:58 UTC (= 19:18:58 CET)
refs to galion-backup.sh: [galion-backup.service.d/90-deploy-t273u.conf (comment), galion-backup.service, /opt/galion/offsite/README.md, /opt/galion/offsite/galion-offsite-encrypt.sh (comment)]
refs to helper ELF galion-backup (verify): [/opt/galion/rust/bin/galion-restore-test.sh, /opt/galion/rust/bin/galion-backup.sh, /opt/galion/offsite/README.md]
RESULT: 0 failed
```
The slot-269 drift test still passes at 21:57:19–21:57:29 CET:
`real_unit_live` 16/16 PASS, `RESULT: 0 failed`.

## Nerve (127.0.0.1:8670/state), 2026-10-27 ~21:58 CET
The nerve returned HTTP 200, but its state has no `galion-backup` component.
Recent nerve alerts concern other components (galion-one-mouth, edge tunnel,
cloudflared, hermes-rs restarts). The cortex was not used. Backup health is
therefore observed only through systemd and the journal (above).
See docs/galion-backup-fold.md under "Observability gap found".
