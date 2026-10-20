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

## 2. `galion-guard` (crates/galion-guard)

One Rust binary, std threads, no async runtime, no unsafe code. Library
modules (all `#[cfg(test)]`-covered):

| Module | What it does |
|---|---|
| `filter` | The 22 sshd `failregex` rules + `prefregex` of fail2ban 1.0.2 `sshd.conf` `mode = normal`, re-expressed for the `regex` crate (see 2.1). Lines whose prefix does not name `sshd`/`sshd-session` are ignored. Lines > 8 KiB are ignored. |
| `session` | Multi-line (per connection, `host sshd[pid]: `) state: pending failures without an address (rules 15/17), `nofail` lines, multi-user connections, `Accepted` (gained) and `forget` (disconnect). Bounded (4096 connections, 10 min idle TTL). |
| `jail` | fail2ban's failure counting (2.2), ban time with increment, repeat-offender weighting, history retention, expiry, never-ban protections (2.4), memory bound (65 536 tracked addresses). |
| `nft` | `Backend` trait; `Nft` (system `nft`, script on stdin) and `DryRun` (in memory, logs the scripts it would run). |
| `source` | Rotation-safe `FileTail` for auth.log and a `journalctl --follow --output=json` follower. |
| `state` | Persistent JSON state (atomic: temp file, fsync, rename, fsync dir; mode 0600); corrupt state is moved aside and the daemon starts with no bans (fail open). |
| `engine` | Line -> filter -> session -> jail -> firewall + state; clock-injected, so the daemon, `replay` and the tests run the same code. |
| `daemon` | Source thread, Unix control socket (0600 in a 0700 dir), main loop with 1 s ticks. |
| `config` | TOML config; defaults = the VPS jail. `ignoreself` from `/proc/net/fib_trie` + `/proc/net/if_inet6`. |

CLI: `run`, `status [--json]`, `unban <ip>|--all`, `ban <ip> [duration]`,
`stop` (save state and exit; systemd `ExecStop`), `check-config`,
`test-filter <file|->` (like `fail2ban-regex`), `replay <file|-> [--json]
[--drain]` (replays a log with its own timestamps through the engine and a
dry-run firewall), `teardown` (delete the nft table), `version`.

Size: release x86_64-musl 1.77 MB, aarch64-musl 1.41 MB (static).
`cargo check` passes for aarch64-linux-android and armv7-unknown-linux-musleabihf.

### 2.1 Filter translation (regex crate has no look-around, no conditionals)

* IPv6 tail `(?<=:):` -> `:` (every preceding repetition item already ends in
  `:`, so the look-behind is always true at that point: identical language).
* Rules 2 and 3, `(?:(?! from ).)*?` and `(?(cond_user): |(?:(?! from ).)*$)`:
  a lazy user capture plus a post-check (accept if the user is one token and
  the tail starts with `": "`, or if neither the user (invalid-user form) nor
  the tail contains `" from "`). This keeps fail2ban's protection against a
  user name that embeds `from <victim>`.
* Digits written `[0-9]`; every captured address is parsed with `std::net`;
  IPv4-mapped IPv6 (`::ffff:a.b.c.d`) is folded to IPv4 everywhere.
* `<HOST>` that is not an IP literal (`dns` group): fail2ban `usedns = warn`
  would resolve it; we never do DNS (sshd runs `UseDNS no`, and a PTR answer
  is attacker data). Reported as "non-IP host", never banned. 0 such lines
  in 50 h of real VPS logs.
* **Hardening, stricter than fail2ban** (found by the fuzzer): an address
  must end at a token boundary (end, space, tab, `]`, `)`, `:`, `,`). Without
  it, `from 2001:db8::Failed ...` yields `2001:db8::fa` (fail2ban's own regex
  does the same). sshd never logs such lines, so parity on real logs is
  unaffected.
* `sshd-session` accepted next to `sshd` (OpenSSH >= 9.8 splits per-session
  processes; the VPS runs 9.6, so no effect today).

