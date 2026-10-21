<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: guard-rust (galion-guard), slot 38, ports 19380-19389

All times Europe/Zagreb, CEST (+02:00). Box: Debian 13, unprivileged user.

## A. Namespace run with the real kernel nf_tables (2026-10-20 21:26:17 +02:00; historical bash run, superseded by A2)

`scripts/real/guard-rust.sh`: builds the release binary and
`examples/real_peer`, then `unshare -rn` (user + network namespace; root
only inside it). Real `nft` 1.1.3 (Debian debs unpacked with `dpkg -x`).
Stand-in sshd: `real_peer` on 10.99.0.1:19381 (our port range; the jail's
`ports = [19381]` stands in for 22). Attackers 10.99.0.66-70, allowlisted
admin 10.99.0.7, bystander 10.99.0.8, all as /32 on the namespace's lo;
clients `curl --interface <src> telnet://10.99.0.1:19381`. Config: file
source (`poll_ms = 200`), maxretry 5, findtime 60s, bantime 20s, increment
on, `ignoreip = 127.0.0.1/8, ::1, 10.99.0.7`, `protect_accepted = 24h`.
Synthesized auth.log lines in the VPS shapes with live RFC 3339 stamps.

Result: **38 passed, 0 failed (backend=nft)**. Full output:

