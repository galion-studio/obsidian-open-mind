<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# galion-desk base unit before Phase A: two staged options (wave-4 slot 372)

Branch `swarm/pyarchive-base-unit-stage` off `swarm/py-archive-rollback-note` dd74d6c0. All times Europe/Zagreb
CET (+01:00). **Staged only. Nothing was installed, no unit file on the VPS was written, no daemon-reload, no
restart.** Installing either option needs the owner's unit-edit approval (decision D-345-1 or D-345-2, see
`docs/py-archive-plan.md` §8).

## 1. The trap (live, read-only probe 2026-10-28 06:21:55 CET)

| File | sha256 | Key lines |
|---|---|---|
| `/etc/systemd/system/galion-desk.service` (171 B) | see below | line 6 `ExecStart=/opt/galion/bin/galion-desk` (old ELF, spawns `python3 desk-hermes.py`) |
| `/etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf` (382 B) | see below | line 2 `# Rollback: rm this drop-in; …`, lines 5-8 `ExecStart=` reset, Rust ExecStart, 2 × `Environment=` |

Full hashes: base `a1ebbdd3a80ad573310df646ce112324e803b3dc6c4e71a8d0d971364f2dbcbf`, drop-in
`e3a0e10acb135cd92aa06f17fb381f1c0f4d6f83b027ce2211f9f52212318d9a`. Effective (`systemctl show`):
`ExecStart=/opt/galion/rust/bin/galion-desk`, `Environment=DESK_BIND=127.0.0.1:8646
DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`, active/running, MainPID 264677, NRestarts 0, enabled, systemd 255.

Phase A (2026-11-03) archives `/opt/galion/bin/galion-desk` and `desk-hermes.py`. After that, following the
drop-in's own line-2 rollback ("rm this drop-in") revives the base line, points at a missing file, and the unit
loops on `Restart=on-failure`.

## 2. Option D-345-1: fold the Rust ExecStart into the base unit (recommended)

Staged file: `deploy/staged/galion-desk/galion-desk.service`. Diff against the live base (repo-only `# SPDX-` and
`# STAGED` marker lines left out, because the install step filters them):

```diff
--- live:/etc/systemd/system/galion-desk.service
+++ staged (installed form)
@@
+# Folded per D-345-1 (slot 372): the base unit no longer names the old ELF that Phase A archives.
+# Rollback: restore the two backed-up unit files first, then daemon-reload (docs/ops/pyarchive-base-unit-stage.md section 4).
 [Unit]
 Description=Galion desk
 After=network.target
 
 [Service]
-ExecStart=/opt/galion/bin/galion-desk
+ExecStart=/opt/galion/rust/bin/galion-desk
+Environment=DESK_BIND=127.0.0.1:8646
+Environment=DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes
 Restart=on-failure
 RestartSec=2
 
 [Install]
 WantedBy=multi-user.target
```

and the drop-in `rust-desk-hermes.conf` is retired (moved to the backup folder, not deleted). The effective
ExecStart + Environment are the same strings as the live merged unit (checked by the CLI test
`staged_fold_d345_1_removes_the_trap`), so the running process needs no restart.

Install steps (NOT executed; for the owner-approved window, as root on the VPS):