### 2.2 Counting, ban times, history (all fail2ban 1.0.2 behaviour)

* **Window** (not a plain sliding window): per address a retry count and an
  interval start. When a failure arrives more than `findtime` after the
  interval start, the count is rescaled to the observed rate over one
  findtime, `round_half_even(retry / (t - first) * findtime)`, and the start
  moves to `t - findtime`; then the failure is added. An address whose last
  failure is older than findtime starts from zero. Ban when the count
  reaches `maxretry`. (A plain sliding window was the first version; the
  real-log comparison in 3.3 showed fail2ban bans slow-but-steady attackers
  that a sliding window misses, so this was changed to match.)
* **Ban time**: `bantime * factor * 2^min(n-1, 20)` for the n-th ban
  (1h, 2h, 4h, ... 32h for the 6th), optional `bantime_maxtime` cap (unset
  on the VPS, so unset in the staged config too; recommended: `1w`).
* **Repeat offenders count faster** (fail2ban observer): with k earlier bans
  on record, one failure weighs `2^k / 2 + 1` = 2, 3, 5, 9 ... capped at
  maxretry. So a returning attacker is banned after 3, then 2, then 1 failure.
* **History retention**: a ban record is kept until
  `ban start + 3 * ban length + purge_age` (fail2ban `_purge_bips`,
  `outDatedFactor = 3`, `dbpurgeage = 1d`). fail2ban only purges hourly, so it
  can remember a record up to 1 h longer; we do not imitate that lag.
* Lines older than findtime (journal backlog at start) are not counted; log
  times in the future are clamped to now.

### 2.3 nftables backend: evaluation and decision

| Option | Licence | Verdict |
|---|---|---|
| `rustables` 0.9 (pure-Rust netlink nf_tables) | GPL-3.0-or-later | not allowed |
| `nftnl` 0.9 / `mnl` 0.3 | MIT/Apache, but FFI to C libnftnl/libmnl (GPL-2.0 / LGPL-2.1) | C libraries to link; not pure Rust; libnftnl is GPL |
| `nftables` 0.6 (JSON API) | MIT OR Apache-2.0 | itself shells out to `nft -j`; adds nothing over our 40 lines |
| `neli` 0.7 / `netlink-packet-netfilter` 0.4 | BSD-3 / MIT | generic netlink only; nf_tables batch messages (NFT_MSG_NEWSETELEM, set element timeouts, transactions) would be hand-written: ~1-2 kLOC of kernel ABI we would own and test against each kernel |

Decision: **shell out to the system `nft`** (`/usr/sbin/nft -f -`, script on
stdin, argv only, never a shell). Every address in a script is a parsed
`IpAddr`, the table name is validated (`[A-Za-z0-9_-]{1,32}`), ports are
`u16`, so nothing from a log reaches nft as text. Revisit if a permissively
licensed pure-Rust nf_tables crate matures. The `Backend` trait makes that a
drop-in change.

Ruleset (own table, independent of fail2ban's `f2b-table`, so both can run
side by side): `table inet galion-guard` with sets `addr4`/`addr6`
(`flags timeout`), chain `input` (`hook input priority filter - 1; policy
accept`), rules: accept 127.0.0.0/8 and ::1, accept the allowlist, then
`tcp dport { 22 } ip saddr @addr4 reject` and the ip6 twin (fail2ban uses
`reject` too). Ban = add/delete/add of the element with `timeout <n>s` in
one transaction (upsert with a fresh timeout); unban = add 1s + delete (never
fails on an absent element). On start the daemon re-applies live bans with
their remaining time and removes elements it has no record of.

journald: the `systemd`/`journald`/`sd-journal` crates are LGPL/AGPL (not
allowed) or C bindings; `systemd-journal-reader` 0.1 (Apache-2.0) is young
and does not follow a live journal. So the daemon runs `journalctl --follow
--output=json --lines=all` with `--after-cursor=<saved cursor>` (or
`--since=@<now - findtime>` on first start) and the jail's matches, and
restarts it after 5 s if it exits.

