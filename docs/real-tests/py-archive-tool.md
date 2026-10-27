<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: galion-pyarchive (wave-4 slot 330)

Tool: `crates/galion-pyarchive` (`galion-pyarchive plan|verify`), dry run of Phase A from
`docs/py-archive-plan.md` (slot 308, 98601101). All times Europe/Zagreb CET (+01:00). No secrets appear below.
It is a CLI, not a server, so the slot's port range 22000-22009 was not used.

## What the tool proves

`plan` lists and SHA-256-hashes the Phase A set (`/opt/galion/bin/{desk-hermes.py,galion-desk,pw.py,
__pycache__/*,replicate-picture.py}`), then scans for references:

- systemd system + user units (timers included) in `/etc`, `/run`, `/usr/local/lib`, `/lib`, `/usr/lib`
  with real precedence: masked units (`-> /dev/null`), shadowed fragments/drop-ins, backslash continuations,
  and `Key=` reset semantics (an empty `ExecStart=` in a drop-in turns the base line into `overridden`).
  Unknown keys are treated as lists, so an unknown key can never hide a live line.
- cron: `/etc/crontab`, `/etc/anacrontab`, `/etc/cron.{d,hourly,daily,weekly,monthly}/*`, `/var/spool/cron{,/crontabs}/*`.
- running processes: `/proc/<pid>/exe` and `cmdline`.
- `.py`/`.pyc` items are also matched by basename (`cd /opt/galion/bin && python3 pw.py`), with path-boundary
  checks (`/opt/galion/rust/bin/galion-desk` does not count as `/opt/galion/bin/galion-desk`).

PASS = 5 files present + 0 `active` refs + 0 unreadable files. Lines with credential-like words are redacted.
`--out NEWDIR` writes `phase-a.{plan.txt,sha256,files,json}` and refuses an existing dir or anything under
`/opt/galion` or `/etc`. The crate has no remove/rename/chmod/link/subprocess calls (unit test greps its source).

## Run 1: VPS, read-only (2026-10-28 00:18:39–00:18:56 CET)

`GALION_REAL=1 GALION_PYARCHIVE_SSH=root@<vps> GALION_PYARCHIVE_BIN=<x86_64 musl release> cargo test -p galion-pyarchive --test real_vps -- --nocapture --test-threads 1`

The test copies the 557 744 B static binary into a fresh `/tmp/pyarchive-330-<ts>/` on the VPS, runs
`plan --json --out /tmp/pyarchive-330-<ts>/plan`, then `verify --manifest .../phase-a.sha256`, and compares
`stat -c '%n %s %Y %i'` of the 5 files before/after (identical: nothing moved).

```
vps: verdict "PASS" files 5 bytes 4752180 units 338 cron 9 procs 35 active 0 overridden 1 masked 0 shadowed 0 comment 2 unreadable 0
vps verify exit Some(0)
test result: ok. 2 passed; 0 failed
```

Plan text from the VPS (host ip50, generated 2026-10-28T00:18:51+01:00):

```
files (5):
       7889 B  0755  2026-10-10 11:28 CEST (+02:00)  /opt/galion/bin/desk-hermes.py  sha256 28a393f061319a56c34c51d4347009c0197489cb96b93d1b7230bfca62827e0e
    4735088 B  0755  2026-10-10 11:02 CEST (+02:00)  /opt/galion/bin/galion-desk  sha256 555cc44314aa3d6a2ffe2ca4273cee20c566fb1296ac43c0df7e1ca0a9fd11e6
       2499 B  0755  2026-10-10 13:22 CEST (+02:00)  /opt/galion/bin/pw.py  sha256 f4ab1ddd831f22f3b0ce1de55a3ca2cbe0bff109a2090107ccdd51f14f4c1355
       3735 B  0644  2026-10-10 13:23 CEST (+02:00)  /opt/galion/bin/__pycache__/pw.cpython-312.pyc  sha256 46ee9d7cea2ecb4d3642d08b562313f583f5b7cfe01185be5011f0cddf5fe122
       2969 B  0755  2026-10-10 19:50 CEST (+02:00)  /opt/galion/bin/replicate-picture.py  sha256 a27a684efa3d97f10f5b8ed8d2d4f26e890f05f70c4c4448ece0478ae19a8219
  total 4752180 B
scanned: 338 units (system+user, timers included), 9 cron files, 35 processes
references: active 0  overridden 1  shadowed 0  masked 0  comment 2
  [overridden] unit /etc/systemd/system/galion-desk.service:6 -> /opt/galion/bin/galion-desk  | ExecStart=/opt/galion/bin/galion-desk
  [comment] unit /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf:3 -> /opt/galion/bin/desk-hermes.py
  [comment] unit /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf:3 -> /opt/galion/bin/galion-desk
WARNING rollback dependency: an overridden unit line still names a Phase A file.
verdict: PASS
```

Cross-checks by hand (read-only, 00:19 CET): `systemctl show -p ExecStart galion-desk` → `/opt/galion/rust/bin/galion-desk`;
`systemctl is-active galion-desk` → active; VPS is LXC (`systemd-detect-virt` = lxc), `ls -d /proc/[0-9]*` = 38 and
`ps -e` = 38 (the 35 scanned = minus the probe shell/ssh/tool itself); only python processes are OS
`fail2ban-server` and `networkd-dispatcher`; no node. Nerve `127.0.0.1:8670/state` (cortex not consulted): no
incident for galion-desk; open incidents are unrelated (inc-865945dd galion-one mouth stuck loading 6.0 h, inc-06ffb32f edge).

Left on the VPS (not deleted, per slot rule "NO deletes"): `/tmp/pyarchive-330-1793143087/` and
`/tmp/pyarchive-330-1793143122/` (binary + plan outputs, 372 KB each on disk, mode 0700 dir, root). Owner may remove.

## Run 2: build box `/` (same test, 00:18 CET)

```
box: verdict "FAIL" missing 5 units 285 cron 3 procs 116 active 0
```

Expected FAIL (Phase A files only exist on the VPS); 0 unit/cron refs. The first attempt at 00:18:04 saw 2 transient
`process` hits on the shared box (other agents' ssh probes naming VPS paths); the box test now asserts only
unit/cron refs and prints process hits.

## Fixture tests (`cargo test -p galion-pyarchive`): 7 unit + 11 CLI, all pass

VPS-shaped tree PASS with 1 overridden + rollback warning and an unchanged tree snapshot; live unit FAIL; drop-in without
`ExecStart=` reset keeps base line active (FAIL); `/run` service with continuation line + timer FAIL; masked + shadowed
not counted; cron basename hit FAIL while the commented line is not; fake `/proc/4242/cmdline` FAIL, PASS with `--no-procs`;
credential line redacted; missing file FAIL; `--out` + `verify` PASS then tamper → MISMATCH exit 1, re-use of `--out` exit 2;
`--out /opt/galion/...` refused; bad usage exit 2. SHA-256 NIST vectors (`abc`, empty) checked.

## Finding for Phase A (owner)

`/etc/systemd/system/galion-desk.service` line 6 still says `ExecStart=/opt/galion/bin/galion-desk`; only the drop-in
`rust-desk-hermes.conf` overrides it. The drop-in's documented rollback ("rm this drop-in; daemon-reload; restart") would
start the old ELF again, which after Phase A would be gone. Before/with Phase A: either fold the Rust ExecStart into the
base unit (separate unit-edit approval) or change the rollback note to "restore from phase-a.tar first".
