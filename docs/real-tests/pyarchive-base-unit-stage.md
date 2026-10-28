<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: pyarchive base-unit stage (wave-4 slot 372)

Branch `swarm/pyarchive-base-unit-stage` off `swarm/py-archive-rollback-note` dd74d6c0. Times Europe/Zagreb CET
(+01:00). VPS read-only for the live system: all work in a fresh `/var/tmp/unitstage-372-<ts>/` (0700), removed at
the end; no write under /etc, no daemon-reload, no restart. CLI/ssh only, so slot ports 22420-22429 were not used.
No secrets below.

## Live probe (06:21:55 CET)

`systemctl cat galion-desk.service`: base line 6 `ExecStart=/opt/galion/bin/galion-desk`; drop-in
`rust-desk-hermes.conf` line 2 `# Rollback: rm this drop-in; systemctl daemon-reload; systemctl restart galion-desk`,
`ExecStart=` reset, `ExecStart=/opt/galion/rust/bin/galion-desk`, `Environment=DESK_BIND=127.0.0.1:8646`,
`Environment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`. sha256 base `a1ebbdd3…8d9a`… (full in
`docs/ops/pyarchive-base-unit-stage.md` §1). systemd 255 (255.4-1ubuntu8).

## Real test (06:25:44–06:26:36 CET)

`GALION_REAL=1 GALION_PYARCHIVE_SSH=root@<vps> GALION_PYARCHIVE_BIN=<x86_64 musl release, 565 936 B> cargo test -p galion-pyarchive --test real_vps_stage -- --nocapture`

```
verify gd372live.service: exit Some(0)
verify gd372fold.service: exit Some(0)
verify gd372reword.service: exit Some(0)
verify gd372badbase.service: exit Some(1)
gd372badbase.service: Command /opt/galion/bin/galion-desk-missing-372 is not executable: No such file or directory
verify gd372baddrop.service: exit Some(1)
gd372baddrop.service: Command /opt/galion/rust/bin/galion-desk-missing-372 is not executable: No such file or directory
tree copy size 299K
plan live: exit Some(0) verdict "PASS" files 5 units 338 active 0 overridden_base 1 stale_notes 1
plan reword D-345-2: exit Some(0) verdict "PASS" files 5 units 338 active 0 overridden_base 1 stale_notes 0
plan fold D-345-1: exit Some(0) verdict "PASS" files 5 units 338 active 0 overridden_base 0 stale_notes 0
live before/after: (identical)
MainPID=264677  NRestarts=0  ExecStart path=/opt/galion/rust/bin/galion-desk
Environment=DESK_BIND=127.0.0.1:8646 DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes  ActiveState=active
DropInPaths=/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf
a1ebbdd3a80ad573310df646ce112324e803b3dc6c4e71a8d0d971364f2dbcbf  /etc/systemd/system/galion-desk.service
e3a0e10acb135cd92aa06f17fb381f1c0f4d6f83b027ce2211f9f52212318d9a  /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf
test real_vps_staged_unit_options ... ok   (1 passed; 50.75 s)
```

What this proves:
- `systemd-analyze verify` (offline loader, PID 1 untouched) accepts the folded base (D-345-1) and the reworded
  drop-in (D-345-2) exactly as they would be installed (marker lines filtered), and the live pair.
- The negative controls fail with a precise message, so verify really checks ExecStart paths, including ones set in
  a drop-in next to the unit (the `gd372baddrop` case), i.e. the green results are meaningful.
- On a copy of the live systemd + cron trees and the 5 Phase A files, `galion-pyarchive plan --root`: live state
  reports the trap + stale note; D-345-2 keeps the trap flag but no stale note; D-345-1 removes the trap. All PASS,
  0 active references.
- Live galion-desk unit files and `systemctl show` are byte-identical before and after; folder removed
  (`ls /var/tmp/unitstage-372-*`: no such file, 06:27 CET).

Nerve (`127.0.0.1:8670/state`, 06:27 CET; cortex `:8674/health` 200, not consulted for this): galion-desk
`healthy`, `active/running`, pid 264677, port 8646, `restarts_1h` 0, `rss_kb` 1784; `:8646/health` 200. Open
incident is the unrelated `edge` (cloudflared, tuner-quick).

## Local gates

06:28–06:57 CET: `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` ok;
`cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`
(hermes-tools excluded: its sandboxed.rs tests would run the sandbox on the box, sandbox_vps_only): 383 results,
2030 passed, 0 failed, 18 ignored. A first fail-fast run hit the known `room` fd flake
(`default_and_fixture_do_not_open_a_socket`, fixed on slot 350 facc119e, not on this base); green on rerun.
`galion-pyarchive` 8 unit + 16 CLI. `scripts/no-python.sh` ok. musl release x86_64 565 936 B, aarch64 487 104 B
(`--help` runs under qemu-aarch64); `cargo check` armv7-unknown-linux-musleabihf, armv7-linux-androideabi,
aarch64-linux-android ok. No new dependencies.
CLI tests added: `staged_fold_d345_1_removes_the_trap` (effective ExecStart/Environment equal to the live merge;
plan PASS, overridden_base 0; forgotten drop-in still no trap; tree unchanged), `staged_reword_d345_2_keeps_flag_drops_stale_note`
(assignments identical, overridden_base 1, stale notes 0); unit test `restore_first_wording`. Mutation check: with
the dd74d6c0 lib (no `restore_first`), `staged_reword_d345_2_keeps_flag_drops_stale_note` FAILS (15 passed, 1 failed).
