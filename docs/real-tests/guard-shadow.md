<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: guard-shadow (galion-guard shadow mode), slot 123, ports 19930-19939

All times Europe/Zagreb, CEST (+02:00). Box: Debian 13, unprivileged user.
The /tmp path in the output is a temp dir on the box, removed by the test.

## A. Real binary in shadow mode (2026-10-21 04:46 +02:00)

`GALION_REAL=1 cargo-slot test --release -p galion-guard --test real_shadow -- --nocapture`

Release `galion-guard`, `backend = "shadow"`, file source (`poll_ms = 100`),
maxretry 3, findtime 60 s, bantime 4 s, `protect_accepted = "0"`,
`ignoreip = ["192.0.2.7"]` (stands in for the owner), `nft` pointed at a
path that does not exist (any firewall attempt would log an error).
Synthesized auth.log lines with live stamps; documentation addresses only.

```
running 1 test
[2026-10-21T04:46:17+02:00] check-config: ok: source=file backend=shadow table=galion-guard ports=[19930] maxretry=3 findtime=60s bantime=4s increment=true factor=1 maxtime=None purge_age=86400s protect_accepted=0s allowlist=3 entries ignoreself=false
[2026-10-21T04:46:17+02:00] started: backend="shadow" ready=true shadow={"firewall_writes":0,"log":"/tmp/guard-real-shadow-3701723/shadow.log","write_errors":0,"zone":"zagreb"}
[2026-10-21T04:46:17+02:00] after 3 failures: currently_banned=1 total_failed=3 protected_hits=3
[2026-10-21T04:46:17+02:00] status text shows SHADOW mode line
[2026-10-21T04:46:17+02:00] teardown: shadow backend: no firewall table was ever created; nothing to remove (nft not run)
[2026-10-21T04:46:21+02:00] shadow log (14 lines):
   | 2026-10-21 04:46:17,000 galion-guard.filter     [3701726]: INFO    [sshd] Found 203.0.113.66 - 2026-10-21 04:46:17
   | 2026-10-21 04:46:17,000 galion-guard.shadow     [3701726]: INFO    [sshd] Ignore <protected address withheld> by Allowlist
   | 2026-10-21 04:46:17,000 galion-guard.filter     [3701726]: INFO    [sshd] Found 203.0.113.66 - 2026-10-21 04:46:17
   | 2026-10-21 04:46:17,000 galion-guard.shadow     [3701726]: INFO    [sshd] Ignore <protected address withheld> by Allowlist
   | 2026-10-21 04:46:17,000 galion-guard.filter     [3701726]: INFO    [sshd] Found 203.0.113.66 - 2026-10-21 04:46:17
   | 2026-10-21 04:46:17,000 galion-guard.actions    [3701726]: NOTICE  [sshd] Ban 203.0.113.66
   | 2026-10-21 04:46:17,000 galion-guard.shadow     [3701726]: INFO    [sshd] would ban 203.0.113.66 until 2026-10-21T04:46:21+02:00 (3 failures; no firewall write)
   | 2026-10-21 04:46:17,000 galion-guard.shadow     [3701726]: INFO    [sshd] Ignore <protected address withheld> by Allowlist
   | 2026-10-21 04:46:17,000 galion-guard.actions    [3701726]: NOTICE  [sshd] Ban 203.0.113.67
   | 2026-10-21 04:46:17,000 galion-guard.shadow     [3701726]: INFO    [sshd] would ban 203.0.113.67 until 2026-10-21T04:47:17+02:00 (0 failures, manual; no firewall write)
   | 2026-10-21 04:46:17,000 galion-guard.actions    [3701726]: NOTICE  [sshd] Unban 203.0.113.67
   | 2026-10-21 04:46:17,000 galion-guard.shadow     [3701726]: INFO    [sshd] unban reason: operator
   | 2026-10-21 04:46:21,000 galion-guard.actions    [3701726]: NOTICE  [sshd] Unban 203.0.113.66
   | 2026-10-21 04:46:21,000 galion-guard.shadow     [3701726]: INFO    [sshd] unban reason: expired
[2026-10-21T04:46:21+02:00] compare (same events, UTC vs Zagreb) exit 0:
   | galion-guard shadow comparison (sshd jail)
   | window: 2026-10-21T04:46:17+02:00 .. 2026-10-21T04:46:21+02:00 (4s), match tolerance 5 s
   | 
   | metric                           fail2ban   galion-guard
   | failures (Found)                        3              3
   | failing addresses                       1              1
   | bans                                    2              2
   | banned addresses                        2              2
   | increased bans (#>=2)                   0              0
   | ban numbers                          #1x2           #1x2
   | unbans (of these bans)                  2              2
   | restored bans                           0              0   (restarts only, not compared)
   | unbans of earlier bans                  0              0   (bans from before the window, not compared)
   | 
   | bans matched: 2 same second, 0 within 5 s (max |delta| 0 s)
   | bans only fail2ban: 0   only galion-guard: 0   ban-number mismatches: 0
   | addresses with a different failure count: 0
   | 
   | verdict: PARITY (galion-guard would have done exactly what fail2ban did)
   | 
   | fail2ban ban               galion-guard ban           delta        address (ban #)
   | 2026-10-21T04:46:17+02:00  2026-10-21T04:46:17+02:00  =            203.0.113.66 (#1 / #1)
   | 2026-10-21T04:46:17+02:00  2026-10-21T04:46:17+02:00  =            203.0.113.67 (#1 / #1)
[2026-10-21T04:46:21+02:00] compare (fail2ban ban removed) exit 3: parity=false only_guard=1
test real_shadow_mode ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.12s
```