```
[21:26:19+02:00] backend=nft work=/tmp/guard-rust-real.XXXXXX
[21:26:19+02:00] == P1 setup
   | table inet galion-guard {
   | 	set addr4 {
   | 		type ipv4_addr
   | 		flags timeout
   | 	}
   | 
   | 	set addr6 {
   | 		type ipv6_addr
   | 		flags timeout
   | 	}
   | 
   | 	chain input {
   | 		type filter hook input priority filter - 1; policy accept;
   | 		ip saddr 127.0.0.0/8 accept
   | 		ip6 saddr ::1 accept
   | 		ip saddr { 10.99.0.7, 127.0.0.0/8 } accept
   | 		ip6 saddr ::1 accept
   | 		tcp dport 19381 ip saddr @addr4 reject with icmp port-unreachable
   | 		tcp dport 19381 ip6 saddr @addr6 reject with icmpv6 port-unreachable
   | 	}
   | }
[21:26:19+02:00] PASS nft table/chain/sets created with timeout flag
[21:26:19+02:00] PASS attacker reaches the stand-in sshd before any failure
[21:26:19+02:00] == P2 ban after maxretry=5 within findtime
[21:26:20+02:00] PASS 4 failures: not banned
[21:26:20+02:00] PASS 4 failures counted
[21:26:21+02:00] PASS 5th failure: banned
[21:26:21+02:00] PASS ban is in the kernel set
   | 		elements = { 10.99.0.66 timeout 20s expires 19s272ms }
[21:26:21+02:00] PASS attacker now rejected
[21:26:21+02:00] PASS allowlisted admin still connects
[21:26:21+02:00] PASS unrelated address still connects
[21:26:21+02:00]    ban until 2026-10-20T21:26:40+02:00, ban_number 1
[21:26:21+02:00] == P3 never ban loopback / allowlist
[21:26:23+02:00] PASS still exactly one ban
[21:26:23+02:00] PASS 30 protected hits
[21:26:23+02:00] PASS manual ban of loopback refused
[21:26:23+02:00] PASS manual ban of allowlisted admin refused
[21:26:23+02:00] PASS daemon log never names a protected address
[21:26:23+02:00] == P4 restart persistence (clean stop, then kill -9)
[21:26:23+02:00] PASS ban survives in the kernel while the daemon is down
[21:26:23+02:00] PASS attacker still rejected with the daemon down
[21:26:23+02:00] PASS ban restored from state after clean stop
[21:26:24+02:00] PASS ban restored after kill -9
[21:26:24+02:00] PASS until unchanged across restarts
[21:26:24+02:00] == P5 expiry on time
[21:26:40+02:00]    unbanned after 20s (bantime 20s)
[21:26:40+02:00] PASS unban within 20..22 s
[21:26:40+02:00] PASS kernel set no longer has it
[21:26:40+02:00] PASS attacker reaches sshd again
[21:26:40+02:00] == P6 increment (2nd ban = 2 x bantime) and CLI unban
[21:26:41+02:00] PASS second ban is ban #2
[21:26:41+02:00]    remaining 39s
[21:26:41+02:00] PASS second ban lasts 40 s
   | unbanned 10.99.0.66
[21:26:41+02:00] PASS CLI unban lifts it at once
[21:26:41+02:00] PASS status shows no bans
[21:26:41+02:00] == P7 log rotation (rename; late lines in the old file)
[21:26:42+02:00] PASS failures across rotation add up to a ban
[21:26:43+02:00] PASS copy-truncate: new lines still read
[21:26:43+02:00] == P8 successful login protects a roaming admin
[21:26:44+02:00] PASS no ban after a recent successful login
[21:26:44+02:00] == P9 kernel timeout lifts bans even if the daemon is dead
[21:26:44+02:00] PASS 10.99.0.67 still blocked right after the crash
[21:27:05+02:00] PASS 10.99.0.67 open again with no daemon running (kernel timeout)
[21:27:05+02:00] PASS restart after expiry: stale ban dropped
[21:27:05+02:00] == P10 ignoreself: this host's own addresses are never banned
[21:27:06+02:00] PASS local address not banned
[21:27:06+02:00] PASS self addresses detected
[21:27:07+02:00] == P11 journald source (real child process; fake journalctl replays JSON)
[21:27:08+02:00] PASS journal entries ban 10.99.0.70
[21:27:09+02:00] PASS journalctl restarted from the saved cursor
[21:27:09+02:00] PASS first start used --since (findtime backlog)
   | args: --follow --output=json --no-pager --quiet --lines=all --since=@1792524367 _COMM=sshd
   | args: --follow --output=json --no-pager --quiet --lines=all --after-cursor=s=guard;i=5 _COMM=sshd
[21:27:09+02:00] == P12 teardown
removed table inet galion-guard
[21:27:09+02:00] PASS table removed
[21:27:09+02:00] daemon log (last 12 lines):
   | 2026-10-20T21:27:06+02:00 galion-guard: failure from a protected address ignored (SelfAddress)
   | 2026-10-20T21:27:06+02:00 galion-guard: failure from a protected address ignored (SelfAddress)
   | 2026-10-20T21:27:07+02:00 galion-guard: stopped by control request (bans stay in the kernel with their timeouts)
   | 2026-10-20T21:27:07+02:00 galion-guard: starting: source=journal backend=nft maxretry=5 findtime=60s bantime=20s increment=true allowlist=3 entries, self=0 addresses
   | 2026-10-20T21:27:07+02:00 galion-guard: restored 0 active bans
   | 2026-10-20T21:27:07+02:00 galion-guard: running
   | 2026-10-20T21:27:07+02:00 galion-guard: BAN 10.99.0.70 until 2026-10-20T21:27:27+02:00 (ban #1, 5 failures)
   | 2026-10-20T21:27:08+02:00 galion-guard: stopped by control request (bans stay in the kernel with their timeouts)
   | 2026-10-20T21:27:08+02:00 galion-guard: starting: source=journal backend=nft maxretry=5 findtime=60s bantime=20s increment=true allowlist=3 entries, self=0 addresses
   | 2026-10-20T21:27:08+02:00 galion-guard: restored 1 active bans
   | 2026-10-20T21:27:09+02:00 galion-guard: running
   | 2026-10-20T21:27:09+02:00 galion-guard: stopped by control request (bans stay in the kernel with their timeouts)
[21:27:09+02:00] RESULT: 38 passed, 0 failed (backend=nft)
```

Earlier runs on the same day found two real bugs that are fixed and now
covered by unit tests: copy-truncate refilled past the old offset was not
detected (fixed with a first-line fingerprint), and the script's own
`nft list | grep -q` check raced with `pipefail` (script fix).


## A2. Rust port: tests/real_netns.rs (2026-10-20 23:13:06 +02:00)

`GALION_REAL=1 cargo test --release -p galion-guard --test real_netns`
replaces `scripts/real/guard-rust.sh` (deleted, Rust-only rule) and
`examples/real_peer.rs` (deleted). Same namespace, addresses, config and
checks; stand-in sshd is now a listener thread on 10.99.0.1:19471 (slot 47
range, private netns), clients are source-bound sockets instead of
`curl --interface`, JSON via serde_json instead of `jq`, the fake journalctl
is the test binary itself (`harness = false`). Result: **38 passed, 0
failed (backend=nft)**.

