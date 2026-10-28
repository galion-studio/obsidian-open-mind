<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: py-archive rollback note (wave-4 slot 345)

Branch `swarm/py-archive-rollback-note` off `swarm/py-archive-tool` 4a7dbd97. All times Europe/Zagreb CET (+01:00).
Read-only on the VPS: no deletes, no unit edits, no restarts. CLI only, so the slot ports 22150-22159 were not used.
No secrets below (unit lines with credential-like words are redacted by the tool).

## What changed

- `galion-pyarchive plan` records every base-unit assignment that names a Phase A file but is overridden by a
  drop-in (`Scan.overrides`, JSON `overridden_base[]`, text `OVERRIDDEN-BASE`): base file:line, `[section]`, key,
  target, overriding file:line + text, the Phase A files to restore together (`Item.needs`; old desk ELF needs
  `desk-hermes.py`), and any comment in the overriding drop-in mentioning "rollback" (`STALE ROLLBACK NOTE`).
- `safe_rollback()` prints the exact order: `mv -n` archived files back, `grep | sha256sum -c -` those lines of the
  manifest, `test -x`, then (unit-edit approval) remove the drop-in, `daemon-reload`, `systemctl show -p ExecStart`,
  restart + `is-active`. Text only, never executed (source guard test still bans mutating calls).
- Proposed commands: archive step now `mv -n` (no overwrite) and a new `# ROLLBACK of Phase A` block with the exact
  inverse `mv -n` per file + `sha256sum -c`; an `# OVERRIDDEN-BASE UNITS (n)` block when any exist.
- `docs/py-archive-plan.md` §5 row A rollback text fixed ("restore first"), new §8.

## VPS read-only probe (2026-10-28 03:25:20 CET)

`systemctl cat galion-desk.service` (base line 6 `ExecStart=/opt/galion/bin/galion-desk`; drop-in line 2
`# Rollback: rm this drop-in; systemctl daemon-reload; systemctl restart galion-desk`, line 5 `ExecStart=`, line 6
`ExecStart=/opt/galion/rust/bin/galion-desk`); `systemctl show` ExecStart = `/opt/galion/rust/bin/galion-desk`,
DropInPaths = that drop-in; `is-active` active.

## Real test: VPS (2026-10-28 03:27:56–03:28:15 CET)

`GALION_REAL=1 GALION_PYARCHIVE_SSH=root@<vps> GALION_PYARCHIVE_BIN=<x86_64 musl release, 565 936 B> cargo test -p galion-pyarchive --test real_vps -- --nocapture --test-threads 1`

```
box: verdict "FAIL" missing 5 units 285 cron 3 procs 112 active 0
vps: verdict "PASS" files 5 bytes 4752180 units 338 cron 9 procs 36 active 0 overridden 1 masked 0 shadowed 0 comment 2 unreadable 0
vps verify exit Some(0)
vps overridden-base: "galion-desk.service" "/etc/systemd/system/galion-desk.service":6 "ExecStart"= -> "/opt/galion/bin/galion-desk" by "/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf":6 notes [{"file":"/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf","line":2,"text":"# Rollback: rm this drop-in; systemctl daemon-reload; systemctl restart galion-desk"}]
  # STALE rollback note /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf:2 ("rm this drop-in") is UNSAFE after Phase A: do steps 1-3 first
  mv -n /opt/galion/archive/py-bin-20261103/opt__galion__bin__galion-desk /opt/galion/bin/galion-desk
  mv -n /opt/galion/archive/py-bin-20261103/opt__galion__bin__desk-hermes.py /opt/galion/bin/desk-hermes.py
  grep -E '  /opt/galion/bin/galion-desk$|  /opt/galion/bin/desk-hermes\.py$' /opt/galion/archive/py-bin-20261103/phase-a.sha256 | sha256sum -c -   # expect 2 OK
  test -x /opt/galion/bin/galion-desk
  # 3. only now the unit rollback: remove .../rust-desk-hermes.conf (unit-edit approval), then:
  systemctl daemon-reload
  systemctl show -p ExecStart galion-desk.service   # must show /opt/galion/bin/galion-desk
  systemctl restart galion-desk.service && systemctl is-active galion-desk.service
test result: ok. 2 passed; 0 failed
```

`stat` of the 5 Phase A files identical before/after (nothing moved). Step 2 checked for real (read-only, 03:28 CET)
against the manifest the VPS run wrote: `/opt/galion/bin/desk-hermes.py: OK`, `/opt/galion/bin/galion-desk: OK`;
`test -x` ok; galion-desk active, ExecStart still Rust. Nerve `127.0.0.1:8670/state` (cortex not consulted):
galion-desk `active/running`, port 8646, `restarts_1h` 0, no galion-desk incident; open incident is the unrelated
`edge` (cloudflared, tuner-quick).

Left on the VPS (no deletes per slot rule; tmpfiles-cleaned): `/tmp/pyarchive-345-1793154480/` (binary + plan,
382 K) and `/tmp/.n345` (nerve `/state` copy used for the grep above). Owner may remove.

## Local gates (03:29–03:35 CET)

- `cargo test -p galion-pyarchive`: 7 unit + 14 CLI (3 new: `overridden_base_is_flagged_with_exact_safe_rollback`
  checks every field + step order + exact-inverse `mv` per file + tree unchanged; `override_without_note_and_bare_reset`
  covers a drop-in without a note and a bare `ExecStart=` reset on a second unit; `no_override_means_no_unit_rollback_section`).
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 389 suites, 2048 passed, 0 failed, 18 ignored.
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` ok; `scripts/no-python.sh` ok.
- musl release x86_64 565 936 B, aarch64 486 272 B; `cargo check` armv7-unknown-linux-musleabihf and
  armv7-linux-androideabi ok. No new dependencies.