```sh
B=/root/unit-backup-$(date +%Y%m%d-%H%M%S); mkdir -m 0700 "$B"
cp -a /etc/systemd/system/galion-desk.service /etc/systemd/system/galion-desk.service.d "$B"/
sha256sum "$B"/galion-desk.service "$B"/galion-desk.service.d/rust-desk-hermes.conf   # expect a1ebbdd3… / e3a0e10a…
mkdir -m 0700 /tmp/stage372
grep -v -e '^# SPDX-' -e '^# STAGED' galion-desk.service > /tmp/stage372/galion-desk.service   # staged file copied over
systemd-analyze verify /tmp/stage372/galion-desk.service                                   # must print nothing, exit 0
install -m 0644 -o root -g root /tmp/stage372/galion-desk.service /etc/systemd/system/galion-desk.service
mv /etc/systemd/system/galion-desk.service.d/rust-desk-hermes.conf "$B"/retired-rust-desk-hermes.conf
rmdir /etc/systemd/system/galion-desk.service.d
systemctl daemon-reload
systemctl show -p ExecStart -p Environment -p DropInPaths -p MainPID galion-desk.service
#   ExecStart path=/opt/galion/rust/bin/galion-desk, same Environment, DropInPaths= (empty), MainPID unchanged
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8646/health                 # 200
galion-pyarchive plan | grep -c OVERRIDDEN-BASE                                            # 0
```

Optional: one `systemctl restart galion-desk` at a quiet time to prove the folded unit starts on its own (the
nerve watches `galion-desk`; expect `restarts_1h` 1, status healthy).

## 3. Option D-345-2: keep the drop-in, reword its rollback note

Staged file: `deploy/staged/galion-desk/rust-desk-hermes.conf`. Comment-only change (assignments identical):

```diff
--- live:rust-desk-hermes.conf
+++ staged (installed form)
 # t314u 2026-10-27: cut over live galion-desk to Rust desk-hermes.
-# Rollback: rm this drop-in; systemctl daemon-reload; systemctl restart galion-desk
-# (old /opt/galion/bin/galion-desk + desk-hermes.py left untouched).
+# Rollback: restore Phase A files first if archived (mv -n back + sha256sum -c, docs/py-archive-plan.md section 8), then rm this drop-in; systemctl daemon-reload; systemctl restart galion-desk
+# (old /opt/galion/bin/galion-desk + desk-hermes.py untouched until Phase A, 2026-11-03; base ExecStart still names them).
 [Service]
 ExecStart=
 ExecStart=/opt/galion/rust/bin/galion-desk
```

Install: back up as in §2, `grep -v -e '^# SPDX-' -e '^# STAGED' rust-desk-hermes.conf >` the live path,
`systemd-analyze verify`, `systemctl daemon-reload` (comments only, no restart needed). The trap stays (base still
names the old ELF); only the instruction next to it becomes safe. `galion-pyarchive plan` keeps reporting
`OVERRIDDEN-BASE` with the exact safe rollback, but no longer prints `STALE ROLLBACK NOTE` (slot 372 change: a
rollback comment whose line says "restore … first" is not stale).

## 4. Rollback of either option

Before Phase A: `cp -a "$B"/galion-desk.service /etc/systemd/system/`, put `rust-desk-hermes.conf` back into
`/etc/systemd/system/galion-desk.service.d/` (mkdir if needed), `sha256sum` both against the hashes in §1,
`systemctl daemon-reload`, `systemctl show -p ExecStart galion-desk.service` must still show the Rust path. After
Phase A this rollback is still safe, because it restores the base **and** the drop-in together (the Rust ExecStart
stays effective). Never restore only the base without the drop-in after Phase A.

## 5. Comparison

| | D-345-1 fold | D-345-2 reword |
|---|---|---|
| Trap after Phase A | gone (base names Rust binary) | stays, documented |
| Live effect | daemon-reload; same effective ExecStart/Env; no restart needed | daemon-reload; comments only |
| `galion-pyarchive plan` | overridden_base 0 | overridden_base 1, stale notes 0 |
| Matches repo staged style | yes (staged units are single files) | no |
| Risk | one unit rewrite (verified offline, §6) | none, but relies on people reading the note |

Recommendation: D-345-1 before 2026-11-03 20:05, then Phase A. D-345-2 is the fallback if no unit-edit window.

## 6. Evidence

`docs/real-tests/pyarchive-base-unit-stage.md` (VPS read-only `systemd-analyze verify` incl. negative controls, and
`galion-pyarchive plan --root` on a throwaway copy of the live trees in all three states).