```
[23:13:06+02:00] backend=nft work=/tmp/guard-rust-real.2561811
[23:13:06+02:00] == P1 setup
   | table inet galion-guard {
   | 	set addr4 {
   | 		type ipv4_addr
   | 		flags timeout
   | 	}
   | 
   | 	set addr6 {
   | 		type ipv6_addr
   | 		flags timeout
   | 	}
   | 
   | 	chain input {
   | 		type filter hook input priority filter - 1; policy accept;
   | 		ip saddr 127.0.0.0/8 accept
   | 		ip6 saddr ::1 accept
   | 		ip saddr { 10.99.0.7, 127.0.0.0/8 } accept
   | 		ip6 saddr ::1 accept
   | 		tcp dport 19471 ip saddr @addr4 reject with icmp port-unreachable
   | 		tcp dport 19471 ip6 saddr @addr6 reject with icmpv6 port-unreachable
   | 	}
   | }
[23:13:06+02:00] PASS nft table/chain/sets created with timeout flag
[23:13:06+02:00] PASS attacker reaches the stand-in sshd before any failure
[23:13:06+02:00] == P2 ban after maxretry=5 within findtime
[23:13:07+02:00] PASS 4 failures: not banned
[23:13:07+02:00] PASS 4 failures counted
[23:13:08+02:00] PASS 5th failure: banned
[23:13:08+02:00] PASS ban is in the kernel set
   | 		elements = { 10.99.0.66 timeout 20s expires 19s312ms }
[23:13:09+02:00] PASS attacker now rejected
[23:13:09+02:00] PASS allowlisted admin still connects
[23:13:09+02:00] PASS unrelated address still connects
[23:13:09+02:00]    ban until 2026-10-20T23:13:27+02:00, ban_number 1
[23:13:09+02:00] == P3 never ban loopback / allowlist
[23:13:11+02:00] PASS still exactly one ban
[23:13:11+02:00] PASS 30 protected hits
[23:13:11+02:00] PASS manual ban of loopback refused
[23:13:11+02:00] PASS manual ban of allowlisted admin refused
[23:13:11+02:00] PASS daemon log never names a protected address
[23:13:11+02:00] == P4 restart persistence (clean stop, then kill -9)
[23:13:11+02:00] PASS ban survives in the kernel while the daemon is down
[23:13:12+02:00] PASS attacker still rejected with the daemon down
[23:13:12+02:00] PASS ban restored from state after clean stop
[23:13:13+02:00] PASS ban restored after kill -9
[23:13:13+02:00] PASS until unchanged across restarts
[23:13:13+02:00] == P5 expiry on time
[23:13:27+02:00]    unbanned after 20s (bantime 20s)
[23:13:27+02:00] PASS unban within 20..22 s
[23:13:27+02:00] PASS kernel set no longer has it
[23:13:27+02:00] PASS attacker reaches sshd again
[23:13:27+02:00] == P6 increment (2nd ban = 2 x bantime) and CLI unban
[23:13:28+02:00] PASS second ban is ban #2
[23:13:28+02:00]    remaining 39s
[23:13:28+02:00] PASS second ban lasts 40 s
   | unbanned 10.99.0.66
[23:13:28+02:00] PASS CLI unban lifts it at once
[23:13:28+02:00] PASS status shows no bans
[23:13:28+02:00] == P7 log rotation (rename; late lines in the old file)
[23:13:29+02:00] PASS failures across rotation add up to a ban
[23:13:30+02:00] PASS copy-truncate: new lines still read
[23:13:30+02:00] == P8 successful login protects a roaming admin
[23:13:31+02:00] PASS no ban after a recent successful login
[23:13:31+02:00] == P9 kernel timeout lifts bans even if the daemon is dead
[23:13:32+02:00] PASS 10.99.0.67 still blocked right after the crash
[23:13:53+02:00] PASS 10.99.0.67 open again with no daemon running (kernel timeout)
[23:13:53+02:00] PASS restart after expiry: stale ban dropped
[23:13:53+02:00] == P10 ignoreself: this host's own addresses are never banned
[23:13:54+02:00] PASS local address not banned
[23:13:54+02:00] PASS self addresses detected
[23:13:54+02:00] == P11 journald source (real child process; fake journalctl replays JSON)
[23:13:56+02:00] PASS journal entries ban 10.99.0.70
[23:13:57+02:00] PASS journalctl restarted from the saved cursor
[23:13:57+02:00] PASS first start used --since (findtime backlog)
   | args: --follow --output=json --no-pager --quiet --lines=all --since=@1792530775 _COMM=sshd
   | args: --follow --output=json --no-pager --quiet --lines=all --after-cursor=s=guard;i=5 _COMM=sshd
[23:13:57+02:00] == P12 teardown
   | removed table inet galion-guard
[23:13:57+02:00] PASS table removed
[23:13:57+02:00] daemon log (last 12 lines):
   | 2026-10-20T23:13:54+02:00 galion-guard: failure from a protected address ignored (SelfAddress)
   | 2026-10-20T23:13:54+02:00 galion-guard: failure from a protected address ignored (SelfAddress)
   | 2026-10-20T23:13:54+02:00 galion-guard: stopped by control request (bans stay in the kernel with their timeouts)
   | 2026-10-20T23:13:54+02:00 galion-guard: starting: source=journal backend=nft maxretry=5 findtime=60s bantime=20s increment=true allowlist=3 entries, self=0 addresses
   | 2026-10-20T23:13:54+02:00 galion-guard: restored 0 active bans
   | 2026-10-20T23:13:55+02:00 galion-guard: running
   | 2026-10-20T23:13:55+02:00 galion-guard: BAN 10.99.0.70 until 2026-10-20T23:14:15+02:00 (ban #1, 5 failures)
   | 2026-10-20T23:13:56+02:00 galion-guard: stopped by control request (bans stay in the kernel with their timeouts)
   | 2026-10-20T23:13:56+02:00 galion-guard: starting: source=journal backend=nft maxretry=5 findtime=60s bantime=20s increment=true allowlist=3 entries, self=0 addresses
   | 2026-10-20T23:13:56+02:00 galion-guard: restored 1 active bans
   | 2026-10-20T23:13:56+02:00 galion-guard: running
   | 2026-10-20T23:13:57+02:00 galion-guard: stopped by control request (bans stay in the kernel with their timeouts)
[23:13:57+02:00] RESULT: 38 passed, 0 failed (backend=nft)
```