Checked by the test as well: daemon log contains `SHADOW MODE` and no
`firewall error`; the canary `nft` path never appeared; `192.0.2.7` is in
neither the shadow log nor the daemon log; shadow log mode is 0600; manual
`ban` of the allowlisted address is refused.

## B. Past replay vs the live fail2ban (read-only, 2026-10-21 04:41 +02:00)

```
ssh slavik-vps "journalctl -q --no-pager -o short-iso-precise --since '2026-10-18 18:10:05' _SYSTEMD_UNIT=sshd.service + _COMM=sshd" > $W/journal.log   # 33 230 lines
ssh slavik-vps "cat /var/log/fail2ban.log" > $W/fail2ban.log                                                                                      # 1 678 lines
galion-guard replay journal.log --config parity.toml --shadow-log shadow-parity.log      # protect_accepted = "0"
  -> replay: 33230 lines, 19348 matched, 121 bans, 5 still banned at the end
galion-guard replay journal.log --config staged.toml --shadow-log shadow-staged.log      # protect_accepted = "24h"
  -> replay: 33230 lines, 19348 matched, 121 bans, 5 still banned at the end
galion-guard compare fail2ban.log shadow-parity.log --fail2ban-zone utc --shadow-zone zagreb --mask --timeline   # exit 0
galion-guard compare fail2ban.log shadow-staged.log ...                                                           # exit 0, identical report
rm -rf $W     # temp copies deleted right after
```

(`--since '2026-10-18 18:10:05'` is in the VPS's own zone, UTC; that is
2026-10-18 20:10:05 +02:00.) `$W` was a 0700 `mktemp -d` dir on the box.
Report, addresses masked (first 10 of 121 timeline rows):

```
galion-guard shadow comparison (sshd jail)
window: 2026-10-18T20:15:49+02:00 .. 2026-10-21T04:30:08+02:00 (2d 8h 14m 19s), match tolerance 5 s

metric                           fail2ban   galion-guard
failures (Found)                      983            983
failing addresses                     185            185
bans                                  121            121
banned addresses                      108            108
increased bans (#>=2)                  13             13
ban numbers                  #1x108 #2x7 #3x3 #4x1 #5x1 #6x1 #1x108 #2x7 #3x3 #4x1 #5x1 #6x1
unbans (of these bans)                116            116
restored bans                           0              0   (restarts only, not compared)
unbans of earlier bans                  4              0   (bans from before the window, not compared)

bans matched: 44 same second, 77 within 5 s (max |delta| 2 s)
bans only fail2ban: 0   only galion-guard: 0   ban-number mismatches: 0
addresses with a different failure count: 0

verdict: PARITY (galion-guard would have done exactly what fail2ban did)

fail2ban ban               galion-guard ban           delta        address (ban #)
2026-10-18T21:14:13+02:00  2026-10-18T21:14:12+02:00  -1 s         ip-1 (#1 / #1)
2026-10-18T23:29:27+02:00  2026-10-18T23:29:26+02:00  -1 s         ip-2 (#1 / #1)
2026-10-18T23:59:07+02:00  2026-10-18T23:59:06+02:00  -1 s         ip-3 (#1 / #1)
2026-10-19T00:54:58+02:00  2026-10-19T00:54:58+02:00  =            ip-4 (#1 / #1)
2026-10-19T02:02:27+02:00  2026-10-19T02:02:27+02:00  =            ip-5 (#1 / #1)
2026-10-19T02:03:02+02:00  2026-10-19T02:03:01+02:00  -1 s         ip-4 (#2 / #2)
2026-10-19T02:04:01+02:00  2026-10-19T02:04:01+02:00  =            ip-6 (#1 / #1)
2026-10-19T03:43:51+02:00  2026-10-19T03:43:50+02:00  -1 s         ip-7 (#1 / #1)
2026-10-19T04:05:44+02:00  2026-10-19T04:05:43+02:00  -1 s         ip-4 (#3 / #3)
2026-10-19T05:22:35+02:00  2026-10-19T05:22:35+02:00  =            ip-8 (#1 / #1)
... (111 more rows: 76 x '-1 s', 1 x '-2 s', 44 x '=' in total; no ONLY F2B / ONLY GUARD rows)
```

Journal match width (read-only count, last 24 h at 04:43 +02:00): fail2ban's
match `_SYSTEMD_UNIT=sshd.service + _COMM=sshd` 10 065 lines; galion-guard's
wider match (+ `ssh.service`, `sshd-session`) 10 065 lines.

`fail2ban-client status sshd` at 04:40 +02:00 (addresses masked): currently
failed 1, total failed 1304, currently banned 5, total banned 125.