### 2.4 Fail-safe: how the admin is never locked out

1. Loopback (127.0.0.0/8, ::1, IPv4-mapped forms) and unspecified addresses
   are hard-coded never-ban, independent of config; config always adds them
   to the allowlist; a `/0` allowlist entry is rejected.
2. `ignoreip` allowlist and `ignoreself` (this host's addresses) are checked
   before counting, before every ban (including manual `ban`), on restore,
   and every tick (an allowlist change lifts an existing ban).
3. The nft chain accepts loopback and allowlisted sources **before** the
   reject rules, so even a wrong set element cannot block them.
4. `protect_accepted = "24h"` (not in fail2ban): an address that logged in
   successfully with a key in the last 24 h is never banned and is unbanned
   at once when it logs in. Set `"0"` for strict parity.
5. Every kernel element has a timeout: if the daemon crashes or is stopped,
   bans still lift on schedule (tested: P9). No permanent bans (`-1`
   rejected).
6. Only `tcp dport 22` is filtered, chain policy accept, no other ports.
7. Errors fail open: firewall failure = no ban (retried every 30 s);
   unreadable state = start with no bans; DNS hosts never banned.
8. Logs and `status` never print protected addresses (only the reason and
   counts); `status` shows the allowlist size, not its entries.
9. Rollback is one command: `galion-guard teardown` removes the table.

### 2.5 Sources

* `FileTail`: keeps the old file open across rename rotation and drains it
  to EOF before switching (no lost lines); copy-truncate is detected by size
  and by an FNV-1a fingerprint of the first line (so truncate-and-refill
  past the old offset is caught); partial last lines are buffered (64 KiB
  cap); 4 MiB per poll; position (dev, inode, offset, fingerprint) is saved
  and only reused if the same file is still there.
* Journal: see 2.3; cursor saved in the state file.

## 3. Tests

### 3.1 Unit and replay tests (`cargo test -p galion-guard`): 62 tests

50 unit tests (filter rules, injection, IPv6/mapped/bracketed, boundary,
sessions, window rescale incl. a real VPS sequence, increment, weighting,
retention, maxtime, overflow, protections, restore, bounds, state atomicity
and corruption, nft scripts and JSON parsing, config validation, /proc
parsers, tail rotation/truncation/resume, journal JSON), 9 replay tests on
synthesized VPS-shaped logs (`tests/replay.rs`, fixture
`tests/fixtures/auth-vps-shapes.log`), 2 fuzz tests, 1 gated real-daemon
test (`GALION_REAL=1`).

### 3.2 Fuzzing (`tests/fuzz_parser.rs`)

Std-only xorshift mutation fuzzer over the corpus (insert/delete/swap/
duplicate, token splicing with ` from `, `::ffff:`, brackets, NUL, U+FFFD,
long tails ...), through stamp parsing, filter, sessions, the full engine
and journal JSON. Invariants: no panic, a matched address always appears in
the line, loopback never banned. Runs: 60 s 5.6 M inputs, 2 x 45 s 4.4 M,
120 s 9.6 M inputs: clean after the boundary fix. One real finding (the
`2001:db8::Fa...` case in 2.1). `GALION_FUZZ_SECS=<n> [GALION_FUZZ_SEED=<s>]`.

### 3.3 Parity against the live fail2ban (read-only, aggregates only)

The VPS journal for the jail (`_SYSTEMD_UNIT=sshd.service + _COMM=sshd`)
from fail2ban's restart (2026-10-18 20:10:05 +02:00) to 2026-10-20 21:25
+02:00 was streamed over ssh into `galion-guard test-filter` / `replay` on
the box, compared with `/var/log/fail2ban.log` for the same window, and the
temporary copies were deleted at once. Only counts are recorded here.

| Metric | fail2ban 1.0.2 | galion-guard |
|---|---|---|
| journal lines | 29 641 | 29 641 |
| failures (filter `Found`) | 791 from 153 addresses | 791 from 153 addresses; per-address counts identical for all 153 |
| bans | 102 | 102, same addresses; 101 at the same second, 1 within 5 s |
| increased bans (n >= 2) | 10 (5x 2h, 2x 4h, 8h, 16h, 1d 8h) | 10, same distribution |

Failure counts matched from the first version. Intermediate versions (plain
sliding window; no repeat-offender weighting; history = ban end + 1 d)
missed several of fail2ban's bans; each difference was traced to fail2ban
1.0.2 behaviour (`FailTicket.adjustTime`, observer `failureFound`,
`_purge_bips`) and re-implemented (2.2), giving the table above.

### 3.4 Real-life run (scripts/real/guard-rust.sh)

Release binary, unprivileged `unshare -rn` user+network namespace, real
kernel nf_tables via `nft` 1.1.3 from Debian debs unpacked with `dpkg -x`
(no root, host firewall untouched), stand-in sshd on 10.99.0.1:19381
(`examples/real_peer.rs`), clients bound to extra lo addresses with
`curl --interface`. 38/38 checks pass; output in docs/real-tests/guard-rust.md.

## 4. Known differences from fail2ban (deliberate)

* No DNS resolution of non-IP hosts (fail2ban `usedns = warn` would resolve).
* Address boundary hardening (2.1).
* `protect_accepted` (2.4 item 4), on by default; `"0"` = parity.
* Kernel-side timeouts on set elements (fail2ban's set has none and relies on
  the daemon to unban).
* Own nft table `galion-guard` (chain `input`, sets `addr4`/`addr6`), not
  `f2b-table`; IPv6 set always present.
* History purge without fail2ban's up-to-1 h hourly-purge lag.
* Only the `sshd` jail (the only one on the VPS). No `recidive`, no
  `bantime.rndtime`, no `bantime.multipliers`, no `overalljails`.
* The OpenSSH 9 `Disconnecting authenticating user ... Too many
  authentication failures` form is not counted, exactly like the stock
  filter (rule 14 `maximum authentication attempts exceeded` already counts
  that connection once).

## 5. Rollout (needs Slavik's approval; nothing done)

Staged files only: `deploy/staged/galion-guard.service`,
`deploy/staged/galion-guard.toml`. Proposed steps:

1. Shadow: install `/usr/local/bin/galion-guard` (x86_64-musl build),
   `/etc/galion-guard/guard.toml` from the staged file **plus the owner's
   address in `ignoreip`** (copy from `jail.d/galion-sshd.local`), backend
   `dry-run`. Start the unit next to fail2ban; compare `galion-guard status`
   and `dry-run.nft` with `fail2ban-client status sshd` for a few days.
2. Cutover: `systemctl stop fail2ban && systemctl disable fail2ban`, set
   `backend = "nft"`, restart galion-guard. Optionally seed current bans
   with `galion-guard ban <ip> <remaining>`.
3. Rollback: `systemctl stop galion-guard && galion-guard teardown &&
   systemctl enable --now fail2ban`.

Unit hardening: root with `CapabilityBoundingSet=CAP_NET_ADMIN
CAP_DAC_READ_SEARCH`, `ProtectSystem=strict`, `RestrictAddressFamilies=
AF_UNIX AF_NETLINK`, `MemoryMax=64M`, `ProtectKernelModules=yes` (nf_tables
modules are already loaded on the VPS because fail2ban uses them; on a fresh
host load them first). `ExecStop=galion-guard stop` saves state; bans stay
in the kernel with their timeouts.

## 6. Open issues

* journald tested with a fake `journalctl` (real child process, real
  cursor/`--since` handling) because the box has no journald; the real
  journalctl JSON field set was checked on the VPS (`journalctl -o json`
  field names only).
* No live-VPS run of the daemon (not allowed); parity is from replaying the
  real journal (3.3).
* Rule 17 / OpenSSH 9 form left at stock parity; adding it would count one
  more failure per `MaxAuthTries` connection (a policy change for Slavik).
* `nft` dependency at runtime (2.3).
