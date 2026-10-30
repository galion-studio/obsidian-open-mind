<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Cutover doc: galion-backup (galion-backup.timer + galion-backup.service, no listener)

- Status: **NOT CUT OVER.** The bash script `galion-backup.sh` from ws/foundation still runs every 6 h. The Rust `galion-backup run` port is built and tested but **not deployed**.
- **Approved: No** for the Rust cutover. Open owner items: deploy timing, plus B1 gzip level and B2 the ZFS `size_kb` value (slot 119, CALL-RECORD ~03:53).
  The 2026-10-18 install (ws/foundation + offsite drop-in) has no approval line in CALL-RECORD, which starts 2026-10-20.
- Related: on 2026-10-21 04:31 the Rust `galion-deploy backup` used the Rust galion-backup once, by hand, for the cutover snapshot (`/var/backups/galion/cutover-20261021`, CALL-RECORD 04:31). The timer was not changed.
- Written 2026-10-21 17:30 CEST (+02:00) by wave-4 slot 203 from [../galion-backup.md](../galion-backup.md), CALL-RECORD and read-only VPS probes at 17:22:05 and 17:23:35 (+02:00) ([evidence](../real-tests/docs-cutover-missing.md)).

## 1. What runs (probe 17:22:05)

| Item | Value |
|---|---|
| timer | `/etc/systemd/system/galion-backup.timer` (158 B, sha256 `d590eccac113006c9ed6e2140914da84e5a84babe788abc3992317f81ba52d52`), enabled, `OnCalendar=*-*-* 00/6:17:00`, `RandomizedDelaySec=5min`, `Persistent=true`; last 14:17:49, next 20:21:46 |
| service | `/etc/systemd/system/galion-backup.service` (195 B, sha256 `49f17ae6e0980b7c5fcad0bc019e4e3d44235f9d3a40a403d182edc7aee1e067`), static oneshot, `Nice=19`, `IOSchedulingClass=idle`, `UMask=0077`; last result success, exit 0 at 14:17:52 |
| drop-in | `galion-backup.service.d/offsite.conf` (2026-10-18 22:22:07, sha256 `e370c03716b738eeff3228538e8dd73ece668f5ba459b0e9bf3e8f5ca5e196ff`): `ExecStartPost=/opt/galion/offsite/galion-offsite-encrypt.sh`, `ExecStopPost=… --perms-only` |
| script | `/opt/galion/rust/bin/galion-backup.sh`, 3,026 B, sha256 `0897efc0c8314c023ede9604098c2e55a2ef861d9bbc90a951bbe19584cf80a4` = `e43d7ab:deploy/foundation/galion-backup.sh` |
| helper binary | `/opt/galion/rust/bin/galion-backup`, 1,599,472 B, sha256 `3df3f88d9087c34ecba7f95758703603930a0ef89a724ad5a59be71937911d09` (2026-10-18; helper subcommands only, it **predates** `run`) |
| offsite hook | `/opt/galion/offsite/galion-offsite-encrypt.sh`, 2,937 B, sha256 `ed7dbfa6cbc8fcad251e9ee4be6d4a3a4cb96675276b5484a98165693701f87b` (not a Galion Rust port; out of scope) |
| output | `/var/backups/galion/galion-<stamp>.tar.gz`, 12 kept (oldest 2026-10-18 22:22, newest `galion-20261021-141749+0200.tar.gz` 18,612,783 B); `.age` copies in `offsite/` (12 kept) |

Today's runs (journal): 02:21:06 ok, 08:18:21 ok, 14:17:52 ok. Each was followed by `galion-offsite-encrypt` `msg=ok new=1 kept=12`.

## 2. Verify (on the VPS, read-only)

```
systemctl list-timers galion-backup.timer --no-pager                       # next run within 6 h
systemctl show galion-backup.service -p Result -p ExecMainStatus           # success / 0
journalctl -u galion-backup.service -n 20 --no-pager | grep 'msg="ok"'     # one per run
ls -1t /var/backups/galion/galion-*.tar.gz | head -1                       # newest stamp < 6 h 10 min old
sha256sum /opt/galion/rust/bin/galion-backup.sh                            # 0897efc0…80a4
```

Known log quirk (B2, confirmed 17:23): the run logs `size_kb=1` (14:17) because `du -k` runs before ZFS has allocated the blocks.
The same file now shows `du -k` 18,191 KiB, and `stat` shows 18,612,783 B. The Rust port copies the same `du` behaviour (`crates/galion-backup/src/backup.rs`, `du_kb`).

## 3. Prepared Rust cutover (NOT executed, needs approval)

Candidate: `galion-backup` from `swarm/shared-tar-crate` 820740e (rust-ports 26a2a8e + cutover-prep a5ac49b + shared galion-tar). Parity vs bash 26/26, `files.tar` byte-identical
(CALL-RECORD ~04:20). Perf on a VPS-sized tree: bash 3.46 s vs Rust musl 4.30 s (+25 %), less RAM (5.8–7.4 MiB vs 10.2) (slot 119).
Shape: install as `/opt/galion/rust/bin/galion-backup-<sha>` (do not overwrite the 2026-10-18 helper the script still calls), back up the unit, and add
`/etc/systemd/system/galion-backup.service.d/50-rust-bin.conf` with `ExecStart=` + `ExecStart=/opt/galion/rust/bin/galion-backup-<sha> run`.
Keep `offsite.conf` unchanged. Verify with one manual `systemctl start galion-backup.service` (owner-approved) and compare the archive layout with the previous run.
Avoid the DST fold: no live change 2026-10-25 01:00–04:00 (freeze rule being drafted by slot 210, queued).

Rollback for that future cutover (composed here, not rehearsed): `rm /etc/systemd/system/galion-backup.service.d/50-rust-bin.conf && systemctl daemon-reload`.
It is a oneshot, so no restart is needed; the next timer run uses the bash script again.

## 4. Open points

- B1 gzip level (6 or 5), B2 `size_kb` fix (use the file length) and deploy timing are owner decisions (slot 119).
- The nerve has no spec for `galion-backup.timer` (plans/nerve.md T2.3). galion-health checks only that the timer is active, not that the last run succeeded.

**2026-10-30 (B2 fixed, Mate approval):** branch `swarm/backup-size-fix-20261030` logs `size_kb` from the real byte length (`fs::metadata().len()`) plus `size_bytes=`, fsyncs the archive before the rename and DEST after it. The bash parity test drops `size_bytes` and checks it against the archive length.
