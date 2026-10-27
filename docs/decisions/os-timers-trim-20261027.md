<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# OS timers / networkd-dispatcher / postfix trim memo (slot 285, 2026-10-27 ~17:57 CET +01:00)

Read-only probes only (`systemctl list-timers --all`, `status`, `is-enabled`, `cat`, `ss -ltnp`, `journalctl` reads, `postconf -h`, `mailq`). Nothing disabled, masked or written. Times converted to Europe/Zagreb CET +01:00.

## Timers observed (15)
| Timer | Last run (CET) | Proposal | Risk if removed |
|---|---|---|---|
| galion-backup.timer (00/6:17, ±5 min) | 27 Oct 13:17 | **keep** | High: our only backup schedule |
| sysstat-collect.timer (10 min) | 27 Oct 17:50 | keep (cheap, feeds capacity data) | Low |
| sysstat-summary.timer | 27 Oct 01:07 | keep | Low |
| logrotate.timer | 27 Oct 01:00 | **keep** | High: disk fill |
| systemd-tmpfiles-clean.timer | 26 Oct 23:01 | **keep** | Medium: /tmp growth |
| apt-daily.timer | 27 Oct 17:44 | keep | Low-Med: stale package lists |
| apt-daily-upgrade.timer (unattended-upgrades) | 27 Oct 07:07 | keep (security patches); owner may move to a window | Medium: unpatched CVEs if removed; minor restart risk if kept |
| dpkg-db-backup.timer | 27 Oct 01:00 | keep | Low |
| e2scrub_all.timer | 25 Oct 04:10 | keep | Low |
| man-db.timer | 27 Oct 07:23 | optional remove | ~None (man index only) |
| motd-news.timer | 27 Oct 16:01 | remove-after-owner-yes | None; also stops an outbound call to Canonical |
| fstrim.timer | never (enabled, no next) | optional; check VPS disk supports discard first | Low |
| ua-timer.timer (Ubuntu Pro) | never | remove-after-owner-yes if no Pro subscription | Low |
| systemd-sysupdate.timer | never | remove-after-owner-yes (unused on Ubuntu) | None |
| systemd-sysupdate-reboot.timer | never | remove-after-owner-yes | None; removes a latent auto-reboot path |

## networkd-dispatcher (item 7)
- Active since 22 Oct 22:46 CET, ~10 MB RSS, 107 ms CPU total; systemd-networkd active, NetworkManager inactive.
- `/etc/networkd-dispatcher/*.d` hook directories are all **empty** → the daemon does nothing.
- Proposal: **remove-after-owner-yes** (disable, not purge). Risk: Low; only matters if a future hook is added. Rollback: `systemctl enable --now`.

## postfix (item 8)
- `postfix.service` active; master listens **127.0.0.1:25 and [::1]:25 only** (`inet_interfaces = loopback-only`), relayhost empty, mydestination local only.
- Queue empty; 0 `status=sent` in last 5 days; 1 postfix/smtp journal line in 30 days. Effectively unused (local cron/unattended-upgrades mail sink only).
- Proposal: **optional / remove-after-owner-yes**. Risk: Low-Medium — cron and unattended-upgrades lose their local mail sink (output silently dropped; `/usr/sbin/sendmail` callers may error). Prerequisite: grep crontabs for MAILTO and our services for sendmail use before disabling. No exposure today (loopback only), so keeping is also safe.

## Summary trim set
Keep: galion-backup, logrotate, tmpfiles-clean, sysstat×2, apt-daily×2, dpkg-db-backup, e2scrub_all.
Optional: man-db, fstrim, postfix.
Remove-after-owner-yes: motd-news, ua-timer, systemd-sysupdate, systemd-sysupdate-reboot, networkd-dispatcher.
Saving is small (~10–30 MB RAM, a few wakeups/day); main value is less noise and fewer outbound/auto-reboot paths.

## Owner decisions
- **D-285-1**: Disable motd-news, ua-timer, systemd-sysupdate(+reboot) timers? (yes / no)
- **D-285-2**: Disable networkd-dispatcher (empty hooks)? (yes / no)
- **D-285-3**: Postfix: keep loopback-only (status quo) / disable after MAILTO+sendmail audit?
- **D-285-4**: Unattended upgrades: keep as-is / pin to a maintenance window / manual only?
- **D-285-5**: fstrim and man-db: leave / enable fstrim / disable man-db?
Any execution is a separate, owner-approved slot (this memo changes nothing on the VPS).