## B. Real daemon through its CLI (GALION_REAL=1)

`GALION_REAL=1 cargo test -p galion-guard --test real_daemon`
(2026-10-20 21:29 +02:00, full suite 62 passed): real binary, file source, dry-run firewall; ban
after 5 lines, `fw.nft` contains `add element inet galion-guard addr4 {
203.0.113.77 timeout ...`, manual `ban 203.0.113.78 1m` ok, `ban 127.0.0.1`
and `ban 198.51.100.9` (allowlisted) refused, `stop` + restart restores both
bans, `unban`, text `status`, `unban --all`, `stop`: **1 passed**.

## C. Parity with the live fail2ban (read-only VPS probes, 2026-10-20 21:27 +02:00)

VPS journal (`journalctl _SYSTEMD_UNIT=sshd.service + _COMM=sshd -o
short-iso-precise`) and `/var/log/fail2ban.log` from 2026-10-18 20:10:05
+02:00 (fail2ban restart) to 2026-10-20 21:25:26 +02:00, streamed to the
box, processed with `galion-guard test-filter` / `replay`, deleted
immediately. Counts only (no addresses recorded):

```
journal lines: 29641
Lines: 29641, matched: 16913, failures: 791 from 153 addresses, accepted logins: 4159, non-IP hosts: 0
fail2ban filter Found: 791  Ban: 102  Increase: 10
galion-guard replay: bans=102 increased=10
per-address ban multiset: f2b-only=0 ours-only=0 common=102
ban time agreement within 1 s: 101
ban time agreement within 5 s: 1
addresses: f2b=153 ours=153 with differing failure counts=0
```

## D. Fuzzing (box, release build)

```
fuzz seed 1792522975950765232, 60 s  -> 5605923 inputs, no panic, no invented address, no loopback ban
fuzz seed 1792524046363504040, 45 s  -> FAILED before the fix: invented address 2001:db8::fa from
    "ip50 sshd[1013]: Failed password for Invalid user  from 2001:db8::Failed password for 66 r 66 port 40011"
  after the boundary fix (same seed, 45 s) -> 4398640 inputs, clean
fuzz seed 1792522975950765232, 45 s  -> 4368809 inputs, clean
fuzz seed 1792524250526249596, 120 s -> 9595622 inputs, clean
```

## E. Replay of the fixture (`galion-guard replay tests/fixtures/auth-vps-shapes.log --drain`)

```
2026-10-20T20:02:30+02:00 BAN   203.0.113.10 until 2026-10-20T21:02:30+02:00 (ban #1, 5 failures, 1h)
2026-10-20T20:04:00+02:00 PROTECTED failure ignored (Loopback)   (x6, incl. ::1)
2026-10-20T21:02:30+02:00 UNBAN 203.0.113.10 (expired)
replay: 27 lines, 19 matched, 1 bans, 0 still banned at the end
```

No processes left running: the namespace script kills the daemon and peer
and deletes its table; the namespace disappears with the script.
