<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# guard-rust: `galion-guard`, a Rust replacement for the VPS fail2ban

Branch `swarm/guard-rust` (base main 13a296c + merge of swarm/nerve 5802d0e),
slug `guard-rust`, ports 19380-19389. Started 2026-10-20 20:43 +02:00 (CEST).

Nothing was changed or deployed on the VPS. Every VPS command below was a
read-only probe (`fail2ban-client status/get`, `grep`/`cat` of config files,
`nft list`, `journalctl` reads, `sshd -T`). No files were written there.

## 1. What the VPS runs today (probed 2026-10-20 20:43-20:50 +02:00)

| Item | Value |
|---|---|
| fail2ban | 1.0.2 (Python), `fail2ban.service` active since 2026-10-18 20:10:04 +02:00 |
| Host | Ubuntu, kernel 6.8.12-43-pve, nftables v1.0.9, system TZ Etc/UTC |
| Jails | **1**: `sshd` |
| Config files | `/etc/fail2ban/jail.conf` (stock), `jail.d/defaults-debian.conf` (stock Debian), `jail.d/galion-sshd.local` (ours, 2026-10-18). No `jail.local`, no `fail2ban.local`, `fail2ban.d/` empty, no custom `action.d`/`filter.d` files (all stock, dated 2022-11-09). |
| Backend | `systemd` (journald), journal match `_SYSTEMD_UNIT=sshd.service + _COMM=sshd` (`+` = OR; the unit on this host is actually `ssh.service`, so matching works through `_COMM=sshd`) |
| Filter | stock `filter.d/sshd.conf`, `mode = normal` (22 failregex, no ignoreregex), `maxlines` 1, `usedns = warn`, `maxmatches = 5` |
| maxretry | 5 |
| findtime | 10m (600 s) |
| bantime | 1h (3600 s) |
| bantime.increment | **true**, factor 1 (default), formula default `bantime * 2^min(n-1, 20)` for the n-th ban, `bantime.maxtime` unset (no cap), `bantime.overalljails` false |
| ignoreip | 3 entries: `127.0.0.0/8`, `::1`, and one public IPv4 (the owner's; value deliberately not recorded here) |
| ignoreself | true (default) |
| Ban action | `nftables` (multiport type), `port = ssh` (22), `protocol = tcp`, `blocktype = reject` |
| nft objects | `table inet f2b-table` -> `chain f2b-chain { type filter hook input priority filter - 1; policy accept; }` -> rule `tcp dport 22 ip saddr @addr-set-sshd reject with icmp port-unreachable`; set `addr-set-sshd { type ipv4_addr; }` with **no kernel timeout flag** (fail2ban removes elements itself). The IPv6 set `addr6-set-sshd` is only created on the first IPv6 ban (none yet). |
| Persistence | `/var/lib/fail2ban/fail2ban.sqlite3`, `dbpurgeage = 1d`, `dbmaxmatches = 10` (bans are restored on restart) |
| sshd | port 22, `PasswordAuthentication no`, `KbdInteractiveAuthentication no`, `PermitRootLogin without-password`, `MaxAuthTries 3`, `UseDNS no`, `LogLevel INFO` |
| Logs | journald, and rsyslog writes `/var/log/auth.log` with RFC 3339 timestamps (`2026-10-20T18:49:14.028938+00:00 host ...`), rotated weekly (`auth.log.1`, `auth.log.2.gz`) |

Counters at probe time: currently failed 0, total failed 1097, currently
banned 5, total banned 105 (since the 2026-10-18 20:10 +02:00 restart);
`fail2ban.log` holds 115 `Ban` and 100 `Unban` lines. Increments are real:
the log shows e.g. `Increase Ban ... (6 # 1d 8h ...)` (6th ban = 32 h) and
`(2 # 2h ...)`, which confirms the formula `1h * 2^(n-1)`.

sshd message mix over the last 24 h (IPs, ports, users and PIDs masked;
top lines): 2434 `Connection closed by IP port N`, 845 `Accepted publickey`,
604 `Received disconnect ...:11: disconnected by user`, 517 `Received
disconnect ...:11: Bye Bye [preauth]`, 463 `Invalid user U from IP port N`,
333 `Disconnected from invalid user U IP port N [preauth]`, 291 `Connection
closed by authenticating user U IP port N [preauth]`, 202 `Disconnected from
authenticating user ...`, 127 `Connection reset by authenticating user ...`,
87 `fatal: Timeout before authentication for IP port N`, 5 `error: maximum
authentication attempts exceeded for U from IP port N ssh2 [preauth]`, 5
`Disconnecting authenticating user U IP port N: Too many authentication
failures [preauth]`. The synthesized replay corpus in
`crates/galion-guard/tests/` uses exactly these shapes.

Note: with password auth off, most counted failures are `Invalid user`
(rule 5) and `maximum authentication attempts exceeded` (rule 14). The
OpenSSH 9 form `Disconnecting authenticating user U IP port N: Too many
authentication failures` is matched by no stock rule (rule 17 only knows the
old `Disconnecting: Too many authentication failures` form). `galion-guard`
keeps that parity on purpose (see section 4).
